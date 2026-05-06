use anyhow::{Result, bail};
use hl_parser::ast::*;
use super::ir::*;

pub fn lower_ast(nodes: &[Node]) -> Result<HlProgram> {
    let mut prog = HlProgram::new();

    let mut func_defs: Vec<(String, Vec<Node>)> = Vec::new();
    let mut toplevel:  Vec<&Node>               = Vec::new();

    for node in nodes {
        match node {
            Node::FuncDef { name, body } => func_defs.push((name.clone(), body.clone())),
            _                             => toplevel.push(node),
        }
    }

    for (name, body) in &func_defs {
        let instrs = lower_nodes(body, &mut prog)?;
        prog.functions.push(HlFunction { name: name.clone(), instrs });
    }

    let main_instrs = lower_nodes_slice(&toplevel, &mut prog)?;
    prog.functions.insert(0, HlFunction { name: "__hl_main".into(), instrs: main_instrs });

    Ok(prog)
}

fn lower_nodes(nodes: &[Node], prog: &mut HlProgram) -> Result<Vec<HlInstr>> {
    let refs: Vec<&Node> = nodes.iter().collect();
    lower_nodes_slice(&refs, prog)
}

fn lower_nodes_slice(nodes: &[&Node], prog: &mut HlProgram) -> Result<Vec<HlInstr>> {
    let mut instrs = Vec::new();
    for node in nodes {
        lower_node(node, prog, &mut instrs)?;
    }
    Ok(instrs)
}

fn lower_node(node: &Node, prog: &mut HlProgram, out: &mut Vec<HlInstr>) -> Result<()> {
    match node {
        Node::LineComment(_) | Node::DocComment(_) | Node::BlockComment(_) => {
            out.push(HlInstr::Nop);
        }

        Node::Print { parts } => {
            let s   = parts_to_string(parts);
            let idx = prog.intern(&s);
            if has_vars(parts) {
                out.push(HlInstr::PrintInterp { idx });
            } else {
                out.push(HlInstr::Print { idx });
            }
        }

        Node::QuickCall { name, args } => {
            let name_s   = name.clone();
            let args_s   = parts_to_string(args);
            let name_idx = prog.intern(&name_s);
            let args_idx = prog.intern(&args_s);
            out.push(HlInstr::QuickCall { name_idx, args_idx });
        }

        Node::Command { raw, mode, interpolate } => {
            let trimmed = raw.trim();
            if trimmed.starts_with("echo ") || trimmed == "echo" {
                bail!("'echo' jest zabronione w Hacker Lang. Uzyj '~>'.");
            }

            let cmd_idx = prog.intern(trimmed);
            let has_at  = trimmed.contains('@');

            let cmd_mode = match mode {
                CommandMode::Plain => {
                    if has_at { CmdMode::WithVars } else { CmdMode::Plain }
                }
                CommandMode::Sudo => {
                    if has_at { CmdMode::WithVarsSudo } else { CmdMode::Sudo }
                }
                CommandMode::Isolated => {
                    if has_at { CmdMode::WithVarsIso } else { CmdMode::Isolated }
                }
                CommandMode::IsolatedSudo     => CmdMode::IsolatedSudo,
                CommandMode::WithVars         => CmdMode::WithVars,
                CommandMode::WithVarsSudo     => CmdMode::WithVarsSudo,
                CommandMode::WithVarsIsolated => CmdMode::WithVarsIso,
            };

            let _ = interpolate;
            out.push(HlInstr::RunCmd { cmd_idx, mode: cmd_mode });
        }

        // *> komenda — uruchom przez hsh
        Node::HshCommand { raw } => {
            let cmd_idx = prog.intern(raw.trim());
            out.push(HlInstr::RunHsh { cmd_idx });
        }

        // & komenda — uruchom w tle
        Node::Background { raw } => {
            let cmd_idx = prog.intern(raw.trim());
            out.push(HlInstr::RunBackground { cmd_idx });
        }

        // _N body — powtorz N razy
        Node::RepeatN { count, body } => {
            let body_instrs = lower_nodes(body, prog)?;
            out.push(HlInstr::RepeatN { count: *count, body: body_instrs });
        }

        // << plik.hl — pomijamy w kompilacji (wymaga runtime)
        Node::FileImport { path, .. } => {
            let msg = format!("[hl-bc] FileImport '{}' — pomijane w trybie kompilacji", path);
            let idx = prog.intern(&msg);
            out.push(HlInstr::Print { idx });
        }

        // :* goroutine — pomijamy w kompilacji (wymaga watki)
        Node::Goroutine { .. } => {
            let msg = "[hl-bc] Goroutine — pomijane w trybie kompilacji (wymaga interpretera)";
            let idx = prog.intern(msg);
            out.push(HlInstr::Print { idx });
        }

        // :** channel — pomijamy
        Node::Channel { name } => {
            let msg = format!("[hl-bc] Channel '{}' — pomijane w trybie kompilacji", name);
            let idx = prog.intern(&msg);
            out.push(HlInstr::Print { idx });
        }

        Node::ChannelOp { name, .. } => {
            let msg = format!("[hl-bc] ChannelOp '{}' — pomijane w trybie kompilacji", name);
            let idx = prog.intern(&msg);
            out.push(HlInstr::Print { idx });
        }

        Node::VarDecl { name, value, .. } => {
            let name_idx = prog.intern(name);
            let (val_s, interp) = varvalue_to_string(value);
            let val_idx = prog.intern(&val_s);
            let has_at = val_s.contains('@');
            if interp || has_at {
                out.push(HlInstr::SetVarInterp { name_idx, val_idx });
            } else {
                out.push(HlInstr::SetVar { name_idx, val_idx });
            }
        }

        Node::Export { name, value } => {
            let name_idx = prog.intern(name);
            match value {
                ExportValue::Single(parts) => {
                    let val_s   = parts_to_string(parts);
                    let val_idx = prog.intern(&val_s);
                    if has_vars(parts) || val_s.contains('@') {
                        out.push(HlInstr::ExportVarInterp { name_idx, val_idx });
                    } else {
                        out.push(HlInstr::ExportVar { name_idx, val_idx });
                    }
                }
                ExportValue::List(items) => {
                    let item_idxs: Vec<u32> = items.iter()
                        .map(|p| { let s = parts_to_string(p); prog.intern(&s) })
                        .collect();
                    out.push(HlInstr::ExportList { name_idx, items: item_idxs });
                }
            }
        }

        Node::VarRef(name) => {
            let template = format!("@{}", name);
            let idx = prog.intern(&template);
            out.push(HlInstr::PrintInterp { idx });
        }

        Node::Dependency { name } => {
            let name_idx = prog.intern(name);
            prog.deps.push(name.clone());
            out.push(HlInstr::Dep { name_idx });
        }

        Node::Import { .. } => {
            out.push(HlInstr::Nop);
        }

        Node::FuncDef { .. } => {
            out.push(HlInstr::Nop);
        }

        Node::FuncCall { name } => {
            let name_idx = prog.intern(name);
            out.push(HlInstr::CallFunc { func_idx: name_idx });
        }

        Node::Conditional { condition, body } => {
            let body_instrs = lower_nodes(body, prog)?;
            match condition {
                ConditionKind::Ok  => out.push(HlInstr::CondOk  { body: body_instrs }),
                ConditionKind::Err => out.push(HlInstr::CondErr { body: body_instrs }),
            }
        }

        Node::Block(nodes) => {
            lower_nodes(nodes, prog).map(|i| out.extend(i))?;
        }

        // Gen 2 — for-in: emit as repeated cmd (lowered to loop unroll in interpreter)
        Node::ForIn { var, iterable, body } => {
            let iter_str = parts_to_string(iterable);
            let var_idx  = prog.intern(var);
            let iter_idx = prog.intern(&iter_str);
            // In compiler: store as SetVar + body (simplified — full loop requires runtime)
            out.push(HlInstr::SetVar { name_idx: var_idx, val_idx: iter_idx });
            let body_instrs = lower_nodes(body, prog)?;
            out.extend(body_instrs);
        }

        // Gen 2 — while: emit body once (loop requires runtime support)
        Node::WhileLoop { condition: _, body } => {
            let body_instrs = lower_nodes(body, prog)?;
            out.extend(body_instrs);
        }

        // Gen 2 — switch/match
        Node::MatchExpr { subject, arms } => {
            let subj_str = parts_to_string(subject);
            let subj_idx = prog.intern(&subj_str);
            // Emit first arm body as approximation (full match requires runtime)
            if let Some(arm) = arms.first() {
                let body_instrs = lower_nodes(&arm.body, prog)?;
                out.extend(body_instrs);
            }
        }

        // Gen 2 — arithmetic: use shell $( expr )
        Node::Arithmetic { expr, assign_to } => {
            // Build: sh -c 'echo $((expr))'  — using concat! to avoid $ ambiguity
            let sh_cmd = {
                let mut s = String::from("sh -c 'echo $((");
                s.push_str(expr);
                s.push_str("))'");
                s
            };
            let cmd_idx = prog.intern(&sh_cmd);
            out.push(HlInstr::RunCmd { cmd_idx, mode: CmdMode::Plain });

            if let Some(var) = assign_to {
                let var_idx = prog.intern(var);
                let expr_wrapped = format!("$(( {} ))", expr);
                let val_idx = prog.intern(&expr_wrapped);
                out.push(HlInstr::SetVar { name_idx: var_idx, val_idx });
            }
        }

        // Gen 2 — pipe to var
        Node::PipeToVar { command, mode, var_name } => {
            let cmd  = format!("{} |> {}", command, var_name);
            let idx  = prog.intern(&cmd);
            let sudo = matches!(mode, CommandMode::Sudo | CommandMode::IsolatedSudo | CommandMode::WithVarsSudo);
            let cm   = if sudo { CmdMode::Sudo } else { CmdMode::Plain };
            out.push(HlInstr::RunCmd { cmd_idx: idx, mode: cm });
        }

        // Gen 2 — HackerOS API
        Node::HackerOsApi { tool, args } => {
            let bin      = tool.binary_name();
            let args_str = parts_to_string(args);
            let full_cmd = if args_str.trim().is_empty() {
                bin.to_string()
            } else {
                format!("{} {}", bin, args_str)
            };
            let cmd_idx = prog.intern(&full_cmd);
            out.push(HlInstr::RunCmd { cmd_idx, mode: CmdMode::Plain });
        }
    }
    Ok(())
}

fn parts_to_string(parts: &[StringPart]) -> String {
    parts.iter().map(|p| match p {
        StringPart::Literal(s) => s.clone(),
        StringPart::Var(v)     => format!("@{}", v),
    }).collect()
}

fn has_vars(parts: &[StringPart]) -> bool {
    parts.iter().any(|p| matches!(p, StringPart::Var(_)))
}

fn varvalue_to_string(v: &VarValue) -> (String, bool) {
    match v {
        VarValue::String(s)       => (s.clone(), false),
        VarValue::Number(n)       => (n.to_string(), false),
        VarValue::Int(n)          => (n.to_string(), false),
        VarValue::Float(n)        => (n.to_string(), false),
        VarValue::Bool(b)         => (b.to_string(), false),
        VarValue::Interpolated(p) => (parts_to_string(p), true),
        VarValue::CmdOutput(cmd)  => (format!("$({})", cmd), false),
        VarValue::Arithmetic(e)   => (format!("$(( {} ))", e), false),
        VarValue::List(_)         => (String::new(), false),
        VarValue::Map(_)          => (String::new(), false),
    }
}
