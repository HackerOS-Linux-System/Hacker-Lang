use std::process::{Command, Stdio};
use anyhow::{Result, bail};
use smallvec::SmallVec;
use tracing::debug;
use crate::ast::*;
use crate::env::{Env, Value};
use crate::deps::resolve_dependency;
use crate::libs::resolve_import;
use crate::quick::exec_quick;

pub struct ExecResult {
    pub exit_code: i32,
    pub stdout:    Option<String>,
}

impl ExecResult {
    #[inline] pub fn ok()         -> Self { Self { exit_code: 0, stdout: None } }
    #[inline] pub fn err(n: i32)  -> Self { Self { exit_code: n, stdout: None } }
    #[inline] pub fn is_ok(&self) -> bool { self.exit_code == 0 }
}

// ── Wykryj czy komenda wymaga powłoki ────────────────────────────────────────

/// Zwraca true jeśli komenda zawiera operatory które muszą być obsłużone przez sh:
///   &&  ||  |  ;  redirecty  podstawienia  globy  zmienne środowiskowe
#[inline]
fn needs_shell(cmd: &str) -> bool {
    // Operatory logiczne i potoki
    cmd.contains("&&")
    || cmd.contains("||")
    // Potok — spacja|spacja żeby nie łapać /dev/null| itp.
    || cmd.contains(" | ")
    || cmd.starts_with("| ")
    // Separator komend
    || cmd.contains(';')
    // Przekierowania (nie łap -> operatora HL)
    || cmd.contains(" > ")
    || cmd.contains(" >> ")
    || cmd.contains(" < ")
    // Podstawienia
    || cmd.contains('`')
    || cmd.contains("$(")
    // Zmienne środowiskowe powłoki (nie @zmienne HL)
    || cmd.contains("$HOME")
    || cmd.contains("$USER")
    || cmd.contains("$PATH")
    || cmd.contains("$1")
    || cmd.contains("$2")
    || cmd.contains("${")
    // Globy
    || (cmd.contains('*') && !cmd.contains("://"))
}

// ── Shell-split dla prostych komend ──────────────────────────────────────────

#[inline]
fn shell_words(s: &str) -> SmallVec<[String; 8]> {
    let mut words: SmallVec<[String; 8]> = SmallVec::new();
    let mut cur  = String::with_capacity(32);
    let mut in_s = false;
    let mut in_d = false;

    for c in s.chars() {
        match c {
            '\'' if !in_d => in_s = !in_s,
            '"'  if !in_s => in_d = !in_d,
            ' ' | '\t' if !in_s && !in_d => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    cur = String::with_capacity(32);
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { words.push(cur); }
    words
}

// ── Główna funkcja uruchamiania ───────────────────────────────────────────────

fn run_command(
    raw:      &str,
    sudo:     bool,
    isolated: bool,
    env:      &mut Env,
    capture:  bool,
) -> Result<ExecResult> {
    let expanded = env.interpolate(raw);
    let trimmed  = expanded.trim();
    debug!("run: {}", trimmed);

    // Jeśli komenda zawiera operatory powłoki — użyj sh -c
    if needs_shell(trimmed) {
        return run_via_shell(trimmed, sudo, isolated, capture);
    }

    // Prosta komenda — exec bezpośrednio
    let parts = shell_words(trimmed);
    if parts.is_empty() { return Ok(ExecResult::ok()); }

    build_and_run(parts, sudo, isolated, capture)
}

// ── sh -c dla komend z operatorami ───────────────────────────────────────────

/// Uruchom przez `sh -c "cmd"`.
/// Tryby:
///   plain:    sh -c "cmd"
///   sudo:     sudo sh -c "cmd"
///   isolated: unshare --mount --pid --net --fork -- sh -c "cmd"
///   iso+sudo: sudo unshare --mount --pid --net --fork -- sh -c "cmd"
fn run_via_shell(
    cmd:      &str,
    sudo:     bool,
    isolated: bool,
    capture:  bool,
) -> Result<ExecResult> {
    let sh_args  = vec!["-c".to_string(), cmd.to_string()];
    let iso_args = vec![
        "--mount".to_string(), "--pid".into(),
        "--net".into(), "--fork".into(), "--".into(),
        "sh".into(), "-c".into(), cmd.to_string(),
    ];

    let (prog, args): (String, Vec<String>) = match (sudo, isolated) {
        (false, false) => ("sh".into(), sh_args),
        (true,  false) => ("sudo".into(), {
            let mut a = vec!["sh".into(), "-c".into()];
            a.push(cmd.to_string());
            a
        }),
        (false, true)  => ("unshare".into(), iso_args),
        (true,  true)  => ("sudo".into(), {
            let mut a = vec!["unshare".into()];
            a.extend(iso_args);
            a
        }),
    };

    exec_process(prog, args, capture)
}

// ── Bezpośrednie execv dla prostych komend ────────────────────────────────────

fn build_and_run(
    parts:    SmallVec<[String; 8]>,
    sudo:     bool,
    isolated: bool,
    capture:  bool,
) -> Result<ExecResult> {
    let (prog, args): (String, Vec<String>) = match (sudo, isolated) {
        (false, false) => {
            let mut it = parts.into_iter();
            let p = it.next().unwrap();
            (p, it.collect())
        }
        (true, false) => {
            ("sudo".into(), parts.into_iter().collect())
        }
        (false, true) => {
            let mut a = vec![
                "--mount".into(), "--pid".into(),
                "--net".into(), "--fork".into(), "--".into(),
            ];
            a.extend(parts.into_iter());
            ("unshare".into(), a)
        }
        (true, true) => {
            let mut iso = vec![
                "--mount".into(), "--pid".into(),
                "--net".into(), "--fork".into(), "--".into(),
            ];
            iso.extend(parts.into_iter());
            let mut a = vec!["unshare".into()];
            a.extend(iso);
            ("sudo".into(), a)
        }
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
        return Ok(ExecResult {
            exit_code: code,
            stdout:    Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        });
    }

    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    let code = cmd.status()?.code().unwrap_or(1);
    Ok(ExecResult { exit_code: code, stdout: None })
}

// ── AST executor ─────────────────────────────────────────────────────────────

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
        Node::LineComment(_) | Node::DocComment(_) | Node::BlockComment(_) => {
            Ok(ExecResult::ok())
        }

        Node::Print { parts } => {
            let msg = env.resolve_string_parts(parts);
            println!("{}", msg);
            Ok(ExecResult::ok())
        }

        Node::QuickCall { name, args } => {
            exec_quick(name, args, env)
        }

        Node::Command { raw, mode, .. } => {
            let trimmed = raw.trim();

            if trimmed.starts_with("echo ") || trimmed == "echo" {
                bail!(
                    "Operator 'echo' jest zabroniony w Hacker Lang.\n\
Użyj '~> {}' zamiast '> echo {}'",
trimmed.trim_start_matches("echo").trim(),
                      trimmed.trim_start_matches("echo").trim()
                );
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

        Node::VarRef(name) => {
            let s = env.get_var(name).to_string_val();
            println!("{}", s);
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
