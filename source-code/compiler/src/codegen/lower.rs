use anyhow::{Result, bail};
use hl_parser::ast::*;
use super::ir::*;

/// Opuszcz cale AST do HlProgram
pub fn lower_ast(nodes: &[Node]) -> Result<HlProgram> {
    let mut prog = HlProgram::new();

    // Zbierz definicje funkcji najpierw
    let mut func_defs: Vec<(String, Vec<Node>)> = Vec::new();
    let mut toplevel:  Vec<&Node>               = Vec::new();

    for node in nodes {
        match node {
            Node::FuncDef { name, body } => func_defs.push((name.clone(), body.clone())),
            _                             => toplevel.push(node),
        }
    }

    // Lowering funkcji uzytkownika
    for (name, body) in &func_defs {
        let instrs = lower_nodes(body, &mut prog)?;
        prog.functions.push(HlFunction { name: name.clone(), instrs });
    }

    // Lowering kodu glownego jako __hl_main
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
            // args zawsze moga miec @zmienne — zawsze przez interp path
            out.push(HlInstr::QuickCall { name_idx, args_idx });
        }

        Node::Command { raw, mode, interpolate } => {
            let trimmed = raw.trim();
            if trimmed.starts_with("echo ") || trimmed == "echo" {
                bail!("'echo' jest zabronione w Hacker Lang. Uzyj '~>'.");
            }

            let cmd_idx = prog.intern(trimmed);

            // Sprawdz czy komenda zawiera @zmienne (nawet jesli interpolate=false w AST)
            // Jesli tak, ZAWSZE uzyj trybu z interpolacja — inaczej zmienna nie zostanie
            // rozwiazana w skompilowanej binarce i pojawi sie literalne "@VAR"
            let has_at = trimmed.contains('@');

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

            let _ = interpolate; // juz obsluzone powyzej przez has_at

            out.push(HlInstr::RunCmd { cmd_idx, mode: cmd_mode });
        }

        Node::VarDecl { name, value } => {
            let name_idx = prog.intern(name);
            let (val_s, interp) = varvalue_to_string(value);
            let val_idx = prog.intern(&val_s);
            // Sprawdz tez czy wartosc zawiera @zmienne
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
                    // Export tez moze miec @zmienne
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
            // @name standalone — wypisz wartosc zmiennej przez interp
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
            // Importy pomijane w trybie kompilacji (stdlib embedded w runtime C)
            out.push(HlInstr::Nop);
        }

        Node::FuncDef { .. } => {
            // Zebrane wczesniej w lower_ast
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
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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
        VarValue::Bool(b)         => (b.to_string(), false),
        VarValue::Interpolated(p) => (parts_to_string(p), true),
        VarValue::CmdOutput(cmd)  => (format!("$({})", cmd), false),
    }
}
