use anyhow::{bail, Result};
use hl_compiler::bytecode::*;
use crate::runtime::{RuntimeState, RtVal};
use std::process::{Command, Stdio};

pub struct BytecodeInterpreter<'a> {
    pub module: &'a HlModule,
    pub state:  RuntimeState,
}

impl<'a> BytecodeInterpreter<'a> {
    pub fn new(module: &'a HlModule) -> Self {
        Self {
            module,
            state: RuntimeState::new(module.main_regs as usize),
        }
    }

    /// Uruchom główny blok modułu
    pub fn run(&mut self) -> Result<i32> {
        // Znajdź koniec głównego bloku (przed pierwszą funkcją)
        let main_end = self.module.funcs.entries.first()
        .map(|f| f.start_insn as usize)
        .unwrap_or(self.module.instructions.len());

        self.exec_block(0, main_end)?;
        Ok(self.state.last_exit)
    }

    /// Wykonaj blok instrukcji [start, end)
    fn exec_block(&mut self, start: usize, end: usize) -> Result<ExecSignal> {
        let mut pc = start;
        while pc < end {
            match self.exec_insn(pc)? {
                ExecSignal::Next        => pc += 1,
                ExecSignal::Jump(off)   => pc = off as usize,
                ExecSignal::Return      => return Ok(ExecSignal::Return),
                ExecSignal::FuncCall(name_idx) => {
                    self.exec_func(name_idx)?;
                    pc += 1;
                }
            }
        }
        Ok(ExecSignal::Next)
    }

    fn exec_insn(&mut self, pc: usize) -> Result<ExecSignal> {
        let insn = match self.module.instructions.get(pc) {
            Some(i) => i.clone(),
            None    => return Ok(ExecSignal::Return),
        };

        match insn {
            Instruction::Nop | Instruction::SourceLine { .. } => {}

            Instruction::LoadStr { dst, idx } => {
                let s = self.module.consts.strings.get(idx as usize)
                .cloned().unwrap_or_default();
                self.state.set_reg(dst, RtVal::Str(s));
            }
            Instruction::LoadNum { dst, idx } => {
                let n = self.module.consts.numbers.get(idx as usize)
                .copied().unwrap_or(0.0);
                self.state.set_reg(dst, RtVal::Num(n));
            }
            Instruction::LoadBool { dst, val } => {
                self.state.set_reg(dst, RtVal::Bool(val));
            }
            Instruction::LoadNil { dst } => {
                self.state.set_reg(dst, RtVal::Nil);
            }

            Instruction::GetVar { dst, name } => {
                let name_str = self.const_str(name);
                let val = self.state.get_var(name, &name_str);
                self.state.set_reg(dst, val);
            }
            Instruction::SetVar { name, src } => {
                let val = self.state.get_reg(src).clone();
                self.state.set_var(name, val);
            }
            Instruction::SetEnv { name, src } => {
                let val = self.state.get_reg(src).to_str();
                let name_str = self.const_str(name);
                std::env::set_var(&name_str, &val);
                self.state.set_var(name, RtVal::Str(val));
            }

            // ── Arytmetyka ───────────────────────────────────────────────────
            Instruction::Add { dst, a, b } => {
                let va = self.state.get_reg(a).to_f64();
                let vb = self.state.get_reg(b).to_f64();
                self.state.set_reg(dst, RtVal::Num(va + vb));
            }
            Instruction::Sub { dst, a, b } => {
                let va = self.state.get_reg(a).to_f64();
                let vb = self.state.get_reg(b).to_f64();
                self.state.set_reg(dst, RtVal::Num(va - vb));
            }
            Instruction::Mul { dst, a, b } => {
                let va = self.state.get_reg(a).to_f64();
                let vb = self.state.get_reg(b).to_f64();
                self.state.set_reg(dst, RtVal::Num(va * vb));
            }
            Instruction::Div { dst, a, b } => {
                let va = self.state.get_reg(a).to_f64();
                let vb = self.state.get_reg(b).to_f64();
                let r  = if vb == 0.0 { 0.0 } else { va / vb };
                self.state.set_reg(dst, RtVal::Num(r));
            }
            Instruction::Mod { dst, a, b } => {
                let va = self.state.get_reg(a).to_f64() as i64;
                let vb = self.state.get_reg(b).to_f64() as i64;
                let r  = if vb == 0 { 0 } else { va % vb };
                self.state.set_reg(dst, RtVal::Num(r as f64));
            }
            Instruction::Neg { dst, src } => {
                let v = self.state.get_reg(src).to_f64();
                self.state.set_reg(dst, RtVal::Num(-v));
            }

            // ── Porównania ───────────────────────────────────────────────────
            Instruction::CmpEq { dst, a, b } => {
                let eq = self.state.get_reg(a).eq_val(self.state.get_reg(b));
                self.state.set_reg(dst, RtVal::Bool(eq));
            }
            Instruction::CmpNe { dst, a, b } => {
                let ne = !self.state.get_reg(a).eq_val(self.state.get_reg(b));
                self.state.set_reg(dst, RtVal::Bool(ne));
            }
            Instruction::CmpLt { dst, a, b } => {
                let r = self.state.get_reg(a).to_f64() < self.state.get_reg(b).to_f64();
                self.state.set_reg(dst, RtVal::Bool(r));
            }
            Instruction::CmpLe { dst, a, b } => {
                let r = self.state.get_reg(a).to_f64() <= self.state.get_reg(b).to_f64();
                self.state.set_reg(dst, RtVal::Bool(r));
            }
            Instruction::CmpGt { dst, a, b } => {
                let r = self.state.get_reg(a).to_f64() > self.state.get_reg(b).to_f64();
                self.state.set_reg(dst, RtVal::Bool(r));
            }
            Instruction::CmpGe { dst, a, b } => {
                let r = self.state.get_reg(a).to_f64() >= self.state.get_reg(b).to_f64();
                self.state.set_reg(dst, RtVal::Bool(r));
            }

            // ── Konwersje ────────────────────────────────────────────────────
            Instruction::ToString { dst, src } => {
                let s = self.state.get_reg(src).to_str();
                self.state.set_reg(dst, RtVal::Str(s));
            }
            Instruction::ToNumber { dst, src } => {
                let n = self.state.get_reg(src).to_f64();
                self.state.set_reg(dst, RtVal::Num(n));
            }
            Instruction::Truthy { dst, src } => {
                // Truthy check dla warunków while
                // Szczególny przypadek: src może być stringiem wyrażenia warunku
                let val = self.state.get_reg(src).clone();
                let result = match &val {
                    RtVal::Str(s) => {
                        // Ewaluuj wyrażenie porównania jeśli zawiera operator
                        eval_condition_str(s, &self.state)
                    }
                    other => other.is_truthy(),
                };
                self.state.set_reg(dst, RtVal::Bool(result));
            }

            // ── String concat ────────────────────────────────────────────────
            Instruction::Concat { dst, parts } => {
                let mut buf = String::new();
                for &r in &parts {
                    buf.push_str(&self.state.get_reg(r).to_str());
                }
                self.state.set_reg(dst, RtVal::Str(buf));
            }

            // ── Output ───────────────────────────────────────────────────────
            Instruction::Print { src } => {
                println!("{}", self.state.get_reg(src).to_str());
            }

            // ── Sterowanie ───────────────────────────────────────────────────
            Instruction::JumpIfFalse { cond, offset } => {
                if !self.state.get_reg(cond).is_truthy() {
                    return Ok(ExecSignal::Jump(offset));
                }
            }
            Instruction::JumpIfTrue { cond, offset } => {
                if self.state.get_reg(cond).is_truthy() {
                    return Ok(ExecSignal::Jump(offset));
                }
            }
            Instruction::Jump { offset } => {
                return Ok(ExecSignal::Jump(offset));
            }
            Instruction::Return { .. } => {
                return Ok(ExecSignal::Return);
            }

            // ── Wywołania funkcji ────────────────────────────────────────────
            Instruction::CallFunc { name } => {
                return Ok(ExecSignal::FuncCall(name));
            }

            Instruction::CallQuick { name, arg, dst } => {
                let arg_str = self.state.get_reg(arg).to_str();
                let name_str = self.const_str(name);
                let result = exec_quick_fn(&name_str, &arg_str);
                self.state.set_reg(dst, RtVal::Str(result));
            }

            // ── Komendy systemowe ────────────────────────────────────────────
            Instruction::ExecCmd { cmd, mode, dst } => {
                let cmd_str = self.state.get_reg(cmd).to_str();
                let exit_code = exec_system_cmd(&cmd_str, mode, false, &mut self.state)?;
                self.state.set_reg(dst, RtVal::Num(exit_code as f64));
                self.state.last_exit = exit_code;
                // Ustaw _last_exit_code przez name_idx 0 (zakładamy że 0 = "_last_exit_code")
                // W praktyce interpreter używa state.last_exit bezpośrednio
            }

            Instruction::ExecCapture { cmd, mode, dst_ec, dst_out } => {
                let cmd_str = self.state.get_reg(cmd).to_str();
                let (exit_code, stdout) = exec_system_cmd_capture(&cmd_str, mode)?;
                self.state.set_reg(dst_ec,  RtVal::Num(exit_code as f64));
                self.state.set_reg(dst_out, RtVal::Str(stdout));
                self.state.last_exit = exit_code;
            }

            // ── For-in ───────────────────────────────────────────────────────
            Instruction::ForInStart { iter_reg, src } => {
                let src_str = self.state.get_reg(src).to_str();
                let words: Vec<String> = src_str.split_whitespace()
                .map(|s| s.to_string())
                .collect();
                self.state.iters.insert(iter_reg, (words, 0));
            }

            Instruction::ForInNext { iter_reg, dst, end_off } => {
                let should_jump = if let Some((words, idx)) = self.state.iters.get_mut(&iter_reg) {
                    if *idx >= words.len() {
                        true
                    } else {
                        let word = words[*idx].clone();
                        *idx += 1;
                        self.state.set_reg(dst, RtVal::Str(word));
                        false
                    }
                } else {
                    true // Brak iteratora → skocz
                };
                if should_jump {
                    self.state.iters.remove(&iter_reg);
                    return Ok(ExecSignal::Jump(end_off));
                }
            }

            // ── HackerOS API ─────────────────────────────────────────────────
            Instruction::HackerOsCall { tool, args, dst } => {
                let tool_str = self.const_str(tool);
                let args_str = self.state.get_reg(args).to_str();
                if which::which(&tool_str).is_err() {
                    eprintln!("\x1b[33m[hl ||]\x1b[0m Narzędzie '{}' nie jest zainstalowane.", tool_str);
                    self.state.set_reg(dst, RtVal::Num(127.0));
                } else {
                    let cmd = if args_str.is_empty() {
                        tool_str.clone()
                    } else {
                        format!("{} {}", tool_str, args_str)
                    };
                    let ec = exec_system_cmd(&cmd, CmdMode::Plain, false, &mut self.state)?;
                    self.state.set_reg(dst, RtVal::Num(ec as f64));
                }
            }
        }

        Ok(ExecSignal::Next)
    }

    fn exec_func(&mut self, name_idx: u32) -> Result<()> {
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
        while pc < end {
            match self.exec_insn(pc)? {
                ExecSignal::Next              => pc += 1,
                ExecSignal::Jump(off)         => pc = off as usize,
                ExecSignal::Return            => break,
                ExecSignal::FuncCall(ni)      => { self.exec_func(ni)?; pc += 1; }
            }
        }

        self.state.call_depth -= 1;
        Ok(())
    }

    #[inline]
    fn const_str(&self, idx: u32) -> String {
        self.module.consts.strings.get(idx as usize)
        .cloned()
        .unwrap_or_default()
    }
}

#[derive(Debug)]
enum ExecSignal {
    Next,
    Jump(u32),
    Return,
    FuncCall(u32),
}

// ── Komendy systemowe ─────────────────────────────────────────────────────────

fn exec_system_cmd(
    cmd: &str,
    mode: CmdMode,
    _capture: bool,
    _state: &mut RuntimeState,
) -> Result<i32> {
    // Obsługa specjalnych prefixów generowanych przez lower.rs
    if cmd.starts_with("__hl_import__") {
        // Import pliku — wykonaj przez hl_core (uproszczenie)
        return Ok(0);
    }
    if cmd.starts_with("& ") {
        // Background
        let actual = &cmd[2..];
        let _ = Command::new("sh")
        .args(["-c", actual])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn();
        return Ok(0);
    }

    let (prog, args, extra) = build_cmd_parts(cmd, mode);
    let status = Command::new(&prog)
    .args(&args)
    .args(&extra)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status();

    match status {
        Ok(s)  => Ok(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("\x1b[31m[hl jit]\x1b[0m Błąd komendy: {}", e);
            Ok(1)
        }
    }
}

fn exec_system_cmd_capture(cmd: &str, mode: CmdMode) -> Result<(i32, String)> {
    let (prog, args, extra) = build_cmd_parts(cmd, mode);
    let out = Command::new(&prog)
    .args(&args)
    .args(&extra)
    .stdin(Stdio::inherit())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output();

    match out {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
            Ok((o.status.code().unwrap_or(1), stdout))
        }
        Err(e) => {
            eprintln!("\x1b[31m[hl jit]\x1b[0m Błąd przechwytywania: {}", e);
            Ok((1, String::new()))
        }
    }
}

fn build_cmd_parts(cmd: &str, mode: CmdMode) -> (String, Vec<String>, Vec<String>) {
    let trimmed = cmd.trim();
    // Sprawdź czy potrzeba powłoki
    let needs_sh = trimmed.contains('|')
    || trimmed.contains(';')
    || trimmed.contains('&')
    || trimmed.contains('>')
    || trimmed.contains('<')
    || trimmed.contains('$')
    || trimmed.contains('*')
    || trimmed.contains('`');

    match mode {
        CmdMode::Plain | CmdMode::WithVars => {
            if needs_sh {
                ("sh".into(), vec!["-c".into(), trimmed.into()], vec![])
            } else {
                let mut parts = split_cmd(trimmed);
                let prog = parts.remove(0);
                (prog, parts, vec![])
            }
        }
        CmdMode::Sudo | CmdMode::WithVarsSudo => {
            if needs_sh {
                ("sudo".into(), vec!["sh".into(), "-c".into(), trimmed.into()], vec![])
            } else {
                let parts = split_cmd(trimmed);
                ("sudo".into(), parts, vec![])
            }
        }
        CmdMode::Isolated | CmdMode::WithVarsIsolated => {
            let args = vec![
                "--mount".into(), "--pid".into(), "--net".into(),
                "--fork".into(), "--".into(), "sh".into(), "-c".into(), trimmed.into(),
            ];
            ("unshare".into(), args, vec![])
        }
        CmdMode::IsolatedSudo => {
            let args = vec![
                "unshare".into(),
                "--mount".into(), "--pid".into(), "--net".into(),
                "--fork".into(), "--".into(), "sh".into(), "-c".into(), trimmed.into(),
            ];
            ("sudo".into(), args, vec![])
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

// ── Quick functions (uproszczone — pełna lista w hl_core::quick) ─────────────

fn exec_quick_fn(name: &str, arg: &str) -> String {
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
        "basename" => std::path::Path::new(arg).file_name()
        .and_then(|n| n.to_str()).unwrap_or("").to_string(),
        "dirname"  => std::path::Path::new(arg).parent()
        .and_then(|p| p.to_str()).unwrap_or(".").to_string(),
        "pid"      => std::process::id().to_string(),
        "nl"       => { println!(); String::new() }
        "hr"       => {
            let w: usize = arg.parse().unwrap_or(60);
            println!("{}", "-".repeat(w));
            String::new()
        }
        "bold"   => { println!("\x1b[1m{}\x1b[0m", arg); String::new() }
        "red"    => { println!("\x1b[31m{}\x1b[0m", arg); String::new() }
        "green"  => { println!("\x1b[32m{}\x1b[0m", arg); String::new() }
        "yellow" => { println!("\x1b[33m{}\x1b[0m", arg); String::new() }
        "cyan"   => { println!("\x1b[36m{}\x1b[0m", arg); String::new() }
        "env"    => std::env::var(arg).unwrap_or_default(),
        "exists" => if std::path::Path::new(arg).exists() { "true".into() } else { "false".into() }
        "isdir"  => if std::path::Path::new(arg).is_dir() { "true".into() } else { "false".into() }
        "isfile" => if std::path::Path::new(arg).is_file() { "true".into() } else { "false".into() }
        "which"  => which::which(arg).map(|p| p.display().to_string()).unwrap_or_default(),
        "read"   => std::fs::read_to_string(arg).unwrap_or_default(),
        _        => {
            eprintln!("\x1b[31m[hl jit]\x1b[0m Nieznana quick-funkcja '::{}'", name);
            String::new()
        }
    }
}

// ── Ewaluacja warunków while ──────────────────────────────────────────────────

fn eval_condition_str(cond: &str, state: &RuntimeState) -> bool {
    let cond = cond.trim();
    if cond.is_empty() { return false; }
    if cond == "true"  { return true;  }
    if cond == "false" { return false; }

    const OPS: &[&str] = &["==", "!=", ">=", "<=", ">", "<"];
    for op in OPS {
        if let Some(pos) = find_op(cond, op) {
            let left_raw  = cond[..pos].trim();
            let right_raw = cond[pos + op.len()..].trim().trim_matches('"');

            let lv = if left_raw.starts_with('@') {
                // Pobierz z vars przez name (uproszczenie — szukamy po stringu)
                state.vars.values()
                .next()
                .map(|v| v.to_str())
                .unwrap_or_default()
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
    Command::new("sh")
    .args(["-c", cond])
    .status()
    .map(|s| s.success())
    .unwrap_or(false)
}

fn find_op(s: &str, op: &str) -> Option<usize> {
    let b = s.as_bytes();
    let op_b = op.as_bytes();
    let op_len = op_b.len();
    let mut i = 0;
    while i + op_len <= b.len() {
        if &b[i..i+op_len] == op_b {
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
