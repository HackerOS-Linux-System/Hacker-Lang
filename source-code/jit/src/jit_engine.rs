
use anyhow::{bail, Result};
use cranelift_codegen::ir::{
    types, AbiParam, Function, InstBuilder, Signature, UserFuncName,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::{settings, Context};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use hl_compiler::bytecode::*;
use std::collections::HashMap;

/// Próg wywołań funkcji/pętli przed JIT kompilacją
const JIT_THRESHOLD: u32 = 50;

/// Skompilowany JIT fragment — wskaźnik do kodu maszynowego
pub struct JitFragment {
    /// fn(regs: *mut u64, vars: *mut u64, reg_count: u32, var_count: u32) -> i32
    pub fn_ptr: unsafe extern "C" fn(*mut u64, *mut u64, u32, u32) -> i32,
}

/// Menedżer JIT — śledzi wywołania i kompiluje hotspoty
pub struct JitEngine {
    /// Liczniki wywołań per funkcja (nazwa → liczba)
    call_counts: HashMap<String, u32>,
    /// Skompilowane fragmenty (nazwa → JitFragment)
    compiled:    HashMap<String, JitFragment>,
    /// Flaga: czy JIT jest aktywny (może być wyłączony dla debugowania)
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

    /// Zgłoś wywołanie funkcji. Jeśli przekroczono próg → skompiluj.
    pub fn record_call(&mut self, name: &str, module: &HlModule) -> bool {
        if !self.enabled { return false; }
        if self.compiled.contains_key(name) { return true; } // Już skompilowana

        let count = self.call_counts.entry(name.to_string()).or_insert(0);
        *count += 1;

        if *count >= JIT_THRESHOLD {
            // Sprawdź czy funkcja nadaje się do JIT (tylko czysta arytmetyka)
            if let Some(entry) = module.funcs.find(name) {
                if is_jit_eligible(module, entry) {
                    match self.jit_compile(name, module, entry) {
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

    /// Sprawdź czy funkcja jest już skompilowana
    pub fn is_compiled(&self, name: &str) -> bool {
        self.compiled.contains_key(name)
    }

    /// Wykonaj skompilowaną funkcję
    /// regs: tablica wartości rejestrów jako f64
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

    /// Kompilacja Cranelift → kod maszynowy
    fn jit_compile(
        &self,
        name: &str,
        module_bc: &HlModule,
        entry: &FuncEntry,
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

        // Sygnatura: fn(regs: *mut u64, vars: *mut u64, reg_count: u32, var_count: u32) -> i32
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
            builder.seal_block(entry_block);

            let regs_ptr = builder.block_params(entry_block)[0];

            // Kompiluj instrukcje funkcji
            compile_func_body(
                &mut builder,
                module_bc,
                entry,
                regs_ptr,
            )?;

            // Zwróć 0 (exit_code OK)
            let zero = builder.ins().iconst(types::I32, 0);
            builder.ins().return_(&[zero]);
            builder.finalize();
        }

        jit_module.define_function(func_id, &mut ctx)?;
        jit_module.finalize_definitions()?;

        let fn_ptr = jit_module.get_finalized_function(func_id);
        // Dopasuj sygnaturę do CompiledTrace::fn_ptr
        let fn_typed: unsafe extern "C" fn(*mut u64, *mut u64, u32, u32) -> i32 =
        unsafe { std::mem::transmute(fn_ptr) };

        std::mem::forget(jit_module);

        Ok(JitFragment { fn_ptr: fn_typed })
    }
}

impl Default for JitEngine {
    fn default() -> Self { Self::new() }
}

/// Sprawdź czy blok funkcji kwalifikuje się do JIT
/// Kwalifikuje się: czysta arytmetyka + ładowanie stałych + SetVar/GetVar
fn is_jit_eligible(module: &HlModule, entry: &FuncEntry) -> bool {
    let start = entry.start_insn as usize;
    let end   = start + entry.insn_count as usize;

    for insn in module.instructions.get(start..end).unwrap_or(&[]) {
        match insn {
            // Dozwolone w JIT
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

            // Niedozwolone — fallback do interpretera
            _ => return false,
        }
    }
    true
}

/// Kompiluj ciało funkcji do IR Cranelift
fn compile_func_body(
    builder: &mut FunctionBuilder,
    module: &HlModule,
    entry: &FuncEntry,
    regs_ptr: cranelift_codegen::ir::Value,
) -> Result<()> {
    let start = entry.start_insn as usize;
    let end   = start + entry.insn_count as usize;
    let insns = match module.instructions.get(start..end) {
        Some(s) => s,
        None    => bail!("Nieprawidłowy zakres instrukcji"),
    };

    // Cranelift 0.132: declare_var(ty) -> Variable  (API changed from 0.113)
    // Pre-pass: znajdź unikalne rejestry, przypisz im Variables przez nowe API
    let mut reg_set: Vec<u32> = Vec::new();
    for insn in insns {
        for reg in insn_regs(insn) {
            if !reg_set.contains(&reg) {
                reg_set.push(reg);
            }
        }
    }
    reg_set.sort();

    // declare_var(ty) -> Variable — nowe API zwraca Variable
    let mut reg_vars: HashMap<u32, Variable> = HashMap::new();
    for &reg in &reg_set {
        let var = builder.declare_var(types::F64);
        reg_vars.insert(reg, var);
    }

    // Zainicjalizuj wszystkie rejestry z tablicy (load z regs_ptr[reg * 8])
    for (&reg_idx, &var) in &reg_vars {
        let offset = (reg_idx as i32) * 8;
        let val = builder.ins().load(
            types::F64,
            cranelift_codegen::ir::MemFlags::new(),
                                     regs_ptr,
                                     offset,
        );
        builder.def_var(var, val);
    }

    // Kompiluj instrukcje
    for insn in insns {
        compile_insn(builder, insn, module, &reg_vars, regs_ptr)?;
    }

    // Zapisz wyniki z powrotem do tablicy rejestrów
    for (&reg_idx, &var) in &reg_vars {
        let val = builder.use_var(var);
        let offset = (reg_idx as i32) * 8;
        builder.ins().store(
            cranelift_codegen::ir::MemFlags::new(),
                            val,
                            regs_ptr,
                            offset,
        );
    }

    Ok(())
}

fn compile_insn(
    builder: &mut FunctionBuilder,
    insn: &Instruction,
    module: &HlModule,
    reg_vars: &HashMap<u32, Variable>,
    _regs_ptr: cranelift_codegen::ir::Value,
) -> Result<()> {
    // Makra inline zamiast closures — Rust nie pozwala na dwa closure borrowujące `builder`
    macro_rules! gv {
        ($r:expr) => {
            if let Some(&v) = reg_vars.get(&$r) {
                builder.use_var(v)
            } else {
                builder.ins().f64const(0.0)
            }
        };
    }
    macro_rules! dv {
        ($r:expr, $val:expr) => {
            if let Some(&v) = reg_vars.get(&$r) {
                builder.def_var(v, $val);
            }
        };
    }

    match insn {
        Instruction::LoadNum { dst, idx } => {
            let n   = module.consts.numbers.get(*idx as usize).copied().unwrap_or(0.0);
            let val = builder.ins().f64const(n);
            dv!(*dst, val);
        }
        Instruction::LoadBool { dst, val } => {
            let n = if *val { 1.0f64 } else { 0.0f64 };
            let v = builder.ins().f64const(n);
            dv!(*dst, v);
        }
        Instruction::LoadNil { dst } => {
            let v = builder.ins().f64const(0.0);
            dv!(*dst, v);
        }
        Instruction::GetVar { dst, .. } => { let _ = dst; }
        Instruction::SetVar { src, .. }    => { let _ = src; }

        Instruction::Add { dst, a, b } => {
            let va = gv!(*a); let vb = gv!(*b);
            let r  = builder.ins().fadd(va, vb);
            dv!(*dst, r);
        }
        Instruction::Sub { dst, a, b } => {
            let va = gv!(*a); let vb = gv!(*b);
            let r  = builder.ins().fsub(va, vb);
            dv!(*dst, r);
        }
        Instruction::Mul { dst, a, b } => {
            let va = gv!(*a); let vb = gv!(*b);
            let r  = builder.ins().fmul(va, vb);
            dv!(*dst, r);
        }
        Instruction::Div { dst, a, b } => {
            let va = gv!(*a); let vb = gv!(*b);
            let r  = builder.ins().fdiv(va, vb);
            dv!(*dst, r);
        }
        Instruction::Neg { dst, src } => {
            let v = gv!(*src);
            let r = builder.ins().fneg(v);
            dv!(*dst, r);
        }
        Instruction::CmpLt { dst, a, b } => {
            let va    = gv!(*a); let vb = gv!(*b);
            let cmp   = builder.ins().fcmp(
                cranelift_codegen::ir::condcodes::FloatCC::LessThan, va, vb
            );
            let one_f  = builder.ins().f64const(1.0);
            let zero_f = builder.ins().f64const(0.0);
            let r      = builder.ins().select(cmp, one_f, zero_f);
            dv!(*dst, r);
        }
        Instruction::ToNumber { dst, src } => {
            let v = gv!(*src);
            dv!(*dst, v);
        }

        Instruction::Nop | Instruction::Return { .. } => {}
        _ => { /* pozostałe instrukcje nie są kompilowane do JIT */ }
    }

    Ok(())
}

/// Zbierz wszystkie rejestry użyte w instrukcji
fn insn_regs(insn: &Instruction) -> Vec<u32> {
    match insn {
        Instruction::LoadNum  { dst, .. } => vec![*dst],
        Instruction::LoadBool { dst, .. } => vec![*dst],
        Instruction::LoadNil  { dst }     => vec![*dst],
        Instruction::GetVar   { dst, .. } => vec![*dst],
        Instruction::SetVar   { src, .. } => vec![*src],
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
//
// Kompiluje blok instrukcji jako pseudo-funkcję przez Cranelift JIT.
// Wywoływany z interpreter.rs::try_compile_trace.

pub fn compile_trace_entry(
    module_bc: &HlModule,
    entry: &FuncEntry,
) -> anyhow::Result<crate::interpreter::CompiledTrace> {
    let engine = JitEngine::new();
    engine.jit_compile(&entry.name, module_bc, entry)
    .map(|frag| crate::interpreter::CompiledTrace {
        fn_ptr:      frag.fn_ptr,
         exit_offset: entry.start_insn + entry.insn_count,
    })
}
