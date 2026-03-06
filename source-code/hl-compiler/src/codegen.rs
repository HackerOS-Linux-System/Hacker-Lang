use crate::ir::{
    IrArm, IrCmpOp, IrFunction, IrLit, IrModule,
    IrOp, IrOperand, IrType, IrVar,
};
use colored::*;
use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::values::{
    BasicValueEnum, FloatValue, FunctionValue, IntValue, PointerValue,
};
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// ─────────────────────────────────────────────────────────────
// Globalny licznik unikalnych nazw LLVM (thread-safe)
// ─────────────────────────────────────────────────────────────
static GLOBAL_CTR: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
pub(crate) fn uid(prefix: &str) -> String {
    format!("{}_{}", prefix, GLOBAL_CTR.fetch_add(1, Ordering::Relaxed))
}

// ─────────────────────────────────────────────────────────────
// ArenaScope — stan aktywnego :: bloku w bieżącej funkcji.
// ─────────────────────────────────────────────────────────────
pub(crate) struct ArenaScope<'ctx> {
    pub scope_ptr: PointerValue<'ctx>,
}

// ─────────────────────────────────────────────────────────────
// Codegen — główna struktura backendu LLVM
// ─────────────────────────────────────────────────────────────
pub struct Codegen<'ctx> {
    pub(crate) ctx:     &'ctx Context,
    pub        module:  Module<'ctx>,
    pub(crate) builder: Builder<'ctx>,
    pub(crate) verbose: bool,

    pub extern_libs: Vec<(String, bool)>,

    pub(crate) system_fn:     FunctionValue<'ctx>,
    pub(crate) setenv_fn:     FunctionValue<'ctx>,
    pub(crate) snprintf_fn:   FunctionValue<'ctx>,
    pub(crate) fprintf_fn:    FunctionValue<'ctx>,
    pub(crate) stderr_global: inkwell::values::GlobalValue<'ctx>,

    pub(crate) gc_malloc_fn: FunctionValue<'ctx>,
    pub(crate) gc_unmark_fn: FunctionValue<'ctx>,
    pub(crate) gc_sweep_fn:  FunctionValue<'ctx>,
    pub(crate) gc_full_fn:   FunctionValue<'ctx>,

    pub(crate) arena_enter_fn: FunctionValue<'ctx>,
    pub(crate) arena_alloc_fn: FunctionValue<'ctx>,
    pub(crate) arena_reset_fn: FunctionValue<'ctx>,
    pub(crate) arena_exit_fn:  FunctionValue<'ctx>,

    pub(crate) exit_fn: FunctionValue<'ctx>,

    pub(crate) hl_functions: HashMap<String, FunctionValue<'ctx>>,
    pub(crate) string_cache: HashMap<String, PointerValue<'ctx>>,

    pub(crate) nounwind_attr: Attribute,
    pub(crate) noreturn_attr: Attribute,
    pub(crate) cold_attr:     Attribute,
    pub(crate) inline_attr:   Attribute,
    pub(crate) noinline_attr: Attribute,

    pub(crate) slots: HashMap<String, (PointerValue<'ctx>, IrType)>,
    pub(crate) tmps:  HashMap<String, BasicValueEnum<'ctx>>,

    pub(crate) arena_scope: Option<ArenaScope<'ctx>>,
    pub(crate) defers:      Vec<String>,
}

impl<'ctx> Codegen<'ctx> {
    // ─────────────────────────────────────────────────────────
    // Konstruktor
    // ─────────────────────────────────────────────────────────
    pub fn new(ctx: &'ctx Context, verbose: bool) -> Self {
        let module  = ctx.create_module("hacker_module");
        let builder = ctx.create_builder();

        let i32_t  = ctx.i32_type();
        let i64_t  = ctx.i64_type();
        let void_t = ctx.void_type();
        let ptr_t  = ctx.ptr_type(AddressSpace::default());

        let system_fn = module.add_function(
            "system",
            i32_t.fn_type(&[ptr_t.into()], false),
                                            Some(Linkage::External),
        );
        let setenv_fn = module.add_function(
            "setenv",
            i32_t.fn_type(&[ptr_t.into(), ptr_t.into(), i32_t.into()], false),
                                            Some(Linkage::External),
        );
        let snprintf_fn = module.add_function(
            "snprintf",
            i32_t.fn_type(&[ptr_t.into(), i64_t.into(), ptr_t.into()], true),
                                              Some(Linkage::External),
        );
        let fprintf_fn = module.add_function(
            "fprintf",
            i32_t.fn_type(&[ptr_t.into(), ptr_t.into()], true),
                                             Some(Linkage::External),
        );

        let stderr_global = module.add_global(ptr_t, None, "stderr");
        stderr_global.set_linkage(Linkage::External);

        let gc_malloc_fn = module.add_function(
            "gc_malloc",
            ptr_t.fn_type(&[i64_t.into()], false),
                                               Some(Linkage::External),
        );
        let gc_unmark_fn = module.add_function(
            "gc_unmark_all",
            void_t.fn_type(&[], false),
                                               Some(Linkage::External),
        );
        let gc_sweep_fn = module.add_function(
            "gc_sweep",
            void_t.fn_type(&[], false),
                                              Some(Linkage::External),
        );
        let gc_full_fn = module.add_function(
            "gc_collect_full",
            void_t.fn_type(&[], false),
                                             Some(Linkage::External),
        );

        let arena_enter_fn = module.add_function(
            "hl_jit_arena_enter",
            void_t.fn_type(&[ptr_t.into(), ptr_t.into(), i64_t.into()], false),
                                                 Some(Linkage::External),
        );
        let arena_alloc_fn = module.add_function(
            "hl_jit_arena_alloc",
            ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false),
                                                 Some(Linkage::External),
        );
        let arena_reset_fn = module.add_function(
            "hl_jit_arena_reset",
            void_t.fn_type(&[ptr_t.into()], false),
                                                 Some(Linkage::External),
        );
        let arena_exit_fn = module.add_function(
            "hl_jit_arena_exit",
            void_t.fn_type(&[ptr_t.into()], false),
                                                Some(Linkage::External),
        );

        let exit_fn = module.add_function(
            "exit",
            void_t.fn_type(&[i32_t.into()], false),
                                          Some(Linkage::External),
        );

        let nounwind_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("nounwind"), 0);
        let noreturn_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("noreturn"), 0);
        let cold_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("cold"), 0);
        let inline_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("alwaysinline"), 0);
        let noinline_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("noinline"), 0);
        let noalias_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("noalias"), 0);

        system_fn   .add_attribute(AttributeLoc::Function, nounwind_attr);
        setenv_fn   .add_attribute(AttributeLoc::Function, nounwind_attr);
        snprintf_fn .add_attribute(AttributeLoc::Function, nounwind_attr);
        fprintf_fn  .add_attribute(AttributeLoc::Function, nounwind_attr);
        gc_malloc_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        gc_malloc_fn.add_attribute(AttributeLoc::Return,   noalias_attr);
        gc_full_fn  .add_attribute(AttributeLoc::Function, nounwind_attr);
        gc_sweep_fn .add_attribute(AttributeLoc::Function, nounwind_attr);
        gc_unmark_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        arena_enter_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        arena_alloc_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        arena_alloc_fn.add_attribute(AttributeLoc::Return,   noalias_attr);
        arena_reset_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        arena_exit_fn .add_attribute(AttributeLoc::Function, nounwind_attr);
        exit_fn.add_attribute(AttributeLoc::Function, noreturn_attr);
        exit_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        exit_fn.add_attribute(AttributeLoc::Function, cold_attr);

        Codegen {
            ctx, module, builder, verbose,
            extern_libs: Vec::new(),
            system_fn, setenv_fn, snprintf_fn, fprintf_fn, stderr_global,
            gc_malloc_fn, gc_unmark_fn, gc_sweep_fn, gc_full_fn,
            arena_enter_fn, arena_alloc_fn, arena_reset_fn, arena_exit_fn,
            exit_fn,
            hl_functions:  HashMap::new(),
            string_cache:  HashMap::new(),
            nounwind_attr, noreturn_attr, cold_attr, inline_attr, noinline_attr,
            slots:       HashMap::new(),
            tmps:        HashMap::new(),
            arena_scope: None,
            defers:      Vec::new(),
        }
    }

    // ─────────────────────────────────────────────────────────
    // emit_module — publiczny entry point
    // ─────────────────────────────────────────────────────────
    pub fn emit_module(&mut self, ir: &IrModule) {
        self.extern_libs = ir.extern_libs.clone();
        self.predeclare_ir_functions(ir);
        self.emit_ir_functions(ir);
        self.emit_ir_main(ir);
    }

    // ─────────────────────────────────────────────────────────
    // predeclare_ir_functions
    // ─────────────────────────────────────────────────────────
    fn predeclare_ir_functions(&mut self, ir: &IrModule) {
        let fn_t = self.ctx.i32_type().fn_type(&[], false);

        let mut fns: Vec<&IrFunction> = ir.functions.iter().collect();
        fns.sort_by_key(|f| &f.name);

        for f in fns {
            let llvm_name = mangle_fn_name(&f.name);
            let func = self.module.add_function(&llvm_name, fn_t, None);
            func.add_attribute(AttributeLoc::Function, self.nounwind_attr);

            let n = f.ops.len();
            if n <= 5 {
                func.add_attribute(AttributeLoc::Function, self.inline_attr);
            } else if n > 50 {
                func.add_attribute(AttributeLoc::Function, self.noinline_attr);
            }

            self.hl_functions.insert(f.name.clone(), func);

            if self.verbose {
                let hint = if n <= 5 { "alwaysinline" }
                else if n > 50 { "noinline" }
                else { "default" };
                eprintln!("{} predeclare  {}  →  {}()  [ops={}, {}]",
                          "[f]".blue(), f.name, llvm_name, n, hint);
            }
        }
    }

    // ─────────────────────────────────────────────────────────
    // emit_ir_functions
    // ─────────────────────────────────────────────────────────
    fn emit_ir_functions(&mut self, ir: &IrModule) {
        let mut fns: Vec<&IrFunction> = ir.functions.iter().collect();
        fns.sort_by_key(|f| &f.name);

        for f in fns {
            if self.verbose {
                eprintln!("{} kompilacja  {} [arena={}, unsafe={}, sig={:?}]",
                          "[f]".green(), f.name, f.is_arena, f.is_unsafe,
                          f.type_sig.as_deref().unwrap_or("-"));
            }

            let func  = self.hl_functions[&f.name];
            let entry = self.ctx.append_basic_block(func, "entry");
            self.builder.position_at_end(entry);

            self.slots.clear();
            self.tmps.clear();
            self.arena_scope = None;
            self.defers.clear();

            let ops = f.ops.clone();
            if !self.emit_ops(&ops, func) {
                self.flush_defers();
                let zero = self.ctx.i32_type().const_int(0, false);
                self.builder.build_return(Some(&zero)).unwrap();
            }
        }
    }

    // ─────────────────────────────────────────────────────────
    // emit_ir_main
    // ─────────────────────────────────────────────────────────
    fn emit_ir_main(&mut self, ir: &IrModule) {
        let i32_t   = self.ctx.i32_type();
        let main_fn = self.module.add_function(
            "main", i32_t.fn_type(&[], false), None);
        main_fn.add_attribute(AttributeLoc::Function, self.nounwind_attr);

        let entry = self.ctx.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);
        self.slots.clear();
        self.tmps.clear();
        self.arena_scope = None;
        self.defers.clear();

        let ops = ir.main.clone();
        if !self.emit_ops(&ops, main_fn) {
            self.flush_defers();
            self.builder.build_call(self.gc_full_fn, &[], "gc_final").unwrap();
            self.builder.build_return(Some(&i32_t.const_int(0, false))).unwrap();
        }
    }

    // ─────────────────────────────────────────────────────────
    // emit_ops — iteruje slice IrOp, zatrzymuje się na terminatorze.
    // Metoda emit_one pochodzi z codegen_emit.rs (osobny impl blok).
    // ─────────────────────────────────────────────────────────
    pub(crate) fn emit_ops(&mut self, ops: &[IrOp], func: FunctionValue<'ctx>) -> bool {
        for op in ops {
            if self.emit_one(op, func) {
                return true;
            }
        }
        false
    }

    // ─────────────────────────────────────────────────────────
    // flush_defers — LIFO przed każdym return/exit
    // ─────────────────────────────────────────────────────────
    pub(crate) fn flush_defers(&mut self) {
        if self.defers.is_empty() { return; }
        for expr in self.defers.iter().rev().cloned().collect::<Vec<_>>() {
            self.emit_system(&expr);
        }
        self.defers.clear();
    }

    // ─────────────────────────────────────────────────────────
    // emit_system — system(cmd) przez LLVM
    // ─────────────────────────────────────────────────────────
    pub(crate) fn emit_system(&mut self, cmd: &str) {
        if self.verbose {
            let preview: String = cmd.chars().take(100).collect();
            eprintln!("    {} {}", "→".dimmed(), preview.dimmed());
        }
        let ptr = self.str_ptr(cmd, "cmd");
        self.builder.build_call(self.system_fn, &[ptr.into()], &uid("sys")).unwrap();
    }

    // ─────────────────────────────────────────────────────────
    // resolve_hl — szuka funkcji HL po nazwie lub suffixie
    // ─────────────────────────────────────────────────────────
    pub(crate) fn resolve_hl(&self, name: &str) -> Option<FunctionValue<'ctx>> {
        let clean = name.trim_start_matches('.');
        if let Some(&f) = self.hl_functions.get(clean) {
            return Some(f);
        }
        self.hl_functions.iter()
        .find(|(k, _)| {
            k.ends_with(&format!(".{}", clean)) || k.as_str() == clean
        })
        .map(|(_, v)| *v)
    }

    // ─────────────────────────────────────────────────────────
    // str_ptr — globalny stały string, z deduplicacją
    // ─────────────────────────────────────────────────────────
    pub(crate) fn str_ptr(&mut self, s: &str, prefix: &str) -> PointerValue<'ctx> {
        if let Some(&cached) = self.string_cache.get(s) {
            return cached;
        }
        let name   = uid(prefix);
        let cs     = self.ctx.const_string(s.as_bytes(), true);
        let arr_t  = cs.get_type();
        let global = self.module.add_global(arr_t, None, &name);
        global.set_initializer(&cs);
        global.set_linkage(Linkage::Internal);
        global.set_constant(true);
        global.set_unnamed_addr(true);
        let z0 = self.ctx.i64_type().const_int(0, false);
        let gep = unsafe {
            self.builder.build_gep(
                arr_t, global.as_pointer_value(), &[z0, z0], &uid("gep"),
            ).unwrap()
        };
        self.string_cache.insert(s.to_string(), gep);
        gep
    }

    // ─────────────────────────────────────────────────────────
    // buf_ptr — i8* do AllocaStrBuf (dla snprintf)
    // ─────────────────────────────────────────────────────────
    pub(crate) fn buf_ptr(&mut self, var: &IrVar) -> PointerValue<'ctx> {
        let arr_t = self.ctx.i8_type().array_type(32);
        let z0    = self.ctx.i32_type().const_int(0, false);

        let slot = if let Some((s, _)) = self.slots.get(&var.0).copied() {
            s
        } else {
            let s = self.builder.build_alloca(arr_t, &uid("dynbuf")).unwrap();
            self.slots.insert(var.0.clone(), (s, IrType::Ptr));
            s
        };

        unsafe {
            self.builder.build_gep(arr_t, slot, &[z0, z0], &uid("bgep")).unwrap()
        }
    }

    // ─────────────────────────────────────────────────────────
    // operand_i64
    // ─────────────────────────────────────────────────────────
    pub(crate) fn operand_i64(&mut self, op: &IrOperand) -> IntValue<'ctx> {
        match op {
            IrOperand::Lit(IrLit::I64(n))  => self.ctx.i64_type().const_int(*n as u64, true),
            IrOperand::Lit(IrLit::Bool(b)) => self.ctx.i64_type().const_int(*b as u64, false),
            IrOperand::Lit(_)              => self.ctx.i64_type().const_zero(),
            IrOperand::Var(v) => {
                if let Some(bv) = self.tmps.get(&v.0).copied() {
                    if let BasicValueEnum::IntValue(iv) = bv { return iv; }
                    if let BasicValueEnum::FloatValue(fv) = bv {
                        return self.builder
                        .build_float_to_signed_int(fv, self.ctx.i64_type(), &uid("f2i"))
                        .unwrap();
                    }
                }
                if let Some((slot, _)) = self.slots.get(&v.0).copied() {
                    return self.builder
                    .build_load(self.ctx.i64_type(), slot, &uid("ldi"))
                    .unwrap()
                    .into_int_value();
                }
                self.ctx.i64_type().const_zero()
            }
        }
    }

    // ─────────────────────────────────────────────────────────
    // operand_f64
    // ─────────────────────────────────────────────────────────
    pub(crate) fn operand_f64(&mut self, op: &IrOperand) -> FloatValue<'ctx> {
        match op {
            IrOperand::Lit(IrLit::F64(f)) => self.ctx.f64_type().const_float(*f),
            IrOperand::Lit(IrLit::I64(n)) => self.ctx.f64_type().const_float(*n as f64),
            IrOperand::Lit(_)             => self.ctx.f64_type().const_float(0.0),
            IrOperand::Var(v) => {
                if let Some(bv) = self.tmps.get(&v.0).copied() {
                    if let BasicValueEnum::FloatValue(fv) = bv { return fv; }
                    if let BasicValueEnum::IntValue(iv) = bv {
                        return self.builder
                        .build_signed_int_to_float(iv, self.ctx.f64_type(), &uid("i2f"))
                        .unwrap();
                    }
                }
                if let Some((slot, _)) = self.slots.get(&v.0).copied() {
                    return self.builder
                    .build_load(self.ctx.f64_type(), slot, &uid("ldf"))
                    .unwrap()
                    .into_float_value();
                }
                self.ctx.f64_type().const_float(0.0)
            }
        }
    }

    // ─────────────────────────────────────────────────────────
    // operand_bool
    // ─────────────────────────────────────────────────────────
    pub(crate) fn operand_bool(&mut self, op: &IrOperand) -> IntValue<'ctx> {
        match op {
            IrOperand::Lit(IrLit::Bool(b)) =>
            self.ctx.bool_type().const_int(*b as u64, false),
            IrOperand::Lit(IrLit::I64(n)) =>
            self.ctx.bool_type().const_int(if *n != 0 { 1 } else { 0 }, false),
            IrOperand::Lit(_) =>
            self.ctx.bool_type().const_zero(),
            IrOperand::Var(v) => {
                if let Some(bv) = self.tmps.get(&v.0).copied() {
                    if let BasicValueEnum::IntValue(iv) = bv { return iv; }
                }
                self.ctx.bool_type().const_zero()
            }
        }
    }

    // ─────────────────────────────────────────────────────────
    // ops_to_shell — serializuje Vec<IrOp> do stringa shell
    // ─────────────────────────────────────────────────────────
    pub(crate) fn ops_to_shell(&mut self, ops: &[IrOp]) -> String {
        let mut parts: Vec<String> = Vec::new();

        for op in ops {
            match op {
                IrOp::SysCall { cmd, sudo } =>
                parts.push(if *sudo { wrap_sudo(cmd) } else { cmd.clone() }),

                IrOp::CallHL { name, args } => {
                    let a = args.as_deref()
                    .filter(|a| !a.is_empty())
                    .map(|a| format!(" {}", a))
                    .unwrap_or_default();
                    parts.push(format!(".{}{}", name, a));
                }
                IrOp::CallModule { module, method, args } => {
                    let a = args.as_deref()
                    .filter(|a| !a.is_empty())
                    .map(|a| format!(" {}", a))
                    .unwrap_or_default();
                    parts.push(format!("{}.{}{}", module, method, a));
                }
                IrOp::CallExt { cmd, sudo } =>
                parts.push(if *sudo { wrap_sudo(cmd) } else { cmd.clone() }),

                IrOp::SetEnv { key, val } | IrOp::SetLocal { key, val } =>
                parts.push(format!("export {}={}", key, val)),

                IrOp::SetEnvDyn { key, expr } | IrOp::SetLocalDyn { key, expr } =>
                parts.push(format!("export {}={}", key, expr)),

                IrOp::I64ToEnv { key, src, .. } =>
                parts.push(format!("export {}={}", key, operand_to_shell(src))),

                IrOp::F64ToEnv { key, src, .. } =>
                parts.push(format!("export {}={}", key, operand_to_shell(src))),

                IrOp::Log { msg, to_stderr } => {
                    let redirect = if *to_stderr { " >&2" } else { "" };
                    parts.push(format!("echo {}{}", shell_quote(msg), redirect));
                }
                IrOp::Assert { cond, msg } =>
                parts.push(format!(
                    "if ! ( {} ); then echo 'assert: {}' >&2; exit 1; fi",
                                   cond, msg
                )),
                IrOp::TryCatch { try_cmd, catch_cmd } =>
                parts.push(format!("( {} ) || ( {} )", try_cmd, catch_cmd)),

                IrOp::Spawn { cmd, sudo } => {
                    let s = format!("{} &", cmd);
                    parts.push(if *sudo { wrap_sudo(&s) } else { s });
                }
                IrOp::Await { expr } =>
                parts.push(format!("wait {}", expr)),

                IrOp::ResultUnwrap { expr, msg } =>
                parts.push(format!(
                    "{}; if [ $? -ne 0 ]; then echo 'error: {}' >&2; exit 1; fi",
                    expr, msg
                )),

                _ => {}
            }
        }

        if parts.is_empty() { "true".to_string() } else { parts.join("; ") }
    }
}

// ─────────────────────────────────────────────────────────────
// Funkcje pomocnicze (pub(crate) — dostępne z codegen_emit.rs)
// ─────────────────────────────────────────────────────────────

pub(crate) fn mangle_fn_name(name: &str) -> String {
    format!("hl_{}", name
        .replace('.', "_")
        .replace('-', "_")
        .replace(' ', "_"))
}

pub(crate) fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:/".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub(crate) fn operand_to_shell(op: &IrOperand) -> String {
    match op {
        IrOperand::Lit(IrLit::I64(n))  => n.to_string(),
        IrOperand::Lit(IrLit::F64(f))  => f.to_string(),
        IrOperand::Lit(IrLit::Bool(b)) => if *b { "1".into() } else { "0".into() },
        IrOperand::Lit(IrLit::Str(s))  => format!("\"{}\"", s),
        IrOperand::Var(v)              => format!("${}", v.0.trim_start_matches('%')),
    }
}

pub(crate) fn wrap_sudo(cmd: &str) -> String {
    format!("sudo sh -c '{}'", cmd.replace('\'', "'\\''"))
}

pub(crate) fn build_case(cond: &str, arms: &[IrArm]) -> String {
    let mut sh = format!("case {} in\n", cond);
    for arm in arms {
        match &arm.val {
            Some(v) => {
                let clean = v.trim_matches('"').trim_matches('\'');
                sh += &format!("  {}) {};;\n", clean, arm.cmd);
            }
            None => sh += &format!("  *) {};;\n", arm.cmd),
        }
    }
    sh += "esac";
    sh
}

pub(crate) fn icmp_pred(op: IrCmpOp) -> IntPredicate {
    match op {
        IrCmpOp::Eq => IntPredicate::EQ,
        IrCmpOp::Ne => IntPredicate::NE,
        IrCmpOp::Lt => IntPredicate::SLT,
        IrCmpOp::Le => IntPredicate::SLE,
        IrCmpOp::Gt => IntPredicate::SGT,
        IrCmpOp::Ge => IntPredicate::SGE,
    }
}

pub(crate) fn fcmp_pred(op: IrCmpOp) -> FloatPredicate {
    match op {
        IrCmpOp::Eq => FloatPredicate::OEQ,
        IrCmpOp::Ne => FloatPredicate::ONE,
        IrCmpOp::Lt => FloatPredicate::OLT,
        IrCmpOp::Le => FloatPredicate::OLE,
        IrCmpOp::Gt => FloatPredicate::OGT,
        IrCmpOp::Ge => FloatPredicate::OGE,
    }
}
