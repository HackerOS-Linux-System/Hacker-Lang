use std::process::{Command, Stdio};
use anyhow::{Result, bail};
use smallvec::SmallVec;
use tracing::debug;
use hl_parser::ast::*;
use crate::env::{Env, Value};
use crate::deps::resolve_dependency;
use crate::libs::resolve_import;
use crate::quick::exec_quick;

pub struct ExecResult {
    pub exit_code: i32,
    pub stdout:    Option<String>,
}

impl ExecResult {
    #[inline] pub fn ok()        -> Self { Self { exit_code: 0, stdout: None } }
    #[inline] pub fn err(n: i32) -> Self { Self { exit_code: n, stdout: None } }
    #[inline] pub fn is_ok(&self) -> bool { self.exit_code == 0 }
}

// ── Wbudowany exit ────────────────────────────────────────────────────────────
#[inline]
fn try_builtin_exit(cmd: &str) -> Option<i32> {
    let t = cmd.trim();
    if t == "exit" { return Some(0); }
    if let Some(rest) = t.strip_prefix("exit ") {
        return Some(rest.trim().parse::<i32>().unwrap_or(1));
    }
    None
}

// ── Sprawdz czy komenda wymaga powloki ────────────────────────────────────────
#[inline]
fn needs_shell(cmd: &str) -> bool {
    // Szybka sciezka - scan jednym przebiegiem
    let b = cmd.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'&' | b';' | b'`' => return true,
            b'|' if i + 1 < b.len() && b[i+1] != b'>' => return true,
            b'>' if i + 1 < b.len() && (b[i+1] == b'>' || b[i+1] == b' ') => return true,
            b'<' if i + 1 < b.len() && b[i+1] == b' ' => return true,
            b'$' if i + 1 < b.len() && b[i+1] == b'(' => return true,
            b'*' if i + 1 < b.len() => {
                // nie traktuj :// jako glob
                if i + 2 < b.len() && b[i+1] != b'/' { return true; }
            }
            _ => {}
        }
        i += 1;
    }
    // Zmienne srodowiskowe
    cmd.contains("$HOME") || cmd.contains("$USER") || cmd.contains("$PATH")
    || cmd.contains("$1") || cmd.contains("${")
}

// ── Podział na slowa (szybki, bez alokacji gdzie mozna) ──────────────────────
#[inline]
fn shell_words(s: &str) -> SmallVec<[String; 8]> {
    let mut words: SmallVec<[String; 8]> = SmallVec::new();
    let mut cur   = String::with_capacity(32);
    let (mut in_s, mut in_d) = (false, false);
    // Sladz czy bylismy w cudzyslowie (zeby zachowac puste stringy "")
    let mut had_quote = false;
    for c in s.chars() {
        match c {
            '\'' if !in_d => { in_s = !in_s; had_quote = true; }
            '"'  if !in_s => { in_d = !in_d; had_quote = true; }
            ' ' | '\t' if !in_s && !in_d => {
                // Zachowaj puste slowo jesli bylo w cudzyslowie (np. "" lub '')
                if !cur.is_empty() || had_quote {
                    words.push(std::mem::take(&mut cur));
                    cur = String::with_capacity(32);
                    had_quote = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() || had_quote { words.push(cur); }
    words
}

fn run_command(raw: &str, sudo: bool, isolated: bool, interpolate: bool, env: &mut Env, capture: bool) -> Result<ExecResult> {
    let expanded = if interpolate || raw.contains('@') {
        env.interpolate(raw)
    } else {
        raw.to_string()
    };
    let trimmed = expanded.trim();
    debug!("run: {}", trimmed);

    if let Some(code) = try_builtin_exit(trimmed) {
        std::process::exit(code);
    }

    if needs_shell(trimmed) { return run_via_shell(trimmed, sudo, isolated, capture); }
    let parts = shell_words(trimmed);
    if parts.is_empty() { return Ok(ExecResult::ok()); }
    build_and_run(parts, sudo, isolated, capture)
}

fn run_via_shell(cmd: &str, sudo: bool, isolated: bool, capture: bool) -> Result<ExecResult> {
    let (prog, args): (String, Vec<String>) = match (sudo, isolated) {
        (false, false) => ("sh".into(), vec!["-c".into(), cmd.into()]),
        (true,  false) => ("sudo".into(), vec!["sh".into(), "-c".into(), cmd.into()]),
        (false, true)  => ("unshare".into(), vec!["--mount".into(),"--pid".into(),"--net".into(),"--fork".into(),"--".into(),"sh".into(),"-c".into(),cmd.into()]),
        (true,  true)  => ("sudo".into(), vec!["unshare".into(),"--mount".into(),"--pid".into(),"--net".into(),"--fork".into(),"--".into(),"sh".into(),"-c".into(),cmd.into()]),
    };
    exec_process(prog, args, capture)
}

fn build_and_run(parts: SmallVec<[String; 8]>, sudo: bool, isolated: bool, capture: bool) -> Result<ExecResult> {
    let (prog, args): (String, Vec<String>) = match (sudo, isolated) {
        (false, false) => { let mut it = parts.into_iter(); let p = it.next().unwrap(); (p, it.collect()) }
        (true,  false) => ("sudo".into(), parts.into_iter().collect()),
        (false, true)  => { let mut a = vec!["--mount".into(),"--pid".into(),"--net".into(),"--fork".into(),"--".into()]; a.extend(parts); ("unshare".into(), a) }
        (true,  true)  => { let mut iso = vec!["--mount".into(),"--pid".into(),"--net".into(),"--fork".into(),"--".into()]; iso.extend(parts); let mut a = vec!["unshare".into()]; a.extend(iso); ("sudo".into(), a) }
    };
    exec_process(prog, args, capture)
}

fn exec_process(prog: String, args: Vec<String>, capture: bool) -> Result<ExecResult> {
    let mut cmd = Command::new(&prog);
    cmd.args(&args).stdin(Stdio::inherit());
    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let out = cmd.output()?;
        return Ok(ExecResult { exit_code: out.status.code().unwrap_or(1), stdout: Some(String::from_utf8_lossy(&out.stdout).into_owned()) });
    }
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    Ok(ExecResult { exit_code: cmd.status()?.code().unwrap_or(1), stdout: None })
}

fn resolve_export_value(val: &ExportValue, env: &mut Env) -> String {
    match val {
        ExportValue::Single(parts) => env.resolve_string_parts(parts),
        ExportValue::List(items)   => items.iter().map(|p| env.resolve_string_parts(p)).collect::<Vec<_>>().join(":"),
    }
}

// ── Glowna petla wykonania ─────────────────────────────────────────────────────
// Iteracyjna zamiast rekurencyjnej - eliminuje przepelnienie stosu dla duzych skryptow
pub fn exec_nodes(nodes: &[Node], env: &mut Env) -> Result<ExecResult> {
    let mut last = ExecResult::ok();
    for node in nodes {
        let r = exec_node(node, env)?;
        env.last_exit = r.exit_code;
        last = r;
    }
    Ok(last)
}

pub fn exec_node(node: &Node, env: &mut Env) -> Result<ExecResult> {
    match node {
        Node::LineComment(_) | Node::DocComment(_) | Node::BlockComment(_) => Ok(ExecResult::ok()),

        Node::Print { parts } => {
            // Szybka sciezka dla literalow bez interpolacji
            let has_vars = parts.iter().any(|p| matches!(p, StringPart::Var(_)));
            if has_vars {
                println!("{}", env.resolve_string_parts(parts));
            } else {
                // Tylko literaly - laczymy bez alokacji posrednich
                let total: usize = parts.iter().map(|p| if let StringPart::Literal(s) = p { s.len() } else { 0 }).sum();
                let mut out = String::with_capacity(total);
                for p in parts { if let StringPart::Literal(s) = p { out.push_str(s); } }
                println!("{}", out);
            }
            Ok(ExecResult::ok())
        }

        Node::QuickCall { name, args } => exec_quick(name, args, env),

        Node::Command { raw, mode, .. } => {
            let trimmed = raw.trim();
            if let Some(code) = try_builtin_exit(trimmed) { std::process::exit(code); }
            if trimmed.starts_with("echo ") || trimmed == "echo" {
                bail!("'echo' jest zabroniony. Uzyj '~>'.");
            }
            let (sudo, isolated, interpolate) = match mode {
                CommandMode::Plain            => (false, false, false),
                CommandMode::Sudo             => (true,  false, false),
                CommandMode::Isolated         => (false, true,  false),
                CommandMode::IsolatedSudo     => (true,  true,  false),
                CommandMode::WithVars         => (false, false, true),
                CommandMode::WithVarsSudo     => (true,  false, true),
                CommandMode::WithVarsIsolated => (false, true,  true),
            };
            run_command(raw, sudo, isolated, interpolate, env, false)
        }

        Node::HshCommand { raw } => {
            let expanded = env.interpolate(raw);
            let status = Command::new("hsh")
            .args(["-c", expanded.trim()])
            .stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
            .status()
            .map_err(|e| anyhow::anyhow!("hsh nie znaleziony: {}", e))?;
            Ok(ExecResult { exit_code: status.code().unwrap_or(1), stdout: None })
        }

        Node::Background { raw } => {
            let expanded = env.interpolate(raw);
            let child = Command::new("sh")
            .args(["-c", expanded.trim()])
            .stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Blad tla: {}", e))?;
            env.set_var("_bg_pid", Value::Number(child.id() as f64));
            eprintln!("\x1b[90m[hl &] PID={}\x1b[0m", child.id());
            std::mem::forget(child);
            Ok(ExecResult::ok())
        }

        Node::RepeatN { count, body } => {
            let mut last = ExecResult::ok();
            for _ in 0..*count {
                last = exec_nodes(body, env)?;
                env.last_exit = last.exit_code;
            }
            Ok(last)
        }

        Node::FileImport { path, detail } => {
            let expanded = env.interpolate(path);
            if !std::path::Path::new(&expanded).exists() {
                bail!("Import: plik nie istnieje: '{}'", expanded);
            }
            let src = std::fs::read_to_string(&expanded)?;
            if let Some(d) = detail { env.set_var("_import_detail", Value::String(d.clone())); }
            exec_nodes(&hl_parser::parse_source(&src)?, env)
        }

        Node::Goroutine { name, body } => {
            let body_clone = body.clone();
            let name_str   = name.clone().unwrap_or_else(|| "<goroutine>".to_string());
            let mut thread_env = Env::new();
            for (k, v) in &env.vars { thread_env.vars.insert(k.clone(), v.clone()); }
            std::thread::spawn(move || { let _ = exec_nodes(&body_clone, &mut thread_env); });
            eprintln!("\x1b[35m[hl :*] goroutine '{}' uruchomiona\x1b[0m", name_str);
            Ok(ExecResult::ok())
        }

        Node::Channel { name } => {
            env.set_var(&format!("__chan_{}", name), Value::String(String::new()));
            Ok(ExecResult::ok())
        }

        Node::ChannelOp { name, value } => {
            let chan_var = format!("__chan_{}", name);
            if let Some(parts) = value {
                let resolved = env.resolve_string_parts(parts);
                env.set_var(&chan_var, Value::String(resolved));
            } else {
                println!("{}", env.get_var(&chan_var).to_string_val());
            }
            Ok(ExecResult::ok())
        }

        Node::VarDecl { name, typ: _typ, value } => {
            let val = eval_var_value(value, env)?;
            env.set_var(name, val);
            Ok(ExecResult::ok())
        }

        Node::Export { name, value } => {
            let resolved = resolve_export_value(value, env);
            std::env::set_var(name, &resolved);
            env.set_var(name, Value::String(resolved));
            Ok(ExecResult::ok())
        }

        Node::VarRef(name) => {
            println!("{}", env.get_var(name).to_string_val());
            Ok(ExecResult::ok())
        }

        Node::Dependency { name } => {
            match resolve_dependency(name) {
                Ok(r)  => Ok(if r.is_available() { ExecResult::ok() } else { ExecResult::err(1) }),
                Err(e) => { eprintln!("\x1b[31m[hl dep]\x1b[0m {}", e); Ok(ExecResult::err(1)) }
            }
        }

        Node::Import { lib, detail } => {
            resolve_import(lib, detail.as_deref(), env)?;
            Ok(ExecResult::ok())
        }

        Node::FuncDef { name, body } => {
            env.define_function(name.clone(), body.clone());
            Ok(ExecResult::ok())
        }

        Node::FuncCall { name } => {
            match env.get_function(name) {
                Some(body) => exec_nodes(&body, env),
                None       => bail!("Niezdefiniowana funkcja: '{}'", name),
            }
        }

        Node::Conditional { condition, body } => {
            let run = match condition {
                ConditionKind::Ok  => env.last_exit == 0,
                ConditionKind::Err => env.last_exit != 0,
            };
            if run { exec_nodes(body, env) } else { Ok(ExecResult::ok()) }
        }

        Node::ForIn { var, iterable, body } => {
            let iter_str = env.resolve_string_parts(iterable);
            let mut last = ExecResult::ok();
            for item in iter_str.split_whitespace() {
                env.set_var(var, Value::String(item.to_string()));
                last = exec_nodes(body, env)?;
                env.last_exit = last.exit_code;
            }
            Ok(last)
        }

        Node::WhileLoop { condition, body } => {
            let mut iterations = 0usize;
            const MAX_ITER: usize = 1_000_000;
            loop {
                if iterations >= MAX_ITER {
                    bail!("Petla while: przekroczono limit {} iteracji", MAX_ITER);
                }
                iterations += 1;
                let cond_str = env.resolve_string_parts(condition);
                if !eval_condition_fast(&cond_str, env)? { break; }
                let r = exec_nodes(body, env)?;
                env.last_exit = r.exit_code;
            }
            Ok(ExecResult::ok())
        }

        Node::MatchExpr { subject, arms } => {
            let subj = env.resolve_string_parts(subject);
            let mut matched = false;
            let mut wildcard_idx = None;
            for (i, arm) in arms.iter().enumerate() {
                let pattern = arm.pattern.trim();
                if pattern == "*" { wildcard_idx = Some(i); continue; }
                if pattern == subj || env.interpolate(pattern) == subj {
                    exec_nodes(&arm.body, env)?;
                    matched = true;
                    break;
                }
            }
            if !matched {
                if let Some(idx) = wildcard_idx {
                    exec_nodes(&arms[idx].body, env)?;
                }
            }
            Ok(ExecResult::ok())
        }

        Node::Arithmetic { expr, assign_to } => {
            let expanded = env.interpolate(expr);
            let result   = eval_arithmetic_fast(&expanded);
            let result = result.unwrap_or_else(|| eval_arithmetic_shell(&expanded));
            if let Some(var) = assign_to {
                env.set_var(var, Value::String(result.clone()));
            } else {
                println!("{}", result);
            }
            Ok(ExecResult::ok())
        }

        Node::PipeToVar { command, mode, var_name } => {
            let interpolate = true; // PipeToVar zawsze interpoluje
            let sudo     = matches!(mode, CommandMode::Sudo | CommandMode::IsolatedSudo | CommandMode::WithVarsSudo);
            let isolated = matches!(mode, CommandMode::Isolated | CommandMode::IsolatedSudo | CommandMode::WithVarsIsolated);
            let r = run_command(command, sudo, isolated, interpolate, env, true)?;
            let output = r.stdout.unwrap_or_default().trim().to_string();
            env.set_var(var_name, Value::String(output));
            Ok(ExecResult { exit_code: r.exit_code, stdout: None })
        }

        Node::HackerOsApi { tool, args } => {
            let bin = tool.binary_name();
            let args_str = env.resolve_string_parts(args);
            let args_str = args_str.trim();
            if which::which(bin).is_err() {
                eprintln!("\x1b[33m[hl ||]\x1b[0m Narzedzie '{}' nie jest zainstalowane.", bin);
                return Ok(ExecResult::err(127));
            }
            let cmd = if args_str.is_empty() { bin.to_string() } else { format!("{} {}", bin, args_str) };
            run_command(&cmd, false, false, false, env, false)
        }

        Node::Block(nodes) => exec_nodes(nodes, env),
    }
}

// ── Ewaluacja wartosci zmiennej ───────────────────────────────────────────────
fn eval_var_value(value: &VarValue, env: &mut Env) -> Result<Value> {
    Ok(match value {
        VarValue::String(s)       => Value::String(s.clone()),
       VarValue::Int(n)          => Value::Number(*n as f64),
       VarValue::Float(n)        => Value::Number(*n),
       VarValue::Number(n)       => Value::Number(*n),
       VarValue::Bool(b)         => Value::Bool(*b),
       VarValue::Interpolated(p) => Value::String(env.resolve_string_parts(p)),
       VarValue::CmdOutput(cmd)  => {
           let r = run_command(cmd, false, false, true, env, true)?;
           Value::String(r.stdout.unwrap_or_default().trim().to_string())
       }
       VarValue::Arithmetic(expr) => {
           let expanded = env.interpolate(expr);
           let result = eval_arithmetic_fast(&expanded)
           .unwrap_or_else(|| eval_arithmetic_shell(&expanded));
           Value::String(result)
       }
       VarValue::List(items) => {
           Value::List(items.iter().map(|v| match v {
               VarValue::String(s) => Value::String(s.clone()),
                                        VarValue::Int(n)    => Value::Number(*n as f64),
                                        VarValue::Float(n)  => Value::Number(*n),
                                        VarValue::Number(n) => Value::Number(*n),
                                        VarValue::Bool(b)   => Value::Bool(*b),
                                        _                   => Value::Nil,
           }).collect())
       }
       VarValue::Map(_) => Value::String(String::new()),
    })
}

// ── NATYWNA ARYTMETYKA (bez powloki) — GLOWNA OPTYMALIZACJA ──────────────────
//
// Obsluguje: +, -, *, /, %, ** z nawiasami i zmiennymi @var
// Zwraca None jesli wyrazenie zbyt skomplikowane (fallback do sh)
pub fn eval_arithmetic_fast(expr: &str) -> Option<String> {
    let e = expr.trim();
    if e.is_empty() { return Some("0".to_string()); }

    // Prosta ewaluacja przez wbudowany parser
    match eval_expr(e) {
        Some(v) => {
            if v.fract() == 0.0 && v.abs() < 1e15 {
                Some(format!("{}", v as i64))
            } else {
                Some(format!("{}", v))
            }
        }
        None => None,
    }
}

// Rekurencyjny descent parser dla wyrazen arytmetycznych
// Obsluguje: +, -, *, /, %, nawiasy, liczby, zmienne juz rozwiazane
fn eval_expr(s: &str) -> Option<f64> {
    let s = s.trim();
    eval_additive(s, &mut 0)
}

fn eval_additive(s: &str, _: &mut usize) -> Option<f64> {
    let bytes = s.as_bytes();
    // Znajdz ostatni + lub - poza nawiasami (dla lewostronnosci)
    let mut depth = 0i32;
    let mut last_add = None;
    let mut last_sub = None;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            b'+' if depth == 0 && i > 0 => { last_add = Some(i); break; }
            b'-' if depth == 0 && i > 0 && bytes[i-1] != b'*' && bytes[i-1] != b'/' => {
                last_sub = Some(i); break;
            }
            _ => {}
        }
    }

    // Wybierz ostatni operator + lub -
    let split_at = match (last_add, last_sub) {
        (Some(a), Some(b)) => Some(if a > b { (a, '+') } else { (b, '-') }),
        (Some(a), None)    => Some((a, '+')),
        (None, Some(b))    => Some((b, '-')),
        _                  => None,
    };

    if let Some((pos, op)) = split_at {
        let left  = eval_multiplicative(s[..pos].trim())?;
        let right = eval_multiplicative(s[pos+1..].trim())?;
        return Some(if op == '+' { left + right } else { left - right });
    }

    eval_multiplicative(s)
}

fn eval_multiplicative(s: &str) -> Option<f64> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut split = None;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            b'*' if depth == 0 && i + 1 < bytes.len() && bytes[i+1] != b'*' => {
                split = Some((i, '*')); break;
            }
            b'/' if depth == 0 => { split = Some((i, '/')); break; }
            b'%' if depth == 0 => { split = Some((i, '%')); break; }
            _ => {}
        }
    }

    if let Some((pos, op)) = split {
        let left  = eval_unary(s[..pos].trim())?;
        let right = eval_unary(s[pos+1..].trim())?;
        return Some(match op {
            '*' => left * right,
            '/' => if right == 0.0 { 0.0 } else { left / right },
            '%' => if right == 0.0 { 0.0 } else { (left as i64 % right as i64) as f64 },
                    _   => 0.0,
        });
    }

    eval_unary(s)
}

fn eval_unary(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.starts_with('-') {
        return Some(-eval_atom(s[1..].trim())?);
    }
    eval_atom(s)
}

fn eval_atom(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() { return None; }

    // Nawiasy
    if s.starts_with('(') && s.ends_with(')') {
        return eval_expr(&s[1..s.len()-1]);
    }

    // Liczba
    if let Ok(n) = s.parse::<f64>() { return Some(n); }

    None
}

// Fallback do powloki gdy wyrazenie zbyt skomplikowane
fn eval_arithmetic_shell(expr: &str) -> String {
    let sh_expr = format!("echo $(( {} ))", expr);
    if let Ok(out) = Command::new("sh").args(["-c", &sh_expr]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() && s != "0" || expr.trim() == "0" { return s; }
        }
    }
    "0".to_string()
}

// ── WARUNEK WHILE — NAPRAWIONY ────────────────────────────────────────────────
//
// KLUCZOWA POPRAWKA: operatory wieloznakowe (>=, <=, ==, !=)
// muszą byc sprawdzone PRZED jednoznakowymi (>, <)
// Poprzednia implementacja: find(">") trafialo na ">" w ">=" -> right = "= 3" -> bug!
fn eval_condition_fast(cond: &str, env: &mut Env) -> Result<bool> {
    let cond = env.interpolate(cond);
    let cond = cond.trim();

    if cond.is_empty() { return Ok(false); }
    if cond == "true"  { return Ok(true);  }
    if cond == "false" { return Ok(false); }

    // KOLEJNOSC: najpierw dwuznakowe, potem jednoznakowe!
    // Unikamy: find(">") trafiajacego na ">" z ">="
    const OPS: &[&str] = &["==", "!=", ">=", "<=", ">", "<"];

    for op in OPS {
        // Szukaj operatora NIE bedacego czescia dluzszego operatora
        if let Some(pos) = find_operator(cond, op) {
            let left_raw  = cond[..pos].trim();
            let right_raw = cond[pos + op.len()..].trim().trim_matches('"');

            let lv = if left_raw.starts_with('@') {
                env.get_var(&left_raw[1..]).to_string_val()
            } else {
                left_raw.to_string()
            };

            return Ok(match *op {
                "==" => lv == right_raw,
                "!=" => lv != right_raw,
                ">=" => lv.parse::<f64>().unwrap_or(0.0) >= right_raw.parse::<f64>().unwrap_or(0.0),
                      "<=" => lv.parse::<f64>().unwrap_or(0.0) <= right_raw.parse::<f64>().unwrap_or(0.0),
                      ">"  => lv.parse::<f64>().unwrap_or(0.0) >  right_raw.parse::<f64>().unwrap_or(0.0),
                      "<"  => lv.parse::<f64>().unwrap_or(0.0) <  right_raw.parse::<f64>().unwrap_or(0.0),
                      _    => false,
            });
        }
    }

    // @var — truthy check
    if cond.starts_with('@') {
        let val = env.get_var(&cond[1..]).to_string_val();
        return Ok(!val.is_empty() && val != "false" && val != "0");
    }

    // Fallback: komenda shell
    Ok(Command::new("sh").args(["-c", cond]).status().map(|s| s.success()).unwrap_or(false))
}

/// Znajdz operator w stringu, ale nie jako czesc dluzszego operatora
/// Np. szukajac ">" w "@a >= 3" NIE trafia na ">" z ">="
fn find_operator(s: &str, op: &str) -> Option<usize> {
    let b = s.as_bytes();
    let op_b = op.as_bytes();
    let op_len = op_b.len();
    let mut i = 0;
    while i + op_len <= b.len() {
        if &b[i..i+op_len] == op_b {
            // Sprawdz czy nie jest czescia dluzszego operatora
            let ok = match op {
                ">" => i + 1 >= b.len() || b[i+1] != b'=',
                "<" => i + 1 >= b.len() || b[i+1] != b'=',
                _   => true, // dla ==, !=, >=, <= nie ma dluzszych wariantow
            };
            if ok { return Some(i); }
        }
        i += 1;
    }
    None
}
