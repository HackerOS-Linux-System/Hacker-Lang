use crate::ast::*;
use crate::bytecode::*;

// ─────────────────────────────────────────────────────────────
// HlExpr — lokalny typ wyrażenia
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum HlExpr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Var(String),
    Add(Box<HlExpr>, Box<HlExpr>),
    Sub(Box<HlExpr>, Box<HlExpr>),
    Mul(Box<HlExpr>, Box<HlExpr>),
    Div(Box<HlExpr>, Box<HlExpr>),
    Mod(Box<HlExpr>, Box<HlExpr>),
    Neg(Box<HlExpr>),
    Eq (Box<HlExpr>, Box<HlExpr>),
    Ne (Box<HlExpr>, Box<HlExpr>),
    Lt (Box<HlExpr>, Box<HlExpr>),
    Le (Box<HlExpr>, Box<HlExpr>),
    Gt (Box<HlExpr>, Box<HlExpr>),
    Ge (Box<HlExpr>, Box<HlExpr>),
}

// ─────────────────────────────────────────────────────────────
// NumericPayload
// ─────────────────────────────────────────────────────────────
enum NumericPayload {
    Typed { type_: String, expr: HlExpr },
    Arith { expr: HlExpr },
}

fn parse_numeric_payload(val: &str) -> Option<NumericPayload> {
    let rest = val.strip_prefix("__hl_num:")?;
    if let Some(r) = rest.strip_prefix("int:")   {
        let expr: HlExpr = serde_json::from_str(r).ok()?;
        return Some(NumericPayload::Typed { type_: "int".into(), expr });
    }
    if let Some(r) = rest.strip_prefix("float:") {
        let expr: HlExpr = serde_json::from_str(r).ok()?;
        return Some(NumericPayload::Typed { type_: "float".into(), expr });
    }
    if let Some(r) = rest.strip_prefix("bool:")  {
        let expr: HlExpr = serde_json::from_str(r).ok()?;
        return Some(NumericPayload::Typed { type_: "bool".into(), expr });
    }
    if let Some(r) = rest.strip_prefix("str:")   {
        let expr: HlExpr = serde_json::from_str(r).ok()?;
        return Some(NumericPayload::Typed { type_: "str".into(), expr });
    }
    if let Some(r) = rest.strip_prefix("expr:")  {
        let expr: HlExpr = serde_json::from_str(r).ok()?;
        return Some(NumericPayload::Arith { expr });
    }
    None
}

// ─────────────────────────────────────────────────────────────
// NumFor payload
// ─────────────────────────────────────────────────────────────
struct NumForPayload {
    var:   String,
    start: i64,
    end:   i64,
    step:  i64,
    cmd:   String,
}

fn parse_numfor_payload(cmd: &str) -> Option<NumForPayload> {
    let rest = cmd.strip_prefix("__hl_numfor:")?;
    let parts: Vec<&str> = rest.splitn(5, '\0').collect();
    if parts.len() != 5 { return None; }
    Some(NumForPayload {
        var:   parts[0].to_string(),
         start: parts[1].parse().ok()?,
         end:   parts[2].parse().ok()?,
         step:  parts[3].parse().ok()?,
         cmd:   parts[4].to_string(),
    })
}

// ─────────────────────────────────────────────────────────────
// WhileExpr payload
// ─────────────────────────────────────────────────────────────
struct WhileExprPayload {
    lhs: HlExpr,
    op:  CmpOp,
    rhs: HlExpr,
    cmd: String,
}

fn parse_whileexpr_payload(cond: &str, cmd: &str) -> Option<WhileExprPayload> {
    let rest = cond.strip_prefix("__hl_whileexpr:")?;
    let parts: Vec<&str> = rest.splitn(3, '\0').collect();
    if parts.len() != 3 { return None; }
    let lhs: HlExpr = serde_json::from_str(parts[0]).ok()?;
    let op = match parts[1] {
        "==" => CmpOp::Eq, "!=" => CmpOp::Ne,
        "<"  => CmpOp::Lt, "<=" => CmpOp::Le,
        ">"  => CmpOp::Gt, ">=" => CmpOp::Ge,
        _ => return None,
    };
    let rhs: HlExpr = serde_json::from_str(parts[2]).ok()?;
    Some(WhileExprPayload { lhs, op, rhs, cmd: cmd.to_string() })
}

fn parse_retexpr_payload(val: &str) -> Option<HlExpr> {
    let rest = val.strip_prefix("__hl_retexpr:")?;
    serde_json::from_str(rest).ok()
}

// ─────────────────────────────────────────────────────────────
// FIX 1: rewrite_hl_calls_in_expr
//
// Zamienia (.func arg1 arg2) → $(_hl_func arg1 arg2)
// w dowolnym wyrażeniu (cond assert, if cond itp.)
//
// Obsługuje wielokrotne wywołania w jednym wyrażeniu:
//   "(.add 2 3) == 5"           → "$(_hl_add 2 3) == 5"
//   "(.str_len \"x\") == 1"     → "$(_hl_str_len \"x\") == 1"
//   "(.a 1) == (.b 2)"          → "$(_hl_a 1) == $(_hl_b 2)"
// ─────────────────────────────────────────────────────────────
pub fn rewrite_hl_calls_in_expr(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 16);
    let bytes = s.as_bytes();
    let len   = bytes.len();
    let mut i = 0;

    while i < len {
        // Szukaj '('
        if bytes[i] == b'(' && i + 1 < len && bytes[i + 1] == b'.' {
            // Znajdź pasujące zamknięcie ')' z liczeniem zagłębień
            let start = i;
            let mut depth  = 1usize;
            let mut j      = i + 1;
            while j < len && depth > 0 {
                if bytes[j] == b'(' { depth += 1; }
                else if bytes[j] == b')' { depth -= 1; }
                j += 1;
            }
            if depth == 0 {
                // inner = zawartość bez nawiasów zewnętrznych
                let inner = &s[start + 1 .. j - 1]; // bez '(' i ')'
                // inner zaczyna się '.' — usuń kropkę, zamień '.' w nazwie na '_'
                let inner2 = inner.trim_start_matches('.');
                // Wyodrębnij nazwę funkcji (do pierwszej spacji)
                let sp = inner2.find(' ').unwrap_or(inner2.len());
                let fname = &inner2[..sp];
                let fargs = inner2[sp..].trim_start();
                // Rekurencja dla zagnieżdżonych wywołań w argumentach
                let fargs_rewritten = rewrite_hl_calls_in_expr(fargs);
                let fname_bash = fname.replace('.', "_");
                if fargs_rewritten.is_empty() {
                    result.push_str(&format!("$(_hl_{})", fname_bash));
                } else {
                    result.push_str(&format!("$(_hl_{} {})", fname_bash, fargs_rewritten));
                }
                i = j;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

// ─────────────────────────────────────────────────────────────
// Pomocnicy
// ─────────────────────────────────────────────────────────────
pub fn wrap_cond(cond: &str) -> String {
    let t = cond.trim();
    // Najpierw przepisz wywołania HL w condzie
    let rewritten = rewrite_hl_calls_in_expr(t);
    let t2 = rewritten.as_str();

    if t2.starts_with('[') || t2.starts_with("((") || t2.starts_with("[[") {
        return t2.to_string();
    }
    let needs = t2.contains(" == ")
    || t2.contains(" != ")
    || t2.contains(" -eq ")
    || t2.contains(" -ne ")
    || t2.contains(" -lt ")
    || t2.contains(" -le ")
    || t2.contains(" -gt ")
    || t2.contains(" -ge ");
    if needs { format!("[[ {} ]]", t2) } else { t2.to_string() }
}

pub fn is_hl_call(cmd: &str) -> bool {
    let t = cmd.trim();
    if !t.starts_with('.') || t.len() < 2 { return false; }
    let c = t.chars().nth(1).unwrap_or(' ');
    c.is_ascii_alphabetic() || c == '_'
}

pub fn extract_hl_func(cmd: &str) -> String {
    cmd.trim()
    .trim_start_matches('.')
    .split_whitespace()
    .next()
    .unwrap_or("")
    .to_string()
}

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
        return format!("echo {}", r);
    }
    if let Some(r) = t.strip_prefix("> ")  { return r.to_string(); }
    if let Some(r) = t.strip_prefix('>')   { return r.trim().to_string(); }
    // FIX: przepisz wywołania HL w komendach inline
    rewrite_hl_calls_in_expr(t)
}

// ─────────────────────────────────────────────────────────────
// Alokator rejestrów
// ─────────────────────────────────────────────────────────────
struct RegAlloc { next: u8 }

impl RegAlloc {
    fn new() -> Self { Self { next: 0 } }
    fn alloc(&mut self) -> u8 {
        let r = self.next;
        self.next = self.next.wrapping_add(1);
        r
    }
}

// ─────────────────────────────────────────────────────────────
// emit_expr
// ─────────────────────────────────────────────────────────────
pub fn emit_expr(expr: &HlExpr, prog: &mut BytecodeProgram, ra: &mut RegAlloc) -> (u8, bool) {
    match expr {
        HlExpr::Int(n) => {
            let dst = ra.alloc();
            prog.ops.push(OpCode::LoadInt { dst, val: *n });
            (dst, false)
        }
        HlExpr::Float(f) => {
            let dst = ra.alloc();
            prog.ops.push(OpCode::LoadFloat { dst, val: *f });
            (dst, true)
        }
        HlExpr::Bool(b) => {
            let dst = ra.alloc();
            prog.ops.push(OpCode::LoadBool { dst, val: *b });
            (dst, false)
        }
        HlExpr::Str(s) => {
            let str_id = prog.pool.intern(s);
            let dst    = ra.alloc();
            prog.ops.push(OpCode::LoadStr { dst, str_id });
            (dst, false)
        }
        HlExpr::Var(name) => {
            let var_id = prog.pool.intern(name);
            let dst    = ra.alloc();
            prog.ops.push(OpCode::LoadVarI { dst, var_id });
            (dst, false)
        }
        HlExpr::Add(a, b) => emit_binop(a, b, prog, ra, |dst, la, lb, is_float| {
            if is_float { OpCode::AddF { dst, a: la, b: lb } }
            else        { OpCode::AddI { dst, a: la, b: lb } }
        }),
        HlExpr::Sub(a, b) => emit_binop(a, b, prog, ra, |dst, la, lb, is_float| {
            if is_float { OpCode::SubF { dst, a: la, b: lb } }
            else        { OpCode::SubI { dst, a: la, b: lb } }
        }),
        HlExpr::Mul(a, b) => emit_binop(a, b, prog, ra, |dst, la, lb, is_float| {
            if is_float { OpCode::MulF { dst, a: la, b: lb } }
            else        { OpCode::MulI { dst, a: la, b: lb } }
        }),
        HlExpr::Div(a, b) => emit_binop(a, b, prog, ra, |dst, la, lb, is_float| {
            if is_float { OpCode::DivF { dst, a: la, b: lb } }
            else        { OpCode::DivI { dst, a: la, b: lb } }
        }),
        HlExpr::Mod(a, b) => emit_binop(a, b, prog, ra, |dst, la, lb, _| {
            OpCode::ModI { dst, a: la, b: lb }
        }),
        HlExpr::Neg(inner) => {
            let (src, is_float) = emit_expr(inner, prog, ra);
            let dst = ra.alloc();
            if is_float { prog.ops.push(OpCode::NegF { dst, src }); }
            else        { prog.ops.push(OpCode::NegI { dst, src }); }
            (dst, is_float)
        }
        HlExpr::Eq(a, b)  => emit_cmp(a, b, CmpOp::Eq, prog, ra),
        HlExpr::Ne(a, b)  => emit_cmp(a, b, CmpOp::Ne, prog, ra),
        HlExpr::Lt(a, b)  => emit_cmp(a, b, CmpOp::Lt, prog, ra),
        HlExpr::Le(a, b)  => emit_cmp(a, b, CmpOp::Le, prog, ra),
        HlExpr::Gt(a, b)  => emit_cmp(a, b, CmpOp::Gt, prog, ra),
        HlExpr::Ge(a, b)  => emit_cmp(a, b, CmpOp::Ge, prog, ra),
    }
}

fn emit_binop<F>(
    a: &HlExpr, b: &HlExpr,
    prog: &mut BytecodeProgram, ra: &mut RegAlloc,
    make_op: F,
) -> (u8, bool)
where F: Fn(u8, u8, u8, bool) -> OpCode
{
    let (la, fa) = emit_expr(a, prog, ra);
    let (lb, fb) = emit_expr(b, prog, ra);
    let is_float = fa || fb;

    let (la2, lb2) = if is_float {
        let la2 = if !fa {
            let dst = ra.alloc();
            prog.ops.push(OpCode::IntToFloat { dst, src: la });
            dst
        } else { la };
        let lb2 = if !fb {
            let dst = ra.alloc();
            prog.ops.push(OpCode::IntToFloat { dst, src: lb });
            dst
        } else { lb };
        (la2, lb2)
    } else { (la, lb) };

    let dst = ra.alloc();
    prog.ops.push(make_op(dst, la2, lb2, is_float));
    (dst, is_float)
}

fn emit_cmp(
    a: &HlExpr, b: &HlExpr, op: CmpOp,
    prog: &mut BytecodeProgram, ra: &mut RegAlloc,
) -> (u8, bool) {
    let (la, fa) = emit_expr(a, prog, ra);
    let (lb, fb) = emit_expr(b, prog, ra);
    let is_float = fa || fb;

    let (la2, lb2) = if is_float && (!fa || !fb) {
        let la2 = if !fa {
            let dst = ra.alloc(); prog.ops.push(OpCode::IntToFloat { dst, src: la }); dst
        } else { la };
        let lb2 = if !fb {
            let dst = ra.alloc(); prog.ops.push(OpCode::IntToFloat { dst, src: lb }); dst
        } else { lb };
        (la2, lb2)
    } else { (la, lb) };

    if is_float { prog.ops.push(OpCode::CmpF { a: la2, b: lb2, op }); }
    else        { prog.ops.push(OpCode::CmpI { a: la2, b: lb2, op }); }
    (0, false)
}

// ─────────────────────────────────────────────────────────────
// Klasyfikacja body gałęzi
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
    if is_hl_call(cmd) { BranchBody::HlCall(extract_hl_func(cmd)) }
    else               { BranchBody::Shell(shell_inline(cmd)) }
}

// ─────────────────────────────────────────────────────────────
// emit_if_block
// ─────────────────────────────────────────────────────────────
fn emit_if_block(branches: Vec<Branch>, prog: &mut BytecodeProgram) {
    let mut end_jumps: Vec<usize> = Vec::new();

    for branch in branches {
        let jif_idx: Option<usize> = branch.cond.map(|cond| {
            let cond_id = prog.pool.intern(&cond);
            let idx     = prog.ops.len();
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
// emit_match_block
// ─────────────────────────────────────────────────────────────
fn emit_match_block(
    cond: &str,
    arms: &[(String, String)],
                    sudo: bool,
                    prog: &mut BytecodeProgram,
) {
    if arms.is_empty() { return; }

    // FIX: cond może zawierać $var — użyj go bezpośrednio w case
    // Jeśli cond to "$var" → case "${var}" in
    let case_expr = if cond.starts_with('$') {
        // Zamień $var na "${var}" dla bezpieczeństwa
        let varname = cond.trim_start_matches('$');
        format!("\"${{{}}}\"", varname)
    } else {
        cond.to_string()
    };

    let mut sh = format!("case {} in\n", case_expr);
    for (val, cmd) in arms {
        let clean_val = if val == "_" { "*".to_string() }
        else {
            // Obsługuj wartości liczbowe i stringowe
            let v = val.trim().trim_matches('"').trim_matches('\'').to_string();
            v
        };
        let cmd_inline = shell_inline(cmd);
        sh += &format!("  {}) {};;\n", clean_val, cmd_inline);
    }
    sh += "esac";
    let cmd_id = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo });
}

// ─────────────────────────────────────────────────────────────
// emit_pipe
// ─────────────────────────────────────────────────────────────
fn emit_pipe(steps: &[String], sudo: bool, prog: &mut BytecodeProgram) {
    if steps.is_empty() { return; }
    let all_hl = steps.iter().all(|s| is_hl_call(s.trim()));
    if all_hl {
        for step in steps {
            let fname   = extract_hl_func(step);
            let func_id = prog.pool.intern(&fname);
            prog.ops.push(OpCode::CallFunc { func_id });
        }
    } else {
        let parts: Vec<String> = steps.iter().map(|s| {
            let t = s.trim();
            if is_hl_call(t) {
                // FIX: moduły HL w pipe → _hl_Module_method
                module_call_to_bash(t.trim_start_matches('.'), "")
            } else {
                shell_inline(t)
            }
        }).collect();
        let sh     = parts.join(" | ");
        let cmd_id = prog.pool.intern(&sh);
        prog.ops.push(OpCode::Exec { cmd_id, sudo });
    }
}

// ─────────────────────────────────────────────────────────────
// FIX 2: module_call_to_bash
//
// Zamienia path modułu na wywołanie bash:
//   "Logger.log"  args  →  "_hl_Logger_log args"
//   "list.filter" args  →  "_hl_list_filter args"
//   ".Logger.log" args  →  "_hl_Logger_log args"  (z wiodącą kropką)
//
// Konwencja _hl_<Module>_<method> jest spójna z vm.rs stdlibs.
// ─────────────────────────────────────────────────────────────
fn module_call_to_bash(path: &str, args: &str) -> String {
    let clean  = path.trim_start_matches('.');
    let bash_fn = format!("_hl_{}", clean.replace('.', "_"));
    if args.is_empty() {
        bash_fn
    } else {
        // FIX: przepisz wywołania HL w argumentach
        let args_rewritten = rewrite_hl_calls_in_expr(args);
        format!("{} {}", bash_fn, args_rewritten)
    }
}

// ─────────────────────────────────────────────────────────────
// emit_assign_local — wspólna logika dla AssignLocal + AssignExpr
// ─────────────────────────────────────────────────────────────
fn emit_assign_local(
    key: &str, val: &str, is_raw: bool, is_global: bool,
    prog: &mut BytecodeProgram,
) {
    // Próbuj numeric payload
    if let Some(payload) = parse_numeric_payload(val) {
        let mut ra = RegAlloc::new();
        match payload {
            NumericPayload::Typed { type_, expr } => {
                let (src, is_float) = emit_expr(&expr, prog, &mut ra);
                let var_id = prog.pool.intern(key);
                let src2 = if type_ == "float" && !is_float {
                    let dst = ra.alloc();
                    prog.ops.push(OpCode::IntToFloat { dst, src });
                    dst
                } else { src };
                if type_ == "float" || is_float {
                    prog.ops.push(OpCode::StoreVarF { var_id, src: src2 });
                    prog.ops.push(OpCode::FloatToStr { var_id, src: src2 });
                } else {
                    prog.ops.push(OpCode::StoreVarI { var_id, src: src2 });
                    prog.ops.push(OpCode::IntToStr   { var_id, src: src2 });
                }
            }
            NumericPayload::Arith { expr } => {
                let (src, is_float) = emit_expr(&expr, prog, &mut ra);
                let var_id = prog.pool.intern(key);
                if is_float {
                    prog.ops.push(OpCode::StoreVarF { var_id, src });
                    prog.ops.push(OpCode::FloatToStr { var_id, src });
                } else {
                    prog.ops.push(OpCode::StoreVarI { var_id, src });
                    prog.ops.push(OpCode::IntToStr   { var_id, src });
                }
            }
        }
        return;
    }

    // Sprawdź result unwrap
    if val.contains(" ?! ") {
        if let Some((expr, msg)) = parse_result_unwrap(val) {
            emit_result_unwrap_assign(key, &expr, &msg, is_global, prog);
            return;
        }
    }

    // FIX: przepisz wywołania HL w wartości
    let val_rewritten = rewrite_hl_calls_in_expr(val);

    let key_id = prog.pool.intern(key);
    let val_id = prog.pool.intern(&val_rewritten);
    if is_global {
        prog.ops.push(OpCode::SetEnv   { key_id, val_id });
    } else {
        prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw });
    }
}

// ─────────────────────────────────────────────────────────────
// Pomocnik: parse result unwrap
// ─────────────────────────────────────────────────────────────
fn parse_result_unwrap(val: &str) -> Option<(String, String)> {
    let pos = val.find(" ?! ")?;
    let expr = val[..pos].trim().to_string();
    let msg  = val[pos + 4..].trim().trim_matches('"').to_string();
    Some((expr, msg))
}

// ─────────────────────────────────────────────────────────────
// emit_result_unwrap_assign
// ─────────────────────────────────────────────────────────────
fn emit_result_unwrap_assign(
    key: &str, expr: &str, msg: &str, is_global: bool,
    prog: &mut BytecodeProgram,
) {
    let clean  = expr.trim_start_matches('.');
    let bailout = format!(
        "( {} ) || {{ echo '{}' >&2; exit 1; }}",
                          clean, msg
    );
    let bailout_id = prog.pool.intern(&bailout);
    prog.ops.push(OpCode::Exec { cmd_id: bailout_id, sudo: false });

    let capture = format!("$({})", clean);
    let key_id  = prog.pool.intern(key);
    let val_id  = prog.pool.intern(&capture);
    if is_global {
        prog.ops.push(OpCode::SetEnv   { key_id, val_id });
    } else {
        prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: false });
    }
}

// ─────────────────────────────────────────────────────────────
// emit_collection_mut — $var.method args
// ─────────────────────────────────────────────────────────────
fn emit_collection_mut(var: &str, method: &str, args: &str, sudo: bool, prog: &mut BytecodeProgram) {
    let sh = match method {
        "push" => format!("_HL_COLL_PUSH {} {}", var, args),
        "pop"  => format!("_HL_COLL_POP {}", var),
        "set"  => format!("_HL_COLL_SET {} {}", var, args),
        "del"  => format!("_HL_COLL_DEL {} {}", var, args),
        "get"  => format!("_HL_COLL_GET {} {}", var, args),
        other  => format!("_HL_COLL_{} {} {}", other.to_uppercase(), var, args),
    };
    let cmd_id = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo });
}

// ─────────────────────────────────────────────────────────────
// emit_defer
// ─────────────────────────────────────────────────────────────
fn emit_defer(expr: &str, sudo: bool, prog: &mut BytecodeProgram) {
    let clean  = expr.trim_start_matches('.');
    let sh     = format!("_HL_DEFER_PUSH {}", clean);
    let cmd_id = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo });
}

// ─────────────────────────────────────────────────────────────
// FIX 3: emit_module_call
//
// PRZED: hl_module_Logger_log args
// PO:    _hl_Logger_log args
//
// Konwencja _hl_<Module>_<method> pozwala vm.rs dostarczyć
// te funkcje jako bash functions w stdlib.
// ─────────────────────────────────────────────────────────────
fn emit_module_call(path: &str, args: &str, sudo: bool, prog: &mut BytecodeProgram) {
    let sh     = module_call_to_bash(path, args);
    let cmd_id = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo });
}

// ─────────────────────────────────────────────────────────────
// emit_lambda — { $x -> body } standalone
// ─────────────────────────────────────────────────────────────
fn emit_lambda(params: &[String], body: &str, prog: &mut BytecodeProgram) {
    let params_str = params.join(",");
    let sh         = format!("_HL_LAMBDA_PUSH {} : {}", params_str, body);
    let cmd_id     = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo: false });
}

// ─────────────────────────────────────────────────────────────
// emit_assign_lambda
// ─────────────────────────────────────────────────────────────
fn emit_assign_lambda(
    key: &str, params: &[String], body: &str,
    is_raw: bool, is_global: bool,
    prog: &mut BytecodeProgram,
) {
    let params_str = params.join(",");
    let encoded    = format!("__hl_lambda:{}:{}", params_str, body);
    let key_id     = prog.pool.intern(key);
    let val_id     = prog.pool.intern(&encoded);
    if is_global {
        prog.ops.push(OpCode::SetEnv   { key_id, val_id });
    } else {
        prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw });
    }
}

// ─────────────────────────────────────────────────────────────
// emit_recur
// ─────────────────────────────────────────────────────────────
fn emit_recur(args: &str, sudo: bool, prog: &mut BytecodeProgram) {
    if !args.is_empty() {
        let set_args_id = prog.pool.intern(&format!("_HL_RECUR_ARGS {}", args));
        prog.ops.push(OpCode::Exec { cmd_id: set_args_id, sudo });
    }
    let recur_id = prog.pool.intern("_HL_RECUR");
    prog.ops.push(OpCode::Exec { cmd_id: recur_id, sudo });
}

// ─────────────────────────────────────────────────────────────
// FIX 4: emit_destruct_list — [head | tail] = $source
//
// source może być:
//   "$var"      → ${var[0]}, ${var[@]:1}   (bash array)
//   "[1,2,3]"   → tworzy tymczasową tablicę __hl_tmp_arr
// ─────────────────────────────────────────────────────────────
fn emit_destruct_list(head: &str, tail: &str, source: &str, prog: &mut BytecodeProgram) {
    let src_trimmed = source.trim();

    if src_trimmed.starts_with('[') && src_trimmed.ends_with(']') {
        // Literalna lista — utwórz tymczasową tablicę bash
        let inner   = &src_trimmed[1..src_trimmed.len() - 1];
        let items: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        let arr_items = items.join(" ");
        // declare -a __hl_tmp_arr=( items )
        let decl_sh = format!("declare -a __hl_tmp_arr=( {} )", arr_items);
        let decl_id = prog.pool.intern(&decl_sh);
        prog.ops.push(OpCode::Exec { cmd_id: decl_id, sudo: false });

        let hk = prog.pool.intern(head);
        let hv = prog.pool.intern("${__hl_tmp_arr[0]}");
        prog.ops.push(OpCode::SetLocal { key_id: hk, val_id: hv, is_raw: false });

        let tk = prog.pool.intern(tail);
        let tv = prog.pool.intern("${__hl_tmp_arr[@]:1}");
        prog.ops.push(OpCode::SetLocal { key_id: tk, val_id: tv, is_raw: false });
    } else {
        // Zmienna — normalny przypadek
        let varname = src_trimmed.trim_start_matches('$');
        let head_val = format!("${{{}[0]}}", varname);
        let tail_val = format!("${{{}[@]:1}}", varname);

        let hk = prog.pool.intern(head);
        let hv = prog.pool.intern(&head_val);
        prog.ops.push(OpCode::SetLocal { key_id: hk, val_id: hv, is_raw: false });

        let tk = prog.pool.intern(tail);
        let tv = prog.pool.intern(&tail_val);
        prog.ops.push(OpCode::SetLocal { key_id: tk, val_id: tv, is_raw: false });
    }
}

// ─────────────────────────────────────────────────────────────
// FIX 5: emit_destruct_map — {field1, field2} = $source
//
// PRZED: ${source[field]}          — nieprawidłowy identyfikator bash
// PO:    ${source["field"]}        — bash associative array
//      + declare -A source jeśli nie zadeklarowano
// ─────────────────────────────────────────────────────────────
fn emit_destruct_map(fields: &[String], source: &str, prog: &mut BytecodeProgram) {
    let src = source.trim_start_matches('$');

    // Emituj declare -A dla associative array (idempotentne w bash)
    let decl_sh = format!("declare -A {} 2>/dev/null || true", src);
    let decl_id = prog.pool.intern(&decl_sh);
    prog.ops.push(OpCode::Exec { cmd_id: decl_id, sudo: false });

    for field in fields {
        // FIX: ${source["field"]} zamiast ${source[field]}
        let val    = format!("${{{}[\"{}\"]}}", src, field);
        let key_id = prog.pool.intern(field);
        let val_id = prog.pool.intern(&val);
        prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: false });
    }
}

// ─────────────────────────────────────────────────────────────
// emit_adt_def
// ─────────────────────────────────────────────────────────────
fn emit_adt_def(
    name: &str,
    variants: &[(String, Vec<(String, String)>)],
                prog: &mut BytecodeProgram,
) {
    for (variant, fields) in variants {
        let fields_str: Vec<String> = fields.iter()
        .map(|(f, t)| format!("{}:{}", f, t))
        .collect();
        let fields_joined = fields_str.join(",");
        let sh = if fields_joined.is_empty() {
            format!("_HL_ADT_DEF {} {}", name, variant)
        } else {
            format!("_HL_ADT_DEF {} {} {}", name, variant, fields_joined)
        };
        let cmd_id = prog.pool.intern(&sh);
        prog.ops.push(OpCode::Exec { cmd_id, sudo: false });
    }
}

// ─────────────────────────────────────────────────────────────
// compile_body — główna pętla
// ─────────────────────────────────────────────────────────────
pub fn compile_body(nodes: &[ProgramNode], prog: &mut BytecodeProgram) {
    let mut i = 0;
    while i < nodes.len() {
        let node = &nodes[i];
        let sudo = node.is_sudo;

        match &node.content {
            CommandType::If { cond, cmd } => {
                let mut branches = vec![Branch {
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

            CommandType::Match { cond } => {
                let mut arms: Vec<(String, String)> = Vec::new();
                i += 1;
                while i < nodes.len() {
                    if let CommandType::MatchArm { val, cmd } = &nodes[i].content {
                        arms.push((val.clone(), cmd.clone()));
                        i += 1;
                    } else { break; }
                }
                emit_match_block(cond, &arms, sudo, prog);
                continue;
            }

            CommandType::MatchArm { .. } => { i += 1; continue; }

            CommandType::Pipe(steps) => {
                emit_pipe(steps, sudo, prog);
                i += 1;
                continue;
            }

            CommandType::ArenaDef { .. } => {
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
// compile_arena_func
// ─────────────────────────────────────────────────────────────
fn compile_arena_func(
    name: &str,
    size: &str,
    nodes: &[ProgramNode],
    prog: &mut BytecodeProgram,
) {
    let name_id = prog.pool.intern(name);
    let size_id = prog.pool.intern(size);
    prog.ops.push(OpCode::ArenaEnter { name_id, size_id });
    compile_body(nodes, prog);
    prog.ops.push(OpCode::ArenaExit);
    prog.ops.push(OpCode::Return);
}

// ─────────────────────────────────────────────────────────────
// FIX 6: compile_node — Assert z (.func args)
// ─────────────────────────────────────────────────────────────
fn compile_node(node: &ProgramNode, prog: &mut BytecodeProgram) {
    let sudo = node.is_sudo;

    match &node.content {

        CommandType::RawNoSub(s) | CommandType::RawSub(s) => {
            let cmd_id = prog.pool.intern(s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::Isolated(s) => {
            let cmd    = format!("( {} )", s);
            let cmd_id = prog.pool.intern(&cmd);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::AssignEnv { key, val } => {
            // FIX: przepisz HL calls w wartości
            let val_r  = rewrite_hl_calls_in_expr(val);
            let key_id = prog.pool.intern(key);
            let val_id = prog.pool.intern(&val_r);
            prog.ops.push(OpCode::SetEnv { key_id, val_id });
        }

        CommandType::AssignLocal { key, val, is_raw } => {
            emit_assign_local(key, val, *is_raw, false, prog);
        }

        CommandType::AssignExpr { key, expr, is_raw, is_global } => {
            emit_assign_local(key, expr, *is_raw, *is_global, prog);
        }

        CommandType::Const { key, val } => {
            let key_id = prog.pool.intern(key);
            let val_id = prog.pool.intern(val);
            prog.ops.push(OpCode::SetConst { key_id, val_id });
        }

        CommandType::Loop { count, cmd } => {
            if *count == 0 {
                if let Some(nf) = parse_numfor_payload(cmd) {
                    let var_id = prog.pool.intern(&nf.var);
                    let cmd_id = prog.pool.intern(&nf.cmd);
                    prog.ops.push(OpCode::NumForExec {
                        var_id, start: nf.start, end: nf.end,
                        step: nf.step, cmd_id, sudo,
                    });
                    return;
                }
            }
            let s      = format!("for _hl_i in $(seq 1 {}); do {}; done", count, cmd);
            let cmd_id = prog.pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::While { cond, cmd } => {
            if let Some(we) = parse_whileexpr_payload(cond, cmd) {
                let mut ra       = RegAlloc::new();
                let (lhs_reg, _) = emit_expr(&we.lhs, prog, &mut ra);
                let (rhs_reg, _) = emit_expr(&we.rhs, prog, &mut ra);
                let cmd_id       = prog.pool.intern(&we.cmd);
                prog.ops.push(OpCode::WhileExprExec {
                    lhs_reg, op: we.op, rhs_reg, cmd_id, sudo,
                });
                return;
            }
            let s      = format!("while {}; do {}; done", wrap_cond(cond), cmd);
            let cmd_id = prog.pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::For { var, in_, cmd } => {
            let s      = format!("for {} in {}; do {}; done", var, in_, cmd);
            let cmd_id = prog.pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::Background(s) => {
            let bg     = format!("{} &", s);
            let cmd_id = prog.pool.intern(&bg);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::Spawn(task) => {
            let clean  = task.trim().trim_start_matches('.');
            let cmd_id = prog.pool.intern(clean);
            prog.ops.push(OpCode::SpawnBg { cmd_id, sudo });
        }

        CommandType::AssignSpawn { key, task } => {
            let clean  = task.trim().trim_start_matches('.');
            let key_id = prog.pool.intern(key);
            let cmd_id = prog.pool.intern(clean);
            prog.ops.push(OpCode::SpawnAssign { key_id, cmd_id, sudo });
        }

        CommandType::Await(expr) => {
            let expr_id = prog.pool.intern(expr.trim());
            prog.ops.push(OpCode::AwaitPid { expr_id });
        }

        CommandType::AssignAwait { key, expr } => {
            let key_id  = prog.pool.intern(key);
            let expr_id = prog.pool.intern(expr.trim());
            prog.ops.push(OpCode::AwaitAssign { key_id, expr_id });
        }

        CommandType::Call { path, args } => {
            let fname   = path.trim_start_matches('.');
            let func_id = prog.pool.intern(fname);
            if !args.is_empty() {
                let key_id = prog.pool.intern("_HL_ARGS");
                // FIX: przepisz HL calls w argumentach
                let args_r = rewrite_hl_calls_in_expr(args);
                let val_id = prog.pool.intern(&args_r);
                prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: false });
            }
            prog.ops.push(OpCode::CallFunc { func_id });
        }

        CommandType::ModuleCall { path, args } => {
            emit_module_call(path, args, sudo, prog);
        }

        CommandType::Plugin { name, args, is_super } => {
            let name_id = prog.pool.intern(name);
            let args_id = prog.pool.intern(args);
            prog.ops.push(OpCode::Plugin { name_id, args_id, sudo: *is_super });
        }

        CommandType::Log(msg) => {
            let s      = format!("echo {}", msg);
            let cmd_id = prog.pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::Out(val) => {
            if let Some(expr) = parse_retexpr_payload(val) {
                let mut ra          = RegAlloc::new();
                let (src, is_float) = emit_expr(&expr, prog, &mut ra);
                if is_float { prog.ops.push(OpCode::ReturnF { src }); }
                else        { prog.ops.push(OpCode::ReturnI { src }); }
                return;
            }
            // FIX: przepisz HL calls w out
            let val_r  = rewrite_hl_calls_in_expr(val);
            let val_id = prog.pool.intern(&val_r);
            prog.ops.push(OpCode::SetOut { val_id });
        }

        CommandType::End { code } => {
            prog.ops.push(OpCode::Exit(*code));
        }

        CommandType::Lock { key, val } => {
            let key_id = prog.pool.intern(key);
            let val_id = prog.pool.intern(val);
            prog.ops.push(OpCode::Lock { key_id, val_id });
        }

        CommandType::Unlock { key } => {
            let key_id = prog.pool.intern(key);
            prog.ops.push(OpCode::Unlock { key_id });
        }

        // ── FIX 7: Assert ────────────────────────────────────
        // (.add 2 3) == 5  →  $(_hl_add 2 3) == 5
        // Przepisujemy cond PRZED internowaniem do pool.
        CommandType::Assert { cond, msg } => {
            let cond_rewritten = rewrite_hl_calls_in_expr(cond);
            let cond_id = prog.pool.intern(&cond_rewritten);
            let msg_id  = msg.as_deref().map(|m| prog.pool.intern(m));
            prog.ops.push(OpCode::Assert { cond_id, msg_id });
        }

        CommandType::Try { try_cmd, catch_cmd } => {
            // FIX: przepisz HL calls w obu gałęziach
            let try_r   = rewrite_hl_calls_in_expr(try_cmd);
            let catch_r = rewrite_hl_calls_in_expr(catch_cmd);
            let s      = format!("( {} ) || ( {} )", try_r, catch_r);
            let cmd_id = prog.pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::ResultUnwrap { expr, msg } => {
            let clean  = expr.trim_start_matches('.');
            let sh     = format!(
                "( {} ) || {{ echo '{}' >&2; exit 1; }}",
                                 clean, msg
            );
            let cmd_id = prog.pool.intern(&sh);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::CollectionMut { var, method, args } => {
            emit_collection_mut(var, method, args, sudo, prog);
        }

        CommandType::Lambda { params, body } => {
            emit_lambda(params, body, prog);
        }

        CommandType::AssignLambda { key, params, body, is_raw, is_global } => {
            emit_assign_lambda(key, params, body, *is_raw, *is_global, prog);
        }

        CommandType::Recur { args } => {
            emit_recur(args, sudo, prog);
        }

        CommandType::DestructList { head, tail, source } => {
            emit_destruct_list(head, tail, source, prog);
        }

        CommandType::DestructMap { fields, source } => {
            emit_destruct_map(fields, source, prog);
        }

        CommandType::AdtDef { name, variants } => {
            emit_adt_def(name, variants, prog);
        }

        CommandType::Defer { expr } => {
            emit_defer(expr, sudo, prog);
        }

        CommandType::PipeLine { step } => {
            let s      = shell_inline(step);
            let cmd_id = prog.pool.intern(&s);
            prog.ops.push(OpCode::PipeExec { cmd_id, sudo });
        }

        CommandType::DoBlock { key, body } => {
            compile_body(body, prog);
            let key_id = prog.pool.intern(key);
            let val_id = prog.pool.intern("${_HL_DO_OUT}");
            prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: false });
        }

        CommandType::TestBlock { desc, body } => {
            let header    = format!("_HL_TEST_BEGIN {}", desc);
            let header_id = prog.pool.intern(&header);
            prog.ops.push(OpCode::Exec { cmd_id: header_id, sudo: false });
            compile_body(body, prog);
            let footer    = format!("_HL_TEST_END {}", desc);
            let footer_id = prog.pool.intern(&footer);
            prog.ops.push(OpCode::Exec { cmd_id: footer_id, sudo: false });
        }

        CommandType::Interface { name, methods } => {
            let methods_str = methods.join(",");
            let sh          = format!("_HL_IFACE_DEF {} {}", name, methods_str);
            let cmd_id      = prog.pool.intern(&sh);
            prog.ops.push(OpCode::Exec { cmd_id, sudo: false });
        }

        CommandType::ImplDef { class, interface } => {
            let sh     = format!("_HL_IMPL_DEF {} {}", class, interface);
            let cmd_id = prog.pool.intern(&sh);
            prog.ops.push(OpCode::Exec { cmd_id, sudo: false });
        }

        CommandType::ScopeDef => {
            let cmd_id = prog.pool.intern("_HL_SCOPE_ENTER");
            prog.ops.push(OpCode::Exec { cmd_id, sudo: false });
        }

        CommandType::FuncDefGeneric { .. } => {}

        CommandType::Extern { .. }
        | CommandType::Enum   { .. }
        | CommandType::Struct { .. }
        | CommandType::Import { .. } => {}

        CommandType::If      { .. }
        | CommandType::Elif   { .. }
        | CommandType::Else   { .. }
        | CommandType::Match  { .. }
        | CommandType::MatchArm { .. }
        | CommandType::Pipe(_)
        | CommandType::ArenaDef { .. } => {}
    }
}

// ─────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────
pub fn compile_to_bytecode(ast: &AnalysisResult) -> BytecodeProgram {
    let mut prog = BytecodeProgram::new();
    compile_body(&ast.main_body, &mut prog);
    prog.ops.push(OpCode::Exit(0));

    for (name, (is_arena_fn, _sig, nodes)) in &ast.functions {
        prog.functions.insert(name.clone(), prog.ops.len());
        if *is_arena_fn {
            let size = extract_arena_size(_sig.as_deref().unwrap_or("[arena:64b]"));
            compile_arena_func(name, &size, nodes, &mut prog);
        } else {
            compile_body(nodes, &mut prog);
            prog.ops.push(OpCode::Return);
        }
    }

    prog
}

// ─────────────────────────────────────────────────────────────
// Pomocnik: wyciągnij rozmiar areny
// ─────────────────────────────────────────────────────────────
fn extract_arena_size(sig: &str) -> String {
    if let Some(inner) = sig.strip_prefix("[arena:") {
        if let Some(size) = inner.strip_suffix(']') {
            return size.to_string();
        }
    }
    "64kb".to_string()
}
