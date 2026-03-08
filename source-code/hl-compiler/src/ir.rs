use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

static IR_CTR: AtomicU32 = AtomicU32::new(0);

pub fn ir_uid(prefix: &str) -> IrVar {
    IrVar(format!("%{}_{}", prefix, IR_CTR.fetch_add(1, Ordering::Relaxed)))
}

// ─────────────────────────────────────────────────────────────
// IrVar
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IrVar(pub String);
impl fmt::Display for IrVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.0) }
}
impl IrVar {
    pub fn named(name: &str) -> Self { IrVar(format!("%{}", name)) }
}

// ─────────────────────────────────────────────────────────────
// IrType
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrType { I64, F64, Bool, Ptr }
impl fmt::Display for IrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { IrType::I64=>"i64", IrType::F64=>"f64", IrType::Bool=>"bool", IrType::Ptr=>"ptr" }.fmt(f)
    }
}

// ─────────────────────────────────────────────────────────────
// IrLit
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
pub enum IrLit { I64(i64), F64(f64), Bool(bool), Str(String) }
impl fmt::Display for IrLit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrLit::I64(n)  => write!(f, "{}", n),
            IrLit::F64(v)  => write!(f, "{}", v),
            IrLit::Bool(b) => write!(f, "{}", if *b { "1" } else { "0" }),
            IrLit::Str(s)  => write!(f, "{:?}", s),
        }
    }
}

// ─────────────────────────────────────────────────────────────
// IrOperand
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum IrOperand { Var(IrVar), Lit(IrLit) }
impl fmt::Display for IrOperand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { IrOperand::Var(v) => write!(f, "{}", v), IrOperand::Lit(l) => write!(f, "{}", l) }
    }
}
impl IrOperand {
    pub fn ty(&self, vars: &HashMap<IrVar, IrType>) -> IrType {
        match self {
            IrOperand::Lit(IrLit::I64(_))  => IrType::I64,
            IrOperand::Lit(IrLit::F64(_))  => IrType::F64,
            IrOperand::Lit(IrLit::Bool(_)) => IrType::Bool,
            IrOperand::Lit(IrLit::Str(_))  => IrType::Ptr,
            IrOperand::Var(v)              => vars.get(v).copied().unwrap_or(IrType::I64),
        }
    }
}

// ─────────────────────────────────────────────────────────────
// IrCmpOp / IrBinOp
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrCmpOp { Eq, Ne, Lt, Le, Gt, Ge }
impl fmt::Display for IrCmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { IrCmpOp::Eq=>"==", IrCmpOp::Ne=>"!=", IrCmpOp::Lt=>"<", IrCmpOp::Le=>"<=", IrCmpOp::Gt=>">", IrCmpOp::Ge=>">=" }.fmt(f)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrBinOp { Add, Sub, Mul, Div, Mod }
impl fmt::Display for IrBinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { IrBinOp::Add=>"+", IrBinOp::Sub=>"-", IrBinOp::Mul=>"*", IrBinOp::Div=>"/", IrBinOp::Mod=>"%" }.fmt(f)
    }
}

// ─────────────────────────────────────────────────────────────
// IrOp
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum IrOp {
    // Alokacja
    AllocaI64    { var: IrVar, init: Option<i64> },
    AllocaF64    { var: IrVar, init: Option<f64> },
    AllocaBool   { var: IrVar, init: Option<bool> },
    AllocaStrBuf { var: IrVar, size: u32 },
    // Store/Load
    StoreI64  { dst: IrVar, val: IrOperand },
    StoreF64  { dst: IrVar, val: IrOperand },
    StoreBool { dst: IrVar, val: IrOperand },
    LoadI64   { dst: IrVar, src: IrVar },
    LoadF64   { dst: IrVar, src: IrVar },
    // Arytmetyka
    BinI64 { dst: IrVar, lhs: IrOperand, op: IrBinOp, rhs: IrOperand },
    BinF64 { dst: IrVar, lhs: IrOperand, op: IrBinOp, rhs: IrOperand },
    NegI64 { dst: IrVar, src: IrOperand },
    NegF64 { dst: IrVar, src: IrOperand },
    // Porównania
    CmpI64 { dst: IrVar, lhs: IrOperand, op: IrCmpOp, rhs: IrOperand },
    CmpF64 { dst: IrVar, lhs: IrOperand, op: IrCmpOp, rhs: IrOperand },
    // Konwersje
    IntToFloat { dst: IrVar, src: IrOperand },
    FloatToInt { dst: IrVar, src: IrOperand },
    I64ToEnv   { key: String, src: IrOperand, buf: IrVar },
    F64ToEnv   { key: String, src: IrOperand, buf: IrVar },
    // Środowisko
    SetEnv      { key: String, val: String },
    SetEnvDyn   { key: String, expr: String },
    SetLocal    { key: String, val: String },
    SetLocalDyn { key: String, expr: String },
    // Output
    SetOut      { val: String },
    SetOutI64   { src: IrOperand, buf: IrVar },
    SetOutF64   { src: IrOperand, buf: IrVar },
    // Shell
    SysCall { cmd: String, sudo: bool },
    // Wywołania
    CallHL     { name: String, args: Option<String> },
    CallModule { module: String, method: String, args: Option<String> },
    CallExt    { cmd: String, sudo: bool },
    Return,
    // Pętle
    NumFor { var: IrVar, start: IrOperand, end: IrOperand, step: IrOperand, env_key: String, body: Vec<IrOp> },
    WhileShell { cond: String, body: Vec<IrOp> },
    RepeatN    { count: u64, body: Vec<IrOp> },
    ForIn      { var: String, expr: String, body: Vec<IrOp> },
    // Warunek / Match
    IfChain   { branches: Vec<IrBranch> },
    MatchCase { cond: String, arms: Vec<IrArm> },
    // Pipe
    Pipe     { steps: Vec<IrPipeStep> },
    PipeLine { step: String },
    // Async
    Spawn        { cmd: String, sudo: bool },
    SpawnAssign  { key: String, cmd: String, sudo: bool },
    Await        { expr: String },
    AwaitAssign  { key: String, expr: String },
    // Arena — IZOLOWANE od GC
    ArenaEnter    { name: String, size_spec: String, size_bytes: u64 },
    ArenaAllocPtr { dst: IrVar, size: u64 },
    ArenaReset    { name: String },
    ArenaExit     { name: String },
    // GC
    GcAlloc { var: IrVar, size: u64 },
    GcFree,
    GcFull,
    // Kolekcje
    CollectionMut { var: String, method: String, args: String },
    // Error
    TryCatch     { try_cmd: String, catch_cmd: String },
    ResultUnwrap { expr: String, msg: String },
    // Lambda
    Lambda      { params: Vec<String>, body: String },
    StoreLambda { key: String, params: Vec<String>, body: String, is_global: bool },
    // Rekurencja ogonowa
    TailCall    { args: String },
    // Destrukturyzacja
    DestructList { head: String, tail: String, source: String },
    DestructMap  { fields: Vec<String>, source: String },
    // Bloki
    DoBlock   { key: String, body: Vec<IrOp> },
    TestBlock { desc: String, body: Vec<IrOp> },
    ScopeBlock { body: Vec<IrOp> },
    // Defer
    Defer { expr: String },
    // Misc
    Assert  { cond: String, msg: String },
    Plugin  { path: String, args: String, sudo: bool },
    Log     { msg: String, to_stderr: bool },
    Lock    { key: String, size: u64 },
    Unlock  { key: String },
    Exit    { code: i32 },
    Comment { text: String },
    Nop,
}

// ─────────────────────────────────────────────────────────────
// Struktury pomocnicze
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct IrBranch { pub cond: Option<String>, pub body: Vec<IrOp>, pub sudo: bool }
#[derive(Debug, Clone)]
pub struct IrArm    { pub val:  Option<String>, pub cmd: String }
#[derive(Debug, Clone)]
pub struct IrPipeStep { pub cmd: String, pub is_hl: bool }

// ─────────────────────────────────────────────────────────────
// IrFunction + IrModule
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct IrFunction {
    pub name:       String,
    pub is_unsafe:  bool,
    pub is_arena:   bool,
    pub arena_spec: Option<String>,
    pub type_sig:   Option<String>,
    pub ops:        Vec<IrOp>,
}
#[derive(Debug, Default)]
pub struct IrModule {
    pub functions:   Vec<IrFunction>,
    pub main:        Vec<IrOp>,
    pub extern_libs: Vec<(String, bool)>,
}
impl IrModule { pub fn new() -> Self { Self::default() } }

// ─────────────────────────────────────────────────────────────
// HlExpr
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum HlExpr {
    Int(i64), Float(f64), Bool(bool), Str(String), Var(String),
    Add(Box<HlExpr>, Box<HlExpr>), Sub(Box<HlExpr>, Box<HlExpr>),
    Mul(Box<HlExpr>, Box<HlExpr>), Div(Box<HlExpr>, Box<HlExpr>),
    Mod(Box<HlExpr>, Box<HlExpr>), Neg(Box<HlExpr>),
    Eq(Box<HlExpr>, Box<HlExpr>),  Ne(Box<HlExpr>, Box<HlExpr>),
    Lt(Box<HlExpr>, Box<HlExpr>),  Le(Box<HlExpr>, Box<HlExpr>),
    Gt(Box<HlExpr>, Box<HlExpr>),  Ge(Box<HlExpr>, Box<HlExpr>),
}

// ─────────────────────────────────────────────────────────────
// Payload parsery
// ─────────────────────────────────────────────────────────────
pub enum NumericPayload { Typed { type_: String, expr: HlExpr }, Arith { expr: HlExpr } }

pub fn parse_numeric_payload(val: &str) -> Option<NumericPayload> {
    let rest = val.strip_prefix("__hl_num:")?;
    if let Some(r) = rest.strip_prefix("int:")   { return Some(NumericPayload::Typed { type_: "int".into(),   expr: serde_json::from_str(r).ok()? }); }
    if let Some(r) = rest.strip_prefix("float:") { return Some(NumericPayload::Typed { type_: "float".into(), expr: serde_json::from_str(r).ok()? }); }
    if let Some(r) = rest.strip_prefix("bool:")  { return Some(NumericPayload::Typed { type_: "bool".into(),  expr: serde_json::from_str(r).ok()? }); }
    if let Some(r) = rest.strip_prefix("str:")   { return Some(NumericPayload::Typed { type_: "str".into(),   expr: serde_json::from_str(r).ok()? }); }
    if let Some(r) = rest.strip_prefix("expr:")  { return Some(NumericPayload::Arith { expr: serde_json::from_str(r).ok()? }); }
    None
}

pub struct NumForPayload { pub var: String, pub start: i64, pub end: i64, pub step: i64, pub cmd: String }

pub fn parse_numfor_payload(cmd: &str) -> Option<NumForPayload> {
    let rest = cmd.strip_prefix("__hl_numfor:")?;
    let parts: Vec<&str> = rest.splitn(5, '\0').collect();
    if parts.len() != 5 { return None; }
    Some(NumForPayload { var: parts[0].to_string(), start: parts[1].parse().ok()?, end: parts[2].parse().ok()?, step: parts[3].parse().ok()?, cmd: parts[4].to_string() })
}

pub struct WhileExprPayload { pub lhs: HlExpr, pub op: IrCmpOp, pub rhs: HlExpr, pub cmd: String }

pub fn parse_whileexpr_payload(cond: &str, cmd: &str) -> Option<WhileExprPayload> {
    let rest = cond.strip_prefix("__hl_whileexpr:")?;
    let parts: Vec<&str> = rest.splitn(3, '\0').collect();
    if parts.len() != 3 { return None; }
    let lhs: HlExpr = serde_json::from_str(parts[0]).ok()?;
    let op = match parts[1] { "==" => IrCmpOp::Eq, "!=" => IrCmpOp::Ne, "<" => IrCmpOp::Lt, "<=" => IrCmpOp::Le, ">" => IrCmpOp::Gt, ">=" => IrCmpOp::Ge, _ => return None };
    let rhs: HlExpr = serde_json::from_str(parts[2]).ok()?;
    Some(WhileExprPayload { lhs, op, rhs, cmd: cmd.to_string() })
}

pub fn parse_retexpr_payload(val: &str) -> Option<HlExpr> {
    serde_json::from_str(val.strip_prefix("__hl_retexpr:")?).ok()
}

// ─────────────────────────────────────────────────────────────
// parse_size_spec — mirror aa.c hl_arena_parse_size
// ─────────────────────────────────────────────────────────────
pub fn parse_size_spec(spec: &str) -> u64 {
    let s = spec.trim().to_lowercase();
    let num_end = s.find(|c: char| c.is_alphabetic()).unwrap_or(s.len());
    let digits: u64 = s[..num_end].parse().unwrap_or(1);
    match s[num_end..].trim() {
        "gb" => digits * 1024 * 1024 * 1024,
        "mb" => digits * 1024 * 1024,
        "kb" => digits * 1024,
        _    => digits,
    }
}

// ─────────────────────────────────────────────────────────────
// IrBuilder
// ─────────────────────────────────────────────────────────────
use crate::ast::{AnalysisResult, CommandType, ProgramNode};
use crate::paths::get_plugins_root;
use colored::*;

pub struct IrBuilder {
    pub verbose: bool,
    vars:        HashMap<String, (IrVar, IrType)>,
    var_types:   HashMap<IrVar, IrType>,
}

impl IrBuilder {
    pub fn new(verbose: bool) -> Self {
        Self { verbose, vars: HashMap::new(), var_types: HashMap::new() }
    }

    fn reg(&mut self, name: &str, var: IrVar, ty: IrType) {
        self.var_types.insert(var.clone(), ty);
        self.vars.insert(name.to_string(), (var, ty));
    }

    pub fn lower(&mut self, ast: &AnalysisResult) -> IrModule {
        let mut module = IrModule::new();

        for node in ast.main_body.iter()
            .chain(ast.functions.values().flat_map(|(_, _, n)| n.iter()))
            {
                if let CommandType::Extern { path, static_link } = &node.content {
                    module.extern_libs.push((path.clone(), *static_link));
                }
            }
            module.extern_libs.dedup();

        let mut names: Vec<&String> = ast.functions.keys().collect();
        names.sort();
        for name in names {
            let (is_arena, sig, nodes) = &ast.functions[name];
            self.vars.clear(); self.var_types.clear();

            // FIX: size_spec jest teraz Option<String> — używamy resolve_arena_size
            // żeby wyciągnąć rozmiar z węzła ArenaDef lub z sygnatury funkcji.
            let arena_spec = if *is_arena {
                let from_node = nodes.iter().find_map(|n| {
                    if let CommandType::ArenaDef { size_spec, .. } = &n.content {
                        size_spec.clone()   // Option<String> — bierzemy wprost
                    } else {
                        None
                    }
                });
                Some(crate::ast::resolve_arena_size(
                    from_node.as_deref(),
                                                    sig.as_deref(),   // fallback: rozmiar z sygnatury "[arena:512kb]"
                                                    "1mb",
                ))
            } else {
                None
            };

            let mut ops = Vec::new();
            if *is_arena {
                let spec = arena_spec.as_deref().unwrap_or("1mb");
                ops.push(IrOp::ArenaEnter {
                    name:       name.clone(),
                         size_spec:  spec.to_string(),
                         size_bytes: parse_size_spec(spec),
                });
            }
            ops.extend(self.lower_body(nodes));
            if *is_arena {
                ops.push(IrOp::ArenaExit { name: name.clone() });
            }

            module.functions.push(IrFunction {
                name:       name.clone(),
                                  is_unsafe:  false,
                                  is_arena:   *is_arena,
                                  arena_spec,
                                  type_sig:   sig.clone(),
                                  ops,
            });
        }

        self.vars.clear(); self.var_types.clear();
        module.main = self.lower_body(&ast.main_body);
        module
    }

    fn lower_body(&mut self, nodes: &[ProgramNode]) -> Vec<IrOp> {
        let mut ops = Vec::new();
        let mut i = 0;
        while i < nodes.len() {
            let node = &nodes[i];
            let sudo = node.is_sudo;
            match &node.content {
                CommandType::If { cond, cmd } => {
                    let mut branches = vec![IrBranch { cond: Some(cond.clone()), body: vec![self.lower_single_cmd(cmd, sudo)], sudo }];
                    i += 1;
                    loop {
                        if i >= nodes.len() { break; }
                        match &nodes[i].content {
                            CommandType::Elif { cond, cmd } => {
                                branches.push(IrBranch { cond: Some(cond.clone()), body: vec![self.lower_single_cmd(cmd, nodes[i].is_sudo)], sudo: nodes[i].is_sudo });
                                i += 1;
                            }
                            CommandType::Else { cmd } => {
                                branches.push(IrBranch { cond: None, body: vec![self.lower_single_cmd(cmd, nodes[i].is_sudo)], sudo: nodes[i].is_sudo });
                                i += 1;
                                break;
                            }
                            _ => break,
                        }
                    }
                    ops.push(IrOp::IfChain { branches });
                    continue;
                }
                CommandType::Match { cond } => {
                    let mut arms = Vec::new();
                    i += 1;
                    while i < nodes.len() {
                        if let CommandType::MatchArm { val, cmd } = &nodes[i].content {
                            arms.push(IrArm {
                                val: if val == "_" { None } else { Some(val.clone()) },
                                      cmd: cmd.clone(),
                            });
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    ops.push(IrOp::MatchCase { cond: cond.clone(), arms });
                    continue;
                }
                CommandType::MatchArm { .. } => { i += 1; continue; }
                CommandType::Pipe(steps) => {
                    let ir = steps.iter().map(|s| {
                        let t = s.trim();
                        let is_hl = is_hl_call(t);
                        IrPipeStep {
                            cmd:   if is_hl { t.trim_start_matches('.').to_string() } else { t.to_string() },
                                              is_hl,
                        }
                    }).collect();
                    ops.push(IrOp::Pipe { steps: ir });
                    i += 1;
                    continue;
                }
                // FIX: size_spec jest teraz Option<String> — używamy resolve_arena_size
                CommandType::ArenaDef { name, size_spec, body } => {
                    let spec = crate::ast::resolve_arena_size(
                        size_spec.as_deref(),
                                                              None,
                                                              "1mb",
                    );
                    let bytes = parse_size_spec(&spec);
                    ops.push(IrOp::ArenaEnter {
                        name:       name.clone(),
                             size_spec:  spec,
                             size_bytes: bytes,
                    });
                    ops.extend(self.lower_body(body));
                    ops.push(IrOp::ArenaExit { name: name.clone() });
                    i += 1;
                    continue;
                }
                CommandType::DoBlock { key, body } => {
                    let inner = self.lower_body(body);
                    ops.push(IrOp::DoBlock { key: key.clone(), body: inner });
                    i += 1;
                    continue;
                }
                CommandType::TestBlock { desc, body } => {
                    let inner = self.lower_body(body);
                    ops.push(IrOp::TestBlock { desc: desc.clone(), body: inner });
                    i += 1;
                    continue;
                }
                _ => { self.lower_node(node, &mut ops); }
            }
            i += 1;
        }
        ops
    }

    fn lower_single_cmd(&self, cmd: &str, sudo: bool) -> IrOp {
        let t = cmd.trim();
        if is_hl_call(t) {
            let parts: Vec<&str> = t.splitn(2, ' ').collect();
            IrOp::CallHL {
                name: parts[0].trim_start_matches('.').to_string(),
                args: parts.get(1).filter(|a| !a.is_empty()).map(|a| a.to_string()),
            }
        } else {
            IrOp::SysCall { cmd: if sudo { wrap_sudo(cmd) } else { cmd.to_string() }, sudo: false }
        }
    }

    fn lower_node(&mut self, node: &ProgramNode, ops: &mut Vec<IrOp>) {
        let sudo = node.is_sudo;
        match &node.content {
            CommandType::RawNoSub(cmd) | CommandType::RawSub(cmd) => {
                ops.push(IrOp::SysCall { cmd: if sudo { wrap_sudo(cmd) } else { cmd.clone() }, sudo: false });
            }
            CommandType::Isolated(cmd) => {
                let c = format!("( {} )", cmd);
                ops.push(IrOp::SysCall { cmd: if sudo { wrap_sudo(&c) } else { c }, sudo: false });
            }
            CommandType::Background(cmd) => { ops.push(IrOp::Spawn { cmd: cmd.clone(), sudo }); }
            CommandType::Log(msg) => { ops.push(IrOp::Log { msg: msg.clone(), to_stderr: true }); }
            CommandType::Out(val) => {
                if let Some(expr) = parse_retexpr_payload(val) {
                    let buf = ir_uid("outbuf");
                    ops.push(IrOp::AllocaStrBuf { var: buf.clone(), size: 32 });
                    let (expr_ops, (ty, operand)) = self.lower_expr_flat(&expr, ops);
                    ops.extend(expr_ops);
                    if ty == IrType::F64 { ops.push(IrOp::SetOutF64 { src: operand, buf }); }
                    else                 { ops.push(IrOp::SetOutI64 { src: operand, buf }); }
                } else {
                    ops.push(IrOp::SetOut { val: val.clone() });
                }
            }
            CommandType::AssignEnv { key, val } => {
                if is_dyn(val) { ops.push(IrOp::SetEnvDyn { key: key.clone(), expr: val.clone() }); }
                else           { ops.push(IrOp::SetEnv    { key: key.clone(), val:  val.clone() }); }
            }
            CommandType::AssignLocal { key, val, is_raw } => {
                if let Some(p) = parse_numeric_payload(val) { self.lower_numeric_assign(key, p, ops); }
                else if is_dyn(val) { ops.push(IrOp::SetLocalDyn { key: key.clone(), expr: val.clone() }); }
                else                { ops.push(IrOp::SetLocal    { key: key.clone(), val:  val.clone() }); }
                let _ = is_raw;
            }
            CommandType::AssignExpr { key, expr, is_raw: _, is_global } => {
                if *is_global { ops.push(IrOp::SetEnvDyn   { key: key.clone(), expr: expr.clone() }); }
                else          { ops.push(IrOp::SetLocalDyn { key: key.clone(), expr: expr.clone() }); }
            }
            CommandType::Const { key, val } => {
                if is_dyn(val) { ops.push(IrOp::SetEnvDyn { key: key.clone(), expr: val.clone() }); }
                else           { ops.push(IrOp::SetEnv    { key: key.clone(), val:  val.clone() }); }
                if self.verbose { eprintln!("{} IR Const: %{} = {}", "[%]".yellow(), key, val); }
            }
            CommandType::Loop { count, cmd } => {
                if *count == 0 {
                    if let Some(nf) = parse_numfor_payload(cmd) {
                        let var = IrVar::named(&nf.var);
                        self.reg(&nf.var, var.clone(), IrType::I64);
                        ops.push(IrOp::AllocaI64 { var: var.clone(), init: Some(nf.start) });
                        let body = vec![self.lower_single_cmd(&nf.cmd, sudo)];
                        ops.push(IrOp::NumFor {
                            var,
                            start:   IrOperand::Lit(IrLit::I64(nf.start)),
                                 end:     IrOperand::Lit(IrLit::I64(nf.end)),
                                 step:    IrOperand::Lit(IrLit::I64(nf.step)),
                                 env_key: nf.var.clone(),
                                 body,
                        });
                        return;
                    }
                }
                let sh = format!("for _hl_i in $(seq 1 {}); do {}; done", count, cmd);
                ops.push(IrOp::RepeatN { count: *count, body: vec![IrOp::SysCall { cmd: sh, sudo }] });
            }
            CommandType::While { cond, cmd } => {
                if let Some(we) = parse_whileexpr_payload(cond, cmd) {
                    let cond_sh = whileexpr_to_shell(&we.lhs, we.op, &we.rhs);
                    ops.push(IrOp::WhileShell { cond: cond_sh, body: vec![self.lower_single_cmd(&we.cmd, sudo)] });
                } else {
                    ops.push(IrOp::WhileShell { cond: cond.clone(), body: vec![IrOp::SysCall { cmd: cmd.clone(), sudo }] });
                }
            }
            CommandType::For { var, in_, cmd } => {
                ops.push(IrOp::ForIn { var: var.clone(), expr: in_.clone(), body: vec![IrOp::SysCall { cmd: cmd.clone(), sudo }] });
            }
            CommandType::Call { path, args } => {
                let t = path.trim();
                if is_hl_call(t) {
                    ops.push(IrOp::CallHL {
                        name: t.trim_start_matches('.').to_string(),
                             args: if args.is_empty() { None } else { Some(args.clone()) },
                    });
                } else {
                    let cmd = if args.is_empty() { t.to_string() } else { format!("{} {}", t, args) };
                    ops.push(IrOp::CallExt { cmd, sudo });
                }
            }
            CommandType::ModuleCall { path, args } => {
                let (module, method) = path.split_once('.').unwrap_or(("", path.as_str()));
                ops.push(IrOp::CallModule {
                    module: module.to_string(),
                         method: method.to_string(),
                         args:   if args.is_empty() { None } else { Some(args.clone()) },
                });
            }
            CommandType::Spawn(task)  => { ops.push(IrOp::Spawn       { cmd: task.trim().trim_start_matches('.').to_string(), sudo }); }
            CommandType::AssignSpawn  { key, task } => { ops.push(IrOp::SpawnAssign { key: key.clone(), cmd: task.trim().trim_start_matches('.').to_string(), sudo }); }
            CommandType::Await(expr)  => { ops.push(IrOp::Await        { expr: expr.trim().to_string() }); }
            CommandType::AssignAwait  { key, expr } => { ops.push(IrOp::AwaitAssign { key: key.clone(), expr: expr.trim().to_string() }); }
            CommandType::ArenaAlloc   { key, arena_name: _, size } => {
                let dst = IrVar::named(key);
                self.reg(key, dst.clone(), IrType::Ptr);
                ops.push(IrOp::ArenaAllocPtr { dst, size: *size });
            }
            CommandType::ArenaReset { arena_name } => { ops.push(IrOp::ArenaReset { name: arena_name.clone() }); }
            CommandType::ArenaFree  { arena_name } => { ops.push(IrOp::ArenaExit  { name: arena_name.clone() }); }
            CommandType::CollectionMut { var, method, args } => {
                ops.push(IrOp::CollectionMut { var: var.clone(), method: method.clone(), args: args.clone() });
            }
            CommandType::Try          { try_cmd, catch_cmd } => { ops.push(IrOp::TryCatch     { try_cmd: try_cmd.clone(), catch_cmd: catch_cmd.clone() }); }
            CommandType::ResultUnwrap { expr, msg }          => { ops.push(IrOp::ResultUnwrap { expr: expr.clone(), msg: msg.clone() }); }
            CommandType::Lambda       { params, body }       => { ops.push(IrOp::Lambda       { params: params.clone(), body: body.clone() }); }
            CommandType::AssignLambda { key, params, body, is_raw: _, is_global } => {
                ops.push(IrOp::StoreLambda { key: key.clone(), params: params.clone(), body: body.clone(), is_global: *is_global });
            }
            CommandType::Recur { args }                      => { ops.push(IrOp::TailCall    { args: args.clone() }); }
            CommandType::DestructList { head, tail, source } => { ops.push(IrOp::DestructList { head: head.clone(), tail: tail.clone(), source: source.clone() }); }
            CommandType::DestructMap  { fields, source }     => { ops.push(IrOp::DestructMap  { fields: fields.clone(), source: source.clone() }); }
            CommandType::ScopeDef                            => { ops.push(IrOp::ScopeBlock   { body: vec![] }); }
            CommandType::PipeLine { step }                   => { ops.push(IrOp::PipeLine     { step: step.clone() }); }
            CommandType::Defer { expr }                      => { ops.push(IrOp::Defer        { expr: expr.clone() }); }
            CommandType::Assert { cond, msg }                => {
                ops.push(IrOp::Assert {
                    cond: cond.clone(),
                         msg:  msg.as_deref().unwrap_or("Assertion failed").to_string(),
                });
            }
            CommandType::Plugin { name, args, is_super } => {
                let root = get_plugins_root();
                let bin  = root.join(name);
                let hl   = root.join(format!("{}.hl", name));
                let path = if bin.exists() {
                    bin.to_str().unwrap_or(name).to_string()
                } else if hl.exists() {
                    format!("hl {}", hl.display())
                } else {
                    if self.verbose { eprintln!("{} Plugin '{}' nie znaleziony", "[!]".yellow(), name); }
                    return;
                };
                ops.push(IrOp::Plugin { path, args: args.clone(), sudo: *is_super });
            }
            CommandType::Lock { key, val } => {
                let size = val.parse::<u64>().unwrap_or(64);
                ops.push(IrOp::Lock { key: key.clone(), size });
                let var = IrVar::named(key);
                self.reg(key, var.clone(), IrType::Ptr);
                ops.push(IrOp::GcAlloc { var, size });
            }
            CommandType::Unlock { key } => {
                ops.push(IrOp::Unlock { key: key.clone() });
                ops.push(IrOp::GcFree);
            }
            CommandType::End { code } => {
                ops.push(IrOp::GcFull);
                ops.push(IrOp::Exit { code: *code });
            }
            CommandType::Extern { path, static_link } => {
                ops.push(IrOp::Comment { text: format!("extern {} static={}", path, static_link) });
            }
            CommandType::Enum      { name, variants } => {
                ops.push(IrOp::Comment { text: format!("enum {} [{}]", name, variants.join(", ")) });
            }
            CommandType::Struct    { name, fields } => {
                ops.push(IrOp::Comment { text: format!("struct {} [{}]", name, fields.iter().map(|(k,t)| format!("{}:{}", k, t)).collect::<Vec<_>>().join(", ")) });
            }
            CommandType::AdtDef    { name, variants } => {
                ops.push(IrOp::Comment { text: format!("adt {} ({} variants)", name, variants.len()) });
            }
            CommandType::Interface { name, methods } => {
                ops.push(IrOp::Comment { text: format!("interface {} [{}]", name, methods.join(", ")) });
            }
            CommandType::ImplDef   { class, interface } => {
                ops.push(IrOp::Comment { text: format!("impl {} for {}", class, interface) });
            }
            CommandType::FuncDefGeneric { name, sig } => {
                ops.push(IrOp::Comment { text: format!("generic fn {} sig={}", name, sig) });
            }
            CommandType::Import    { resource, namespace } => {
                ops.push(IrOp::Comment { text: format!("import {:?}{}", resource, namespace.as_deref().map(|n| format!(" in {}", n)).unwrap_or_default()) });
            }
            CommandType::SysDep(dep) => {
                ops.push(IrOp::Comment { text: format!("sysdep {}", dep) });
            }
            // Pochłaniane przez lower_body
            CommandType::If { .. } | CommandType::Elif { .. } | CommandType::Else { .. }
            | CommandType::Match { .. } | CommandType::MatchArm { .. } | CommandType::Pipe(_)
            | CommandType::ArenaDef { .. } | CommandType::DoBlock { .. } | CommandType::TestBlock { .. } => {}
        }
    }

    fn lower_numeric_assign(&mut self, key: &str, payload: NumericPayload, ops: &mut Vec<IrOp>) {
        let buf = ir_uid("buf");
        ops.push(IrOp::AllocaStrBuf { var: buf.clone(), size: 32 });
        match payload {
            NumericPayload::Typed { type_, expr } => {
                let (expr_ops, (expr_ty, operand)) = self.lower_expr_flat(&expr, ops);
                ops.extend(expr_ops);
                match type_.as_str() {
                    "float" => {
                        let var = IrVar::named(key);
                        ops.push(IrOp::AllocaF64 { var: var.clone(), init: None });
                        let final_op = if expr_ty != IrType::F64 {
                            let c = ir_uid("cvtf");
                            self.var_types.insert(c.clone(), IrType::F64);
                            ops.push(IrOp::IntToFloat { dst: c.clone(), src: operand });
                            IrOperand::Var(c)
                        } else { operand };
                        ops.push(IrOp::StoreF64 { dst: var.clone(), val: final_op.clone() });
                        ops.push(IrOp::F64ToEnv { key: key.to_string(), src: final_op, buf });
                        self.reg(key, var, IrType::F64);
                    }
                    _ => {
                        let var = IrVar::named(key);
                        ops.push(IrOp::AllocaI64 { var: var.clone(), init: None });
                        let final_op = if expr_ty == IrType::F64 {
                            let c = ir_uid("cvti");
                            self.var_types.insert(c.clone(), IrType::I64);
                            ops.push(IrOp::FloatToInt { dst: c.clone(), src: operand });
                            IrOperand::Var(c)
                        } else { operand };
                        ops.push(IrOp::StoreI64 { dst: var.clone(), val: final_op.clone() });
                        ops.push(IrOp::I64ToEnv { key: key.to_string(), src: final_op, buf });
                        self.reg(key, var, IrType::I64);
                    }
                }
            }
            NumericPayload::Arith { expr } => {
                let (expr_ops, (ty, operand)) = self.lower_expr_flat(&expr, ops);
                ops.extend(expr_ops);
                let var = IrVar::named(key);
                if ty == IrType::F64 {
                    ops.push(IrOp::AllocaF64 { var: var.clone(), init: None });
                    ops.push(IrOp::StoreF64  { dst: var.clone(), val: operand.clone() });
                    ops.push(IrOp::F64ToEnv  { key: key.to_string(), src: operand, buf });
                    self.reg(key, var, IrType::F64);
                } else {
                    ops.push(IrOp::AllocaI64 { var: var.clone(), init: None });
                    ops.push(IrOp::StoreI64  { dst: var.clone(), val: operand.clone() });
                    ops.push(IrOp::I64ToEnv  { key: key.to_string(), src: operand, buf });
                    self.reg(key, var, IrType::I64);
                }
            }
        }
    }

    fn lower_expr_flat(&mut self, expr: &HlExpr, _p: &mut Vec<IrOp>) -> (Vec<IrOp>, (IrType, IrOperand)) {
        match expr {
            HlExpr::Int(n)   => (vec![], (IrType::I64,  IrOperand::Lit(IrLit::I64(*n)))),
            HlExpr::Float(f) => (vec![], (IrType::F64,  IrOperand::Lit(IrLit::F64(*f)))),
            HlExpr::Bool(b)  => (vec![], (IrType::Bool, IrOperand::Lit(IrLit::Bool(*b)))),
            HlExpr::Str(s)   => (vec![], (IrType::Ptr,  IrOperand::Lit(IrLit::Str(s.clone())))),
            HlExpr::Var(name) => {
                let (var, ty) = self.vars.get(name.as_str()).cloned().unwrap_or_else(|| {
                    let v = IrVar::named(name);
                    self.var_types.insert(v.clone(), IrType::I64);
                    (v, IrType::I64)
                });
                let tmp = ir_uid("lv");
                self.var_types.insert(tmp.clone(), ty);
                let load = if ty == IrType::F64 {
                    IrOp::LoadF64 { dst: tmp.clone(), src: var }
                } else {
                    IrOp::LoadI64 { dst: tmp.clone(), src: var }
                };
                (vec![load], (ty, IrOperand::Var(tmp)))
            }
            HlExpr::Add(a,b) => self.lower_binop_flat(IrBinOp::Add, a, b),
            HlExpr::Sub(a,b) => self.lower_binop_flat(IrBinOp::Sub, a, b),
            HlExpr::Mul(a,b) => self.lower_binop_flat(IrBinOp::Mul, a, b),
            HlExpr::Div(a,b) => self.lower_binop_flat(IrBinOp::Div, a, b),
            HlExpr::Mod(a,b) => self.lower_binop_flat(IrBinOp::Mod, a, b),
            HlExpr::Neg(inner) => {
                let (mut sub_ops, (ty, op)) = self.lower_expr_flat(inner, _p);
                let dst = ir_uid("neg");
                self.var_types.insert(dst.clone(), ty);
                sub_ops.push(if ty == IrType::F64 {
                    IrOp::NegF64 { dst: dst.clone(), src: op }
                } else {
                    IrOp::NegI64 { dst: dst.clone(), src: op }
                });
                (sub_ops, (ty, IrOperand::Var(dst)))
            }
            HlExpr::Eq(a,b) => self.lower_cmp_flat(IrCmpOp::Eq, a, b),
            HlExpr::Ne(a,b) => self.lower_cmp_flat(IrCmpOp::Ne, a, b),
            HlExpr::Lt(a,b) => self.lower_cmp_flat(IrCmpOp::Lt, a, b),
            HlExpr::Le(a,b) => self.lower_cmp_flat(IrCmpOp::Le, a, b),
            HlExpr::Gt(a,b) => self.lower_cmp_flat(IrCmpOp::Gt, a, b),
            HlExpr::Ge(a,b) => self.lower_cmp_flat(IrCmpOp::Ge, a, b),
        }
    }

    fn lower_binop_flat(&mut self, op: IrBinOp, a: &HlExpr, b: &HlExpr) -> (Vec<IrOp>, (IrType, IrOperand)) {
        let mut d = vec![];
        let (mut ops_a, (ta, oa)) = self.lower_expr_flat(a, &mut d);
        let (ops_b,     (tb, ob)) = self.lower_expr_flat(b, &mut d);
        ops_a.extend(ops_b);
        let is_f = ta == IrType::F64 || tb == IrType::F64;
        let ty   = if is_f { IrType::F64 } else { IrType::I64 };
        let dst  = ir_uid(if is_f { "f64" } else { "i64" });
        self.var_types.insert(dst.clone(), ty);
        let oa2 = if is_f && ta != IrType::F64 {
            let c = ir_uid("cvt"); self.var_types.insert(c.clone(), IrType::F64);
            ops_a.push(IrOp::IntToFloat { dst: c.clone(), src: oa });
            IrOperand::Var(c)
        } else { oa };
        let ob2 = if is_f && tb != IrType::F64 {
            let c = ir_uid("cvt"); self.var_types.insert(c.clone(), IrType::F64);
            ops_a.push(IrOp::IntToFloat { dst: c.clone(), src: ob });
            IrOperand::Var(c)
        } else { ob };
        ops_a.push(if is_f {
            IrOp::BinF64 { dst: dst.clone(), lhs: oa2, op, rhs: ob2 }
        } else {
            IrOp::BinI64 { dst: dst.clone(), lhs: oa2, op, rhs: ob2 }
        });
        (ops_a, (ty, IrOperand::Var(dst)))
    }

    fn lower_cmp_flat(&mut self, op: IrCmpOp, a: &HlExpr, b: &HlExpr) -> (Vec<IrOp>, (IrType, IrOperand)) {
        let mut d = vec![];
        let (mut ops_a, (ta, oa)) = self.lower_expr_flat(a, &mut d);
        let (ops_b,     (tb, ob)) = self.lower_expr_flat(b, &mut d);
        ops_a.extend(ops_b);
        let is_f = ta == IrType::F64 || tb == IrType::F64;
        let dst  = ir_uid("cmp");
        self.var_types.insert(dst.clone(), IrType::Bool);
        let oa2 = if is_f && ta != IrType::F64 {
            let c = ir_uid("cvt"); self.var_types.insert(c.clone(), IrType::F64);
            ops_a.push(IrOp::IntToFloat { dst: c.clone(), src: oa });
            IrOperand::Var(c)
        } else { oa };
        let ob2 = if is_f && tb != IrType::F64 {
            let c = ir_uid("cvt"); self.var_types.insert(c.clone(), IrType::F64);
            ops_a.push(IrOp::IntToFloat { dst: c.clone(), src: ob });
            IrOperand::Var(c)
        } else { ob };
        ops_a.push(if is_f {
            IrOp::CmpF64 { dst: dst.clone(), lhs: oa2, op, rhs: ob2 }
        } else {
            IrOp::CmpI64 { dst: dst.clone(), lhs: oa2, op, rhs: ob2 }
        });
        (ops_a, (IrType::Bool, IrOperand::Var(dst)))
    }
}

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────
fn is_hl_call(t: &str) -> bool {
    t.starts_with('.') && t.len() > 1
    && t.chars().nth(1).map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false)
}
fn is_dyn(val: &str) -> bool { val.contains('$') || val.contains('`') || val.contains("$(") }
fn wrap_sudo(cmd: &str) -> String { format!("sudo sh -c '{}'", cmd.replace('\'', "'\\''")) }

fn whileexpr_to_shell(lhs: &HlExpr, op: IrCmpOp, rhs: &HlExpr) -> String {
    let o = match op { IrCmpOp::Eq=>"-eq", IrCmpOp::Ne=>"-ne", IrCmpOp::Lt=>"-lt", IrCmpOp::Le=>"-le", IrCmpOp::Gt=>"-gt", IrCmpOp::Ge=>"-ge" };
    format!("[ {} {} {} ]", hlexpr_shell(lhs), o, hlexpr_shell(rhs))
}
fn hlexpr_shell(e: &HlExpr) -> String {
    match e {
        HlExpr::Int(n)   => n.to_string(),
        HlExpr::Float(f) => f.to_string(),
        HlExpr::Bool(b)  => if *b { "1".into() } else { "0".into() },
        HlExpr::Str(s)   => format!("\"{}\"", s),
        HlExpr::Var(name)=> format!("${}", name),
        HlExpr::Add(a,b) => format!("$(( {} + {} ))", hlexpr_shell(a), hlexpr_shell(b)),
        HlExpr::Sub(a,b) => format!("$(( {} - {} ))", hlexpr_shell(a), hlexpr_shell(b)),
        HlExpr::Mul(a,b) => format!("$(( {} * {} ))", hlexpr_shell(a), hlexpr_shell(b)),
        HlExpr::Div(a,b) => format!("$(( {} / {} ))", hlexpr_shell(a), hlexpr_shell(b)),
        HlExpr::Mod(a,b) => format!("$(( {} % {} ))", hlexpr_shell(a), hlexpr_shell(b)),
        HlExpr::Neg(e)   => format!("$(( -{} ))", hlexpr_shell(e)),
        _                => "0".into(),
    }
}

// ─────────────────────────────────────────────────────────────
// IrPrinter
// ─────────────────────────────────────────────────────────────
pub struct IrPrinter { indent: usize, out: String }
impl Default for IrPrinter { fn default() -> Self { Self::new() } }

impl IrPrinter {
    pub fn new() -> Self { Self { indent: 0, out: String::with_capacity(4096) } }
    fn pad(&self) -> String { "  ".repeat(self.indent) }
    fn line(&mut self, s: impl AsRef<str>) { self.out.push_str(&self.pad()); self.out.push_str(s.as_ref()); self.out.push('\n'); }

    pub fn dump(&mut self, module: &IrModule) -> String {
        self.out.clear();
        self.line("; hacker-lang IR v2");
        self.line("; gc.c  — cały program (zmienne, listy, mapy, domknięcia)");
        self.line("; libaa.a — TYLKO :: bloki (ArenaEnter..ArenaExit, zero GC)");
        self.line("; ──────────────────────────────────────────────────────────");
        self.out.push('\n');
        for (p, s) in &module.extern_libs {
            self.line(format!("{} {:?}", if *s { "LINK_STATIC" } else { "LINK_DYN   " }, p));
        }
        if !module.extern_libs.is_empty() { self.out.push('\n'); }
        let mut fi: Vec<usize> = (0..module.functions.len()).collect();
        fi.sort_by_key(|&i| &module.functions[i].name);
        for i in fi {
            let f = &module.functions[i];
            self.line(format!("FN {:?}{}{}{}",
                              f.name,
                              if f.is_unsafe { " [unsafe]" } else { "" },
                                  if f.is_arena  { format!(" [arena:{}]", f.arena_spec.as_deref().unwrap_or("?")) } else { String::new() },
                                      f.type_sig.as_deref().map(|s| format!(" [sig: {}]", s)).unwrap_or_default(),
            ));
            self.indent += 1;
            for op in &f.ops.clone() { self.emit_op(op); }
            self.indent -= 1;
            self.line("END_FN");
            self.out.push('\n');
        }
        self.line("MAIN");
        self.indent += 1;
        for op in &module.main.clone() { self.emit_op(op); }
        self.indent -= 1;
        self.line("END_MAIN");
        self.out.clone()
    }

    fn emit_op(&mut self, op: &IrOp) {
        match op {
            IrOp::AllocaI64    { var, init }    => self.line(format!("ALLOCA_I64   {} {}", var, init.map(|n| n.to_string()).unwrap_or_default())),
            IrOp::AllocaF64    { var, init }    => self.line(format!("ALLOCA_F64   {} {}", var, init.map(|n| n.to_string()).unwrap_or_default())),
            IrOp::AllocaBool   { var, init }    => self.line(format!("ALLOCA_BOOL  {} {}", var, init.map(|b| if b {"1"} else {"0"}).unwrap_or(""))),
            IrOp::AllocaStrBuf { var, size }    => self.line(format!("ALLOCA_BUF   {} [{}]", var, size)),
            IrOp::StoreI64  { dst, val }        => self.line(format!("STORE_I64    {} = {}", dst, val)),
            IrOp::StoreF64  { dst, val }        => self.line(format!("STORE_F64    {} = {}", dst, val)),
            IrOp::StoreBool { dst, val }        => self.line(format!("STORE_BOOL   {} = {}", dst, val)),
            IrOp::LoadI64   { dst, src }        => self.line(format!("LOAD_I64     {} <- {}", dst, src)),
            IrOp::LoadF64   { dst, src }        => self.line(format!("LOAD_F64     {} <- {}", dst, src)),
            IrOp::BinI64 { dst, lhs, op, rhs } => self.line(format!("BIN_I64      {} = {} {} {}", dst, lhs, op, rhs)),
            IrOp::BinF64 { dst, lhs, op, rhs } => self.line(format!("BIN_F64      {} = {} {} {}", dst, lhs, op, rhs)),
            IrOp::NegI64 { dst, src }           => self.line(format!("NEG_I64      {} = -{}", dst, src)),
            IrOp::NegF64 { dst, src }           => self.line(format!("NEG_F64      {} = -{}", dst, src)),
            IrOp::CmpI64 { dst, lhs, op, rhs } => self.line(format!("CMP_I64      {} = {} {} {}", dst, lhs, op, rhs)),
            IrOp::CmpF64 { dst, lhs, op, rhs } => self.line(format!("CMP_F64      {} = {} {} {}", dst, lhs, op, rhs)),
            IrOp::IntToFloat { dst, src }       => self.line(format!("INT_TO_F64   {} = (f64){}", dst, src)),
            IrOp::FloatToInt { dst, src }       => self.line(format!("F64_TO_INT   {} = (i64){}", dst, src)),
            IrOp::I64ToEnv   { key, src, buf }  => self.line(format!("I64_TO_ENV   {:?} <- {}  buf={}", key, src, buf)),
            IrOp::F64ToEnv   { key, src, buf }  => self.line(format!("F64_TO_ENV   {:?} <- {}  buf={}", key, src, buf)),
            IrOp::SetEnv      { key, val }      => self.line(format!("SETENV       {:?} = {:?}", key, val)),
            IrOp::SetEnvDyn   { key, expr }     => self.line(format!("SETENV_DYN   {:?} = {:?}", key, expr)),
            IrOp::SetLocal    { key, val }      => self.line(format!("SETLOCAL     {:?} = {:?}", key, val)),
            IrOp::SetLocalDyn { key, expr }     => self.line(format!("SETLOCAL_DYN {:?} = {:?}", key, expr)),
            IrOp::SetOut    { val }             => self.line(format!("SET_OUT      {:?}", val)),
            IrOp::SetOutI64 { src, buf }        => self.line(format!("SET_OUT_I64  {}  buf={}", src, buf)),
            IrOp::SetOutF64 { src, buf }        => self.line(format!("SET_OUT_F64  {}  buf={}", src, buf)),
            IrOp::SysCall { cmd, sudo }         => self.line(format!("SYSCALL{}     {:?}", if *sudo {"_SUDO"} else {""}, cmd)),
            IrOp::CallHL     { name, args }     => match args {
                Some(a) => self.line(format!("CALL_HL      {:?}  args={:?}", name, a)),
                None    => self.line(format!("CALL_HL      {:?}", name)),
            },
            IrOp::CallModule { module, method, args } => match args {
                Some(a) => self.line(format!("CALL_MOD     {}.{}  args={:?}", module, method, a)),
                None    => self.line(format!("CALL_MOD     {}.{}", module, method)),
            },
            IrOp::CallExt { cmd, sudo }         => self.line(format!("CALL_EXT{}    {:?}", if *sudo {"_SUDO"} else {""}, cmd)),
            IrOp::Return                        => self.line("RETURN"),
            IrOp::NumFor { var, start, end, step, env_key, body } => {
                self.line(format!("NUM_FOR      {} {} {} {}  env={:?}", var, start, end, step, env_key));
                self.indent += 1; for op in body { self.emit_op(op); } self.indent -= 1;
                self.line("END_FOR");
            }
            IrOp::WhileShell { cond, body } => {
                self.line(format!("WHILE        {:?}", cond));
                self.indent += 1; for op in body { self.emit_op(op); } self.indent -= 1;
                self.line("END_WHILE");
            }
            IrOp::RepeatN { count, body } => {
                self.line(format!("REPEAT       {}", count));
                self.indent += 1; for op in body { self.emit_op(op); } self.indent -= 1;
                self.line("END_REPEAT");
            }
            IrOp::ForIn { var, expr, body } => {
                self.line(format!("FOR_IN       {:?} in {:?}", var, expr));
                self.indent += 1; for op in body { self.emit_op(op); } self.indent -= 1;
                self.line("END_FOR_IN");
            }
            IrOp::IfChain { branches } => {
                for (bi, br) in branches.iter().enumerate() {
                    match &br.cond {
                        Some(c) if bi == 0 => self.line(format!("IF           {:?}", c)),
                        Some(c)            => self.line(format!("ELIF         {:?}", c)),
                        None               => self.line("ELSE"),
                    }
                    self.indent += 1; for op in &br.body { self.emit_op(op); } self.indent -= 1;
                }
                self.line("END_IF");
            }
            IrOp::MatchCase { cond, arms } => {
                self.line(format!("MATCH        {:?}", cond));
                self.indent += 1;
                for arm in arms {
                    match &arm.val {
                        Some(v) => self.line(format!("ARM          {:?}  {:?}", v, arm.cmd)),
                        None    => self.line(format!("ARM_DEFAULT  {:?}", arm.cmd)),
                    }
                }
                self.indent -= 1;
                self.line("END_MATCH");
            }
            IrOp::Pipe { steps } => {
                self.line(format!("PIPE         {}",
                                  steps.iter().map(|s| if s.is_hl { format!("[HL]{}", s.cmd) } else { s.cmd.clone() }).collect::<Vec<_>>().join(" | ")
                ));
            }
            IrOp::PipeLine { step }                      => self.line(format!("PIPELINE     {:?}", step)),
            IrOp::Spawn       { cmd, sudo }              => self.line(format!("SPAWN{}       {:?}", if *sudo {"_SUDO"} else {""}, cmd)),
            IrOp::SpawnAssign { key, cmd, sudo }         => self.line(format!("SPAWN_ASSIGN{} {:?} {:?}", if *sudo {"_SUDO"} else {""}, key, cmd)),
            IrOp::Await       { expr }                   => self.line(format!("AWAIT        {:?}", expr)),
            IrOp::AwaitAssign { key, expr }              => self.line(format!("AWAIT_ASSIGN {:?} {:?}", key, expr)),
            IrOp::ArenaEnter    { name, size_spec, size_bytes } => {
                self.line(format!("; ┌── arena: {} ────────────────────────────────", name));
                self.line(format!("ARENA_ENTER  {:?} [{}] = {} bytes", name, size_spec, size_bytes));
            }
            IrOp::ArenaAllocPtr { dst, size }            => self.line(format!("ARENA_ALLOC  {} {} bytes", dst, size)),
            IrOp::ArenaReset    { name }                 => self.line(format!("ARENA_RESET  {:?}", name)),
            IrOp::ArenaExit     { name }                 => {
                self.line(format!("ARENA_EXIT   {:?}", name));
                self.line(format!("; └── arena: {} ────────────────────────────────", name));
            }
            IrOp::GcAlloc { var, size }                  => self.line(format!("GC_ALLOC     {} {}", var, size)),
            IrOp::GcFree                                 => self.line("GC_FREE"),
            IrOp::GcFull                                 => self.line("GC_FULL"),
            IrOp::CollectionMut { var, method, args }    => self.line(format!("COLLECTION   ${}.{} {:?}", var, method, args)),
            IrOp::TryCatch     { try_cmd, catch_cmd }   => { self.line(format!("TRY          {:?}", try_cmd)); self.line(format!("CATCH        {:?}", catch_cmd)); }
            IrOp::ResultUnwrap { expr, msg }             => self.line(format!("UNWRAP       {:?} {:?}", expr, msg)),
            IrOp::Lambda      { params, body }           => self.line(format!("LAMBDA       [{}] {:?}", params.join(", "), body)),
            IrOp::StoreLambda { key, params, body, is_global } => self.line(format!("STORE_LAMBDA {:?} [{}] {:?}  scope={}", key, params.join(", "), body, if *is_global {"global"} else {"local"})),
            IrOp::TailCall    { args }                   => self.line(format!("TAIL_CALL    {:?}", args)),
            IrOp::DestructList { head, tail, source }   => self.line(format!("DESTRUCT_LIST [{} | {}] = {:?}", head, tail, source)),
            IrOp::DestructMap  { fields, source }        => self.line(format!("DESTRUCT_MAP  {{{}}} = {:?}", fields.join(", "), source)),
            IrOp::DoBlock  { key, body } => {
                self.line(format!("DO_BLOCK     {:?}", key));
                self.indent += 1; for op in body { self.emit_op(op); } self.indent -= 1;
                self.line("END_DO");
            }
            IrOp::TestBlock { desc, body } => {
                self.line(format!("TEST_BLOCK   {:?}", desc));
                self.indent += 1; for op in body { self.emit_op(op); } self.indent -= 1;
                self.line("END_TEST");
            }
            IrOp::ScopeBlock { body } => {
                self.line("SCOPE_BEGIN");
                self.indent += 1; for op in body { self.emit_op(op); } self.indent -= 1;
                self.line("SCOPE_END");
            }
            IrOp::Defer   { expr }                       => self.line(format!("DEFER        {:?}", expr)),
            IrOp::Assert  { cond, msg }                  => self.line(format!("ASSERT       {:?} {:?}", cond, msg)),
            IrOp::Plugin  { path, args, sudo }           => self.line(format!("PLUGIN{}      {:?} {:?}", if *sudo {"_SUDO"} else {""}, path, args)),
            IrOp::Log     { msg, to_stderr }             => self.line(format!("LOG{}         {:?}", if *to_stderr {"_ERR"} else {""}, msg)),
            IrOp::Lock    { key, size }                  => self.line(format!("LOCK         {:?} {}", key, size)),
            IrOp::Unlock  { key }                        => self.line(format!("UNLOCK       {:?}", key)),
            IrOp::Exit    { code }                       => self.line(format!("EXIT         {}", code)),
            IrOp::Comment { text }                       => self.line(format!("; {}", text)),
            IrOp::Nop                                    => self.line("NOP"),
        }
    }
}
