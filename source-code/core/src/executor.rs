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
        ExportValue::List(items) => items.iter().map(|p| env.resolve_string_parts(p)).collect::<Vec<_>>().join(":"),
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
                bail!("Operator 'echo' jest zabroniony w Hacker Lang.\nUzyj '~> {}' zamiast '> echo {}'",
                    trimmed.trim_start_matches("echo").trim(), trimmed.trim_start_matches("echo").trim());
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

        // *> komenda — uruchom przez hsh -c "komenda"
        Node::HshCommand { raw } => {
            let cmd = raw.trim();
            debug!("hsh: {}", cmd);
            let status = Command::new("hsh")
                .args(["-c", cmd])
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .map_err(|e| anyhow::anyhow!("Nie mozna uruchomic hsh: {}\nUpewnij sie ze hsh jest zainstalowany na HackerOS.", e))?;
            let code = status.code().unwrap_or(1);
            Ok(ExecResult { exit_code: code, stdout: None })
        }

        // & komenda — uruchom w tle (nie czekaj na zakonczenie)
        Node::Background { raw } => {
            let expanded = env.interpolate(raw);
            let trimmed  = expanded.trim().to_string();
            debug!("background: {}", trimmed);

            // Zawsze przez sh -c aby obsluc przekierowania i potoki
            let child = Command::new("sh")
                .args(["-c", &trimmed])
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| anyhow::anyhow!("Nie mozna uruchomic w tle: {}", e))?;

            // Zapisz PID do zmiennej _bg_pid (dostepne przez @_bg_pid)
            env.set_var("_bg_pid", Value::Number(child.id() as f64));
            eprintln!("\x1b[90m[hl &] PID={}\x1b[0m", child.id());

            // Nie czekamy — proces dziala w tle
            // Zapobiegamy zombie przez detach (w pelnej impl. uzyj prctl/double-fork)
            std::mem::forget(child);
            Ok(ExecResult::ok())
        }

        // _N > cmd — powtorz N razy
        Node::RepeatN { count, body } => {
            let mut last = ExecResult::ok();
            for _ in 0..*count {
                last = exec_nodes(body, env)?;
                env.last_exit = last.exit_code;
            }
            Ok(last)
        }

        // << plik.hl — import zewnetrznego pliku .hl
        Node::FileImport { path, detail } => {
            let expanded_path = env.interpolate(path);
            let file_path = std::path::Path::new(&expanded_path);
            if !file_path.exists() {
                bail!("Plik do importu nie istnieje: '{}'", expanded_path);
            }
            let src = std::fs::read_to_string(file_path)?;
            let nodes = hl_parser::parse_source(&src)?;

            // Jezeli sa szczegoly — ustaw jako zmienna przed wykonaniem
            if let Some(d) = detail {
                env.set_var("_import_detail", Value::String(d.clone()));
            }
            exec_nodes(&nodes, env)
        }

        // :* blok done — goroutine (uproszczone: wykonaj w osobnym watku)
        Node::Goroutine { body } => {
            // Klonuj body do watku
            // W pelnej implementacji nalezy uzyc kanalów do komunikacji
            // Tutaj: uruchom asynchronicznie w osobnym watku
            let body_clone = body.clone();
            let mut thread_env = Env::new();
            // Skopiuj zmienne do watku
            for (k, v) in &env.vars {
                thread_env.vars.insert(k.clone(), v.clone());
            }

            std::thread::spawn(move || {
                let _ = exec_nodes(&body_clone, &mut thread_env);
            });

            eprintln!("\x1b[35m[hl :*]\x1b[0m Goroutine uruchomiona");
            Ok(ExecResult::ok())
        }

        // :** nazwa — zadeklaruj kanal (uproszczone: zmienna kolejkowa)
        Node::Channel { name } => {
            let chan_var = format!("__chan_{}", name);
            env.set_var(&chan_var, Value::String(String::new()));
            eprintln!("\x1b[35m[hl :**]\x1b[0m Kanal '{}' zadeklarowany", name);
            Ok(ExecResult::ok())
        }

        // *-- nazwa — operacja na kanale
        Node::ChannelOp { name, value } => {
            let chan_var = format!("__chan_{}", name);
            if let Some(parts) = value {
                // Wyslij wartosc do kanalu
                let val = env.resolve_string_parts(parts);
                env.set_var(&chan_var, Value::String(val));
            } else {
                // Odbierz wartosc z kanalu (wypisz)
                let val = env.get_var(&chan_var).to_string_val();
                println!("{}", val);
            }
            Ok(ExecResult::ok())
        }

        Node::VarDecl { name, value } => {
            let val = match value {
                VarValue::String(s)       => Value::String(s.clone()),
                VarValue::Number(n)       => Value::Number(*n),
                VarValue::Bool(b)         => Value::Bool(*b),
                VarValue::Interpolated(p) => Value::String(env.resolve_string_parts(p)),
                VarValue::CmdOutput(cmd)  => {
                    let r = run_command(cmd, false, false, env, true)?;
                    Value::String(r.stdout.unwrap_or_default().trim().to_string())
                }
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

        Node::Block(nodes) => exec_nodes(nodes, env),
    }
}
