#![allow(dead_code)]
use colored::Colorize;
use inkwell::attributes::AttributeLoc;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::types::{BasicMetadataTypeEnum, BasicTypeEnum};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue, ValueKind};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use hl_plsa::{parse_file, AnalysisResult, Expr, ProgramNode, Stmt, Value as HlVal};
// ─────────────────────────────────────────────────────────────
// AST optimiser
// ─────────────────────────────────────────────────────────────
fn fold_node(node: &ProgramNode) -> ProgramNode {
    ProgramNode {
        line_num: node.line_num,
        is_sudo: node.is_sudo,
        content: fold_stmt(&node.content),
        original_text: node.original_text.clone(),
        span: node.span,
    }
}
fn fold_stmt(s: &Stmt) -> Stmt {
    match s {
        Stmt::Function { name, params, ret_ty, body, is_quick } => Stmt::Function {
            name: name.clone(),
            params: params.clone(),
            ret_ty: ret_ty.clone(),
            body: body.iter().map(fold_stmt).collect(),
            is_quick: *is_quick,
        },
        Stmt::If { cond, body, else_ifs, else_body } => Stmt::If {
            cond: cond.clone(),
            body: body.iter().map(fold_stmt).collect(),
            else_ifs: else_ifs.iter().map(|(c, b)| (c.clone(), b.iter().map(fold_stmt).collect())).collect(),
                else_body: else_body.as_ref().map(|b| b.iter().map(fold_stmt).collect()),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: cond.clone(),
            body: body.iter().map(fold_stmt).collect(),
        },
        Stmt::For { var, iter, body } => Stmt::For {
            var: var.clone(),
            iter: iter.clone(),
            body: body.iter().map(fold_stmt).collect(),
        },
        Stmt::ForIndexed { idx, var, iter, body } => Stmt::ForIndexed {
            idx: idx.clone(),
            var: var.clone(),
            iter: iter.clone(),
            body: body.iter().map(fold_stmt).collect(),
        },
        Stmt::Repeat { count, body } => Stmt::Repeat {
            count: *count,
            body: body.iter().map(fold_stmt).collect(),
        },
        Stmt::Try { body, catches, finally, else_body } => Stmt::Try {
            body: body.iter().map(fold_stmt).collect(),
            catches: catches
            .iter()
            .map(|(v, t, b)| (v.clone(), t.clone(), b.iter().map(fold_stmt).collect()))
            .collect(),
            finally: finally.as_ref().map(|b| b.iter().map(fold_stmt).collect()),
            else_body: else_body.as_ref().map(|b| b.iter().map(fold_stmt).collect()),
        },
        Stmt::Match { expr, arms } => Stmt::Match {
            expr: expr.clone(),
            arms: arms
            .iter()
            .map(|(e, b)| (e.clone(), b.iter().map(fold_stmt).collect()))
            .collect(),
        },
        Stmt::Background(nodes) => Stmt::Background(nodes.iter().map(fold_stmt).collect()),
        Stmt::Object { name, fields, methods } => Stmt::Object {
            name: name.clone(),
            fields: fields.clone(),
            methods: methods
            .iter()
            .map(|(k, (p, r, b))| (k.clone(), (p.clone(), r.clone(), b.iter().map(fold_stmt).collect())))
            .collect(),
        },
        other => other.clone(),
    }
}
pub fn optimise(ast: &mut AnalysisResult) {
    ast.main_body = ast.main_body.iter().map(fold_node).collect();
    for (_name, (_params, _ret, body, _is_quick)) in ast.functions.iter_mut() {
        *body = body.iter().map(fold_node).collect();
    }
}
// ─────────────────────────────────────────────────────────────
// String interner
// ─────────────────────────────────────────────────────────────
struct Strings<'ctx> {
    cache: HashMap<String, PointerValue<'ctx>>,
}
impl<'ctx> Strings<'ctx> {
    fn new() -> Self {
        Strings { cache: HashMap::new() }
    }
    fn get(&mut self, s: &str, m: &Module<'ctx>, ctx: &'ctx Context) -> PointerValue<'ctx> {
        if let Some(&p) = self.cache.get(s) {
            return p;
        }
        let cs = ctx.const_string(s.as_bytes(), true);
        let gv = m.add_global(cs.get_type(), Some(AddressSpace::default()), &format!("s{}", self.cache.len()));
        gv.set_initializer(&cs);
        gv.set_constant(true);
        gv.set_unnamed_addr(true);
        gv.set_linkage(Linkage::Private);
        let z = ctx.i32_type().const_zero();
        let ptr = unsafe { gv.as_pointer_value().const_gep(cs.get_type(), &[z, z]) };
        self.cache.insert(s.to_string(), ptr);
        ptr
    }
}
// ─────────────────────────────────────────────────────────────
// Code generator
// ─────────────────────────────────────────────────────────────
struct CG<'ctx> {
    ctx: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    syms: HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>,
    cur_fn: Option<FunctionValue<'ctx>>,
    loops: Vec<(BasicBlock<'ctx>, BasicBlock<'ctx>)>,
    strs: Strings<'ctx>,
}
impl<'ctx> CG<'ctx> {
    fn new(ctx: &'ctx Context, module: Module<'ctx>) -> Self {
        CG {
            ctx,
            module,
            builder: ctx.create_builder(),
            syms: HashMap::new(),
            cur_fn: None,
            loops: Vec::new(),
            strs: Strings::new(),
        }
    }
    fn ty(&self, name: &str) -> BasicTypeEnum<'ctx> {
        match name {
            "i8" | "u8" => self.ctx.i8_type().into(),
            "i16" | "u16" => self.ctx.i16_type().into(),
            "i32" | "u32" => self.ctx.i32_type().into(),
            "i64" | "u64" => self.ctx.i64_type().into(),
            "bool" => self.ctx.bool_type().into(),
            "f32" => self.ctx.f32_type().into(),
            "f64" => self.ctx.f64_type().into(),
            "str" | "string" => self.ctx.ptr_type(AddressSpace::default()).into(),
            _ => self.module.get_struct_type(name)
            .map(Into::into)
            .unwrap_or_else(|| self.ctx.i64_type().into()),
        }
    }
    fn expr(&mut self, e: &Expr) -> BasicValueEnum<'ctx> {
        match e {
            Expr::Lit(HlVal::I32(n)) => self.ctx.i64_type().const_int((*n as i64) as u64, false).into(),
            Expr::Lit(HlVal::F64(f)) => self.ctx.f64_type().const_float(*f).into(),
            Expr::Lit(HlVal::Bool(b)) => self.ctx.bool_type().const_int(*b as u64, false).into(),
            Expr::Lit(HlVal::Str(s)) => self.strs.get(s, &self.module, self.ctx).into(),
            Expr::Var(name) => {
                if let Some((ptr, ty)) = self.syms.get(name).cloned() {
                    self.builder.build_load(ty, ptr, name).unwrap_or_else(|_| self.ctx.i64_type().const_zero().into())
                } else {
                    self.ctx.i64_type().const_zero().into()
                }
            }
            Expr::BinOp { op, left, right } => {
                let l = self.expr(left);
                let r = self.expr(right);
                let li = l.into_int_value();
                let ri = r.into_int_value();
                match op.as_str() {
                    "+" => self.builder.build_int_add(li, ri, "add").unwrap().into(),
                    "-" => self.builder.build_int_sub(li, ri, "sub").unwrap().into(),
                    "*" => self.builder.build_int_mul(li, ri, "mul").unwrap().into(),
                    "/" => self.builder.build_int_signed_div(li, ri, "div").unwrap().into(),
                    "==" => self.builder.build_int_compare(IntPredicate::EQ, li, ri, "eq").unwrap().into(),
                    "!=" => self.builder.build_int_compare(IntPredicate::NE, li, ri, "ne").unwrap().into(),
                    "<" => self.builder.build_int_compare(IntPredicate::SLT, li, ri, "lt").unwrap().into(),
                    ">" => self.builder.build_int_compare(IntPredicate::SGT, li, ri, "gt").unwrap().into(),
                    "<=" => self.builder.build_int_compare(IntPredicate::SLE, li, ri, "le").unwrap().into(),
                    ">=" => self.builder.build_int_compare(IntPredicate::SGE, li, ri, "ge").unwrap().into(),
                    _ => self.ctx.i64_type().const_zero().into(),
                }
            }
            Expr::Call { name, args } => {
                let args_vals: Vec<BasicValueEnum<'ctx>> = args.iter().map(|a| self.expr(a)).collect();
                if let Some(func) = self.module.get_function(name) {
                    let args_meta: Vec<BasicMetadataValueEnum<'ctx>> = args_vals.iter().map(|&v| v.into()).collect();
                    let call = self.builder.build_call(
                        func,
                        &args_meta,
                        "call",
                    ).unwrap();
                    match call.try_as_basic_value() {
                        ValueKind::Basic(val) => val,
                        _ => self.ctx.i64_type().const_zero().into()
                    }
                } else {
                    self.ctx.i64_type().const_zero().into()
                }
            }
            _ => self.ctx.i64_type().const_zero().into(),
        }
    }
    fn nodes(&mut self, nodes: &[ProgramNode]) {
        for node in nodes {
            self.node(node);
        }
    }
    fn node(&mut self, node: &ProgramNode) {
        self.stmt(&node.content, node.is_sudo);
    }
    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt, false);
        }
    }
    fn stmt(&mut self, stmt: &Stmt, is_sudo: bool) {
        let Some(fn_val) = self.cur_fn else { return; };
        match stmt {
            Stmt::Raw { mode, cmd } => {
                let cmd_str = if is_sudo {
                    if cmd.starts_with("sudo ") { cmd.clone() } else { format!("sudo {}", cmd) }
                } else {
                    cmd.clone()
                };
                if mode == ">" && cmd_str.starts_with("echo ") {
                    let msg = cmd_str[5..].trim();
                    let fmt = self.strs.get("%s\n", &self.module, self.ctx);
                    let text = self.strs.get(msg, &self.module, self.ctx);
                    let pf = self.printf();
                    self.builder.build_call(pf, &[fmt.into(), text.into()], "echo").unwrap();
                } else {
                    let ptr = self.strs.get(&cmd_str, &self.module, self.ctx);
                    let sys = self.system();
                    self.builder.build_call(sys, &[ptr.into()], "system").unwrap();
                }
            }
            Stmt::AssignLocal { key, ty, val } | Stmt::AssignGlobal { key, ty, val } => {
                let tn = ty.as_deref().unwrap_or("i64");
                let ty_e = self.ty(tn);
                let alloca = self.builder.build_alloca(ty_e, key).unwrap();
                let value = self.expr(val);
                self.builder.build_store(alloca, value).unwrap();
                self.syms.insert(key.clone(), (alloca, ty_e));
                if tn.contains("str") || tn == "string" {
                    self.gc_root(alloca);
                }
            }
            Stmt::Return { expr } => {
                let v = self.expr(expr);
                self.builder.build_return(Some(&v)).unwrap();
            }
            Stmt::If { cond, body, else_ifs: _, else_body: _ } => {
                let then_bb = self.ctx.append_basic_block(fn_val, "then");
                let merge_bb = self.ctx.append_basic_block(fn_val, "ifcont");
                let cond_val = self.expr(cond).into_int_value();
                self.builder.build_conditional_branch(cond_val, then_bb, merge_bb).unwrap();
                self.builder.position_at_end(then_bb);
                self.stmts(body);
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }
                self.builder.position_at_end(merge_bb);
            }
            Stmt::Function { name, params, ret_ty, body, is_quick: _ } => {
                let temp_body: Vec<ProgramNode> = body.iter().map(|s| ProgramNode {
                    line_num: 0,
                    is_sudo: false,
                    content: s.clone(),
                                                                  original_text: String::new(),
                                                                  span: (0, 0),
                }).collect();
                self.emit_fn(name, params, ret_ty.as_deref(), &temp_body);
            }
            Stmt::Repeat { count, body } => {
                let desugar = Stmt::For {
                    var: "i".to_string(),
                    iter: Expr::Lit(HlVal::Str(format!("0..{}", count))),
                    body: body.clone(),
                };
                self.stmt(&desugar, is_sudo);
            }
            _ => { /* pomijamy na razie inne konstrukcje */ }
        }
    }
    fn emit_fn(
        &mut self,
        name: &str,
        params: &[(String, String)],
               ret_ty_str: Option<&str>,
               body: &[ProgramNode],
    ) {
        let ret_ty = ret_ty_str.map(|s| self.ty(s));
        let param_types: Vec<BasicMetadataTypeEnum<'ctx>> = params
        .iter()
        .map(|(_, t)| self.ty(t).into())
        .collect();
        let fn_type = match ret_ty {
            Some(BasicTypeEnum::IntType(it)) => it.fn_type(&param_types, false),
            Some(BasicTypeEnum::FloatType(ft)) => ft.fn_type(&param_types, false),
            Some(BasicTypeEnum::PointerType(pt)) => pt.fn_type(&param_types, false),
            _ => self.ctx.void_type().fn_type(&param_types, false),
        };
        let func = self.module.add_function(name, fn_type, None);
        if body.len() < 12 {
            let id = inkwell::attributes::Attribute::get_named_enum_kind_id("alwaysinline");
            let attr = self.ctx.create_enum_attribute(id, 0);
            func.add_attribute(AttributeLoc::Function, attr);
        }
        let entry = self.ctx.append_basic_block(func, "entry");
        let prev_fn = self.cur_fn;
        self.cur_fn = Some(func);
        self.builder.position_at_end(entry);
        for (i, (pname, pty_str)) in params.iter().enumerate() {
            let pty = self.ty(pty_str);
            let alloca = self.builder.build_alloca(pty, pname).unwrap();
            let param_val = func.get_nth_param(i as u32).unwrap();
            self.builder.build_store(alloca, param_val).unwrap();
            self.syms.insert(pname.clone(), (alloca, pty));
        }
        self.nodes(body);
        if ret_ty_str.is_none() && self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            self.builder.build_return(None).unwrap();
        }
        self.cur_fn = prev_fn;
    }
    fn system(&self) -> FunctionValue<'ctx> {
        self.module.get_function("system").unwrap_or_else(|| {
            let ptr_t = self.ctx.ptr_type(AddressSpace::default());
            self.module.add_function(
                "system",
                self.ctx.i32_type().fn_type(&[ptr_t.into()], false),
                                     Some(Linkage::External),
            )
        })
    }
    fn printf(&self) -> FunctionValue<'ctx> {
        self.module.get_function("printf").unwrap_or_else(|| {
            let ptr_t = self.ctx.ptr_type(AddressSpace::default());
            self.module.add_function(
                "printf",
                self.ctx.i32_type().fn_type(&[ptr_t.into()], true),
                                     Some(Linkage::External),
            )
        })
    }
    fn gc_root(&self, ptr: PointerValue<'ctx>) {
        let null = self.ctx.ptr_type(AddressSpace::default()).const_null();
        let gcroot = self.module.get_function("llvm.gcroot").unwrap_or_else(|| {
            let ptr_t = self.ctx.ptr_type(AddressSpace::default());
            let void_t = self.ctx.void_type();
            self.module.add_function(
                "llvm.gcroot",
                void_t.fn_type(&[ptr_t.into(), ptr_t.into()], false),
                                     Some(Linkage::External),
            )
        });
        self.builder.build_call(gcroot, &[ptr.into(), null.into()], "gcroot").unwrap();
    }
}
// ─────────────────────────────────────────────────────────────
// Główna funkcja kompilacji
// ─────────────────────────────────────────────────────────────
pub fn compile_command(file: String, output: String, verbose: bool) -> bool {
    let mut seen_libs = HashSet::new();
    let parse_result = parse_file(&file, true, verbose, &mut seen_libs);
    let mut ast = match parse_result {
        Ok(ast) => ast,
        Err(errors) => {
            for error in errors {
                eprintln!("{}", error);
            }
            return false;
        }
    };
    optimise(&mut ast);
    if verbose {
        println!("{} Generating LLVM IR...", "[]".green());
    }
    let ctx = Context::create();
    let module = ctx.create_module("hackerlang");
    let mut cg = CG::new(&ctx, module);
    let i32_ty = ctx.i32_type();
    let main_fn = cg.module.add_function("main", i32_ty.fn_type(&[], false), None);
    let entry = ctx.append_basic_block(main_fn, "entry");
    cg.builder.position_at_end(entry);
    cg.cur_fn = Some(main_fn);
    cg.nodes(&ast.main_body);
    if cg.builder.get_insert_block().unwrap().get_terminator().is_none() {
        cg.builder.build_return(Some(&i32_ty.const_zero())).unwrap();
    }
    cg.cur_fn = None;
    for (name, (params, ret_ty, body, _)) in &ast.functions {
        cg.emit_fn(name, params, ret_ty.as_deref(), body);
    }
    // ─────────────────────────────────────────────────────────────
    // NEW PASS MANAGER (LLVM 14+)
    // ─────────────────────────────────────────────────────────────
    Target::initialize_native(&InitializationConfig::default()).unwrap();
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).unwrap();
    let tm = target.create_target_machine(
        &triple,
        "generic",
        "",
        OptimizationLevel::Aggressive,
        RelocMode::PIC,
        CodeModel::Default,
    ).unwrap();
    if verbose {
        println!("{} Optimizing with New Pass Manager...", "[]".green());
    }
    let pb_options = PassBuilderOptions::create();
    pb_options.set_verify_each(true);
    pb_options.set_debug_logging(verbose);
    if let Err(e) = cg.module.run_passes("default<O3>", &tm, pb_options) {
        eprintln!("Optimization failed: {:?}", e);
        return false;
    }
    let obj_path = format!("{}.o", output);
    if let Err(e) = tm.write_to_file(&cg.module, FileType::Object, Path::new(&obj_path)) {
        eprintln!("Failed to write object file: {}", e);
        return false;
    }
    if verbose {
        println!("{} Linking...", "[]".green());
    }
    let mut linker = Command::new("clang");
    linker
    .arg(&obj_path)
    .arg("-o")
    .arg(&output)
    .arg("-fuse-ld=lld")
    .arg("-O3")
    .arg("-march=native")
    .arg("-Wl,--gc-sections")
    .arg("-Wl,--strip-all")
    .arg("-lhl_runtime");
    if let Ok(status) = linker.status() {
        if status.success() {
            let _ = std::fs::remove_file(&obj_path);
            if verbose {
                println!("{} {}", "[OK]".green(), output);
            }
            true
        } else {
            eprintln!("{} Linking failed", "[ERROR]".red());
            false
        }
    } else {
        eprintln!("Cannot run clang");
        false
    }
}
