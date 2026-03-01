use crate::ast::*;
use crate::bytecode::*;

// ─────────────────────────────────────────────────────────────
// Pomocniki
// ─────────────────────────────────────────────────────────────

/// Opakuj warunek w [[ ]] gdy zawiera operatory porównania.
pub fn wrap_cond(cond: &str) -> String {
    let t = cond.trim();
    if t.starts_with('[') || t.starts_with("((") || t.starts_with("[[") {
        return t.to_string();
    }
    let needs = t.contains(" == ")
    || t.contains(" != ")
    || t.contains(" -eq ")
    || t.contains(" -ne ")
    || t.contains(" -lt ")
    || t.contains(" -le ")
    || t.contains(" -gt ")
    || t.contains(" -ge ");
    if needs { format!("[[ {} ]]", t) } else { t.to_string() }
}

/// Czy cmd to wywołanie funkcji HL?
pub fn is_hl_call(cmd: &str) -> bool {
    let t = cmd.trim();
    if !t.starts_with('.') || t.len() < 2 { return false; }
    let c = t.chars().nth(1).unwrap_or(' ');
    c.is_ascii_alphabetic() || c == '_'
}

/// Wyciągnij nazwę funkcji z HL call: ".init $a" → "init"
pub fn extract_hl_func(cmd: &str) -> String {
    cmd.trim()
    .trim_start_matches('.')
    .split_whitespace()
    .next()
    .unwrap_or("")
    .to_string()
}

/// Tłumaczy cmd na shell — obsługuje: log, end, > prefix, out
pub fn shell_inline(cmd: &str) -> String {
    let t = cmd.trim();
    if let Some(r) = t.strip_prefix("log ") {
        return format!("echo {}", r);
    }
    if let Some(r) = t.strip_prefix("end ") {
        return format!("exit {}", r.trim().parse::<i32>().unwrap_or(0));
    }
    if t == "end" { return "exit 0".to_string(); }
    if let Some(r) = t.strip_prefix("out ") {
        return format!("export _HL_OUT={}", r);
    }
    if let Some(r) = t.strip_prefix("> ") { return r.to_string(); }
    if let Some(r) = t.strip_prefix('>')   { return r.trim().to_string(); }
    t.to_string()
}

// ─────────────────────────────────────────────────────────────
// Klasyfikacja body gałęzi if/elif/else
// ─────────────────────────────────────────────────────────────
pub struct Branch {
    pub cond: Option<String>,
    pub body: BranchBody,
    pub sudo: bool,
}

pub enum BranchBody {
    Shell(String),
    HlCall(String),
}

fn classify(cmd: &str) -> BranchBody {
    if is_hl_call(cmd) {
        BranchBody::HlCall(extract_hl_func(cmd))
    } else {
        BranchBody::Shell(shell_inline(cmd))
    }
}

// ─────────────────────────────────────────────────────────────
// emit_if_block — if/elif/else → JumpIfFalse/Jump z backpatchingiem
// ─────────────────────────────────────────────────────────────
fn emit_if_block(branches: Vec<Branch>, prog: &mut BytecodeProgram) {
    let mut end_jumps: Vec<usize> = Vec::new();

    for branch in branches {
        let jif_idx: Option<usize> = branch.cond.map(|cond| {
            let cond_id = prog.pool.intern(&cond);
            let idx = prog.ops.len();
            prog.ops.push(OpCode::JumpIfFalse { cond_id, target: 0 });
            idx
        });

        match branch.body {
            BranchBody::Shell(cmd) => {
                let cmd_id = prog.pool.intern(&cmd);
                prog.ops.push(OpCode::Exec { cmd_id, sudo: branch.sudo });
            }
            BranchBody::HlCall(fname) => {
                let func_id = prog.pool.intern(&fname);
                prog.ops.push(OpCode::CallFunc { func_id });
            }
        }

        let jump_idx = prog.ops.len();
        prog.ops.push(OpCode::Jump { target: 0 });
        end_jumps.push(jump_idx);

        if let Some(idx) = jif_idx {
            let next = prog.ops.len();
            if let OpCode::JumpIfFalse { target, .. } = &mut prog.ops[idx] {
                *target = next;
            }
        }
    }

    let end = prog.ops.len();
    for idx in end_jumps {
        if let OpCode::Jump { target } = &mut prog.ops[idx] {
            *target = end;
        }
    }
}

// ─────────────────────────────────────────────────────────────
// emit_match_block — match + MatchArm* → jeden Exec(case..esac)
//
// Algorytm:
//   1. Pobierz cond z Match
//   2. Pochłoń wszystkie następne MatchArm
//   3. Zbuduj "case $cond in val) cmd;; * ) cmd;; esac"
//   4. Emituj jeden Exec — zero fork/exec narzutu dla N ramion
// ─────────────────────────────────────────────────────────────
fn emit_match_block(
    cond: &str,
    arms: &[(String, String)],
                    sudo: bool,
                    prog: &mut BytecodeProgram,
) {
    if arms.is_empty() { return; }

    let mut sh = format!("case {} in\n", cond);
    for (val, cmd) in arms {
        let clean_val = if val == "_" {
            "*".to_string()
        } else {
            val.trim_matches('"').trim_matches('\'').to_string()
        };
        sh += &format!("  {}) {};;\n", clean_val, shell_inline(cmd));
    }
    sh += "esac";

    let cmd_id = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo });
}

// ─────────────────────────────────────────────────────────────
// emit_pipe — Pipe → sekwencja CallFunc (all-HL) lub Exec(pipe)
//
// Heurystyka:
//   • Jeśli krok zaczyna się od '.' → traktuj jako HL call
//   • Reszta → shell
//   Jeśli wszystkie HL: sekwencja CallFunc (bez | fork/exec)
//   Jeśli mieszane: shell pipe "a | b | c"
// ─────────────────────────────────────────────────────────────
fn emit_pipe(steps: &[String], sudo: bool, prog: &mut BytecodeProgram) {
    if steps.is_empty() { return; }

    let all_hl = steps.iter().all(|s| is_hl_call(s.trim()));

    if all_hl {
        // Fast path: sekwencja CallFunc, inliner może sflatować
        for step in steps {
            let fname = extract_hl_func(step);
            let func_id = prog.pool.intern(&fname);
            prog.ops.push(OpCode::CallFunc { func_id });
        }
    } else {
        // Slow path: shell pipe
        let parts: Vec<String> = steps.iter().map(|s| {
            let t = s.trim();
            if is_hl_call(t) {
                // Zamień .func na wywołanie shell funkcji HL
                t.trim_start_matches('.').to_string()
            } else {
                shell_inline(t)
            }
        }).collect();
        let sh = parts.join(" | ");
        let cmd_id = prog.pool.intern(&sh);
        prog.ops.push(OpCode::Exec { cmd_id, sudo });
    }
}

// ─────────────────────────────────────────────────────────────
// compile_body — główna pętla kompilacji węzłów
// ─────────────────────────────────────────────────────────────
pub fn compile_body(nodes: &[ProgramNode], prog: &mut BytecodeProgram) {
    let mut i = 0;
    while i < nodes.len() {
        let node = &nodes[i];
        let sudo =  node.is_sudo;

        match &node.content {

            // ── If — zbierz cały blok If + Elif* + Else? ──────
            CommandType::If { cond, cmd } => {
                let mut branches: Vec<Branch> = vec![Branch {
                    cond: Some(wrap_cond(cond)),
                    body: classify(cmd),
                    sudo,
                }];
                i += 1;

                loop {
                    if i >= nodes.len() { break; }
                    match &nodes[i].content {
                        CommandType::Elif { cond, cmd } => {
                            branches.push(Branch {
                                cond: Some(wrap_cond(cond)),
                                          body: classify(cmd),
                                          sudo: nodes[i].is_sudo,
                            });
                            i += 1;
                        }
                        CommandType::Else { cmd } => {
                            branches.push(Branch {
                                cond: None,
                                body: classify(cmd),
                                          sudo: nodes[i].is_sudo,
                            });
                            i += 1;
                            break;
                        }
                        _ => break,
                    }
                }

                emit_if_block(branches, prog);
                continue;
            }

            // ── Match — pochłoń MatchArm i emituj case..esac ──
            CommandType::Match { cond } => {
                let mut arms: Vec<(String, String)> = Vec::new();
                i += 1;

                while i < nodes.len() {
                    if let CommandType::MatchArm { val, cmd } = &nodes[i].content {
                        arms.push((val.clone(), cmd.clone()));
                        i += 1;
                    } else {
                        break;
                    }
                }

                emit_match_block(cond, &arms, sudo, prog);
                continue;
            }

            // ── MatchArm poza Match — ignoruj ─────────────────
            CommandType::MatchArm { .. } => {
                i += 1;
                continue;
            }

            // ── Pipe ──────────────────────────────────────────
            CommandType::Pipe(steps) => {
                emit_pipe(steps, sudo, prog);
                i += 1;
                continue;
            }

            _ => {
                compile_node(node, prog);
                i += 1;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────
// compile_node — kompilacja pojedynczego węzła
// ─────────────────────────────────────────────────────────────
fn compile_node(node: &ProgramNode, prog: &mut BytecodeProgram) {
    let sudo = node.is_sudo;
    let pool = &mut prog.pool;

    match &node.content {

        // ── ISTNIEJĄCE ────────────────────────────────────────
        CommandType::RawNoSub(s) | CommandType::RawSub(s) => {
            let cmd_id = pool.intern(s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::Isolated(s) => {
            let cmd    = format!("( {} )", s);
            let cmd_id = pool.intern(&cmd);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::AssignEnv { key, val } => {
            let key_id = pool.intern(key);
            let val_id = pool.intern(val);
            prog.ops.push(OpCode::SetEnv { key_id, val_id });
        }
        CommandType::AssignLocal { key, val, is_raw } => {
            let key_id = pool.intern(key);
            let val_id = pool.intern(val);
            prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: *is_raw });
        }
        CommandType::Loop { count, cmd } => {
            let s      = format!("for _hl_i in $(seq 1 {}); do {}; done", count, cmd);
            let cmd_id = pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::While { cond, cmd } => {
            let s      = format!("while {}; do {}; done", wrap_cond(cond), cmd);
            let cmd_id = pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::For { var, in_, cmd } => {
            let s      = format!("for {} in {}; do {}; done", var, in_, cmd);
            let cmd_id = pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::Background(s) => {
            let bg     = format!("{} &", s);
            let cmd_id = pool.intern(&bg);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        // ── Call { path, args } — zmiana z Call(String) ──────
        // Emitujemy CallFunc dla ścieżki funkcji.
        // Jeśli args niepuste → SetLocal("_HL_ARGS", args) przed callą.
        CommandType::Call { path, args } => {
            let fname   = path.trim_start_matches('.');
            let func_id = pool.intern(fname);

            if !args.is_empty() {
                // Przekaż argumenty przez zmienną _HL_ARGS
                let key_id = pool.intern("_HL_ARGS");
                let val_id = pool.intern(args);
                prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: false });
            }

            prog.ops.push(OpCode::CallFunc { func_id });
        }

        CommandType::Plugin { name, args, is_super } => {
            let name_id = pool.intern(name);
            let args_id = pool.intern(args);
            prog.ops.push(OpCode::Plugin { name_id, args_id, sudo: *is_super });
        }
        CommandType::Log(msg) => {
            let s      = format!("echo {}", msg);
            let cmd_id = pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::Lock { key, val } => {
            let key_id = pool.intern(key);
            let val_id = pool.intern(val);
            prog.ops.push(OpCode::Lock { key_id, val_id });
        }
        CommandType::Unlock { key } => {
            let key_id = pool.intern(key);
            prog.ops.push(OpCode::Unlock { key_id });
        }
        CommandType::Try { try_cmd, catch_cmd } => {
            let s      = format!("( {} ) || ( {} )", try_cmd, catch_cmd);
            let cmd_id = pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::End { code } => {
            prog.ops.push(OpCode::Exit(*code));
        }

        // ── Out — zwrócenie wartości z funkcji ────────────────
        // Emitujemy SetOut { val_id } — VM zapisze _HL_OUT
        CommandType::Out(val) => {
            let val_id = pool.intern(val);
            prog.ops.push(OpCode::SetOut { val_id });
        }

        // ── % Const — stała ───────────────────────────────────
        CommandType::Const { key, val } => {
            let key_id = pool.intern(key);
            let val_id = pool.intern(val);
            prog.ops.push(OpCode::SetConst { key_id, val_id });
        }

        // ── Spawn — fire & forget ─────────────────────────────
        CommandType::Spawn(task) => {
            let clean  = task.trim().trim_start_matches('.');
            let cmd_id = pool.intern(clean);
            prog.ops.push(OpCode::SpawnBg { cmd_id, sudo });
        }

        // ── AssignSpawn — spawn z przypisaniem PID ────────────
        CommandType::AssignSpawn { key, task } => {
            let clean  = task.trim().trim_start_matches('.');
            let key_id = pool.intern(key);
            let cmd_id = pool.intern(clean);
            prog.ops.push(OpCode::SpawnAssign { key_id, cmd_id, sudo });
        }

        // ── Await — czekaj na zadanie ─────────────────────────
        CommandType::Await(expr) => {
            let expr_id = pool.intern(expr.trim());
            prog.ops.push(OpCode::AwaitPid { expr_id });
        }

        // ── AssignAwait — czekaj i przypisz ──────────────────
        CommandType::AssignAwait { key, expr } => {
            let key_id  = pool.intern(key);
            let expr_id = pool.intern(expr.trim());
            prog.ops.push(OpCode::AwaitAssign { key_id, expr_id });
        }

        // ── Assert — walidacja ────────────────────────────────
        // VM-native: zero fork/exec w happy path
        CommandType::Assert { cond, msg } => {
            let cond_id = pool.intern(cond);
            let msg_id  = msg.as_deref().map(|m| pool.intern(m));
            prog.ops.push(OpCode::Assert { cond_id, msg_id });
        }

        // ── Metadane — ignorowane przez runtime ───────────────
        CommandType::Extern { .. }
        | CommandType::Enum { .. }
        | CommandType::Struct { .. }
        | CommandType::Import { .. } => {}

        // ── Pochłaniane przez compile_body ────────────────────
        CommandType::If    { .. }
        | CommandType::Elif  { .. }
        | CommandType::Else  { .. }
        | CommandType::Match { .. }
        | CommandType::MatchArm { .. }
        | CommandType::Pipe(_) => {}
    }
}

// ─────────────────────────────────────────────────────────────
// Entry point kompilacji
// ─────────────────────────────────────────────────────────────
pub fn compile_to_bytecode(ast: &AnalysisResult) -> BytecodeProgram {
    let mut prog = BytecodeProgram::new();

    // Kompiluj main_body
    compile_body(&ast.main_body, &mut prog);
    prog.ops.push(OpCode::Exit(0));

    // Kompiluj funkcje HL — każda kończy się Return
    // Nowe: trójka (is_unsafe, sig, nodes) zamiast pary
    for (name, (_is_unsafe, _sig, nodes)) in &ast.functions {
        prog.functions.insert(name.clone(), prog.ops.len());
        compile_body(nodes, &mut prog);
        prog.ops.push(OpCode::Return);
    }

    prog
}
