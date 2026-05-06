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

#[inline]
fn needs_shell(cmd: &str) -> bool {
    cmd.contains("&&") || cmd.contains("||")
    || cmd.contains(" | ") || cmd.starts_with("| ")
    || cmd.contains(';')
    || cmd.contains(" > ") || cmd.contains(" >> ") || cmd.contains(" < ")
    || cmd.contains('`') || cmd.contains("$(")
    || cmd.contains("$HOME") || cmd.contains("$USER") || cmd.contains("$PATH")
    || cmd.contains("$1") || cmd.contains("$2") || cmd.contains("${")
    || (cmd.contains('*') && !cmd.contains("://"))
}

#[inline]
fn shell_words(s: &str) -> SmallVec<[String; 8]> {
    let mut words: SmallVec<[String; 8]> = SmallVec::new();
    let mut cur  = String::with_capacity(32);
    let (mut in_s, mut in_d) = (false, false);
    for c in s.chars() {
        match c {
            '\'' if !in_d => in_s = !in_s,
            '"'  if !in_s => in_d = !in_d,
            ' ' | '\t' if !in_s && !in_d => {
                if !cur.is_empty() { words.push(std::mem::take(&mut cur)); cur = String::with_capacity(32); }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { words.push(cur); }
    words
}

fn run_command(raw: &str, sudo: bool, isolated: bool, env: &mut Env, capture: bool) -> Result<ExecResult> {
    let expanded = env.interpolate(raw);
    let trimmed  = expanded.trim();
    debug!("run: {}", trimmed);
    if needs_shell(trimmed) { return run_via_shell(trimmed, sudo, isolated, capture); }
    let parts = shell_words(trimmed);
    if parts.is_empty() { return Ok(ExecResult::ok()); }
    build_and_run(parts, sudo, isolated, capture)
}

fn run_via_shell(cmd: &str, sudo: bool, isolated: bool, capture: bool) -> Result<ExecResult> {
    let sh_args  = vec!["-c".to_string(), cmd.to_string()];
    let iso_args = vec!["--mount".to_string(),"--pid".into(),"--net".into(),"--fork".into(),"--".into(),"sh".into(),"-c".into(),cmd.to_string()];
    let (prog, args): (String, Vec<String>) = match (sudo, isolated) {
        (false, false) => ("sh".into(), sh_args),
        (true,  false) => ("sudo".into(), { let mut a = vec!["sh".into(),"-c".into()]; a.push(cmd.to_string()); a }),
        (false, true)  => ("unshare".into(), iso_args),
        (true,  true)  => ("sudo".into(), { let mut a = vec!["unshare".into()]; a.extend(iso_args); a }),
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
        let out  = cmd.output()?;
        let code = out.status.code().unwrap_or(1);
        return Ok(ExecResult { exit_code: code, stdout: Some(String::from_utf8_lossy(&out.stdout).into_owned()) });
    }
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    let code = cmd.status()?.code().unwrap_or(1);
    Ok(ExecResult { exit_code: code, stdout: None })
}

fn resolve_export_value(val: &ExportValue, env: &mut Env) -> String {
    match val {
        ExportValue::Single(parts) => env.resolve_string_parts(parts),
        ExportValue::List(items)   => items.iter().map(|p| env.resolve_string_parts(p)).collect::<Vec<_>>().join(":"),
    }
}

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
            let msg = env.resolve_string_parts(parts);
            println!("{}", msg);
            Ok(ExecResult::ok())
        }

        Node::QuickCall { name, args } => exec_quick(name, args, env),

        Node::Command { raw, mode, .. } => {
            let trimmed = raw.trim();
            if trimmed.starts_with("echo ") || trimmed == "echo" {
                bail!("'echo' jest zabroniony. Uzyj '~>'.");
            }
            let (sudo, isolated) = match mode {
                CommandMode::Plain            => (false, false),
                CommandMode::Sudo             => (true,  false),
                CommandMode::Isolated         => (false, true),
                CommandMode::IsolatedSudo     => (true,  true),
                CommandMode::WithVars         => (false, false),
                CommandMode::WithVarsSudo     => (true,  false),
                CommandMode::WithVarsIsolated => (false, true),
            };
            run_command(raw, sudo, isolated, env, false)
        }

        Node::HshCommand { raw } => {
            let cmd = raw.trim();
            let status = Command::new("hsh")
                .args(["-c", cmd])
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
            if let Some(d) = detail {
                env.set_var("_import_detail", Value::String(d.clone()));
            }
            let nodes = hl_parser::parse_source(&src)?;
            exec_nodes(&nodes, env)
        }

        Node::Goroutine { name, body } => {
            let body_clone = body.clone();
            let name_str   = name.clone().unwrap_or_else(|| "<goroutine>".to_string());
            let mut thread_env = Env::new();
            for (k, v) in &env.vars { thread_env.vars.insert(k.clone(), v.clone()); }
            std::thread::spawn(move || {
                let _ = exec_nodes(&body_clone, &mut thread_env);
            });
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
                let val = env.resolve_string_parts(parts);
                env.set_var(&chan_var, Value::String(val));
            } else {
                println!("{}", env.get_var(&chan_var).to_string_val());
            }
            Ok(ExecResult::ok())
        }

        // Gen 2 — typowane zmienne
        Node::VarDecl { name, typ: _typ, value } => {
            let val = match value {
                VarValue::String(s)       => Value::String(s.clone()),
                VarValue::Int(n)          => Value::Number(*n as f64),
                VarValue::Float(n)        => Value::Number(*n),
                VarValue::Number(n)       => Value::Number(*n),
                VarValue::Bool(b)         => Value::Bool(*b),
                VarValue::Interpolated(p) => Value::String(env.resolve_string_parts(p)),
                VarValue::CmdOutput(cmd)  => {
                    let r = run_command(cmd, false, false, env, true)?;
                    Value::String(r.stdout.unwrap_or_default().trim().to_string())
                }
                VarValue::Arithmetic(expr) => {
                    let expanded = env.interpolate(expr);
                    let result   = eval_arithmetic(&expanded, env)?;
                    Value::String(result)
                }
                VarValue::List(items) => {
                    let parts: Vec<Value> = items.iter().map(|v| match v {
                        VarValue::String(s)  => Value::String(s.clone()),
                        VarValue::Int(n)     => Value::Number(*n as f64),
                        VarValue::Float(n)   => Value::Number(*n),
                        VarValue::Number(n)  => Value::Number(*n),
                        VarValue::Bool(b)    => Value::Bool(*b),
                        _                    => Value::Nil,
                    }).collect();
                    Value::List(parts)
                }
                VarValue::Map(_) => Value::String(String::new()),
            };
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

        // Gen 2 — for @var in lista
        Node::ForIn { var, iterable, body } => {
            let iter_str = env.resolve_string_parts(iterable);
            let items: Vec<&str> = iter_str.split_whitespace().collect();
            let mut last = ExecResult::ok();
            for item in items {
                env.set_var(var, Value::String(item.to_string()));
                last = exec_nodes(body, env)?;
                env.last_exit = last.exit_code;
            }
            Ok(last)
        }

        // Gen 2 — ?~ warunek (while)
        Node::WhileLoop { condition, body } => {
            let mut iterations = 0usize;
            const MAX_ITER: usize = 100_000;
            loop {
                if iterations >= MAX_ITER {
                    bail!("Petla while: przekroczono limit {} iteracji", MAX_ITER);
                }
                iterations += 1;

                let cond_str = env.resolve_string_parts(condition);
                let cond_result = eval_condition(&cond_str, env)?;
                if !cond_result { break; }

                let r = exec_nodes(body, env)?;
                env.last_exit = r.exit_code;
            }
            Ok(ExecResult::ok())
        }

        // Gen 2 — ? switch @var
        Node::MatchExpr { subject, arms } => {
            let subj = env.resolve_string_parts(subject);
            let mut matched = false;
            for arm in arms {
                let pattern = arm.pattern.trim();
                let matches = pattern == "*" || pattern == subj
                    || env.interpolate(pattern) == subj;
                if matches || (!matched && pattern == "*") {
                    exec_nodes(&arm.body, env)?;
                    matched = true;
                    if pattern != "*" { break; }
                }
            }
            Ok(ExecResult::ok())
        }

        // Gen 2 — $( expr ) -> @var
        Node::Arithmetic { expr, assign_to } => {
            let expanded = env.interpolate(expr);
            let result   = eval_arithmetic(&expanded, env)?;
            if let Some(var) = assign_to {
                env.set_var(var, Value::String(result.clone()));
            }
            println!("{}", result);
            Ok(ExecResult::ok())
        }

        // Gen 2 — > cmd |> @var
        Node::PipeToVar { command, mode, var_name } => {
            let sudo = matches!(mode, CommandMode::Sudo | CommandMode::IsolatedSudo | CommandMode::WithVarsSudo);
            let r = run_command(command, sudo, false, env, true)?;
            let output = r.stdout.unwrap_or_default().trim().to_string();
            env.set_var(var_name, Value::String(output));
            Ok(ExecResult { exit_code: r.exit_code, stdout: None })
        }

        // Gen 2 — || tool args (HackerOS API)
        Node::HackerOsApi { tool, args } => {
            let bin = tool.binary_name();
            let args_str = env.resolve_string_parts(args);
            let args_str = args_str.trim();

            // Sprawdz czy narzedzie jest dostepne
            if which::which(bin).is_err() {
                eprintln!("\x1b[33m[hl ||]\x1b[0m Narzedzie '{}' nie jest zainstalowane.", bin);
                eprintln!("        Zainstaluj przez: hpkg install {} lub lpm install {}", bin, bin);
                return Ok(ExecResult::err(127));
            }

            debug!("hackeros-api: {} {}", bin, args_str);

            let r = if args_str.is_empty() {
                run_command(bin, false, false, env, false)
            } else {
                run_command(&format!("{} {}", bin, args_str), false, false, env, false)
            }?;
            Ok(r)
        }

        Node::Block(nodes) => exec_nodes(nodes, env),
    }
}

// ── Ewaluacja arytmetyki (gen 2) ─────────────────────────────────────────────
// Uzywa sh -c "echo $(( expr ))" jako prosty backend
fn eval_arithmetic(expr: &str, env: &mut Env) -> Result<String> {
    let expanded = env.interpolate(expr);
    // Zastap zmienne HL (@var) ich wartosciami
    let expr_clean = expanded.trim();

    // Uzyj powloki do obliczen
    let sh_expr = format!("echo $(( {} ))", expr_clean);
    let out = Command::new("sh")
        .args(["-c", &sh_expr])
        .output()
        .map_err(|e| anyhow::anyhow!("Arytmetyka: {}", e))?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        // Fallback: sprobuj jako float przez python3
        let py_expr = format!("python3 -c \"print({})\"", expr_clean);
        let out2 = Command::new("sh").args(["-c", &py_expr]).output();
        match out2 {
            Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
            _ => bail!("Nie mozna obliczyc wyrazenia: {}", expr_clean),
        }
    }
}

// ── Ewaluacja warunku while (gen 2) ──────────────────────────────────────────
// Obsluguje: @var == val, @var != val, @var > val, @var < val,
//            @var >= val, @var <= val, ::exists @path, exit 0, itp.
fn eval_condition(cond: &str, env: &mut Env) -> Result<bool> {
    let cond = env.interpolate(cond);
    let cond = cond.trim();

    // Pusty warunek = false
    if cond.is_empty() { return Ok(false); }

    // == / != / < / > / >= / <=
    for op in &["==", "!=", ">=", "<=", ">", "<"] {
        if let Some(pos) = cond.find(op) {
            let left  = cond[..pos].trim();
            let right = cond[pos+op.len()..].trim();
            let lv = env.get_var(left.trim_start_matches('@')).to_string_val();
            let rv = right.trim_matches('"').to_string();
            return Ok(match *op {
                "==" => lv == rv,
                "!=" => lv != rv,
                ">=" => lv.parse::<f64>().unwrap_or(0.0) >= rv.parse::<f64>().unwrap_or(0.0),
                "<=" => lv.parse::<f64>().unwrap_or(0.0) <= rv.parse::<f64>().unwrap_or(0.0),
                ">"  => lv.parse::<f64>().unwrap_or(0.0) >  rv.parse::<f64>().unwrap_or(0.0),
                "<"  => lv.parse::<f64>().unwrap_or(0.0) <  rv.parse::<f64>().unwrap_or(0.0),
                _    => false,
            });
        }
    }

    // true / false
    if cond == "true"  { return Ok(true);  }
    if cond == "false" { return Ok(false); }

    // @var (truthy check: niepusty i != "false" i != "0")
    if cond.starts_with('@') {
        let val = env.get_var(&cond[1..]).to_string_val();
        return Ok(!val.is_empty() && val != "false" && val != "0");
    }

    // Fallback: uruchom jako komende przez sh, sprawdz exit code
    let out = Command::new("sh").args(["-c", cond]).status();
    match out {
        Ok(s) => Ok(s.success()),
        Err(_) => Ok(false),
    }
}
