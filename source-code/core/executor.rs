use std::process::{Command, Stdio};
use anyhow::{Result, bail};
use tracing::{debug, info};
use crate::ast::*;
use crate::env::{Env, Value};
use crate::deps::resolve_dependency;

pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: Option<String>,
}

impl ExecResult {
    pub fn ok() -> Self { Self { exit_code: 0, stdout: None } }
    pub fn err(code: i32) -> Self { Self { exit_code: code, stdout: None } }
    pub fn is_ok(&self) -> bool { self.exit_code == 0 }
}

/// Execute a system command, returning exit code
fn run_command(
    raw: &str,
    sudo: bool,
    isolated: bool,
    env: &Env,
    capture: bool,
) -> Result<ExecResult> {
    // Shell-split the raw command string
    let expanded = env.interpolate(raw);
    debug!("Executing: {}", expanded);

    let parts = shell_words(expanded.trim());
    if parts.is_empty() {
        return Ok(ExecResult::ok());
    }

    let (program, args) = if isolated {
        // Use unshare for namespace isolation (Linux-specific)
        let mut iso_args = vec![
            "--mount".to_string(),
            "--pid".to_string(),
            "--net".to_string(),
            "--fork".to_string(),
            "--".to_string(),
        ];
        if sudo {
            iso_args.insert(0, "unshare".to_string());
            let mut final_args: Vec<String> = vec!["sudo".to_string()];
            final_args.extend(iso_args);
            let bin = parts[0].clone();
            final_args.push(bin);
            final_args.extend_from_slice(&parts[1..]);
            ("sudo".to_string(), final_args[1..].to_vec())
        } else {
            iso_args.extend_from_slice(&parts);
            ("unshare".to_string(), iso_args)
        }
    } else if sudo {
        let mut sudo_args = vec![];
        sudo_args.extend_from_slice(&parts);
        ("sudo".to_string(), sudo_args)
    } else {
        let bin = parts[0].clone();
        (bin, parts[1..].to_vec())
    };

    let mut cmd = Command::new(&program);
    cmd.args(&args);

    // Inherit stdout/stderr unless capturing
    if capture {
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
    } else {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }
    cmd.stdin(Stdio::inherit());

    let output = if capture {
        let out = cmd.output()?;
        let code = out.status.code().unwrap_or(1);
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        return Ok(ExecResult { exit_code: code, stdout: Some(stdout) });
    } else {
        let status = cmd.status()?;
        status.code().unwrap_or(1)
    };

    Ok(ExecResult { exit_code: output, stdout: None })
}

/// Split a shell-like command string into tokens (respects quotes)
fn shell_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '\'' if !in_double => {
                in_single = !in_single;
                i += 1;
            }
            '"' if !in_single => {
                in_double = !in_double;
                i += 1;
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    words.push(current.clone());
                    current.clear();
                }
                i += 1;
            }
            c => {
                current.push(c);
                i += 1;
            }
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Recursively execute a list of AST nodes
pub fn exec_nodes(nodes: &[Node], env: &mut Env) -> Result<ExecResult> {
    let mut last = ExecResult::ok();

    for node in nodes {
        let result = exec_node(node, env)?;
        env.last_exit = result.exit_code;
        last = result;
    }

    Ok(last)
}

/// Execute a single AST node
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

        Node::Command { raw, mode, interpolate: _ } => {
            // Check for forbidden echo in non-:: context
            let trimmed = raw.trim();
            if trimmed.starts_with("echo ") || trimmed == "echo" {
                bail!(
                    "Use ':: message' instead of 'echo' in Hacker Lang commands.\n\
Hint: Replace '> echo ...' with ':: ...'"
                );
            }

            let (sudo, isolated) = match mode {
                CommandMode::Plain => (false, false),
                CommandMode::Sudo => (true, false),
                CommandMode::Isolated => (false, true),
                CommandMode::IsolatedSudo => (true, true),
                CommandMode::WithVars => (false, false),
                CommandMode::WithVarsSudo => (true, false),
            };

            let result = run_command(raw, sudo, isolated, env, false)?;
            Ok(result)
        }

        Node::VarDecl { name, value } => {
            let val = match value {
                VarValue::String(s) => Value::String(s.clone()),
                VarValue::Number(n) => Value::Number(*n),
                VarValue::Bool(b) => Value::Bool(*b),
                VarValue::Interpolated(parts) => {
                    Value::String(env.resolve_string_parts(parts))
                }
                VarValue::CmdOutput(cmd) => {
                    let result = run_command(cmd, false, false, env, true)?;
                    Value::String(
                        result.stdout.unwrap_or_default().trim().to_string()
                    )
                }
            };
            info!("Setting var '{}' = {:?}", name, val);
            env.set_var(name, val);
            Ok(ExecResult::ok())
        }

        Node::VarRef(name) => {
            let val = env.get_var(name);
            println!("{}", val.to_string_val());
            Ok(ExecResult::ok())
        }

        Node::Dependency { name } => {
            match resolve_dependency(name) {
                Ok(dep_result) => {
                    if dep_result.is_available() {
                        Ok(ExecResult::ok())
                    } else {
                        Ok(ExecResult::err(1))
                    }
                }
                Err(e) => {
                    eprintln!("\x1b[31m[hl dep error]\x1b[0m {}", e);
                    Ok(ExecResult::err(1))
                }
            }
        }

        Node::FuncDef { name, body } => {
            env.define_function(name.clone(), body.clone());
            debug!("Defined function '{}'", name);
            Ok(ExecResult::ok())
        }

        Node::FuncCall { name } => {
            let body = env.get_function(name);
            match body {
                Some(nodes) => exec_nodes(&nodes, env),
                None => {
                    bail!("Undefined function: '{}'", name);
                }
            }
        }

        Node::Conditional { condition, body } => {
            let should_run = match condition {
                ConditionKind::Ok => env.last_exit == 0,
                ConditionKind::Err => env.last_exit != 0,
            };
            if should_run {
                exec_nodes(body, env)
            } else {
                Ok(ExecResult::ok())
            }
        }

        Node::Block(nodes) => exec_nodes(nodes, env),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;

    #[test]
    fn test_shell_words() {
        let words = shell_words("nmap -sV -T4 192.168.1.1");
        assert_eq!(words, vec!["nmap", "-sV", "-T4", "192.168.1.1"]);
    }

    #[test]
    fn test_echo_blocked() {
        let mut env = Env::new();
        let node = Node::Command {
            raw: "echo hello".to_string(),
            mode: CommandMode::Plain,
            interpolate: false,
        };
        let result = exec_node(&node, &mut env);
        assert!(result.is_err());
    }
}
