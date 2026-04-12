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

/// Zwraca true jeśli komenda zawiera operatory wymagające powłoki:
///   &&  ||  |  ;  >  >>  <  `...`  $(...)  {  }  *  ?  ~  $VAR
#[inline]
fn needs_shell(cmd: &str) -> bool {
    cmd.contains("&&")
        || cmd.contains("||")
        || cmd.contains(" | ")
        || cmd.starts_with("| ")
        || cmd.ends_with(" |")
        || cmd.contains(';')
        || cmd.contains(" > ")
        || cmd.contains(" >> ")
        || cmd.contains(" < ")
        || cmd.contains('`')
        || cmd.contains("$(")
        || cmd.contains('*')
        || cmd.contains('?')
        || cmd.contains("$HOME")
        || cmd.contains("$USER")
        || cmd.contains("$PATH")
        || cmd.contains("${")
}

// ── Shell-split dla prostych komend (bez operatorów) ─────────────────────────

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

// ── Uruchom komendę ───────────────────────────────────────────────────────────

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
        return run_via_shell(trimmed, sudo, isolated, env, capture);
    }

    // Prosta komenda — exec bezpośrednio
    let parts = shell_words(trimmed);
    if parts.is_empty() { return Ok(ExecResult::ok()); }

    build_and_run(parts, sudo, isolated, capture)
}

/// Uruchom przez `sh -c "..."` (lub `sudo sh -c "..."` / `unshare ... sh -c "..."`)
fn run_via_shell(
    cmd:      &str,
    sudo:     bool,
    isolated: bool,
    _env:     &mut Env,
    capture:  bool,
) -> Result<ExecResult> {
    let (prog, args): (String, Vec<String>) = if isolated {
        let sh_args = vec![
            "--mount".into(), "--pid".into(),
            "--net".into(), "--fork".into(), "--".into(),
            "sh".into(), "-c".into(), cmd.to_string(),
        ];
        if sudo {
            ("sudo".into(), {
                let mut a = vec!["unshare".into()];
                a.extend(sh_args);
                a
            })
        } else {
            ("unshare".into(), sh_args)
        }
    } else if sudo {
        ("sudo".into(), vec!["sh".into(), "-c".into(), cmd.to_string()])
    } else {
        ("sh".into(), vec!["-c".into(), cmd.to_string()])
    };

    let mut cmd_builder = Command::new(&prog);
    cmd_builder.args(&args).stdin(Stdio::inherit());

    if capture {
        cmd_builder.stdout(Stdio::piped()).stderr(Stdio::piped());
        let out  = cmd_builder.output()?;
        let code = out.status.code().unwrap_or(1);
        return Ok(ExecResult {
            exit_code: code,
            stdout:    Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        });
    }

    cmd_builder.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    let code = cmd_builder.status()?.code().unwrap_or(1);
    Ok(ExecResult { exit_code: code, stdout: None })
}

/// Uruchom prostą komendę bezpośrednio przez execv
fn build_and_run(
    parts:    SmallVec<[String; 8]>,
    sudo:     bool,
    isolated: bool,
    capture:  bool,
) -> Result<ExecResult> {
    let (prog, args): (String, Vec<String>) = if isolated {
        let mut iso: Vec<String> = vec![
            "--mount".into(), "--pid".into(),
            "--net".into(), "--fork".into(), "--".into(),
        ];
        if sudo {
            iso.extend(parts.into_iter());
            ("sudo".into(), {
                let mut a = vec!["unshare".into()];
                a.extend(iso);
                a
            })
        } else {
            iso.extend(parts.into_iter());
            ("unshare".into(), iso)
        }
    } else if sudo {
        let all: Vec<String> = parts.into_iter().collect();
        ("sudo".into(), all)
    } else {
        let mut it = parts.into_iter();
        let bin = it.next().unwrap();
        (bin, it.collect())
    };

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
        // ── Comments: no-op ──────────────────────────────────────────────────
        Node::LineComment(_) | Node::DocComment(_) | Node::BlockComment(_) => {
            Ok(ExecResult::ok())
        }

        // ── ~> print ─────────────────────────────────────────────────────────
        Node::Print { parts } => {
            let msg = env.resolve_string_parts(parts);
            println!("{}", msg);
            Ok(ExecResult::ok())
        }

        // ── :: quick-call ────────────────────────────────────────────────────
        Node::QuickCall { name, args } => {
            exec_quick(name, args, env)
        }

        // ── Commands ─────────────────────────────────────────────────────────
        Node::Command { raw, mode, .. } => {
            let trimmed = raw.trim();

            // echo jest zawsze zakazane
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

        // ── Variable declaration ─────────────────────────────────────────────
        Node::VarDecl { name, value } => {
            let val = match value {
                VarValue::String(s)        => Value::String(s.clone()),
                VarValue::Number(n)        => Value::Number(*n),
                VarValue::Bool(b)          => Value::Bool(*b),
                VarValue::Interpolated(p)  => Value::String(env.resolve_string_parts(p)),
                VarValue::CmdOutput(cmd)   => {
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

        // ── Dependencies ─────────────────────────────────────────────────────
        Node::Dependency { name } => {
            match resolve_dependency(name) {
                Ok(r)  => Ok(if r.is_available() { ExecResult::ok() } else { ExecResult::err(1) }),
                Err(e) => { eprintln!("\x1b[31m[hl dep]\x1b[0m {}", e); Ok(ExecResult::err(1)) }
            }
        }

        // ── Imports ──────────────────────────────────────────────────────────
        Node::Import { lib, detail } => {
            resolve_import(lib, detail.as_deref(), env)?;
            Ok(ExecResult::ok())
        }

        // ── Function definition ──────────────────────────────────────────────
        Node::FuncDef { name, body } => {
            env.define_function(name.clone(), body.clone());
            Ok(ExecResult::ok())
        }

        // ── Function call ────────────────────────────────────────────────────
        Node::FuncCall { name } => {
            match env.get_function(name) {
                Some(body) => exec_nodes(&body, env),
                None       => bail!("Niezdefiniowana funkcja: '{}'", name),
            }
        }

        // ── Conditional ──────────────────────────────────────────────────────
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
