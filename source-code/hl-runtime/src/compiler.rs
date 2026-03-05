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
// Pomocniki
// ─────────────────────────────────────────────────────────────
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
        return format!("export _HL_OUT={}", r);
    }
    if let Some(r) = t.strip_prefix("> ")  { return r.to_string(); }
    if let Some(r) = t.strip_prefix('>')   { return r.trim().to_string(); }
    t.to_string()
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
    let mut sh = format!("case {} in\n", cond);
    for (val, cmd) in arms {
        let clean_val = if val == "_" { "*".to_string() }
        else { val.trim_matches('"').trim_matches('\'').to_string() };
        sh += &format!("  {}) {};;\n", clean_val, shell_inline(cmd));
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
            if is_hl_call(t) { t.trim_start_matches('.').to_string() }
            else             { shell_inline(t) }
        }).collect();
        let sh     = parts.join(" | ");
        let cmd_id = prog.pool.intern(&sh);
        prog.ops.push(OpCode::Exec { cmd_id, sudo });
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

    // Sprawdź result unwrap w wartości: val ?! "msg"
    if val.contains(" ?! ") {
        if let Some((expr, msg)) = parse_result_unwrap(val) {
            emit_result_unwrap_assign(key, &expr, &msg, is_global, prog);
            return;
        }
    }

    // Zwykłe przypisanie
    let key_id = prog.pool.intern(key);
    let val_id = prog.pool.intern(val);
    if is_global {
        prog.ops.push(OpCode::SetEnv   { key_id, val_id });
    } else {
        prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw });
    }
}

// ─────────────────────────────────────────────────────────────
// Pomocnik: parse result unwrap z wartości
// ─────────────────────────────────────────────────────────────
fn parse_result_unwrap(val: &str) -> Option<(String, String)> {
    let pos = val.find(" ?! ")?;
    let expr = val[..pos].trim().to_string();
    let msg  = val[pos + 4..].trim().trim_matches('"').to_string();
    Some((expr, msg))
}

// ─────────────────────────────────────────────────────────────
// emit_result_unwrap_assign — key = expr ?! "msg"
//
// Kompiluje do:
//   if ! ( <expr> ); then echo "msg" >&2; exit 1; fi
//   key = $(<expr>)
// przez SetLocal + warunkowe Exec z bailout
// ─────────────────────────────────────────────────────────────
fn emit_result_unwrap_assign(
    key: &str, expr: &str, msg: &str, is_global: bool,
    prog: &mut BytecodeProgram,
) {
    // Bailout jeśli expr jest komendą kończącą się błędem
    let bailout = format!(
        "( {} ) || {{ echo '{}' >&2; exit 1; }}",
                          expr.trim_start_matches('.'), msg
    );
    let bailout_id = prog.pool.intern(&bailout);
    prog.ops.push(OpCode::Exec { cmd_id: bailout_id, sudo: false });

    // Przypisz wynik
    let capture = format!("$({})", expr.trim_start_matches('.'));
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
    // Kompilujemy do shell: _HL_ARR_<var>_push <args> itp.
    // W runtime vm.rs obsługuje SetLocal z flagą specjalną,
    // tutaj emitujemy jako Exec z konwencją _HL_COLL_<method>
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
// emit_defer — defer expr
//
// defer jest rejestrowane na stosie defer w VM przez Exec z
// konwencją _HL_DEFER_PUSH. Wykonanie następuje przy Return/Exit.
// ─────────────────────────────────────────────────────────────
fn emit_defer(expr: &str, sudo: bool, prog: &mut BytecodeProgram) {
    let clean  = expr.trim_start_matches('.');
    let sh     = format!("_HL_DEFER_PUSH {}", clean);
    let cmd_id = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo });
}

// ─────────────────────────────────────────────────────────────
// emit_module_call — module.method args
// ─────────────────────────────────────────────────────────────
fn emit_module_call(path: &str, args: &str, sudo: bool, prog: &mut BytecodeProgram) {
    // path = "http.get", args = "\"url\""
    // kompilujemy do: hl_module_<modul>_<metoda> args
    let sh = if args.is_empty() {
        format!("hl_module_{}", path.replace('.', "_"))
    } else {
        format!("hl_module_{} {}", path.replace('.', "_"), args)
    };
    let cmd_id = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo });
}

// ─────────────────────────────────────────────────────────────
// emit_lambda — { $x -> body } standalone
//
// Lambda standalone (np. jako argument) jest internowana jako
// string w pool i przekazywana do vm.rs jako _HL_LAMBDA_PUSH.
// ─────────────────────────────────────────────────────────────
fn emit_lambda(params: &[String], body: &str, prog: &mut BytecodeProgram) {
    let params_str = params.join(",");
    let sh         = format!("_HL_LAMBDA_PUSH {} : {}", params_str, body);
    let cmd_id     = prog.pool.intern(&sh);
    prog.ops.push(OpCode::Exec { cmd_id, sudo: false });
}

// ─────────────────────────────────────────────────────────────
// emit_assign_lambda — key = { $x -> body }
// ─────────────────────────────────────────────────────────────
fn emit_assign_lambda(
    key: &str, params: &[String], body: &str,
    is_raw: bool, is_global: bool,
    prog: &mut BytecodeProgram,
) {
    // Zakoduj lambdę jako string: "__hl_lambda:<params>:<body>"
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
// emit_recur — recur args
//
// Rekurencja ogonowa: Jump z powrotem na początek bieżącej funkcji.
// Używamy konwencji _HL_RECUR_ARGS dla argumentów, a skok
// jest emitowany jako Exec z konwencją _HL_RECUR, które vm.rs
// traktuje jako skok na bieżący adres bazowy funkcji.
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
// emit_destruct_list — [head | tail] = $source
// ─────────────────────────────────────────────────────────────
fn emit_destruct_list(head: &str, tail: &str, source: &str, prog: &mut BytecodeProgram) {
    // head = ${source}[0], tail = ${source}[@]:1
    // Emitujemy dwa SetLocal
    let head_val = format!("${{{}[0]}}", source.trim_start_matches('$'));
    let tail_val = format!(
        "${{{}[@]:1}}",
        source.trim_start_matches('$')
    );

    let hk = prog.pool.intern(head);
    let hv = prog.pool.intern(&head_val);
    prog.ops.push(OpCode::SetLocal { key_id: hk, val_id: hv, is_raw: false });

    let tk = prog.pool.intern(tail);
    let tv = prog.pool.intern(&tail_val);
    prog.ops.push(OpCode::SetLocal { key_id: tk, val_id: tv, is_raw: false });
}

// ─────────────────────────────────────────────────────────────
// emit_destruct_map — {field1, field2} = $source
// ─────────────────────────────────────────────────────────────
fn emit_destruct_map(fields: &[String], source: &str, prog: &mut BytecodeProgram) {
    let src = source.trim_start_matches('$');
    for field in fields {
        // field = ${source[field]} (bash associative array)
        let val = format!("${{{}[{}]}}", src, field);
        let key_id = prog.pool.intern(field);
        let val_id = prog.pool.intern(&val);
        prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: false });
    }
}

// ─────────────────────────────────────────────────────────────
// emit_adt_def — ==type Name [Variant1 [...], ...]
//
// ADT jest informacją typową — w runtime rejestrujemy konstruktory
// jako callable przez konwencję _HL_ADT_DEF.
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
            // ── Bloki wielowęzłowe ────────────────────────────
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

            // ── Arena :: name [size] def...done ───────────────
            // Funkcja arenowa jest zarejestrowana w ast.functions
            // z flagą is_arena_fn=true. Tutaj emitujemy ArenaEnter
            // na początku ciała i ArenaExit na końcu.
            // compile_to_bytecode() wywołuje compile_arena_func()
            // zamiast compile_body() dla takich funkcji.
            // W main_body ArenaDef to jednolinijkowe odwołanie — ignoruj.
            CommandType::ArenaDef { .. } => {
                // ArenaDef w main_body = deklaracja bez ciała.
                // Ciało jest w ast.functions i obsługiwane przez
                // compile_to_bytecode() przez compile_arena_func().
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
// compile_arena_func — funkcja z :: name [size] def...done
//
// Emituje:
//   ArenaEnter { name_id, size_id }
//   ... ciało (compile_body) ...
//   ArenaExit
//   Return
//
// GC nie jest zaangażowany — arena jest zwalniana jednym
// hl_arena_free() przez ArenaExit → hl_jit_arena_exit().
// ─────────────────────────────────────────────────────────────
fn compile_arena_func(
    name: &str,
    size: &str,
    nodes: &[ProgramNode],
    prog: &mut BytecodeProgram,
) {
    let name_id = prog.pool.intern(name);
    let size_id = prog.pool.intern(size);

    // Wejście w areną
    prog.ops.push(OpCode::ArenaEnter { name_id, size_id });

    // Ciało funkcji — normalnie przez compile_body
    compile_body(nodes, prog);

    // Wyjście z areny — jednorazowy hl_arena_free()
    prog.ops.push(OpCode::ArenaExit);
    prog.ops.push(OpCode::Return);
}

// ─────────────────────────────────────────────────────────────
// compile_node — pojedynczy węzeł
// ─────────────────────────────────────────────────────────────
fn compile_node(node: &ProgramNode, prog: &mut BytecodeProgram) {
    let sudo = node.is_sudo;

    match &node.content {

        // ── Komendy surowe ────────────────────────────────────
        CommandType::RawNoSub(s) | CommandType::RawSub(s) => {
            let cmd_id = prog.pool.intern(s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }
        CommandType::Isolated(s) => {
            let cmd    = format!("( {} )", s);
            let cmd_id = prog.pool.intern(&cmd);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        // ── Przypisania ───────────────────────────────────────
        CommandType::AssignEnv { key, val } => {
            let key_id = prog.pool.intern(key);
            let val_id = prog.pool.intern(val);
            prog.ops.push(OpCode::SetEnv { key_id, val_id });
        }

        CommandType::AssignLocal { key, val, is_raw } => {
            emit_assign_local(key, val, *is_raw, false, prog);
        }

        // AssignExpr: key = expr_z_operatorem / lista / mapa
        CommandType::AssignExpr { key, expr, is_raw, is_global } => {
            emit_assign_local(key, expr, *is_raw, *is_global, prog);
        }

        // ── Stała % ───────────────────────────────────────────
        CommandType::Const { key, val } => {
            let key_id = prog.pool.intern(key);
            let val_id = prog.pool.intern(val);
            prog.ops.push(OpCode::SetConst { key_id, val_id });
        }

        // ── Pętle ─────────────────────────────────────────────
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

        // ── Asynchroniczność ──────────────────────────────────
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

        // ── Wywołania funkcji ─────────────────────────────────
        CommandType::Call { path, args } => {
            let fname   = path.trim_start_matches('.');
            let func_id = prog.pool.intern(fname);
            if !args.is_empty() {
                let key_id = prog.pool.intern("_HL_ARGS");
                let val_id = prog.pool.intern(args);
                prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: false });
            }
            prog.ops.push(OpCode::CallFunc { func_id });
        }

        CommandType::ModuleCall { path, args } => {
            emit_module_call(path, args, sudo, prog);
        }

        // ── Pluginy ───────────────────────────────────────────
        CommandType::Plugin { name, args, is_super } => {
            let name_id = prog.pool.intern(name);
            let args_id = prog.pool.intern(args);
            prog.ops.push(OpCode::Plugin { name_id, args_id, sudo: *is_super });
        }

        // ── I/O ───────────────────────────────────────────────
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
            let val_id = prog.pool.intern(val);
            prog.ops.push(OpCode::SetOut { val_id });
        }

        CommandType::End { code } => {
            prog.ops.push(OpCode::Exit(*code));
        }

        // ── Pamięć / GC ───────────────────────────────────────
        CommandType::Lock { key, val } => {
            let key_id = prog.pool.intern(key);
            let val_id = prog.pool.intern(val);
            prog.ops.push(OpCode::Lock { key_id, val_id });
        }

        CommandType::Unlock { key } => {
            let key_id = prog.pool.intern(key);
            prog.ops.push(OpCode::Unlock { key_id });
        }

        // ── Asercje ───────────────────────────────────────────
        CommandType::Assert { cond, msg } => {
            let cond_id = prog.pool.intern(cond);
            let msg_id  = msg.as_deref().map(|m| prog.pool.intern(m));
            prog.ops.push(OpCode::Assert { cond_id, msg_id });
        }

        // ── Obsługa błędów ────────────────────────────────────
        CommandType::Try { try_cmd, catch_cmd } => {
            let s      = format!("( {} ) || ( {} )", try_cmd, catch_cmd);
            let cmd_id = prog.pool.intern(&s);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        CommandType::ResultUnwrap { expr, msg } => {
            // Standalone: expr ?! "msg" (bez przypisania)
            let sh     = format!(
                "( {} ) || {{ echo '{}' >&2; exit 1; }}",
                                 expr.trim_start_matches('.'), msg
            );
            let cmd_id = prog.pool.intern(&sh);
            prog.ops.push(OpCode::Exec { cmd_id, sudo });
        }

        // ── Kolekcje ──────────────────────────────────────────
        CommandType::CollectionMut { var, method, args } => {
            emit_collection_mut(var, method, args, sudo, prog);
        }

        // ── Lambdy / domknięcia ───────────────────────────────
        CommandType::Lambda { params, body } => {
            emit_lambda(params, body, prog);
        }

        CommandType::AssignLambda { key, params, body, is_raw, is_global } => {
            emit_assign_lambda(key, params, body, *is_raw, *is_global, prog);
        }

        // ── Rekurencja ogonowa ────────────────────────────────
        CommandType::Recur { args } => {
            emit_recur(args, sudo, prog);
        }

        // ── Destrukturyzacja ──────────────────────────────────
        CommandType::DestructList { head, tail, source } => {
            emit_destruct_list(head, tail, source, prog);
        }

        CommandType::DestructMap { fields, source } => {
            emit_destruct_map(fields, source, prog);
        }

        // ── Typy algebraiczne ─────────────────────────────────
        CommandType::AdtDef { name, variants } => {
            emit_adt_def(name, variants, prog);
        }

        // ── Defer ─────────────────────────────────────────────
        CommandType::Defer { expr } => {
            emit_defer(expr, sudo, prog);
        }

        // ── Wieloliniowy pipe ─────────────────────────────────
        CommandType::PipeLine { step } => {
            // Krok potoku jako standalone Exec
            let s      = shell_inline(step);
            let cmd_id = prog.pool.intern(&s);
            prog.ops.push(OpCode::PipeExec { cmd_id, sudo });
        }

        // ── Do-blok ───────────────────────────────────────────
        CommandType::DoBlock { key, body } => {
            // Kompiluj ciało sekwencyjnie, wynik w _HL_DO_OUT
            compile_body(body, prog);
            // Przypisz _HL_DO_OUT do key
            let key_id = prog.pool.intern(key);
            let val_id = prog.pool.intern("${_HL_DO_OUT}");
            prog.ops.push(OpCode::SetLocal { key_id, val_id, is_raw: false });
        }

        // ── Testy jednostkowe ─────────────────────────────────
        CommandType::TestBlock { desc, body } => {
            // Emituj nagłówek testu
            let header     = format!("_HL_TEST_BEGIN {}", desc);
            let header_id  = prog.pool.intern(&header);
            prog.ops.push(OpCode::Exec { cmd_id: header_id, sudo: false });

            // Kompiluj ciało testu (asercje itp.)
            compile_body(body, prog);

            // Emituj zakończenie testu
            let footer    = format!("_HL_TEST_END {}", desc);
            let footer_id = prog.pool.intern(&footer);
            prog.ops.push(OpCode::Exec { cmd_id: footer_id, sudo: false });
        }

        // ── Interfejsy / impl — metadane typowe ──────────────
        // Nie generują kodu wykonywalnego — tylko informacje dla
        // type checkera (przyszłość). Emitujemy marker dla debugowania.
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

        // ── Zasięg leksykalny ─────────────────────────────────
        CommandType::ScopeDef => {
            // ;;scope def tworzy nowy zakres — nie generuje kodu VM,
            // zakres jest obsługiwany przez parser przez push/pop Scope::Class.
            // W runtime wystarczy marker dla debugowania.
            let cmd_id = prog.pool.intern("_HL_SCOPE_ENTER");
            prog.ops.push(OpCode::Exec { cmd_id, sudo: false });
        }

        // ── Generics z constraints — metadane typowe ──────────
        // Sygnatura generyczna jest informacją dla type checkera.
        // Nie generuje kodu wykonywalnego.
        CommandType::FuncDefGeneric { .. } => {}

        // ── Metadane — brak kodu wykonywalnego ────────────────
        CommandType::Extern { .. }
        | CommandType::Enum   { .. }
        | CommandType::Struct { .. }
        | CommandType::Import { .. } => {}

        // ── Pochłaniane przez compile_body ────────────────────
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

    // Kompiluj main body
    compile_body(&ast.main_body, &mut prog);
    prog.ops.push(OpCode::Exit(0));

    // Kompiluj funkcje
    for (name, (is_arena_fn, _sig, nodes)) in &ast.functions {
        prog.functions.insert(name.clone(), prog.ops.len());

        if *is_arena_fn {
            // Funkcja :: name [size] def — wyciągnij size z sygnatury
            // Sygnatura wygląda jak "[arena:512kb]"
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
// Pomocnik: wyciągnij rozmiar areny z sygnatury "[arena:512kb]"
// ─────────────────────────────────────────────────────────────
fn extract_arena_size(sig: &str) -> String {
    // "[arena:512kb]" → "512kb"
    // "[arena:1mb]"   → "1mb"
    // fallback         → "64kb"
    if let Some(inner) = sig.strip_prefix("[arena:") {
        if let Some(size) = inner.strip_suffix(']') {
            return size.to_string();
        }
    }
    "64kb".to_string()
}
