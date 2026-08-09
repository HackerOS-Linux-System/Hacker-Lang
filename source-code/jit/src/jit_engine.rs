use anyhow::{bail, Result};
use cranelift_codegen::ir::condcodes::FloatCC;
use cranelift_codegen::ir::{
    types, AbiParam, Block, Function, InstBuilder, MemFlags, Signature, UserFuncName, Value,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::{settings, Context};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use hl_compiler::bytecode::*;
use std::collections::{HashMap, HashSet};

/// Próg wywołań funkcji/pętli przed JIT kompilacją (whole-function path —
/// obecnie nieużywany w praktyce, patrz komentarz przy `record_call`)
const JIT_THRESHOLD: u32 = 50;

/// Skompilowany JIT fragment — wskaźnik do kodu maszynowego
pub struct JitFragment {
    /// fn(regs: *mut u64, vars: *mut u64, reg_count: u32, var_count: u32) -> i32
    /// Zwraca offset instrukcji, na którym interpreter ma wznowić wykonanie
    /// (patrz interpreter.rs::exec_native_trace — używa TEGO zwróconego i32,
    /// już nie ignoruje go na rzecz sztywnego exit_offset).
    pub fn_ptr: unsafe extern "C" fn(*mut u64, *mut u64, u32, u32) -> i32,
}

/// Menedżer JIT — śledzi wywołania i kompiluje hotspoty.
///
/// UWAGA: `record_call`/`execute_compiled`/whole-function `jit_compile` nie
/// są obecnie wywoływane z żadnego miejsca poza tym plikiem — jedyna REALNA
/// ścieżka wejścia to `compile_trace_entry` (Trace JIT, wołane z
/// `interpreter.rs::try_compile_trace`). Zostawione dla kompletności/API,
/// ale nieużywane w praktyce.
pub struct JitEngine {
    call_counts: HashMap<String, u32>,
    compiled:    HashMap<String, JitFragment>,
    enabled:     bool,
}

impl JitEngine {
    pub fn new() -> Self {
        Self {
            call_counts: HashMap::new(),
            compiled:    HashMap::new(),
            enabled:     !std::env::var("HL_NO_JIT").is_ok(),
        }
    }

    pub fn record_call(&mut self, name: &str, module: &HlModule) -> bool {
        if !self.enabled { return false; }
        if self.compiled.contains_key(name) { return true; }

        let count = self.call_counts.entry(name.to_string()).or_insert(0);
        *count += 1;

        if *count >= JIT_THRESHOLD {
            if let Some(entry) = module.funcs.find(name) {
                if is_jit_eligible(module, entry) {
                    match self.jit_compile(name, module, entry, &HashMap::new()) {
                        Ok(frag) => {
                            tracing::debug!("[jit] skompilowano funkcję '{}'", name);
                            self.compiled.insert(name.to_string(), frag);
                            return true;
                        }
                        Err(e) => {
                            tracing::warn!("[jit] błąd kompilacji '{}': {}", name, e);
                        }
                    }
                }
            }
        }
        false
    }

    pub fn is_compiled(&self, name: &str) -> bool {
        self.compiled.contains_key(name)
    }

    pub fn execute_compiled(
        &self,
        name: &str,
        regs: &mut Vec<u64>,
        vars: &mut Vec<u64>,
    ) -> Option<i32> {
        let frag = self.compiled.get(name)?;
        if regs.is_empty() { regs.resize(64, 0); }
        if vars.is_empty() { vars.resize(64, 0); }
        let result = unsafe {
            (frag.fn_ptr)(
                regs.as_mut_ptr(),
                          vars.as_mut_ptr(),
                          regs.len() as u32,
                          vars.len() as u32,
            )
        };
        Some(result)
    }

    /// Kompilacja Cranelift → kod maszynowy.
    /// `var_slots`: name_idx (stała stringowa nazwy zmiennej) → slot w vars_flat,
    /// WCZEŚNIEJ rozwiązany i zweryfikowany jako bezpieczny (patrz moduł-level doc).
    fn jit_compile(
        &self,
        name: &str,
        module_bc: &HlModule,
        entry: &FuncEntry,
        var_slots: &HashMap<u32, u32>,
    ) -> Result<JitFragment> {
        let flags = settings::Flags::new(settings::builder());
        let isa   = cranelift_native::builder()
        .map_err(|e| anyhow::anyhow!("Brak ISA: {}", e))?
        .finish(flags)?;

        let jit_builder = JITBuilder::with_isa(
            isa,
            cranelift_module::default_libcall_names(),
        );

        let mut jit_module = JITModule::new(jit_builder);

        let mut sig = Signature::new(CallConv::SystemV);
        let ptr_type = jit_module.target_config().pointer_type();
        sig.params.push(AbiParam::new(ptr_type));    // regs: *mut u64
        sig.params.push(AbiParam::new(ptr_type));    // vars: *mut u64
        sig.params.push(AbiParam::new(types::I32));  // reg_count: u32
        sig.params.push(AbiParam::new(types::I32));  // var_count: u32
        sig.returns.push(AbiParam::new(types::I32));

        let func_id = jit_module.declare_function(name, Linkage::Local, &sig)?;
        let mut ctx = Context::new();
        ctx.func = Function::with_name_signature(
            UserFuncName::user(0, 0),
                                                 sig.clone(),
        );

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fn_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);

            let regs_ptr = builder.block_params(entry_block)[0];
            let vars_ptr = builder.block_params(entry_block)[1];

            compile_func_body(&mut builder, module_bc, entry, regs_ptr, vars_ptr, var_slots, entry_block)?;

            // seal_all_blocks() zamiast ręcznego seal_block per blok — udokumentowana
            // metoda Cranelift dla przypadków z cyklami (pętle wstecz), gdzie
            // sekwencyjne sealowanie w trakcie tłumaczenia jest niepraktyczne.
            builder.seal_all_blocks();
            builder.finalize();
        }

        jit_module.define_function(func_id, &mut ctx)?;
        jit_module.finalize_definitions()?;

        let fn_ptr = jit_module.get_finalized_function(func_id);
        let fn_typed: unsafe extern "C" fn(*mut u64, *mut u64, u32, u32) -> i32 =
        unsafe { std::mem::transmute(fn_ptr) };

        std::mem::forget(jit_module);

        Ok(JitFragment { fn_ptr: fn_typed })
    }
}

impl Default for JitEngine {
    fn default() -> Self { Self::new() }
}

/// Sprawdź czy blok instrukcji kwalifikuje się do JIT.
/// Kwalifikuje się: czysta arytmetyka + ładowanie stałych + SetVar/GetVar +
/// porównania + kontrola przepływu. NIGDY `LoadStr`/`ExecCmd`/`CallQuick`/
/// itp. — to jest fundament dowodu bezpieczeństwa opisanego w doku modułu.
fn is_jit_eligible(module: &HlModule, entry: &FuncEntry) -> bool {
    let start = entry.start_insn as usize;
    let end   = start + entry.insn_count as usize;

    for insn in module.instructions.get(start..end).unwrap_or(&[]) {
        match insn {
            Instruction::LoadNum { .. } |
            Instruction::LoadBool { .. } |
            Instruction::LoadNil { .. } |
            Instruction::GetVar { .. } |
            Instruction::SetVar { .. } |
            Instruction::Add { .. } |
            Instruction::Sub { .. } |
            Instruction::Mul { .. } |
            Instruction::Div { .. } |
            Instruction::Mod { .. } |
            Instruction::Neg { .. } |
            Instruction::CmpEq { .. } |
            Instruction::CmpNe { .. } |
            Instruction::CmpLt { .. } |
            Instruction::CmpLe { .. } |
            Instruction::CmpGt { .. } |
            Instruction::CmpGe { .. } |
            Instruction::ToNumber { .. } |
            Instruction::Jump { .. } |
            Instruction::JumpIfFalse { .. } |
            Instruction::JumpIfTrue { .. } |
            Instruction::Return { .. } |
            Instruction::Nop => {}

            _ => return false,
        }
    }
    true
}

/// Dodatkowa reguła bezpieczeństwa TYLKO dla tras (nie whole-function JIT):
/// `Return` w ŚRODKU trasy pętli (nie jako jej ostatnia instrukcja) oznacza
/// wyjście z całej otaczającej funkcji, nie tylko z pętli — to wymagałoby
/// osobnej obsługi sygnału powrotu, której ten kompilator nie implementuje.
/// Zamiast ryzykować złą semantykę, po prostu odrzucamy taką trasę.
pub fn is_trace_safe(module: &HlModule, entry: &FuncEntry) -> bool {
    if !is_jit_eligible(module, entry) { return false; }
    let start = entry.start_insn as usize;
    let end   = start + entry.insn_count as usize;
    let insns = match module.instructions.get(start..end) { Some(s) => s, None => return false };
    for (i, insn) in insns.iter().enumerate() {
        if matches!(insn, Instruction::Return { .. }) && i != insns.len() - 1 {
            return false;
        }
    }
    true
}

/// Kontekst współdzielony między compile_func_body i compile_insn — jeden
/// obiekt zamiast rosnącej listy parametrów.
struct Ctx<'a> {
    module:      &'a HlModule,
    start:       u32,
    end_excl:    u32,
    reg_vars:    HashMap<u32, Variable>,
    var_vars:    HashMap<u32, Variable>,
    var_slots:   HashMap<u32, u32>,
    blocks:      HashMap<u32, Block>,
    exit_blocks: HashMap<u32, Block>,
    regs_ptr:    Value,
    vars_ptr:    Value,
}

impl<'a> Ctx<'a> {
    fn is_internal(&self, off: u32) -> bool { off >= self.start && off < self.end_excl }
}

/// Zapisz wszystkie śledzone rejestry i zmienne z powrotem do pamięci, po
/// czym wyemituj `return offset`. Wywoływane na KAŻDYM punkcie wyjścia z
/// kompilowanego fragmentu (zewnętrzny skok / naturalny koniec zakresu) —
/// dzięki temu interpreter, wznawiając wykonanie od zwróconego offsetu,
/// zawsze widzi spójny, w pełni zapisany stan.
fn emit_store_and_return(builder: &mut FunctionBuilder, ctx: &Ctx, offset: u32) {
    for (&reg_idx, &var) in &ctx.reg_vars {
        let val = builder.use_var(var);
        builder.ins().store(MemFlags::new(), val, ctx.regs_ptr, (reg_idx as i32) * 8);
    }
    for (&name_idx, &var) in &ctx.var_vars {
        let slot = ctx.var_slots[&name_idx];
        let val  = builder.use_var(var);
        builder.ins().store(MemFlags::new(), val, ctx.vars_ptr, (slot as i32) * 8);
    }
    let off_val = builder.ins().iconst(types::I32, offset as i64);
    builder.ins().return_(&[off_val]);
}

/// Zwróć (tworząc przy pierwszym użyciu) blok-stub dla ZEWNĘTRZNEGO celu —
/// blok, który tylko zapisuje stan i zwraca `offset`, bez dalszej logiki.
fn get_or_make_exit_block(builder: &mut FunctionBuilder, ctx: &mut Ctx, offset: u32) -> Block {
    if let Some(&b) = ctx.exit_blocks.get(&offset) { return b; }
    let b = builder.create_block();
    ctx.exit_blocks.insert(offset, b);
    let saved = builder.current_block();
    builder.switch_to_block(b);
    emit_store_and_return(builder, ctx, offset);
    if let Some(prev) = saved { builder.switch_to_block(prev); }
    b
}

/// Kompiluj ciało trasy/funkcji do IR Cranelift z prawdziwym CFG.
fn compile_func_body(
    builder: &mut FunctionBuilder,
    module: &HlModule,
    entry: &FuncEntry,
    regs_ptr: Value,
    vars_ptr: Value,
    var_slots: &HashMap<u32, u32>,
    entry_block: Block,
) -> Result<()> {
    let start = entry.start_insn;
    let end_excl = entry.start_insn + entry.insn_count;
    let insns = match module.instructions.get(start as usize..end_excl as usize) {
        Some(s) => s,
        None    => bail!("Nieprawidłowy zakres instrukcji"),
    };

    let mut reg_set: HashSet<u32> = HashSet::new();
    let mut block_starts: HashSet<u32> = HashSet::new();
    block_starts.insert(start);

    for (i, insn) in insns.iter().enumerate() {
        let off = start + i as u32;
        for r in insn_regs(insn) { reg_set.insert(r); }
        match insn {
            Instruction::Jump { offset } => {
                if *offset >= start && *offset < end_excl { block_starts.insert(*offset); }
            }
            Instruction::JumpIfFalse { offset, .. } | Instruction::JumpIfTrue { offset, .. } => {
                if *offset >= start && *offset < end_excl { block_starts.insert(*offset); }
                if off + 1 < end_excl { block_starts.insert(off + 1); }
            }
            _ => {}
        }
    }

    let mut blocks: HashMap<u32, Block> = HashMap::new();
    blocks.insert(start, entry_block);
    for &off in &block_starts {
        if off == start { continue; }
        blocks.insert(off, builder.create_block());
    }

    let mut reg_vars: HashMap<u32, Variable> = HashMap::new();
    for &reg in &reg_set {
        let var = builder.declare_var(types::F64);
        reg_vars.insert(reg, var);
    }
    let mut var_vars: HashMap<u32, Variable> = HashMap::new();
    for (&name_idx, _slot) in var_slots {
        let var = builder.declare_var(types::F64);
        var_vars.insert(name_idx, var);
    }

    for (&reg_idx, &var) in &reg_vars {
        let val = builder.ins().load(types::F64, MemFlags::new(), regs_ptr, (reg_idx as i32) * 8);
        builder.def_var(var, val);
    }
    for (&name_idx, &var) in &var_vars {
        let slot = var_slots[&name_idx];
        let val  = builder.ins().load(types::F64, MemFlags::new(), vars_ptr, (slot as i32) * 8);
        builder.def_var(var, val);
    }

    let mut ctx = Ctx {
        module, start, end_excl,
        reg_vars, var_vars,
        var_slots: var_slots.clone(),
        blocks,
        exit_blocks: HashMap::new(),
        regs_ptr, vars_ptr,
    };

    let mut last_was_terminator = false;
    for (i, insn) in insns.iter().enumerate() {
        let off = start + i as u32;
        if off != start && block_starts.contains(&off) {
            if !last_was_terminator {
                let target = ctx.blocks[&off];
                builder.ins().jump(target, &[]);
            }
            builder.switch_to_block(ctx.blocks[&off]);
        }
        last_was_terminator = compile_insn(builder, insn, off, &mut ctx)?;
    }

    if !last_was_terminator {
        emit_store_and_return(builder, &ctx, end_excl);
    }

    Ok(())
}

/// Zapisz wynik porównania (i8 z fcmp) jako f64 0.0/1.0 do rejestru docelowego.
fn store_bool_result(builder: &mut FunctionBuilder, ctx: &Ctx, dst: u32, cmp: Value) {
    let one_f  = builder.ins().f64const(1.0);
    let zero_f = builder.ins().f64const(0.0);
    let r      = builder.ins().select(cmp, one_f, zero_f);
    if let Some(&v) = ctx.reg_vars.get(&dst) { builder.def_var(v, r); }
}

/// Kompiluje pojedynczą instrukcję. Zwraca `true`, jeśli wyemitowała
/// terminator bloku (jump/brif/return) — wołający MUSI wtedy przełączyć się
/// na kolejny blok przed kompilacją następnej instrukcji.
fn compile_insn(builder: &mut FunctionBuilder, insn: &Instruction, off: u32, ctx: &mut Ctx) -> Result<bool> {
    macro_rules! gv {
        ($r:expr) => {
            if let Some(&v) = ctx.reg_vars.get(&$r) { builder.use_var(v) }
            else { builder.ins().f64const(0.0) }
        };
    }
    macro_rules! dv {
        ($r:expr, $val:expr) => {
            if let Some(&v) = ctx.reg_vars.get(&$r) { builder.def_var(v, $val); }
        };
    }

    match insn {
        Instruction::LoadNum { dst, idx } => {
            let n   = ctx.module.consts.numbers.get(*idx as usize).copied().unwrap_or(0.0);
            let val = builder.ins().f64const(n);
            dv!(*dst, val);
            Ok(false)
        }
        Instruction::LoadBool { dst, val } => {
            let n = if *val { 1.0f64 } else { 0.0f64 };
            let v = builder.ins().f64const(n);
            dv!(*dst, v);
            Ok(false)
        }
        Instruction::LoadNil { dst } => {
            let v = builder.ins().f64const(0.0);
            dv!(*dst, v);
            Ok(false)
        }

        Instruction::GetVar { dst, name } => {
            if let Some(&var) = ctx.var_vars.get(name) {
                let val = builder.use_var(var);
                dv!(*dst, val);
            } else {
                let v = builder.ins().f64const(0.0);
                dv!(*dst, v);
            }
            Ok(false)
        }
        Instruction::SetVar { name, src } => {
            if let Some(&var) = ctx.var_vars.get(name) {
                let v = gv!(*src);
                builder.def_var(var, v);
            }
            Ok(false)
        }

        Instruction::Add { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let r=builder.ins().fadd(va,vb); dv!(*dst,r); Ok(false) }
        Instruction::Sub { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let r=builder.ins().fsub(va,vb); dv!(*dst,r); Ok(false) }
        Instruction::Mul { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let r=builder.ins().fmul(va,vb); dv!(*dst,r); Ok(false) }
        Instruction::Div { dst, a, b } => {
            let va = gv!(*a); let vb = gv!(*b);
            let zero = builder.ins().f64const(0.0);
            let is_zero = builder.ins().fcmp(FloatCC::Equal, vb, zero);
            let div     = builder.ins().fdiv(va, vb);
            let r       = builder.ins().select(is_zero, zero, div);
            dv!(*dst, r);
            Ok(false)
        }
        Instruction::Mod { dst, a, b } => {
            let va = gv!(*a); let vb = gv!(*b);
            let ia = builder.ins().fcvt_to_sint_sat(types::I64, va);
            let ib = builder.ins().fcvt_to_sint_sat(types::I64, vb);
            let is_zero = builder.ins().icmp_imm(cranelift_codegen::ir::condcodes::IntCC::Equal, ib, 0);
            let one_i   = builder.ins().iconst(types::I64, 1);
            let safe_ib = builder.ins().select(is_zero, one_i, ib);
            let rem     = builder.ins().srem(ia, safe_ib);
            let zero_i  = builder.ins().iconst(types::I64, 0);
            let rem_or_zero = builder.ins().select(is_zero, zero_i, rem);
            let r = builder.ins().fcvt_from_sint(types::F64, rem_or_zero);
            dv!(*dst, r);
            Ok(false)
        }
        Instruction::Neg { dst, src } => { let v=gv!(*src); let r=builder.ins().fneg(v); dv!(*dst,r); Ok(false) }

        Instruction::CmpEq { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::Equal, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpNe { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::NotEqual, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpLt { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::LessThan, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpLe { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::LessThanOrEqual, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpGt { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::GreaterThan, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpGe { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::GreaterThanOrEqual, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }

        Instruction::ToNumber { dst, src } => { let v=gv!(*src); dv!(*dst,v); Ok(false) }

        Instruction::Nop => Ok(false),

        Instruction::Jump { offset } => {
            let target = resolve_block(builder, ctx, *offset);
            builder.ins().jump(target, &[]);
            Ok(true)
        }
        Instruction::JumpIfFalse { cond, offset } => {
            let cond_f = gv!(*cond);
            let zero = builder.ins().f64const(0.0);
            let cond_bool = builder.ins().fcmp(FloatCC::NotEqual, cond_f, zero);
            let fallthrough_off = off + 1;
            let true_block  = resolve_block(builder, ctx, fallthrough_off);
            let false_block = resolve_block(builder, ctx, *offset);
            builder.ins().brif(cond_bool, true_block, &[], false_block, &[]);
            Ok(true)
        }
        Instruction::JumpIfTrue { cond, offset } => {
            let cond_f = gv!(*cond);
            let zero = builder.ins().f64const(0.0);
            let cond_bool = builder.ins().fcmp(FloatCC::NotEqual, cond_f, zero);
            let fallthrough_off = off + 1;
            let true_block  = resolve_block(builder, ctx, *offset);
            let false_block = resolve_block(builder, ctx, fallthrough_off);
            builder.ins().brif(cond_bool, true_block, &[], false_block, &[]);
            Ok(true)
        }
        Instruction::Return { .. } => {
            emit_store_and_return(builder, ctx, ctx.end_excl);
            Ok(true)
        }

        _ => Ok(false),
    }
}

/// Rozwiąż blok docelowy dla danego offsetu: wewnętrzny → istniejący blok
/// z mapy; zewnętrzny → stub zwracający ten offset.
fn resolve_block(builder: &mut FunctionBuilder, ctx: &mut Ctx, off: u32) -> Block {
    if ctx.is_internal(off) {
        ctx.blocks[&off]
    } else {
        get_or_make_exit_block(builder, ctx, off)
    }
}

/// Zbierz wszystkie rejestry użyte w instrukcji (dla deklaracji Variable).
fn insn_regs(insn: &Instruction) -> Vec<u32> {
    match insn {
        Instruction::LoadNum  { dst, .. } => vec![*dst],
        Instruction::LoadBool { dst, .. } => vec![*dst],
        Instruction::LoadNil  { dst }     => vec![*dst],
        Instruction::GetVar    { dst, .. } => vec![*dst],
        Instruction::GetVarDyn { dst, name } => vec![*dst, *name],
        Instruction::SetVar    { src, .. } => vec![*src],
        Instruction::Add { dst, a, b } |
        Instruction::Sub { dst, a, b } |
        Instruction::Mul { dst, a, b } |
        Instruction::Div { dst, a, b } |
        Instruction::Mod { dst, a, b } |
        Instruction::CmpEq { dst, a, b } |
        Instruction::CmpNe { dst, a, b } |
        Instruction::CmpLt { dst, a, b } |
        Instruction::CmpLe { dst, a, b } |
        Instruction::CmpGt { dst, a, b } |
        Instruction::CmpGe { dst, a, b } => vec![*dst, *a, *b],
        Instruction::Neg { dst, src } |
        Instruction::ToString { dst, src } |
        Instruction::ToNumber { dst, src } |
        Instruction::Truthy   { dst, src } => vec![*dst, *src],
        Instruction::JumpIfFalse { cond, .. } |
        Instruction::JumpIfTrue  { cond, .. } => vec![*cond],
        _ => vec![],
    }
}

// ── compile_trace_entry — publiczny entry point dla Trace JIT ─────────────────

pub fn compile_trace_entry(
    module_bc: &HlModule,
    entry: &FuncEntry,
    var_slots: &HashMap<u32, u32>,
) -> anyhow::Result<crate::interpreter::CompiledTrace> {
    if !is_trace_safe(module_bc, entry) {
        anyhow::bail!("trasa niekwalifikująca się do bezpiecznej kompilacji JIT");
    }
    let engine = JitEngine::new();
    engine.jit_compile(&entry.name, module_bc, entry, var_slots)
    .map(|frag| crate::interpreter::CompiledTrace {
        fn_ptr:      frag.fn_ptr,
         exit_offset: entry.start_insn + entry.insn_count,
    })
}
