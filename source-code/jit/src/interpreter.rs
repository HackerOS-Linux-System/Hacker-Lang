use anyhow::{bail, Result};
use hl_compiler::bytecode::*;
use crate::runtime::{RuntimeState, NanVal};
use std::process::{Command, Stdio};

// ── Dispatch signal ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ExecSignal {
    Next,
    Jump(u32),
    Return,
    FuncCall(u32),   // name_idx
    Exit(i32),
}

// ── Trace JIT threshold ───────────────────────────────────────────────────────

const TRACE_THRESHOLD: u32 = 50;

// ── Główny interpreter ────────────────────────────────────────────────────────

pub struct BytecodeInterpreter<'a> {
    pub module:      &'a HlModule,
    pub state:       RuntimeState,
    /// Liczniki wykonań per instrukcja (dla trace JIT)
    exec_counts:     Vec<u32>,
    /// Skompilowane trasy (offset → native fn ptr)
    compiled_traces: rustc_hash::FxHashMap<u32, CompiledTrace>,
}

/// Skompilowana trasa (wynik trace JIT)
pub struct CompiledTrace {
    /// fn(regs: *mut u64, vars: *mut u64, reg_count: u32, var_count: u32) -> i32
    pub fn_ptr: unsafe extern "C" fn(*mut u64, *mut u64, u32, u32) -> i32,
    /// Offset wyjścia z trasy (dokąd skakać po wykonaniu)
    pub exit_offset: u32,
}

impl<'a> BytecodeInterpreter<'a> {
    pub fn new(module: &'a HlModule) -> Self {
        let n = module.instructions.len();
        Self {
            module,
            state:           RuntimeState::new(module.main_regs as usize),
            exec_counts:     vec![0u32; n],
            compiled_traces: rustc_hash::FxHashMap::default(),
        }
    }

    /// Inicjalizuj zmienne HL_VERSION itp.
    pub fn init_hl_vars(&mut self) {
        let k = self.state.interner.intern("HL_VERSION");
        let v = self.state.intern_str("gen 2");
        self.state.set_var(k, v);
        let k2 = self.state.interner.intern("HL_GEN");
        let v2 = self.state.intern_str("2");
        self.state.set_var(k2, v2);
    }

    /// Uruchom główny blok
    pub fn run(&mut self) -> Result<i32> {
        self.init_hl_vars();
        let main_end = self.module.funcs.entries.first()
        .map(|f| f.start_insn as usize)
        .unwrap_or(self.module.instructions.len());
        self.exec_block(0, main_end)?;
        Ok(self.state.last_exit)
    }

    fn exec_block(&mut self, start: usize, end: usize) -> Result<ExecSignal> {
        let mut pc = start;
        while pc < end {
            // ── Trace JIT check ───────────────────────────────────────────
            // Przy JumpIfFalse/Jump które wracają do wcześniejszego offsetu
            // (pętla) — zliczamy i kompilujemy po przekroczeniu progu
            if let Some(Instruction::Jump { offset }) = self.module.instructions.get(pc) {
                let target = *offset as usize;
                if target < pc {
                    // Pętla wsteczna — kandydat do trace JIT
                    // Guard: kompiluj tylko małe pętle (<= 64 instrukcji)
                    let loop_size = pc - target;
                    let count = self.exec_counts.get_mut(pc).map(|c| { *c += 1; *c }).unwrap_or(0);
                    if count == TRACE_THRESHOLD && loop_size <= 64 {
                        // Próbuj skompilować pętlę [target..pc+1]
                        if let Ok(trace) = self.try_compile_trace(target as u32, pc as u32) {
                            self.compiled_traces.insert(target as u32, trace);
                            tracing::debug!("[trace jit] skompilowano pętle @ {} (size={})", target, loop_size);
                        }
                    }
                    // Jeśli pętla jest skompilowana — wykonaj natywnie
                    if let Some(_trace) = self.compiled_traces.get(&(target as u32)) {
                        let result = self.exec_native_trace(target as u32)?;
                        pc = result as usize;
                        continue;
                    }
                }
            }

            match self.exec_insn(pc)? {
                ExecSignal::Next          => pc += 1,
                ExecSignal::Jump(off)     => pc = off as usize,
                ExecSignal::Return        => return Ok(ExecSignal::Return),
                ExecSignal::Exit(code)    => { self.state.last_exit = code; return Ok(ExecSignal::Return); }
                ExecSignal::FuncCall(ni)  => {
                    self.exec_func_by_name_idx(ni)?;
                    pc += 1;
                }
            }
        }
        Ok(ExecSignal::Next)
    }

    /// Wykonaj skompilowaną trasę — przekaż rejestry i zmienne jako raw pointers
    fn exec_native_trace(&mut self, trace_start: u32) -> Result<u32> {
        let trace = match self.compiled_traces.get(&trace_start) {
            Some(t) => t,
            None    => return Ok(trace_start),
        };
        let exit_offset = trace.exit_offset;
        let fn_ptr      = trace.fn_ptr;
        let reg_count   = self.state.regs.len() as u32;
        let var_count   = self.state.vars_flat.len() as u32;

        // SAFETY: NanVal jest #[repr(transparent)] u64 — bezpośredni cast
        let result = unsafe {
            (fn_ptr)(
                self.state.regs.as_mut_ptr() as *mut u64,
                     self.state.vars_flat.as_mut_ptr() as *mut u64,
                     reg_count,
                     var_count,
            )
        };

        self.state.last_exit = result;
        Ok(exit_offset)
    }

    /// Próbuj skompilować trasę [start..end] do kodu maszynowego
    /// Aktualnie: deleguje do JitEngine jeśli blok jest kwalifikowany
    fn try_compile_trace(&self, start: u32, end: u32) -> Result<CompiledTrace> {
        // Trace compilation przez JitEngine — kompiluje blok jako pseudo-funkcję
        let entry = hl_compiler::bytecode::FuncEntry {
            name:       format!("__trace_{}_{}", start, end),
            start_insn: start,
            insn_count: end - start + 1,
        };
        crate::jit_engine::compile_trace_entry(self.module, &entry)
    }

    // ── Dispatch instrukcji ───────────────────────────────────────────────────

    fn exec_insn(&mut self, pc: usize) -> Result<ExecSignal> {
        let insn = match self.module.instructions.get(pc) {
            Some(i) => i.clone(),
            None    => return Ok(ExecSignal::Return),
        };

        // Dispatch — Rust kompilator generuje jump table dla gęstego match
        // Używamy jawnego match zamiast fn ptr table bo Rust optymalizuje to dobrze
        match insn {
            Instruction::Nop | Instruction::SourceLine { .. } => Ok(ExecSignal::Next),

            // ── Ładowanie stałych ─────────────────────────────────────────────
            Instruction::LoadStr { dst, idx } => {
                let s   = self.module.consts.strings.get(idx as usize).map(|s| s.as_str()).unwrap_or("");
                let val = self.state.intern_str(s);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }
            Instruction::LoadNum { dst, idx } => {
                let n = self.module.consts.numbers.get(idx as usize).copied().unwrap_or(0.0);
                self.state.set_reg(dst, NanVal::num(n));
                Ok(ExecSignal::Next)
            }
            Instruction::LoadBool { dst, val } => {
                self.state.set_reg(dst, NanVal::bool(val));
                Ok(ExecSignal::Next)
            }
            Instruction::LoadNil { dst } => {
                self.state.set_reg(dst, NanVal::nil());
                Ok(ExecSignal::Next)
            }

            // ── Zmienne ───────────────────────────────────────────────────────
            Instruction::GetVar { dst, name } => {
                // Inline cache hot path — O(1)
                let val = self.state.get_var(name);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }
            // GetVarDyn: @{arg@_i} — dynamiczna nazwa zmiennej z rejestru
            // Rejestr `name` zawiera string (nazwę zmiennej), np. "arg0".
            // Rozdzielamy borrow: najpierw kopiujemy string (owned), potem internujemy i robimy lookup.
            Instruction::GetVarDyn { dst, name } => {
                // 1. Pobierz wartość rejestru name → string (owned, by uniknąć borrow conflict)
                let name_str: String = {
                    let name_val = self.state.get_reg(name);
                    name_val.to_str_val(&self.state.interner)
                };
                // 2. Intern string → idx (wymaga &mut interner — osobny blok)
                let name_idx = self.state.interner.intern(&name_str);
                // 3. Lookup zmiennej przez idx
                let val = self.state.get_var(name_idx);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }
            Instruction::SetVar { name, src } => {
                let val = self.state.get_reg(src);
                self.state.set_var(name, val);
                // Synchronizuj last_exit jeśli to _last_exit_code
                let le_name = self.const_str_idx("_last_exit_code");
                if name == le_name {
                    self.state.last_exit = val.as_f64() as i32;
                }
                Ok(ExecSignal::Next)
            }
            Instruction::SetEnv { name, src } => {
                let val = self.state.get_reg(src);
                self.state.export_var(name, val);
                Ok(ExecSignal::Next)
            }

            // ── Arytmetyka — bezpośrednio na f64, zero alokacji ───────────────
            Instruction::Add { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() + self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::num(r));
                Ok(ExecSignal::Next)
            }
            Instruction::Sub { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() - self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::num(r));
                Ok(ExecSignal::Next)
            }
            Instruction::Mul { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() * self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::num(r));
                Ok(ExecSignal::Next)
            }
            Instruction::Div { dst, a, b } => {
                let va = self.state.get_reg(a).as_f64();
                let vb = self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::num(if vb == 0.0 { 0.0 } else { va / vb }));
                Ok(ExecSignal::Next)
            }
            Instruction::Mod { dst, a, b } => {
                let va = self.state.get_reg(a).as_f64() as i64;
                let vb = self.state.get_reg(b).as_f64() as i64;
                let r  = if vb == 0 { 0 } else { va % vb };
                self.state.set_reg(dst, NanVal::num(r as f64));
                Ok(ExecSignal::Next)
            }
            Instruction::Neg { dst, src } => {
                let r = -self.state.get_reg(src).as_f64();
                self.state.set_reg(dst, NanVal::num(r));
                Ok(ExecSignal::Next)
            }

            // ── Porównania — fast path dla liczb ─────────────────────────────
            Instruction::CmpEq { dst, a, b } => {
                let va = self.state.get_reg(a);
                let vb = self.state.get_reg(b);
                self.state.set_reg(dst, NanVal::bool(va.eq_val(&vb, &self.state.interner)));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpNe { dst, a, b } => {
                let va = self.state.get_reg(a);
                let vb = self.state.get_reg(b);
                self.state.set_reg(dst, NanVal::bool(!va.eq_val(&vb, &self.state.interner)));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpLt { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() < self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::bool(r));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpLe { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() <= self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::bool(r));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpGt { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() > self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::bool(r));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpGe { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() >= self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::bool(r));
                Ok(ExecSignal::Next)
            }

            // ── Konwersje ─────────────────────────────────────────────────────
            Instruction::ToString { dst, src } => {
                let s   = self.state.get_reg(src).to_str_val(&self.state.interner);
                let val = self.state.intern_str_owned(s);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }
            Instruction::ToNumber { dst, src } => {
                let n = self.state.get_reg(src).as_f64();
                self.state.set_reg(dst, NanVal::num(n));
                Ok(ExecSignal::Next)
            }
            Instruction::Truthy { dst, src } => {
                let val = self.state.get_reg(src);
                let b   = match val.as_str_idx() {
                    Some(idx) => {
                        // Warunek while — ewaluuj wyrażenie porównania
                        let s = self.state.interner.get(idx).to_string();
                        eval_condition_str(&s, &mut self.state)
                    }
                    None => val.is_truthy(&self.state.interner),
                };
                self.state.set_reg(dst, NanVal::bool(b));
                Ok(ExecSignal::Next)
            }

            // ── Concat — internuje wynik ──────────────────────────────────────
            Instruction::Concat { dst, parts } => {
                let mut buf = String::new();
                for &r in &parts {
                    let s = self.state.get_reg(r).to_str_val(&self.state.interner);
                    buf.push_str(&s);
                }
                let val = self.state.intern_str_owned(buf);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }

            // ── Output ────────────────────────────────────────────────────────
            Instruction::Print { src } => {
                println!("{}", self.state.get_reg(src).to_str_val(&self.state.interner));
                Ok(ExecSignal::Next)
            }

            // ── Sterowanie ────────────────────────────────────────────────────
            Instruction::JumpIfFalse { cond, offset } => {
                if !self.state.get_reg(cond).is_truthy(&self.state.interner) {
                    Ok(ExecSignal::Jump(offset))
                } else {
                    Ok(ExecSignal::Next)
                }
            }
            Instruction::JumpIfTrue { cond, offset } => {
                if self.state.get_reg(cond).is_truthy(&self.state.interner) {
                    Ok(ExecSignal::Jump(offset))
                } else {
                    Ok(ExecSignal::Next)
                }
            }
            Instruction::Jump { offset } => Ok(ExecSignal::Jump(offset)),
            Instruction::Return { .. }   => Ok(ExecSignal::Return),

            // ── Wywołania ─────────────────────────────────────────────────────
            Instruction::CallFunc { name } => Ok(ExecSignal::FuncCall(name)),

            Instruction::CallQuick { name, arg, dst } => {
                let arg_str  = self.state.get_reg(arg).to_str_val(&self.state.interner);
                let name_str = self.const_str(name);
                let result   = exec_quick_fn(&name_str, &arg_str, &mut self.state);
                let val      = self.state.intern_str_owned(result);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }

            // ── Komendy systemowe ─────────────────────────────────────────────
            Instruction::ExecCmd { cmd, mode, dst } => {
                let cmd_str   = self.state.get_reg(cmd).to_str_val(&self.state.interner);
                let exit_code = exec_system_cmd(&cmd_str, mode, &mut self.state)?;
                self.state.set_reg(dst, NanVal::num(exit_code as f64));
                self.state.last_exit = exit_code;
                // Ustaw _last_exit_code w zmiennych
                let le_idx = self.state.interner.intern("_last_exit_code");
                self.state.set_var(le_idx, NanVal::num(exit_code as f64));
                Ok(ExecSignal::Next)
            }

            Instruction::ExecCapture { cmd, mode, dst_ec, dst_out } => {
                let cmd_str = self.state.get_reg(cmd).to_str_val(&self.state.interner);
                let (exit_code, stdout) = exec_system_cmd_capture(&cmd_str, mode)?;
                self.state.set_reg(dst_ec, NanVal::num(exit_code as f64));
                let out_val = self.state.intern_str_owned(stdout);
                self.state.set_reg(dst_out, out_val);
                self.state.last_exit = exit_code;
                Ok(ExecSignal::Next)
            }

            // ── For-in ────────────────────────────────────────────────────────
            Instruction::ForInStart { iter_reg, src } => {
                let src_str = self.state.get_reg(src).to_str_val(&self.state.interner);
                // Intern każde słowo — szybsze porównania w pętli
                let words: Vec<u32> = src_str.split_whitespace()
                .map(|w| self.state.interner.intern(w))
                .collect();
                self.state.iters.insert(iter_reg, (words, 0));
                Ok(ExecSignal::Next)
            }

            Instruction::ForInNext { iter_reg, dst, end_off } => {
                let should_jump = if let Some((words, idx)) = self.state.iters.get_mut(&iter_reg) {
                    if *idx >= words.len() {
                        true
                    } else {
                        let word_idx = words[*idx];
                        *idx += 1;
                        self.state.set_reg(dst, NanVal::str_interned(word_idx));
                        false
                    }
                } else { true };

                if should_jump {
                    self.state.iters.remove(&iter_reg);
                    Ok(ExecSignal::Jump(end_off))
                } else {
                    Ok(ExecSignal::Next)
                }
            }

            // ── HackerOS API ──────────────────────────────────────────────────
            Instruction::HackerOsCall { tool, args, dst } => {
                let tool_str = self.const_str(tool);
                let args_str = self.state.get_reg(args).to_str_val(&self.state.interner);
                let cmd = if args_str.is_empty() {
                    tool_str.clone()
                } else {
                    format!("{} {}", tool_str, args_str)
                };
                if which::which(&tool_str).is_err() {
                    eprintln!("\x1b[33m[hl ||]\x1b[0m Narzędzie '{}' nie jest zainstalowane.", tool_str);
                    self.state.set_reg(dst, NanVal::num(127.0));
                } else {
                    let ec = exec_system_cmd(&cmd, CmdMode::Plain, &mut self.state)?;
                    self.state.set_reg(dst, NanVal::num(ec as f64));
                }
                Ok(ExecSignal::Next)
            }
        }
    }

    fn exec_func_by_name_idx(&mut self, name_idx: u32) -> Result<()> {
        self.state.check_call_depth()?;
        let name = self.const_str(name_idx);
        let entry = match self.module.funcs.find(&name) {
            Some(e) => e.clone(),
            None    => bail!("Niezdefiniowana funkcja: '{}'", name),
        };
        self.state.call_depth += 1;
        let start = entry.start_insn as usize;
        let end   = start + entry.insn_count as usize;
        let mut pc = start;
        loop {
            if pc >= end { break; }
            match self.exec_insn(pc)? {
                ExecSignal::Next             => pc += 1,
                ExecSignal::Jump(off)        => pc = off as usize,
                ExecSignal::Return           => break,
                ExecSignal::Exit(code)       => { self.state.last_exit = code; break; }
                ExecSignal::FuncCall(ni)     => { self.exec_func_by_name_idx(ni)?; pc += 1; }
            }
        }
        self.state.call_depth -= 1;
        Ok(())
    }

    #[inline]
    fn const_str(&self, idx: u32) -> String {
        self.module.consts.strings.get(idx as usize).cloned().unwrap_or_default()
    }

    #[inline]
    fn const_str_idx(&mut self, s: &str) -> u32 {
        self.state.interner.intern(s)
    }
}

// ── Komendy systemowe ─────────────────────────────────────────────────────────

fn exec_system_cmd(cmd: &str, mode: CmdMode, _state: &mut RuntimeState) -> Result<i32> {
    // Specjalne prefiksy z lowera
    if let Some(path) = cmd.strip_prefix("__hl_import__ ") {
        // Import w czasie wykonania — placeholder, obsługiwany przez tree-walk executor
        // JIT nie ładuje importów bezpośrednio (brak hl_core w zależnościach jit crate)
        tracing::debug!("[jit import] pomijam: {}", path);
        return Ok(0);
    }
    if let Some(rest) = cmd.strip_prefix("& ") {
        let _ = Command::new("sh").args(["-c", rest])
        .stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .spawn();
        return Ok(0);
    }

    let (prog, args, needs_sh) = build_cmd_parts(cmd, mode);
    let status = if needs_sh {
        Command::new("sh").args(["-c", cmd])
        .stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .status()
    } else {
        Command::new(&prog).args(&args)
        .stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .status()
    };

    match status {
        Ok(s)  => Ok(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("\x1b[31m[hl jit]\x1b[0m Błąd komendy: {}", e);
            Ok(1)
        }
    }
}

fn exec_system_cmd_capture(cmd: &str, mode: CmdMode) -> Result<(i32, String)> {
    let (prog, args, needs_sh) = build_cmd_parts(cmd, mode);
    let out = if needs_sh {
        Command::new("sh").args(["-c", cmd])
        .stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .output()
    } else {
        Command::new(&prog).args(&args)
        .stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .output()
    };
    match out {
        Ok(o)  => Ok((o.status.code().unwrap_or(1), String::from_utf8_lossy(&o.stdout).trim().to_string())),
        Err(e) => { eprintln!("\x1b[31m[hl jit]\x1b[0m Capture error: {}", e); Ok((1, String::new())) }
    }
}

fn build_cmd_parts(cmd: &str, mode: CmdMode) -> (String, Vec<String>, bool) {
    let needs_sh = cmd.contains('|') || cmd.contains(';') || cmd.contains('&')
    || cmd.contains('>') || cmd.contains('<') || cmd.contains('$') || cmd.contains('`')
    || cmd.contains('*') || cmd.contains('~');

    match mode {
        CmdMode::Sudo | CmdMode::WithVarsSudo => {
            if needs_sh {
                ("sudo".into(), vec!["sh".into(), "-c".into(), cmd.into()], false)
            } else {
                let parts = split_cmd(cmd);
                ("sudo".into(), parts, false)
            }
        }
        CmdMode::Isolated | CmdMode::WithVarsIsolated => {
            let a = vec!["--mount","--pid","--net","--fork","--","sh","-c",cmd]
            .into_iter().map(|s| s.to_string()).collect();
            ("unshare".into(), a, false)
        }
        CmdMode::IsolatedSudo => {
            let a = vec!["unshare","--mount","--pid","--net","--fork","--","sh","-c",cmd]
            .into_iter().map(|s| s.to_string()).collect();
            ("sudo".into(), a, false)
        }
        _ => {
            if needs_sh {
                (String::new(), vec![], true) // caller uses sh -c
            } else {
                let mut parts = split_cmd(cmd);
                let prog = if parts.is_empty() { String::new() } else { parts.remove(0) };
                (prog, parts, false)
            }
        }
    }
}

fn split_cmd(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_sq = false;
    let mut in_dq = false;
    for c in cmd.chars() {
        match c {
            '\'' if !in_dq => in_sq = !in_sq,
            '"'  if !in_sq => in_dq = !in_dq,
            ' ' | '\t' if !in_sq && !in_dq => {
                if !cur.is_empty() { parts.push(std::mem::take(&mut cur)); }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { parts.push(cur); }
    parts
}

// ── Quick functions ───────────────────────────────────────────────────────────

fn exec_quick_fn(name: &str, arg: &str, state: &mut RuntimeState) -> String {
    match name {
        "upper"    => arg.to_uppercase(),
        "lower"    => arg.to_lowercase(),
        "len"      => arg.len().to_string(),
        "trim"     => arg.trim().to_string(),
        "rev"      => arg.chars().rev().collect(),
        "abs"      => arg.parse::<f64>().unwrap_or(0.0).abs().to_string(),
        "ceil"     => arg.parse::<f64>().unwrap_or(0.0).ceil().to_string(),
        "floor"    => arg.parse::<f64>().unwrap_or(0.0).floor().to_string(),
        "round"    => arg.parse::<f64>().unwrap_or(0.0).round().to_string(),
        "basename" => std::path::Path::new(arg).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string(),
        "dirname"  => std::path::Path::new(arg).parent().and_then(|p| p.to_str()).unwrap_or(".").to_string(),
        "pid"      => { println!("{}", std::process::id()); String::new() }
        "nl"       => { println!(); String::new() }
        "hr"       => {
            let w: usize = arg.parse().unwrap_or(60);
            println!("{}", "─".repeat(w)); String::new()
        }
        "bold"     => { println!("\x1b[1m{}\x1b[0m", arg); String::new() }
        "red"      => { println!("\x1b[31m{}\x1b[0m", arg); String::new() }
        "green"    => { println!("\x1b[32m{}\x1b[0m", arg); String::new() }
        "yellow"   => { println!("\x1b[33m{}\x1b[0m", arg); String::new() }
        "cyan"     => { println!("\x1b[36m{}\x1b[0m", arg); String::new() }
        "exists"   => {
            let e = std::path::Path::new(arg).exists();
            let k = state.interner.intern("_last_bool");
            state.set_var(k, NanVal::bool(e));
            if !e { String::from("false") } else { String::new() }
        }
        "isdir"    => { let e = std::path::Path::new(arg).is_dir();  e.to_string() }
        "isfile"   => { let e = std::path::Path::new(arg).is_file(); e.to_string() }
        "which"    => which::which(arg).map(|p| p.display().to_string()).unwrap_or_default(),
        "env" | "getenv" => std::env::var(arg).unwrap_or_default(),
        // ::env-path — ścieżka aktywnego środowiska z config.hk, zero subprocess
        "env-path" => {
            use hl_core::config::get_active_env;
            get_active_env()
                .map(|(_n, p)| p.display().to_string())
                .unwrap_or_default()
        }
        "read"     => std::fs::read_to_string(arg).unwrap_or_default(),
        "set"      => {
            if let Some((name, val)) = arg.splitn(2, ' ').collect::<Vec<_>>().as_slice().split_first() {
                let k = state.interner.intern(name);
                let v = state.intern_str(val.first().copied().unwrap_or(""));
                state.set_var(k, v);
            }
            String::new()
        }
        "get"      => {
            let k = state.interner.intern(arg);
            state.get_var(k).to_str_val(&state.interner)
        }
        "unset"    => {
            let k = state.interner.intern(arg);
            state.var_cache.invalidate(k);
            state.var_slots.remove(&k);
            String::new()
        }
        "rand"     => {
            let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64;
            let r = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) % 100;
            r.to_string()
        }
        "date"     => { let o = Command::new("date").arg("+%Y-%m-%d").output().ok(); o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default() }
        "time"     => { let o = Command::new("date").arg("+%H:%M:%S").output().ok(); o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default() }
        _          => {
            eprintln!("\x1b[31m[hl jit]\x1b[0m Nieznana quick-funkcja '::{}'", name);
            String::new()
        }
    }
}

// ── Ewaluacja warunków while ──────────────────────────────────────────────────

fn eval_condition_str(cond: &str, state: &mut RuntimeState) -> bool {
    let cond = cond.trim();
    if cond.is_empty() { return false; }
    if cond == "true"  { return true;  }
    if cond == "false" { return false; }

    const OPS: &[&str] = &["==", "!=", ">=", "<=", ">", "<"];
    for op in OPS {
        if let Some(pos) = find_op(cond, op) {
            let left_raw  = cond[..pos].trim();
            let right_raw = cond[pos + op.len()..].trim().trim_matches('"');

            let lv = if let Some(name) = left_raw.strip_prefix('@') {
                let k = state.interner.intern(name);
                state.get_var(k).to_str_val(&state.interner)
            } else {
                left_raw.to_string()
            };

            return match *op {
                "==" => lv == right_raw,
                "!=" => lv != right_raw,
                ">=" => lv.parse::<f64>().unwrap_or(0.0) >= right_raw.parse::<f64>().unwrap_or(0.0),
                "<=" => lv.parse::<f64>().unwrap_or(0.0) <= right_raw.parse::<f64>().unwrap_or(0.0),
                ">"  => lv.parse::<f64>().unwrap_or(0.0) >  right_raw.parse::<f64>().unwrap_or(0.0),
                "<"  => lv.parse::<f64>().unwrap_or(0.0) <  right_raw.parse::<f64>().unwrap_or(0.0),
                _    => false,
            };
        }
    }

    // Fallback shell
    Command::new("sh").args(["-c", cond]).status().map(|s| s.success()).unwrap_or(false)
}

fn find_op(s: &str, op: &str) -> Option<usize> {
    let b = s.as_bytes(); let ob = op.as_bytes(); let ol = ob.len();
    let mut i = 0;
    while i + ol <= b.len() {
        if &b[i..i+ol] == ob {
            let ok = match op {
                ">" => i + 1 >= b.len() || b[i+1] != b'=',
                "<" => i + 1 >= b.len() || b[i+1] != b'=',
                _   => true,
            };
            if ok { return Some(i); }
        }
        i += 1;
    }
    None
}
