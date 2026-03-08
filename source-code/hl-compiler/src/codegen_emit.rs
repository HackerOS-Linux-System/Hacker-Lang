use crate::codegen::{
    build_case, fcmp_pred, icmp_pred,
    uid, wrap_sudo, Codegen,
};
use crate::ir::{IrBinOp, IrBranch, IrOp, IrPipeStep, IrType, IrVar};
use colored::*;
use inkwell::IntPredicate;
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue, ValueKind};

impl<'ctx> Codegen<'ctx> {
    // ─────────────────────────────────────────────────────────
    // emit_one — główny dispatch po IrOp
    // Zwraca true gdy instrukcja jest terminatorem (Return / Exit)
    // ─────────────────────────────────────────────────────────
    pub(crate) fn emit_one(&mut self, op: &IrOp, func: FunctionValue<'ctx>) -> bool {
        match op {

            // ════════════════════════════════════════════════
            // ALLOCA
            // ════════════════════════════════════════════════

            IrOp::AllocaI64 { var, init } => {
                let i64_t = self.ctx.i64_type();
                let slot  = self.builder.build_alloca(i64_t, &var.0).unwrap();
                if let Some(n) = init {
                    self.builder.build_store(slot, i64_t.const_int(*n as u64, true)).unwrap();
                }
                self.slots.insert(var.0.clone(), (slot, IrType::I64));
            }

            IrOp::AllocaF64 { var, init } => {
                let f64_t = self.ctx.f64_type();
                let slot  = self.builder.build_alloca(f64_t, &var.0).unwrap();
                if let Some(v) = init {
                    self.builder.build_store(slot, f64_t.const_float(*v)).unwrap();
                }
                self.slots.insert(var.0.clone(), (slot, IrType::F64));
            }

            IrOp::AllocaBool { var, init } => {
                let bool_t = self.ctx.bool_type();
                let slot   = self.builder.build_alloca(bool_t, &var.0).unwrap();
                if let Some(b) = init {
                    self.builder.build_store(slot, bool_t.const_int(*b as u64, false)).unwrap();
                }
                self.slots.insert(var.0.clone(), (slot, IrType::Bool));
            }

            IrOp::AllocaStrBuf { var, size } => {
                let arr_t = self.ctx.i8_type().array_type(*size);
                let slot  = self.builder.build_alloca(arr_t, &var.0).unwrap();
                self.slots.insert(var.0.clone(), (slot, IrType::Ptr));
            }

            // ════════════════════════════════════════════════
            // STORE
            // ════════════════════════════════════════════════

            IrOp::StoreI64 { dst, val } => {
                let v = self.operand_i64(val);
                if let Some((slot, _)) = self.slots.get(&dst.0).copied() {
                    self.builder.build_store(slot, v).unwrap();
                }
            }

            IrOp::StoreF64 { dst, val } => {
                let v = self.operand_f64(val);
                if let Some((slot, _)) = self.slots.get(&dst.0).copied() {
                    self.builder.build_store(slot, v).unwrap();
                }
            }

            IrOp::StoreBool { dst, val } => {
                let v = self.operand_bool(val);
                if let Some((slot, _)) = self.slots.get(&dst.0).copied() {
                    self.builder.build_store(slot, v).unwrap();
                }
            }

            // ════════════════════════════════════════════════
            // LOAD
            // ════════════════════════════════════════════════

            IrOp::LoadI64 { dst, src } => {
                if let Some((slot, _)) = self.slots.get(&src.0).copied() {
                    let v = self.builder
                    .build_load(self.ctx.i64_type(), slot, &uid("li")).unwrap();
                    self.tmps.insert(dst.0.clone(), v);
                }
            }

            IrOp::LoadF64 { dst, src } => {
                if let Some((slot, _)) = self.slots.get(&src.0).copied() {
                    let v = self.builder
                    .build_load(self.ctx.f64_type(), slot, &uid("lf")).unwrap();
                    self.tmps.insert(dst.0.clone(), v);
                }
            }

            // ════════════════════════════════════════════════
            // ARYTMETYKA i64
            // ════════════════════════════════════════════════

            IrOp::BinI64 { dst, lhs, op, rhs } => {
                let l = self.operand_i64(lhs);
                let r = self.operand_i64(rhs);
                let v: IntValue = match op {
                    IrBinOp::Add => self.builder.build_int_add(l, r,        &uid("add")).unwrap(),
                    IrBinOp::Sub => self.builder.build_int_sub(l, r,        &uid("sub")).unwrap(),
                    IrBinOp::Mul => self.builder.build_int_mul(l, r,        &uid("mul")).unwrap(),
                    IrBinOp::Div => self.builder.build_int_signed_div(l, r, &uid("div")).unwrap(),
                    IrBinOp::Mod => self.builder.build_int_signed_rem(l, r, &uid("mod")).unwrap(),
                };
                self.tmps.insert(dst.0.clone(), v.into());
            }

            // ════════════════════════════════════════════════
            // ARYTMETYKA f64
            // ════════════════════════════════════════════════

            IrOp::BinF64 { dst, lhs, op, rhs } => {
                let l = self.operand_f64(lhs);
                let r = self.operand_f64(rhs);
                let v = match op {
                    IrBinOp::Add => self.builder.build_float_add(l, r, &uid("fadd")).unwrap(),
                    IrBinOp::Sub => self.builder.build_float_sub(l, r, &uid("fsub")).unwrap(),
                    IrBinOp::Mul => self.builder.build_float_mul(l, r, &uid("fmul")).unwrap(),
                    IrBinOp::Div => self.builder.build_float_div(l, r, &uid("fdiv")).unwrap(),
                    IrBinOp::Mod => self.builder.build_float_rem(l, r, &uid("fmod")).unwrap(),
                };
                self.tmps.insert(dst.0.clone(), v.into());
            }

            // ════════════════════════════════════════════════
            // NEGACJA
            // ════════════════════════════════════════════════

            IrOp::NegI64 { dst, src } => {
                let v = self.operand_i64(src);
                let r = self.builder.build_int_neg(v, &uid("neg")).unwrap();
                self.tmps.insert(dst.0.clone(), r.into());
            }

            IrOp::NegF64 { dst, src } => {
                let v = self.operand_f64(src);
                let r = self.builder.build_float_neg(v, &uid("fneg")).unwrap();
                self.tmps.insert(dst.0.clone(), r.into());
            }

            // ════════════════════════════════════════════════
            // PORÓWNANIA
            // ════════════════════════════════════════════════

            IrOp::CmpI64 { dst, lhs, op, rhs } => {
                let l = self.operand_i64(lhs);
                let r = self.operand_i64(rhs);
                let v = self.builder
                .build_int_compare(icmp_pred(*op), l, r, &uid("icmp")).unwrap();
                self.tmps.insert(dst.0.clone(), v.into());
            }

            IrOp::CmpF64 { dst, lhs, op, rhs } => {
                let l = self.operand_f64(lhs);
                let r = self.operand_f64(rhs);
                let v = self.builder
                .build_float_compare(fcmp_pred(*op), l, r, &uid("fcmp")).unwrap();
                self.tmps.insert(dst.0.clone(), v.into());
            }

            // ════════════════════════════════════════════════
            // KONWERSJE
            // ════════════════════════════════════════════════

            IrOp::IntToFloat { dst, src } => {
                let v = self.operand_i64(src);
                let r = self.builder
                .build_signed_int_to_float(v, self.ctx.f64_type(), &uid("i2f")).unwrap();
                self.tmps.insert(dst.0.clone(), r.into());
            }

            IrOp::FloatToInt { dst, src } => {
                let v = self.operand_f64(src);
                let r = self.builder
                .build_float_to_signed_int(v, self.ctx.i64_type(), &uid("f2i")).unwrap();
                self.tmps.insert(dst.0.clone(), r.into());
            }

            // ════════════════════════════════════════════════
            // I64/F64 → ENV
            // ════════════════════════════════════════════════

            IrOp::I64ToEnv { key, src, buf } => {
                let val     = self.operand_i64(src);
                let buf_ptr = self.buf_ptr(buf);
                let fmt_ptr = self.str_ptr("%lld", "fmt_lld");
                let sz      = self.ctx.i64_type().const_int(32, false);
                self.builder.build_call(
                    self.snprintf_fn,
                    &[buf_ptr.into(), sz.into(), fmt_ptr.into(), val.into()],
                                        &uid("snp"),
                ).unwrap();
                let key_ptr = self.str_ptr(key, "key");
                let ow      = self.ctx.i32_type().const_int(1, false);
                self.builder.build_call(
                    self.setenv_fn,
                    &[key_ptr.into(), buf_ptr.into(), ow.into()],
                                        &uid("senv"),
                ).unwrap();
            }

            IrOp::F64ToEnv { key, src, buf } => {
                let val     = self.operand_f64(src);
                let buf_ptr = self.buf_ptr(buf);
                let fmt_ptr = self.str_ptr("%g", "fmt_g");
                let sz      = self.ctx.i64_type().const_int(32, false);
                self.builder.build_call(
                    self.snprintf_fn,
                    &[buf_ptr.into(), sz.into(), fmt_ptr.into(), val.into()],
                                        &uid("snp"),
                ).unwrap();
                let key_ptr = self.str_ptr(key, "key");
                let ow      = self.ctx.i32_type().const_int(1, false);
                self.builder.build_call(
                    self.setenv_fn,
                    &[key_ptr.into(), buf_ptr.into(), ow.into()],
                                        &uid("senv"),
                ).unwrap();
            }

            // ════════════════════════════════════════════════
            // ŚRODOWISKO
            // ════════════════════════════════════════════════

            IrOp::SetEnv { key, val } | IrOp::SetLocal { key, val } => {
                self.emit_setenv_native(key, val);
            }

            IrOp::SetEnvDyn { key, expr } | IrOp::SetLocalDyn { key, expr } => {
                self.emit_system(&format!("export {}={}", key, expr));
            }

            // ════════════════════════════════════════════════
            // OUTPUT
            // ════════════════════════════════════════════════

            IrOp::SetOut { val } => {
                let is_dynamic = val.contains('$') || val.contains('`') || val.contains("$(");
                if is_dynamic {
                    self.emit_system(&format!("echo {}", val));
                } else {
                    self.emit_print(val);
                }
            }

            IrOp::SetOutI64 { src, buf: _ } => {
                let val = self.operand_i64(src);
                self.emit_print_i64(val);
            }

            IrOp::SetOutF64 { src, buf: _ } => {
                let val = self.operand_f64(src);
                self.emit_print_f64(val);
            }

            // ════════════════════════════════════════════════
            // SYSCALL / CALLEXT
            // ════════════════════════════════════════════════

            IrOp::SysCall { cmd, sudo } => {
                let full = if *sudo { wrap_sudo(cmd) } else { cmd.clone() };
                self.emit_system(&full);
            }

            IrOp::CallExt { cmd, sudo } => {
                let full = if *sudo { wrap_sudo(cmd) } else { cmd.clone() };
                self.emit_system(&full);
            }

            // ════════════════════════════════════════════════
            // CALLHL
            // ════════════════════════════════════════════════

            IrOp::CallHL { name, args } => {
                if let Some(a) = args {
                    if !a.is_empty() {
                        self.emit_system(&format!(
                            "export _HL_ARGS='{}'", a.replace('\'', "'\\''")
                        ));
                    }
                }
                match self.resolve_hl(name) {
                    Some(f) => {
                        self.builder.build_call(f, &[], &uid("call")).unwrap();
                    }
                    None => {
                        if self.verbose {
                            eprintln!("{} CallHL '{}' — brak definicji, fallback shell",
                                      "[!]".yellow(), name);
                        }
                        self.emit_system(name);
                    }
                }
            }

            // ════════════════════════════════════════════════
            // CALLMODULE
            // ════════════════════════════════════════════════

            IrOp::CallModule { module, method, args } => {
                let base = format!("{}.{}", module, method);
                let cmd  = match args.as_deref().filter(|a| !a.is_empty()) {
                    Some(a) => format!("{} {}", base, a),
                    None    => base,
                };
                self.emit_system(&cmd);
            }

            // ════════════════════════════════════════════════
            // RETURN — terminator
            // ════════════════════════════════════════════════

            IrOp::Return => {
                self.flush_defers();
                let zero = self.ctx.i32_type().const_int(0, false);
                self.builder.build_return(Some(&zero)).unwrap();
                return true;
            }

            // ════════════════════════════════════════════════
            // NUMFOR — natywna pętla LLVM
            // ════════════════════════════════════════════════

            IrOp::NumFor { var, start, end, step, env_key, body } => {
                self.emit_num_for(var, start, end, step, env_key, body, func);
            }

            // ════════════════════════════════════════════════
            // PĘTLE SHELLOWE
            // ════════════════════════════════════════════════

            IrOp::WhileShell { cond, body } => {
                let body_sh = self.ops_to_shell(body);
                self.emit_system(&format!("while {}; do {}; done", cond, body_sh));
            }

            IrOp::RepeatN { count, body } => {
                let body_sh = self.ops_to_shell(body);
                self.emit_system(&format!(
                    "for _hl_i in $(seq 1 {}); do {}; done", count, body_sh
                ));
            }

            IrOp::ForIn { var, expr, body } => {
                let body_sh = self.ops_to_shell(body);
                self.emit_system(&format!(
                    "for {} in {}; do {}; done", var, expr, body_sh
                ));
            }

            // ════════════════════════════════════════════════
            // IF / MATCH
            // ════════════════════════════════════════════════

            IrOp::IfChain { branches } => {
                self.emit_if_chain(branches);
            }

            IrOp::MatchCase { cond, arms } => {
                self.emit_system(&build_case(cond, arms));
            }

            // ════════════════════════════════════════════════
            // PIPE
            // ════════════════════════════════════════════════

            IrOp::Pipe { steps } => {
                self.emit_pipe(steps);
            }

            IrOp::PipeLine { step } => {
                let t = step.trim();
                if t.starts_with('.') && t.len() > 1 {
                    let mut parts = t.splitn(2, ' ');
                    let fname = parts.next().unwrap().trim_start_matches('.');
                    let args  = parts.next().unwrap_or("");
                    if !args.is_empty() {
                        self.emit_system(&format!(
                            "export _HL_ARGS='{}'", args.replace('\'', "'\\''")
                        ));
                    }
                    match self.resolve_hl(fname) {
                        Some(f) => { self.builder.build_call(f, &[], &uid("pl")).unwrap(); }
                        None    => { self.emit_system(t); }
                    }
                } else {
                    self.emit_system(t);
                }
            }

            // ════════════════════════════════════════════════
            // ASYNC
            // ════════════════════════════════════════════════

            IrOp::Spawn { cmd, sudo } => {
                let sh = format!("{} &", cmd);
                self.emit_system(&if *sudo { wrap_sudo(&sh) } else { sh });
            }

            IrOp::SpawnAssign { key, cmd, sudo } => {
                let sh = format!("{} & _hl_pid=$!; export {}=$_hl_pid", cmd, key);
                self.emit_system(&if *sudo { wrap_sudo(&sh) } else { sh });
            }

            IrOp::Await { expr } => {
                self.emit_system(&format!("wait {}", expr));
            }

            IrOp::AwaitAssign { key, expr } => {
                let sh = if expr.starts_with('$') {
                    format!("wait {}; export {}=$?", expr, key)
                } else {
                    format!("export {}=$( {} )", key, expr)
                };
                self.emit_system(&sh);
            }

            // ════════════════════════════════════════════════
            // ARENA
            // FIX SEGFAULT: scope pre-alokowany przez
            // emit_arena_scope_alloca() (4624 bajtów) w codegen.rs.
            // ════════════════════════════════════════════════

            IrOp::ArenaEnter { name, size_spec: _, size_bytes } => {
                let scope_ptr = match self.arena_scope {
                    Some(ref s) => s.scope_ptr,
                    None => {
                        if self.verbose {
                            eprintln!("{} ArenaEnter {:?} — brak pre-alloc scope, allokuję inline",
                                      "[!]".yellow(), name);
                        }
                        self.emit_arena_scope_alloca();
                        self.arena_scope.as_ref().unwrap().scope_ptr
                    }
                };
                let name_ptr = self.str_ptr(name, "arena_name");
                let sz       = self.ctx.i64_type().const_int(*size_bytes, false);
                self.builder.build_call(
                    self.arena_enter_fn,
                    &[scope_ptr.into(), name_ptr.into(), sz.into()],
                                        &uid("aenter"),
                ).unwrap();
                if self.verbose {
                    eprintln!("{} ArenaEnter {:?} {} bytes", "[A]".cyan(), name, size_bytes);
                }
            }

            IrOp::ArenaAllocPtr { dst, size } => {
                // FIX: try_as_basic_value() → ValueKind<'ctx>
                // Warianty: ValueKind::Basic(BasicValueEnum) | ValueKind::Instruction(...)
                // Dla funkcji zwracających ptr → Basic(PointerValue(...))
                if let Some(ref scope) = self.arena_scope {
                    let sp  = scope.scope_ptr;
                    let sz  = self.ctx.i64_type().const_int(*size, false);
                    let ret = self.builder.build_call(
                        self.arena_alloc_fn,
                        &[sp.into(), sz.into()],
                                                      &uid("aalloc"),
                    ).unwrap();
                    if let ValueKind::Basic(bv) = ret.try_as_basic_value() {
                        self.tmps.insert(dst.0.clone(), bv);
                        if let BasicValueEnum::PointerValue(pv) = bv {
                            self.slots.insert(dst.0.clone(), (pv, IrType::Ptr));
                        }
                    } else if self.verbose {
                        eprintln!("{} ArenaAllocPtr: call zwrócił void", "[!]".yellow());
                    }
                } else {
                    if self.verbose {
                        eprintln!("{} ArenaAllocPtr poza :: blokiem — fallback gc_malloc",
                                  "[!]".yellow());
                    }
                    let sz  = self.ctx.i64_type().const_int(*size, false);
                    let ret = self.builder
                    .build_call(self.gc_malloc_fn, &[sz.into()], &uid("gcfb")).unwrap();
                    if let ValueKind::Basic(bv) = ret.try_as_basic_value() {
                        self.tmps.insert(dst.0.clone(), bv);
                        if let BasicValueEnum::PointerValue(pv) = bv {
                            self.slots.insert(dst.0.clone(), (pv, IrType::Ptr));
                        }
                    }
                }
            }

            IrOp::ArenaReset { name } => {
                if let Some(ref scope) = self.arena_scope {
                    self.builder.build_call(
                        self.arena_reset_fn,
                        &[scope.scope_ptr.into()],
                                            &uid("areset"),
                    ).unwrap();
                } else if self.verbose {
                    eprintln!("{} ArenaReset {:?} — brak aktywnej areny", "[!]".yellow(), name);
                }
            }

            IrOp::ArenaExit { name } => {
                if let Some(ref scope) = self.arena_scope {
                    self.builder.build_call(
                        self.arena_exit_fn,
                        &[scope.scope_ptr.into()],
                                            &uid("aexit"),
                    ).unwrap();
                    if self.verbose {
                        eprintln!("{} ArenaExit {:?}", "[A]".cyan(), name);
                    }
                } else if self.verbose {
                    eprintln!("{} ArenaExit {:?} — brak aktywnej areny (noop)",
                              "[!]".yellow(), name);
                }
                self.arena_scope = None;
            }

            // ════════════════════════════════════════════════
            // GC
            // FIX: ValueKind::Basic(bv) zamiast ValueKind::Left(bv)
            // ════════════════════════════════════════════════

            IrOp::GcAlloc { var, size } => {
                let sz  = self.ctx.i64_type().const_int(*size, false);
                let ret = self.builder
                .build_call(self.gc_malloc_fn, &[sz.into()], &uid("gcm")).unwrap();
                if let ValueKind::Basic(bv) = ret.try_as_basic_value() {
                    self.tmps.insert(var.0.clone(), bv);
                }
            }

            IrOp::GcFree => {
                self.builder.build_call(self.gc_unmark_fn, &[], &uid("unm")).unwrap();
                self.builder.build_call(self.gc_sweep_fn,  &[], &uid("swp")).unwrap();
            }

            IrOp::GcFull => {
                self.builder.build_call(self.gc_full_fn, &[], &uid("gcf")).unwrap();
            }

            // ════════════════════════════════════════════════
            // TRY / CATCH
            // ════════════════════════════════════════════════

            IrOp::TryCatch { try_cmd, catch_cmd } => {
                self.emit_system(&format!(
                    "( {} ) 2>/dev/null || ( {} )", try_cmd, catch_cmd
                ));
            }

            // ════════════════════════════════════════════════
            // RESULT UNWRAP
            // ════════════════════════════════════════════════

            IrOp::ResultUnwrap { expr, msg } => {
                self.emit_system(&format!(
                    "{}; if [ $? -ne 0 ]; then echo 'error: {}' >&2; exit 1; fi",
                    expr,
                    msg.replace('\'', "'\\''"),
                ));
            }

            // ════════════════════════════════════════════════
            // ASSERT
            // ════════════════════════════════════════════════

            IrOp::Assert { cond, msg } => {
                self.emit_system(&format!(
                    "if ! ( {} ) 2>/dev/null; then echo 'assert: {}' >&2; exit 1; fi",
                                          cond,
                                          msg.replace('\'', "'\\''"),
                ));
            }

            // ════════════════════════════════════════════════
            // LOG
            // ════════════════════════════════════════════════

            IrOp::Log { msg, to_stderr } => {
                let is_dynamic = msg.contains('$') || msg.contains('`') || msg.contains("$(");
                if is_dynamic {
                    let redirect = if *to_stderr { " >&2" } else { "" };
                    self.emit_system(&format!("echo {}{}", msg, redirect));
                } else {
                    self.emit_log(msg, *to_stderr);
                }
            }

            // ════════════════════════════════════════════════
            // KOLEKCJE
            // ════════════════════════════════════════════════

            IrOp::CollectionMut { var, method, args } => {
                let sh = match method.as_str() {
                    "push" => format!("{}=(\"${{{}[@]}}\" {})", var, var, args),
                    "pop"  => format!("unset '{}[${{#{}[@]}}-1]'", var, var),
                    "set"  => {
                        let (k, v) = args.split_once(' ').unwrap_or((args.as_str(), ""));
                        format!("{}[{}]={}", var, k, v)
                    }
                    "del"  => format!("unset '{}[{}]'", var, args),
                    "get"  => format!("echo \"${{{}[{}]}}\"", var, args),
                    other  => {
                        if self.verbose {
                            eprintln!("{} CollectionMut: nieznana metoda '{}'",
                                      "[!]".yellow(), other);
                        }
                        return false;
                    }
                };
                self.emit_system(&sh);
            }

            // ════════════════════════════════════════════════
            // LAMBDA
            // ════════════════════════════════════════════════

            IrOp::Lambda { params, body } => {
                let bindings = params.iter().enumerate()
                .map(|(i, p)| format!("local {}=${}", p, i + 1))
                .collect::<Vec<_>>().join("; ");
                self.emit_system(&format!(
                    "_hl_lambda() {{ {}; {}; }}; export -f _hl_lambda",
                                          bindings, body,
                ));
            }

            IrOp::StoreLambda { key, params, body, is_global } => {
                let bindings = params.iter().enumerate()
                .map(|(i, p)| format!("local {}=${}", p, i + 1))
                .collect::<Vec<_>>().join("; ");
                let decl = format!(
                    "{}() {{ {}; {}; }}; export -f {}",
                                   key, bindings, body, key,
                );
                let sh = if *is_global {
                    format!("export -f {} 2>/dev/null; {}", key, decl)
                } else {
                    decl
                };
                self.emit_system(&sh);
            }

            // ════════════════════════════════════════════════
            // TAILCALL
            // ════════════════════════════════════════════════

            IrOp::TailCall { args } => {
                self.emit_system(&format!(
                    "export _HL_RECUR=1; export _HL_RECUR_ARGS='{}'",
                    args.replace('\'', "'\\''"),
                ));
            }

            // ════════════════════════════════════════════════
            // DESTRUKTURYZACJA
            // ════════════════════════════════════════════════

            IrOp::DestructList { head, tail, source } => {
                self.emit_system(&format!(
                    "{}=\"${{{}[0]}}\"; {}=(\"${{{}[@]:1}}\")",
                                          head, source, tail, source,
                ));
            }

            IrOp::DestructMap { fields, source } => {
                let assignments: Vec<String> = fields.iter()
                .map(|f| format!("{}=\"${{{}[{}]}}\"", f, source, f))
                .collect();
                self.emit_system(&assignments.join("; "));
            }

            // ════════════════════════════════════════════════
            // DO-BLOCK
            // ════════════════════════════════════════════════

            IrOp::DoBlock { key, body } => {
                let body_sh = self.ops_to_shell(body);
                self.emit_system(&format!("export {}=$( {} )", key, body_sh));
            }

            // ════════════════════════════════════════════════
            // SCOPE-BLOCK
            // ════════════════════════════════════════════════

            IrOp::ScopeBlock { body } => {
                if !body.is_empty() {
                    let _ = self.emit_ops(body, func);
                }
            }

            // ════════════════════════════════════════════════
            // TEST-BLOCK
            // ════════════════════════════════════════════════

            IrOp::TestBlock { desc, body } => {
                let header = format!("=== TEST: {} ===", desc);
                self.emit_log(&header, true);
                let _ = self.emit_ops(body, func);
                self.emit_log("=== PASS ===", true);
            }

            // ════════════════════════════════════════════════
            // DEFER
            // ════════════════════════════════════════════════

            IrOp::Defer { expr } => {
                self.defers.push(expr.clone());
                let trap_body: String = self.defers.iter()
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
                self.emit_system(&format!(
                    "trap '{}' EXIT",
                    trap_body.replace('\'', "'\\''"),
                ));
            }

            // ════════════════════════════════════════════════
            // PLUGIN
            // ════════════════════════════════════════════════

            IrOp::Plugin { path, args, sudo } => {
                let cmd = if args.is_empty() {
                    path.clone()
                } else {
                    format!("{} {}", path, args)
                };
                self.emit_system(&if *sudo { wrap_sudo(&cmd) } else { cmd });
            }

            // ════════════════════════════════════════════════
            // LOCK / UNLOCK
            // ════════════════════════════════════════════════

            IrOp::Lock { key, size } => {
                let sz = self.ctx.i64_type().const_int(*size, false);
                self.builder.build_call(self.gc_malloc_fn, &[sz.into()], &uid("lkm")).unwrap();
                let env_key = format!("_HL_LOCK_{}", key);
                self.emit_setenv_native(&env_key, "1");
            }

            IrOp::Unlock { key } => {
                let env_key = format!("_HL_LOCK_{}", key);
                self.emit_setenv_native(&env_key, "");
                self.builder.build_call(self.gc_unmark_fn, &[], &uid("unm")).unwrap();
                self.builder.build_call(self.gc_sweep_fn,  &[], &uid("swp")).unwrap();
            }

            // ════════════════════════════════════════════════
            // EXIT — terminator
            // ════════════════════════════════════════════════

            IrOp::Exit { code } => {
                self.flush_defers();
                if let Some(ref scope) = self.arena_scope {
                    self.builder.build_call(
                        self.arena_exit_fn,
                        &[scope.scope_ptr.into()],
                                            &uid("aexit_on_exit"),
                    ).unwrap();
                }
                self.arena_scope = None;
                self.builder.build_call(self.gc_full_fn, &[], &uid("gcf")).unwrap();
                let cv = self.ctx.i32_type().const_int(*code as u64, true);
                self.builder.build_call(self.exit_fn, &[cv.into()], &uid("exit")).unwrap();
                self.builder.build_unreachable().unwrap();
                return true;
            }

            // ════════════════════════════════════════════════
            // META
            // ════════════════════════════════════════════════

            IrOp::Comment { .. } | IrOp::Nop => {}
        }

        false
    }

    // ─────────────────────────────────────────────────────────
    // emit_num_for — natywna pętla LLVM z phi node
    // ─────────────────────────────────────────────────────────
    pub(crate) fn emit_num_for(
        &mut self,
        var:     &IrVar,
        start:   &crate::ir::IrOperand,
        end:     &crate::ir::IrOperand,
        step:    &crate::ir::IrOperand,
        env_key: &str,
        body:    &[IrOp],
        func:    FunctionValue<'ctx>,
    ) {
        let i64_t   = self.ctx.i64_type();
        let i8_t    = self.ctx.i8_type();
        let buf_arr = i8_t.array_type(32);

        let var_slot = self.builder.build_alloca(i64_t,   &var.0).unwrap();
        let buf_slot = self.builder.build_alloca(buf_arr, &uid("forbuf")).unwrap();
        self.slots.insert(var.0.clone(), (var_slot, IrType::I64));

        let start_val = self.operand_i64(start);
        let end_val   = self.operand_i64(end);
        let step_val  = self.operand_i64(step);

        let bb_hdr  = self.ctx.append_basic_block(func, &uid("for_hdr"));
        let bb_body = self.ctx.append_basic_block(func, &uid("for_body"));
        let bb_inc  = self.ctx.append_basic_block(func, &uid("for_inc"));
        let bb_exit = self.ctx.append_basic_block(func, &uid("for_exit"));

        self.builder.build_unconditional_branch(bb_hdr).unwrap();

        // ── Nagłówek ───────────────────────────────────────
        self.builder.position_at_end(bb_hdr);
        let phi       = self.builder.build_phi(i64_t, &uid("phi")).unwrap();
        let preheader = self.builder.get_insert_block().unwrap();
        phi.add_incoming(&[(&start_val, preheader)]);

        let i_val: IntValue = phi.as_basic_value().into_int_value();
        let cmp = self.builder.build_int_compare(
            IntPredicate::SLT, i_val, end_val, &uid("cmp"),
        ).unwrap();
        self.builder.build_conditional_branch(cmp, bb_body, bb_exit).unwrap();

        // ── Ciało ──────────────────────────────────────────
        self.builder.position_at_end(bb_body);
        self.builder.build_store(var_slot, i_val).unwrap();

        let z0      = self.ctx.i64_type().const_int(0, false);
        let buf_ptr = unsafe {
            self.builder.build_gep(buf_arr, buf_slot, &[z0, z0], &uid("bgep")).unwrap()
        };
        let fmt_ptr = self.str_ptr("%lld", "fmt_lld");
        let sz      = i64_t.const_int(32, false);
        self.builder.build_call(
            self.snprintf_fn,
            &[buf_ptr.into(), sz.into(), fmt_ptr.into(), i_val.into()],
                                &uid("snp"),
        ).unwrap();
        self.emit_setenv_i64(env_key, i_val);

        for body_op in body {
            self.emit_one(body_op, func);
        }
        self.builder.build_unconditional_branch(bb_inc).unwrap();

        // ── Inkrement ──────────────────────────────────────
        self.builder.position_at_end(bb_inc);
        let i_next = self.builder
        .build_int_add(i_val, step_val, &uid("inc")).unwrap();
        phi.add_incoming(&[(&i_next, bb_inc)]);
        self.builder.build_unconditional_branch(bb_hdr).unwrap();

        // ── Wyjście ────────────────────────────────────────
        self.builder.position_at_end(bb_exit);
    }

    // ─────────────────────────────────────────────────────────
    // emit_if_chain
    // ─────────────────────────────────────────────────────────
    pub(crate) fn emit_if_chain(&mut self, branches: &[IrBranch]) {
        if branches.is_empty() { return; }
        let mut sh = String::new();
        for (bi, br) in branches.iter().enumerate() {
            match (&br.cond, bi) {
                (Some(c), 0) => sh += &format!("if {}; then ", c),
                (Some(c), _) => sh += &format!("elif {}; then ", c),
                (None,    _) => sh += "else ",
            }
            sh += &self.ops_to_shell(&br.body);
            sh += "; ";
        }
        sh += "fi";
        self.emit_system(&sh);
    }

    // ─────────────────────────────────────────────────────────
    // emit_pipe
    // ─────────────────────────────────────────────────────────
    pub(crate) fn emit_pipe(&mut self, steps: &[IrPipeStep]) {
        if steps.is_empty() { return; }

        let all_hl = steps.iter().all(|s| s.is_hl);

        if all_hl {
            for step in steps {
                let mut parts = step.cmd.splitn(2, ' ');
                let fname = parts.next().unwrap_or("").trim_start_matches('.');
                let args  = parts.next().unwrap_or("");
                if !args.is_empty() {
                    self.emit_system(&format!(
                        "export _HL_ARGS='{}'", args.replace('\'', "'\\''")
                    ));
                }
                match self.resolve_hl(fname) {
                    Some(f) => { self.builder.build_call(f, &[], &uid("pipe")).unwrap(); }
                    None    => {
                        if self.verbose {
                            eprintln!("{} Pipe step '{}' — brak LLVM fn, fallback shell",
                                      "[!]".yellow(), fname);
                        }
                        self.emit_system(fname);
                    }
                }
            }
        } else {
            let parts: Vec<String> = steps.iter()
            .map(|s| if s.is_hl {
                format!("( {} )", s.cmd.trim_start_matches('.'))
            } else {
                s.cmd.clone()
            })
            .collect();
            self.emit_system(&parts.join(" | "));
        }
    }
}
