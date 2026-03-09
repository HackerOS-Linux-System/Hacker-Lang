use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use dirs;
use miette::NamedSource;
use crate::{
    AnalysisResult, CommandType, LibRef, LibType,
    ParseError, ProgramNode, SourceSpan,
};

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct HlParser;

#[derive(Debug, Clone)]
pub enum LineOp {
    ClassDef(String),
    FuncDef(String, Option<String>),
    FuncDone,
    Call(String, String),
    SysDep(String),
    Lib(LibRef),
    SepCmd(String),
    RawCmd(String),
    ExplCmd(String),
    GlobalVar(String, String),
    LocalVar(String, String, bool),
    Plugin(String, String),
    Loop(u64, String),
    If(String, String),
    Elif(String, String),
    Else(String),
    While(String, String),
    For(String, String, String),
    Bg(String),
    Log(String),
    Lock(String, String),
    Unlock(String),
    Extern(String, bool),
    Enum(String, Vec<String>),
    Import(String, Option<String>),
    Try(String, String),
    Struct(String, Vec<(String, String)>),
    RawBlockStart,
    RawBlockEnd,
    End(i32),
    Out(String),
    Percent(String, String),
    Spawn(String),
    Await(String),
    Assert(String, Option<String>),
    Match(String),
    MatchArm(String, String),
    Pipe(Vec<String>),
    AssignSpawn(String, String),
    AssignAwait(String, String),
    AssignExpr(String, String, bool, bool),
    AssignList(String, Vec<String>, bool, bool),
    CollectionMut(String, String, String),
    InterfaceDef(String, Vec<String>),
    ImplDef(String, String),
    ArenaDef(String, String),
    ResultUnwrap(String, String),
    ModuleCall(String, String),
    Lambda(Vec<String>, String),
    AssignLambda(String, Vec<String>, String, bool, bool),
    Recur(String),
    DestructList(String, String, String),
    DestructMap(Vec<String>, String),
    ScopeDef,
    AdtDef(String, Vec<(String, Vec<(String, String)>)>),
    DoBlock,
    AssignDo(String),
    PipeLine(String),
    TestDef(String),
    Defer(String),
    FuncDefGeneric(String, String),
    CloseBrace,
}

#[derive(Debug, Clone)]
enum Scope { Class(String), Func(String) }

struct AdtBlockState {
    name:       String,
    body_lines: Vec<String>,
    start_line: usize,
    start_off:  usize,
}
struct InterfaceBlockState {
    name:       String,
    methods:    Vec<String>,
    start_line: usize,
    start_off:  usize,
}
struct MapBlockState {
    key:        String,
    is_raw:     bool,
    is_global:  bool,
    lines:      Vec<String>,
    start_line: usize,
    start_off:  usize,
}
struct ListBlockState {
    key:        String,
    is_raw:     bool,
    is_global:  bool,
    items:      Vec<String>,
    start_line: usize,
    start_off:  usize,
}
struct CallListBlockState {
    prefix:     String,
    items:      Vec<String>,
    start_line: usize,
    start_off:  usize,
}
struct LambdaBlockState {
    prefix:     String,
    collected:  Vec<String>,
    start_line: usize,
    start_off:  usize,
}

fn libs_root() -> PathBuf {
    dirs::home_dir().expect("HOME not set").join(".hackeros/hacker-lang/libs")
}
pub fn plugins_root() -> PathBuf {
    dirs::home_dir().expect("HOME not set").join(".hackeros/hacker-lang/plugins")
}
fn lib_path(lib: &LibRef) -> Option<PathBuf> {
    let r = libs_root();
    match lib.lib_type {
        LibType::Core  => Some(r.join("core").join(&lib.name).with_extension("hl")),
        LibType::Bytes => Some(r.join("bytes").join(&lib.name).with_extension("so")),
        LibType::Virus => Some(r.join(".virus").join(&lib.name).with_extension("a")),
        LibType::Vira  => Some(r.join(".virus").join(&lib.name)),
    }
}

fn is_assignment(line: &str) -> Option<(String, String, bool, bool)> {
    let mut s = line;
    let mut is_global = false;
    let mut is_raw = false;
    if let Some(r) = s.strip_prefix('@')      { is_global = true; s = r; }
    else if let Some(r) = s.strip_prefix('~') { is_raw    = true; s = r; }
    let first = s.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' { return None; }
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let key  = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start_matches(|c| c == ' ' || c == '\t');
    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') { return None; }
    Some((key, rest.trim_start_matches(|c| c == ' ' || c == '\t').to_string(), is_raw, is_global))
}

fn is_global_dollar_assign(line: &str) -> Option<(String, String)> {
    let s = line.strip_prefix('@')?.trim_start();
    if s.is_empty() { return None; }
    let eq_pos = s.find('=')?;
    if s[eq_pos..].starts_with("==") { return None; }
    let key = s[..eq_pos].trim().to_string();
    let val = s[eq_pos + 1..].trim().to_string();
    if key.is_empty() { return None; }
    Some((key, val))
}

fn is_percent(line: &str) -> Option<(String, String)> {
    let s = line.strip_prefix('%')?;
    let first = s.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' { return None; }
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let key  = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start_matches(|c| c == ' ' || c == '\t');
    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') { return None; }
    Some((key, rest.trim_start_matches(|c| c == ' ' || c == '\t').to_string()))
}

fn is_spawn_assign(line: &str) -> Option<(String, String)> {
    let (key, val, _, _) = is_assignment(line)?;
    let rest = val.strip_prefix("spawn")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric()) { return None; }
    Some((key, rest.trim_start().to_string()))
}

fn is_await_assign(line: &str) -> Option<(String, String)> {
    let (key, val, _, _) = is_assignment(line)?;
    let rest = val.strip_prefix("await")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric()) { return None; }
    Some((key, rest.trim_start().to_string()))
}

fn is_do_assign(line: &str) -> Option<String> {
    let (key, val, _, _) = is_assignment(line)?;
    if val.trim() == "do" { Some(key) } else { None }
}

fn is_lambda_assign(line: &str) -> Option<(String, Vec<String>, String, bool, bool)> {
    let (key, val, is_raw, is_global) = is_assignment(line)?;
    let (params, body) = parse_lambda_literal(val.trim())?;
    Some((key, params, body, is_raw, is_global))
}

fn parse_lambda_literal(s: &str) -> Option<(Vec<String>, String)> {
    let s = s.strip_prefix('{')?.trim_start();
    let arrow = s.find(" -> ")?;
    let params_str = s[..arrow].trim();
    let rest = &s[arrow + 4..];
    let body = rest.trim_end_matches('}').trim().to_string();
    if body.is_empty() { return None; }
    let params: Vec<String> = params_str.split(',')
    .map(|p| p.trim().to_string())
    .filter(|p| p.starts_with('$'))
    .collect();
    if params.is_empty() { return None; }
    Some((params, body))
}

fn is_expr_assign(line: &str) -> Option<(String, String, bool, bool)> {
    let (key, val, is_raw, is_global) = is_assignment(line)?;
    if val.trim().starts_with('{') && val.contains(" -> ") { return None; }
    let is_list    = val.starts_with('[');
    let is_map     = val.starts_with('{');
    let has_op     = val.contains(" + ")  || val.contains(" - ")  ||
    val.contains(" * ")  || val.contains(" / ")  ||
    val.contains(" % ")  || val.contains(" == ") ||
    val.contains(" != ") || val.contains(" >= ") ||
    val.contains(" <= ") || val.contains(" > ")  ||
    val.contains(" < ")  || val.contains(" && ") ||
    val.contains(" || ") || val.contains(" ?! ");
    let has_interp = val.contains("$(");
    if is_list || is_map || has_op || has_interp { Some((key, val, is_raw, is_global)) } else { None }
}

fn is_collection_mut(line: &str) -> Option<(String, String, String)> {
    let s = line.strip_prefix('$')?;
    let dot_pos = s.find('.')?;
    let var = s[..dot_pos].to_string();
    let rest = &s[dot_pos + 1..];
    for method in &["push", "pop", "set", "del", "get"] {
        if let Some(after) = rest.strip_prefix(method) {
            if after.is_empty() || after.starts_with(' ') || after.starts_with('\t') {
                return Some((var, method.to_string(), after.trim().to_string()));
            }
        }
    }
    None
}

fn is_result_unwrap(line: &str) -> Option<(String, String)> {
    let pos  = line.find(" ?! ")?;
    let expr = line[..pos].trim().to_string();
    let msg  = line[pos + 4..].trim().trim_matches('"').to_string();
    Some((expr, msg))
}

fn is_arena_def(line: &str) -> Option<(String, String)> {
    let s = line.strip_prefix("::")?.trim_start();
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let name = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start().strip_prefix('[')?;
    let end  = rest.find(']')?;
    if rest[end + 1..].trim() != "def" { return None; }
    Some((name, rest[..end].trim().to_string()))
}

fn is_recur(line: &str) -> Option<String> {
    let rest = line.strip_prefix("recur")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') { return None; }
    Some(rest.trim().to_string())
}

fn is_destruct_list(line: &str) -> Option<(String, String, String)> {
    let s = line.trim();
    if !s.starts_with('[') { return None; }
    let end = s.find(']')?;
    let inside = &s[1..end];
    let pipe_pos = inside.find('|')?;
    let head = inside[..pipe_pos].trim().to_string();
    let tail = inside[pipe_pos + 1..].trim().to_string();
    if head.is_empty() || tail.is_empty() { return None; }
    let after = s[end + 1..].trim().strip_prefix('=')?;
    if after.starts_with('=') { return None; }
    Some((head, tail, after.trim().to_string()))
}

fn is_destruct_map(line: &str) -> Option<(Vec<String>, String)> {
    let s = line.trim();
    if !s.starts_with('{') { return None; }
    let end = s.find('}')?;
    let inside = &s[1..end];
    if inside.contains(':') || inside.contains("->") { return None; }
    let fields: Vec<String> = inside.split(',')
    .map(|f| f.trim().to_string())
    .filter(|f| !f.is_empty())
    .collect();
    if fields.is_empty() { return None; }
    let after = s[end + 1..].trim().strip_prefix('=')?;
    if after.starts_with('=') { return None; }
    Some((fields, after.trim().to_string()))
}

fn is_defer(line: &str) -> Option<String> {
    let rest = line.strip_prefix("defer")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') { return None; }
    Some(rest.trim().to_string())
}

fn is_scope_def(line: &str) -> bool { line.trim() == ";;scope def" }

fn is_test_def(line: &str) -> Option<String> {
    let s = line.trim().strip_prefix("==test")?.trim_start();
    if !s.starts_with('"') { return None; }
    let end = s[1..].find('"')?;
    Some(s[1..end + 1].to_string())
}

fn is_pipe_line(line: &str) -> Option<String> {
    let s = line.trim();
    if !s.starts_with("| ") || s.starts_with("|>") { return None; }
    Some(s[2..].trim().to_string())
}

fn is_match_stmt(line: &str) -> Option<String> {
    let s = line.trim().strip_prefix("match")?;
    if s.is_empty() || s.starts_with(|c: char| c.is_alphanumeric() || c == '_') { return None; }
    let s = s.trim_start();
    let end = s.find(" |>").or_else(|| s.find(" >")).or_else(|| s.find(" <|"))
    .unwrap_or(s.len());
    let cond = s[..end].trim().to_string();
    if cond.is_empty() { return None; }
    Some(cond)
}

fn is_match_arm(line: &str) -> Option<(String, String)> {
    for sep in &[" |> ", " <| ", " > "] {
        if let Some(pos) = line.find(sep) {
            let val = line[..pos].trim().to_string();
            let cmd = line[pos + sep.len()..].trim().to_string();
            if !val.is_empty() && !cmd.is_empty() {
                let kw = ["match ", "if ", "while ", "for ", "done", "do", "out",
                "log ", "spawn ", "await ", "defer ", "recur ", "end",
                "assert ", "try ", "==", "::", ";;", "//", "#", ">>", "\\\\", "--"];
                for k in &kw { if val.starts_with(k) { return None; } }
                return Some((val, cmd));
            }
        }
    }
    None
}

fn is_interface_block_start(line: &str) -> Option<String> {
    let s = line.trim().strip_prefix("==interface")?.trim_start();
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let name = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start();
    if !rest.starts_with('[') || rest[1..].contains(']') { return None; }
    Some(name)
}

fn is_interface_oneline(line: &str) -> Option<(String, Vec<String>)> {
    let s = line.trim().strip_prefix("==interface")?.trim_start();
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let name = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start().strip_prefix('[')?;
    let end = rest.rfind(']')?;
    let methods = rest[..end].split(',').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect();
    Some((name, methods))
}

fn is_adt_block_start(line: &str) -> Option<String> {
    let s = line.trim().strip_prefix("==type")?.trim_start();
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let name = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start();
    if !rest.starts_with('[') || rest[1..].contains(']') { return None; }
    Some(name)
}

fn is_adt_oneline(line: &str) -> Option<(String, Vec<(String, Vec<(String, String)>)>)> {
    let s = line.trim().strip_prefix("==type")?.trim_start();
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let name = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start().strip_prefix('[')?;
    let end = rest.rfind(']')?;
    Some((name, parse_adt_body(&rest[..end])))
}

fn parse_adt_body(body: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut variants = Vec::new();
    let mut depth    = 0i32;
    let mut current  = String::new();
    for ch in body.chars() {
        match ch {
            '[' => { depth += 1; current.push(ch); }
            ']' => { depth -= 1; current.push(ch); }
            ',' if depth == 0 => {
                let part = current.trim().to_string();
                if !part.is_empty() { variants.push(parse_adt_variant(&part)); }
                current.clear();
            }
            _ => { current.push(ch); }
        }
    }
    let part = current.trim().to_string();
    if !part.is_empty() { variants.push(parse_adt_variant(&part)); }
    variants
}

fn parse_adt_variant(part: &str) -> (String, Vec<(String, String)>) {
    let part = part.trim();
    if let Some(bracket) = part.find('[') {
        let vname  = part[..bracket].trim().to_string();
        let fstr   = &part[bracket + 1..];
        let fend   = fstr.rfind(']').unwrap_or(fstr.len());
        let fields = fstr[..fend].split(',')
        .filter_map(|f| {
            let f = f.trim();
            let c = f.find(':')?;
            Some((f[..c].trim().to_string(), f[c+1..].trim().to_string()))
        }).collect();
        (vname, fields)
    } else {
        (part.to_string(), Vec::new())
    }
}

fn is_impl_def(line: &str) -> Option<(String, String)> {
    let s = line.trim().strip_prefix(";;")?;
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let class = s[..ident_len].to_string();
    let rest  = s[ident_len..].trim_start().strip_prefix("impl")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') { return None; }
    let rest  = rest.trim_start();
    let iface_len = rest.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if iface_len == 0 { return None; }
    let interface = rest[..iface_len].to_string();
    if rest[iface_len..].trim() != "def" { return None; }
    Some((class, interface))
}

fn is_map_block_start(line: &str) -> Option<(String, bool, bool)> {
    let mut s = line;
    let mut is_global = false;
    let mut is_raw    = false;
    if let Some(r) = s.strip_prefix('@')      { is_global = true; s = r; }
    else if let Some(r) = s.strip_prefix('~') { is_raw    = true; s = r; }
    let first = s.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' { return None; }
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    let key  = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start_matches(|c| c == ' ' || c == '\t');
    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') { return None; }
    if rest.trim_start_matches(|c| c == ' ' || c == '\t').trim() != "{" { return None; }
    Some((key, is_raw, is_global))
}

fn parse_map_line(line: &str) -> Option<(String, String)> {
    let s = line.trim().trim_end_matches(',');
    let colon = s.find(':')?;
    let k = s[..colon].trim().to_string();
    let v = s[colon + 1..].trim().to_string();
    if k.is_empty() { return None; }
    Some((k, v))
}

fn is_list_block_start(line: &str) -> Option<(String, bool, bool)> {
    let mut s = line;
    let mut is_global = false;
    let mut is_raw    = false;
    if let Some(r) = s.strip_prefix('@')      { is_global = true; s = r; }
    else if let Some(r) = s.strip_prefix('~') { is_raw    = true; s = r; }
    let first = s.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' { return None; }
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    let key  = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start_matches(|c| c == ' ' || c == '\t');
    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') { return None; }
    if rest.trim_start_matches(|c| c == ' ' || c == '\t').trim() != "[" { return None; }
    Some((key, is_raw, is_global))
}

// "prefix [" — wywołanie z wieloliniową listą jako arg
fn is_call_list_block_start(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Musi kończyć się " [" (spacja + nawias otwierający)
    if !trimmed.ends_with(" [") { return None; }
    // Nie może być przypisaniem (obsłuży is_list_block_start)
    if is_list_block_start(line).is_some() { return None; }
    let prefix = trimmed.trim_end_matches('[').trim_end().to_string();
    if prefix.is_empty() { return None; }
    Some(prefix)
}

fn is_multiline_lambda_start(line: &str) -> Option<(String, String)> {
    let brace = line.find('{')?;
    let after = &line[brace + 1..];
    if !after.contains(" -> ") || after.contains('}') { return None; }
    Some((line[..brace].to_string(), after.to_string()))
}

fn parse_extern(raw: &str) -> (String, bool) {
    let t = raw.trim();
    if let Some(r) = t.strip_prefix("static") {
        if r.starts_with(|c: char| c.is_whitespace()) { return (r.trim().to_string(), true); }
    }
    (t.to_string(), false)
}

fn split_plugin(raw: &str) -> (String, String) {
    let t = raw.trim();
    match t.find(|c: char| c.is_whitespace()) {
        Some(i) => (t[..i].to_string(), t[i..].trim().to_string()),
        None    => (t.to_string(), String::new()),
    }
}

fn parse_lib_ref(pair: Pair<Rule>) -> LibRef {
    let mut inner = pair.into_inner();
    let lt  = inner.next().unwrap().as_str();
    let nm  = inner.next().unwrap().as_str().to_string();
    let ver = inner.next().map(|p: Pair<Rule>| p.as_str().to_string());
    let lib_type = match lt {
        "core"  => LibType::Core,
        "bytes" => LibType::Bytes,
        "virus" => LibType::Virus,
        "vira"  => LibType::Vira,
        _       => LibType::Core,
    };
    LibRef { lib_type, name: nm, version: ver }
}

fn line_to_op(line_pair: Pair<Rule>) -> Option<LineOp> {
    let mut inner = line_pair.into_inner();
    if inner.peek().map(|p| p.as_rule()) == Some(Rule::sudo) { inner.next(); }
    let stmt = inner.next()?;
    let node = stmt.into_inner().next()?;
    Some(match node.as_rule() {
        Rule::class_def  => LineOp::ClassDef(node.into_inner().next()?.as_str().to_string()),
         Rule::func_def => {
             let mut fi = node.into_inner();
             let name   = fi.next()?.as_str().to_string();
             let sig    = fi.next().map(|p| p.as_str().to_string());
             if let Some(ref s) = sig { if s.contains(" impl ") { return Some(LineOp::FuncDefGeneric(name, s.clone())); } }
             LineOp::FuncDef(name, sig)
         },
         Rule::func_done  => LineOp::FuncDone,
         Rule::call_stmt  => {
             let mut fi = node.into_inner();
             let path   = fi.next()?.as_str().to_string();
             let args   = fi.next().map(|p| p.as_str().to_string()).unwrap_or_default();
             LineOp::Call(path, args)
         },
         Rule::sys_dep    => LineOp::SysDep(node.into_inner().next()?.as_str().to_string()),
         Rule::lib_stmt   => LineOp::Lib(parse_lib_ref(node.into_inner().next()?)),
         Rule::sep_cmd    => LineOp::SepCmd(node.into_inner().next()?.as_str().to_string()),
         Rule::raw_cmd    => LineOp::RawCmd(node.into_inner().next()?.as_str().to_string()),
         Rule::expl_cmd   => { let mut fi = node.into_inner(); fi.next(); LineOp::ExplCmd(fi.next()?.as_str().to_string()) },
         Rule::plugin_stmt => { let (n, a) = split_plugin(node.into_inner().next()?.as_str()); LineOp::Plugin(n, a) },
         Rule::extern_stmt => { let (p, s) = parse_extern(node.into_inner().next()?.as_str()); LineOp::Extern(p, s) },
         Rule::loop_stmt => {
             let mut fi = node.into_inner();
             let n: u64 = fi.next()?.as_str().parse().unwrap_or(0); fi.next();
             LineOp::Loop(n, fi.next()?.as_str().to_string())
         },
         Rule::if_stmt => {
             let mut fi = node.into_inner();
             let c = fi.next()?.as_str().to_string(); fi.next();
             LineOp::If(c, fi.next()?.as_str().to_string())
         },
         Rule::elif_stmt => {
             let mut fi = node.into_inner();
             let c = fi.next()?.as_str().to_string(); fi.next();
             LineOp::Elif(c, fi.next()?.as_str().to_string())
         },
         Rule::else_stmt => { let mut fi = node.into_inner(); fi.next(); LineOp::Else(fi.next()?.as_str().to_string()) },
         Rule::while_stmt => {
             let mut fi = node.into_inner();
             let c = fi.next()?.as_str().to_string(); fi.next();
             LineOp::While(c, fi.next()?.as_str().to_string())
         },
         Rule::for_stmt => {
             let mut fi = node.into_inner();
             let v = fi.next()?.as_str().to_string();
             let i = fi.next()?.as_str().to_string(); fi.next();
             LineOp::For(v, i, fi.next()?.as_str().to_string())
         },
         Rule::bg_stmt    => LineOp::Bg(node.into_inner().next()?.as_str().to_string()),
         Rule::log_stmt   => LineOp::Log(node.into_inner().next()?.as_str().to_string()),
         Rule::lock_stmt  => {
             let mut fi = node.into_inner();
             let k = fi.next()?.into_inner().next()?.as_str().to_string();
             LineOp::Lock(k, fi.next()?.as_str().to_string())
         },
         Rule::unlock_stmt => LineOp::Unlock(node.into_inner().next()?.into_inner().next()?.as_str().to_string()),
         Rule::enum_stmt => {
             let mut fi = node.into_inner();
             let name   = fi.next()?.as_str().to_string();
             LineOp::Enum(name, fi.map(|p: Pair<Rule>| p.as_str().to_string()).collect())
         },
         Rule::struct_stmt => {
             let mut fi = node.into_inner();
             let name   = fi.next()?.as_str().to_string();
             let fields = fi.map(|p: Pair<Rule>| {
                 let mut f = p.into_inner();
                 (f.next().map(|x| x.as_str().to_string()).unwrap_or_default(),
                  f.next().map(|x| x.as_str().to_string()).unwrap_or_default())
             }).collect();
             LineOp::Struct(name, fields)
         },
         Rule::import_stmt => {
             let mut fi = node.into_inner();
             let res    = fi.next()?.as_str().trim_matches('"').to_string();
             LineOp::Import(res, fi.next().map(|p| p.as_str().to_string()))
         },
         Rule::try_stmt => {
             let mut fi = node.into_inner();
             LineOp::Try(fi.next()?.as_str().to_string(), fi.next()?.as_str().to_string())
         },
         Rule::raw_blk_s        => LineOp::RawBlockStart,
         Rule::raw_blk_e        => LineOp::RawBlockEnd,
         Rule::close_brace_stmt => LineOp::CloseBrace,
         Rule::end_stmt => LineOp::End(node.into_inner().next().and_then(|p: Pair<Rule>| p.as_str().parse().ok()).unwrap_or(0)),
         Rule::out_stmt => LineOp::Out(node.into_inner().next().map(|p| p.as_str().to_string()).unwrap_or_default()),
         Rule::percent_stmt => {
             let mut fi = node.into_inner();
             LineOp::Percent(fi.next()?.as_str().to_string(), fi.next()?.as_str().to_string())
         },
         Rule::spawn_stmt  => LineOp::Spawn(node.into_inner().next()?.as_str().to_string()),
         Rule::await_stmt  => LineOp::Await(node.into_inner().next()?.as_str().to_string()),
         Rule::assert_stmt => {
             let mut fi = node.into_inner();
             let cond   = fi.next()?.as_str().trim().to_string();
             LineOp::Assert(cond, fi.next().map(|p| p.as_str().trim_matches('"').to_string()))
         },
         Rule::match_arm => {
             let mut fi = node.into_inner();
             let val    = fi.next()?.as_str().trim().to_string(); fi.next();
             LineOp::MatchArm(val, fi.next()?.as_str().to_string())
         },
         Rule::pipe_stmt      => LineOp::Pipe(node.into_inner().map(|p| p.as_str().to_string()).collect()),
         Rule::pipe_line_stmt => LineOp::PipeLine(node.into_inner().next()?.as_str().to_string()),
         Rule::arena_def => {
             let mut fi = node.into_inner();
             LineOp::ArenaDef(fi.next()?.as_str().to_string(), fi.next()?.as_str().to_string())
         },
         Rule::collection_mut => {
             let raw = node.as_str();
             let s   = raw.strip_prefix('$').unwrap_or(raw);
             let dot = s.find('.').unwrap_or(s.len());
             let rm  = &s[dot + 1..];
             let sp  = rm.find(' ').unwrap_or(rm.len());
             LineOp::CollectionMut(s[..dot].to_string(), rm[..sp].to_string(), rm[sp..].trim().to_string())
         },
         Rule::result_unwrap => {
             if let Some((e, m)) = is_result_unwrap(node.as_str()) { LineOp::ResultUnwrap(e, m) } else { return None; }
         },
         Rule::module_call => {
             let mut fi = node.into_inner();
             let p      = fi.next()?.as_str().to_string();
             LineOp::ModuleCall(p, fi.next().map(|x| x.as_str().trim().to_string()).unwrap_or_default())
         },
         Rule::recur_stmt    => LineOp::Recur(node.into_inner().next()?.as_str().to_string()),
         Rule::defer_stmt    => LineOp::Defer(node.into_inner().next()?.as_str().to_string()),
         Rule::do_stmt       => LineOp::DoBlock,
         Rule::scope_def     => LineOp::ScopeDef,
         Rule::test_def      => LineOp::TestDef(node.into_inner().next()?.as_str().trim_matches('"').to_string()),
         Rule::lambda_lit    => {
             if let Some((p, b)) = parse_lambda_literal(node.as_str()) { LineOp::Lambda(p, b) } else { return None; }
         },
         Rule::destruct_stmt => {
             let raw = node.as_str();
             if let Some((h, t, s)) = is_destruct_list(raw)  { LineOp::DestructList(h, t, s) }
             else if let Some((f, s)) = is_destruct_map(raw) { LineOp::DestructMap(f, s) }
             else { return None; }
         },
         _ => return None,
    })
}

fn suggest(line: &str) -> String {
    let t = line.trim();
    for cmd in &["echo ", "mkdir ", "rm ", "cp ", "mv ", "cat ", "jq ",
        "curl ", "find ", "ls ", "touch ", "chmod ", "chown ",
        "git ", "date ", "printf ", "grep ", "sed ", "awk ",
        "tar ", "df ", "ps ", "free "] {
            if t.starts_with(cmd) { return format!("Brakuje prefiksu komendy — użyj: > {}", t); }
        }
        "Nieznana składnia — dokumentacja: https://hackeros-linux-system.github.io/HackerOS-Website/hacker-lang/docs.html".to_string()
}

fn strip_sudo(trim: &str) -> (&str, bool) {
    if let Some(r) = trim.strip_prefix('^') { (r.trim_start(), true) } else { (trim, false) }
}

fn emit(result: &mut AnalysisResult, scopes: &[Scope], node: Option<ProgramNode>) {
    if let Some(n) = node { push_node(result, scopes, n); }
}

pub fn parse_file(
    path: &str,
    resolve_libs: bool,
    verbose: bool,
    seen_libs: &mut HashSet<String>,
) -> Result<AnalysisResult, Vec<ParseError>> {
    let mut result = AnalysisResult::default();
    let src = match fs::read_to_string(path) {
        Ok(s)  => s,
        Err(e) => return Err(vec![ParseError::IoError { path: path.to_string(), message: e.to_string() }]),
    };
    let offsets: Vec<usize> = {
        let mut v = vec![0usize];
        for line in src.lines() { v.push(v.last().unwrap() + line.len() + 1); }
        v
    };
    let mut errors         = Vec::<ParseError>::new();
    let mut in_blk_comment = false;
    let mut in_raw_block   = false;
    let mut raw_buf        = String::new();
    let mut raw_sudo       = false;
    let mut raw_start_line = 0usize;
    let mut raw_start_off  = 0usize;
    let mut scopes         = Vec::<Scope>::new();
    let mut in_do_block   = false;
    let mut do_key        = String::new();
    let mut do_buf        = Vec::<ProgramNode>::new();
    let mut do_start_line = 0usize;
    let mut in_test_block = false;
    let mut test_desc     = String::new();
    let mut test_buf      = Vec::<ProgramNode>::new();
    let mut adt_block:       Option<AdtBlockState>      = None;
    let mut iface_block:     Option<InterfaceBlockState> = None;
    let mut map_block:       Option<MapBlockState>       = None;
    let mut list_block:      Option<ListBlockState>      = None;
    let mut call_list_block: Option<CallListBlockState>  = None;
    let mut lambda_block:    Option<LambdaBlockState>   = None;

    for (idx, raw_line) in src.lines().enumerate() {
        let off  = offsets[idx];
        let trim = raw_line.trim();
        if trim.is_empty() { continue; }
        if trim == "!!" { in_blk_comment = !in_blk_comment; continue; }
        if in_blk_comment { continue; }
        if trim.starts_with('!') { continue; }

        // ── BLOK: wieloliniowa lambda ──────────────────────────────────────
        if let Some(ref mut lb) = lambda_block {
            let is_close = trim == "}" || trim == "},"
            || (trim.starts_with('}') && trim[1..].trim().is_empty());
            if is_close {
                let body      = lb.collected.join(" ").trim().to_string();
                let full_line = format!("{}{{{}}}", lb.prefix, body);
                let ln = lb.start_line; let so = lb.start_off;
                lambda_block = None;
                if let Some(op) = try_parse_line(&full_line) {
                    if let Some(node) = build_node(ln, false, so, &full_line, op) {
                        if in_do_block        { do_buf.push(node); }
                        else if in_test_block { test_buf.push(node); }
                        else                  { push_node(&mut result, &scopes, node); }
                    }
                }
            } else { lb.collected.push(trim.to_string()); }
            continue;
        }

        // ── BLOK: call_list [\n  item,\n] ─────────────────────────────────
        if let Some(ref mut clb) = call_list_block {
            if trim == "]" {
                let full_line = format!("{} [{}]", clb.prefix, clb.items.join(", "));
                let ln = clb.start_line; let so = clb.start_off;
                call_list_block = None;
                let op = try_parse_line(&full_line).or_else(|| {
                    HlParser::parse(Rule::line, &full_line).ok()
                    .and_then(|mut p| p.next())
                    .and_then(line_to_op)
                });
                if let Some(op) = op {
                    if let Some(node) = build_node(ln, false, so, &full_line, op) {
                        if in_do_block        { do_buf.push(node); }
                        else if in_test_block { test_buf.push(node); }
                        else                  { push_node(&mut result, &scopes, node); }
                    }
                }
            } else {
                let item = trim.trim_end_matches(',').trim().to_string();
                if !item.is_empty() { clb.items.push(item); }
            }
            continue;
        }

        // ── BLOK: lista [\n  item,\n] ──────────────────────────────────────
        if let Some(ref mut lb) = list_block {
            if trim == "]" {
                let list_str = format!("[{}]", lb.items.join(", "));
                let op = if lb.is_global {
                    LineOp::GlobalVar(lb.key.clone(), list_str)
                } else {
                    LineOp::AssignExpr(lb.key.clone(), list_str, lb.is_raw, false)
                };
                let ln = lb.start_line; let so = lb.start_off;
                let orig = format!("{} = [...]", lb.key);
                list_block = None;
                if let Some(node) = build_node(ln, false, so, &orig, op) {
                    if in_do_block        { do_buf.push(node); }
                    else if in_test_block { test_buf.push(node); }
                    else                  { push_node(&mut result, &scopes, node); }
                }
            } else {
                let item = trim.trim_end_matches(',').trim().to_string();
                if !item.is_empty() { lb.items.push(item); }
            }
            continue;
        }

        // ── BLOK: mapa {\n  f: v,\n} ──────────────────────────────────────
        if let Some(ref mut mb) = map_block {
            if trim == "}" {
                let map_str = format!("{{{}}}", mb.lines.join(", "));
                let op = if mb.is_global {
                    LineOp::GlobalVar(mb.key.clone(), map_str)
                } else {
                    LineOp::AssignExpr(mb.key.clone(), map_str, mb.is_raw, false)
                };
                let ln = mb.start_line; let so = mb.start_off;
                let orig = format!("{} = {{...}}", mb.key);
                map_block = None;
                if let Some(node) = build_node(ln, false, so, &orig, op) {
                    if in_do_block        { do_buf.push(node); }
                    else if in_test_block { test_buf.push(node); }
                    else                  { push_node(&mut result, &scopes, node); }
                }
            } else if let Some((k, v)) = parse_map_line(trim) {
                mb.lines.push(format!("{}: {}", k, v));
            }
            continue;
        }

        // ── BLOK: ==type ──────────────────────────────────────────────────
        if let Some(ref mut adt) = adt_block {
            if trim == "]" {
                let variants = parse_adt_body(&adt.body_lines.join(","));
                let node = ProgramNode {
                    line_num: adt.start_line, is_sudo: false,
                    content:  CommandType::AdtDef { name: adt.name.clone(), variants },
                    original_text: format!("==type {} [...]", adt.name),
                    span: (adt.start_off, 0),
                };
                push_node(&mut result, &scopes, node);
                adt_block = None;
            } else {
                let v = trim.trim_end_matches(',').trim().to_string();
                if !v.is_empty() { adt.body_lines.push(v); }
            }
            continue;
        }

        // ── BLOK: ==interface ─────────────────────────────────────────────
        if let Some(ref mut iface) = iface_block {
            if trim == "]" {
                let node = ProgramNode {
                    line_num: iface.start_line, is_sudo: false,
                    content:  CommandType::Interface { name: iface.name.clone(), methods: iface.methods.clone() },
                    original_text: format!("==interface {} [...]", iface.name),
                    span: (iface.start_off, 0),
                };
                push_node(&mut result, &scopes, node);
                iface_block = None;
            } else {
                for m in trim.split(',') {
                    let m = m.trim().to_string();
                    if !m.is_empty() { iface.methods.push(m); }
                }
            }
            continue;
        }

        // ── do...done ─────────────────────────────────────────────────────
        if in_do_block {
            if trim == "done" {
                let node = ProgramNode {
                    line_num: do_start_line, is_sudo: false,
                    content:  CommandType::DoBlock { key: do_key.clone(), body: do_buf.clone() },
                    original_text: format!("{} = do", do_key),
                    span: (offsets[do_start_line.saturating_sub(1)], 0),
                };
                push_node(&mut result, &scopes, node);
                in_do_block = false; do_key.clear(); do_buf.clear();
            } else {
                let (ps, sudo_i) = strip_sudo(trim);
                // Inicjalizuj wieloliniowe bloki wewnątrz do
                if let Some((key, r, g)) = is_list_block_start(ps) {
                    list_block = Some(ListBlockState { key, is_raw: r, is_global: g, items: Vec::new(), start_line: idx+1, start_off: off });
                } else if let Some((key, r, g)) = is_map_block_start(ps) {
                    map_block = Some(MapBlockState { key, is_raw: r, is_global: g, lines: Vec::new(), start_line: idx+1, start_off: off });
                } else if let Some(prefix) = is_call_list_block_start(ps) {
                    call_list_block = Some(CallListBlockState { prefix, items: Vec::new(), start_line: idx+1, start_off: off });
                } else if ps.contains('{') && ps.contains(" -> ") && !ps.contains('}') {
                    if let Some((pfx, after)) = is_multiline_lambda_start(ps) {
                        lambda_block = Some(LambdaBlockState { prefix: pfx, collected: vec![after], start_line: idx+1, start_off: off });
                    } else if let Some(op) = try_parse_line(ps) {
                        if let Some(node) = build_node(idx+1, sudo_i, off, ps, op) { do_buf.push(node); }
                    }
                } else if let Some(op) = try_parse_line(ps) {
                    if let Some(node) = build_node(idx+1, sudo_i, off, ps, op) { do_buf.push(node); }
                }
            }
            continue;
        }

        // ── ==test ────────────────────────────────────────────────────────
        if in_test_block {
            if trim == "]" {
                let node = ProgramNode {
                    line_num: idx + 1, is_sudo: false,
                    content:  CommandType::TestBlock { desc: test_desc.clone(), body: test_buf.clone() },
                    original_text: format!("==test \"{}\" [...]", test_desc),
                    span: (off, 0),
                };
                push_node(&mut result, &scopes, node);
                in_test_block = false; test_desc.clear(); test_buf.clear();
            } else {
                let (ps, sudo_i) = strip_sudo(trim);
                if let Some(op) = try_parse_line(ps) {
                    if let Some(node) = build_node(idx+1, sudo_i, off, ps, op) { test_buf.push(node); }
                }
            }
            continue;
        }

        // ── raw block ─────────────────────────────────────────────────────
        if in_raw_block {
            if trim == "]" {
                let node = ProgramNode {
                    line_num: raw_start_line, is_sudo: raw_sudo,
                    content:  CommandType::RawNoSub(raw_buf.trim().to_string()),
                    original_text: "[ ... ]".to_string(),
                    span: (raw_start_off, raw_buf.len()),
                };
                push_node(&mut result, &scopes, node);
                in_raw_block = false; raw_buf.clear();
            } else { raw_buf.push_str(raw_line); raw_buf.push('\n'); }
            continue;
        }

        let (parse_src, is_sudo) = strip_sudo(trim);
        if is_sudo {
            result.is_potentially_unsafe = true;
            result.safety_warnings.push(format!("Linia {}: sudo (^)", idx + 1));
        }
        let line_num = idx + 1;
        let span     = SourceSpan::new(off.into(), parse_src.len().into());

        // ── Pre-pest etapy ────────────────────────────────────────────────
        if let Some((n, s)) = is_arena_def(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::ArenaDef(n, s))); continue;
        }
        if let Some((v, m, a)) = is_collection_mut(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::CollectionMut(v, m, a))); continue;
        }
        if parse_src.contains(" ?! ") && !parse_src.contains('=') {
            if let Some((e, msg)) = is_result_unwrap(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::ResultUnwrap(e, msg))); continue;
            }
        }
        if parse_src.starts_with("==interface ") {
            if let Some((n, ms)) = is_interface_oneline(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::InterfaceDef(n, ms))); continue;
            }
            if let Some(n) = is_interface_block_start(parse_src) {
                iface_block = Some(InterfaceBlockState { name: n, methods: Vec::new(), start_line: line_num, start_off: off }); continue;
            }
            push_err(&mut errors, path, &src, span, line_num, parse_src); continue;
        }
        if parse_src.starts_with("==type ") {
            if let Some((n, vs)) = is_adt_oneline(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AdtDef(n, vs))); continue;
            }
            if let Some(n) = is_adt_block_start(parse_src) {
                adt_block = Some(AdtBlockState { name: n, body_lines: Vec::new(), start_line: line_num, start_off: off }); continue;
            }
            push_err(&mut errors, path, &src, span, line_num, parse_src); continue;
        }
        if parse_src.starts_with(";;") && parse_src.contains(" impl ") {
            if let Some((c, i)) = is_impl_def(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::ImplDef(c.clone(), i)));
                scopes.push(Scope::Class(c)); continue;
            }
        }
        if parse_src.starts_with("==test ") {
            if let Some(desc) = is_test_def(parse_src) { in_test_block = true; test_desc = desc; test_buf.clear(); continue; }
        }
        if is_scope_def(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::ScopeDef));
            scopes.push(Scope::Class(format!("__scope_{}", line_num))); continue;
        }
        if let Some(e) = is_defer(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::Defer(e))); continue;
        }
        if let Some(a) = is_recur(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::Recur(a))); continue;
        }
        if let Some(s) = is_pipe_line(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::PipeLine(s))); continue;
        }
        // match $var |>  — pre-pest
        if parse_src.starts_with("match ") {
            if let Some(cond) = is_match_stmt(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::Match(cond))); continue;
            }
        }
        if parse_src.starts_with('[') && parse_src.contains('|') && parse_src.contains('=') {
            if let Some((h, t, s)) = is_destruct_list(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::DestructList(h, t, s))); continue;
            }
        }
        if parse_src.starts_with('{') && !parse_src.contains(':') && parse_src.contains('=') {
            if let Some((f, s)) = is_destruct_map(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::DestructMap(f, s))); continue;
            }
        }
        if let Some((k, v)) = is_percent(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::Percent(k, v))); continue;
        }
        if let Some((k, r)) = is_spawn_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignSpawn(k, r))); continue;
        }
        if let Some((k, r)) = is_await_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignAwait(k, r))); continue;
        }
        if let Some(key) = is_do_assign(parse_src) {
            in_do_block = true; do_key = key; do_buf = Vec::new(); do_start_line = line_num; continue;
        }
        if let Some((k, p, b, r, g)) = is_lambda_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignLambda(k, p, b, r, g))); continue;
        }
        if let Some((key, r, g)) = is_map_block_start(parse_src) {
            map_block = Some(MapBlockState { key, is_raw: r, is_global: g, lines: Vec::new(), start_line: line_num, start_off: off }); continue;
        }
        if let Some((key, r, g)) = is_list_block_start(parse_src) {
            list_block = Some(ListBlockState { key, is_raw: r, is_global: g, items: Vec::new(), start_line: line_num, start_off: off }); continue;
        }
        if let Some(prefix) = is_call_list_block_start(parse_src) {
            call_list_block = Some(CallListBlockState { prefix, items: Vec::new(), start_line: line_num, start_off: off }); continue;
        }
        if let Some((k, e, r, g)) = is_expr_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignExpr(k, e, r, g))); continue;
        }
        if parse_src.starts_with('@') {
            if let Some((k, v)) = is_global_dollar_assign(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::GlobalVar(k, v))); continue;
            }
        }
        if let Some((k, v, r, g)) = is_assignment(parse_src) {
            let op = if g { LineOp::GlobalVar(k, v) } else { LineOp::LocalVar(k, v, r) };
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, op)); continue;
        }
        if parse_src.contains('{') && parse_src.contains(" -> ") && !parse_src.contains('}') {
            if let Some((pfx, after)) = is_multiline_lambda_start(parse_src) {
                lambda_block = Some(LambdaBlockState { prefix: pfx, collected: vec![after], start_line: line_num, start_off: off });
                continue;
            }
        }

        // ── Pest ──────────────────────────────────────────────────────────
        let op = match HlParser::parse(Rule::line, parse_src) {
            Ok(mut pairs) => match line_to_op(pairs.next().unwrap()) {
                Some(op) => op,
                None     => { push_err(&mut errors, path, &src, span, line_num, parse_src); continue; },
            },
            Err(_) => { push_err(&mut errors, path, &src, span, line_num, parse_src); continue; },
        };
        match op {
            LineOp::ClassDef(name) => { scopes.push(Scope::Class(name)); },
            LineOp::ScopeDef       => { scopes.push(Scope::Class(format!("__scope_{}", line_num))); },
            LineOp::FuncDef(name, sig) => {
                let full = qualified(&scopes, &name);
                scopes.push(Scope::Func(full.clone()));
                result.functions.insert(full, (false, sig, Vec::new()));
            },
            LineOp::FuncDefGeneric(ref name, ref sig) => {
                let full = qualified(&scopes, name);
                scopes.push(Scope::Func(full.clone()));
                result.functions.insert(full, (false, Some(sig.clone()), Vec::new()));
            },
            LineOp::ArenaDef(ref name, ref size) => {
                let full = qualified(&scopes, name);
                scopes.push(Scope::Func(full.clone()));
                result.functions.insert(full, (false, Some(format!("[arena:{}]", size)), Vec::new()));
            },
            LineOp::FuncDone      => { scopes.pop(); },
            LineOp::RawBlockStart => { in_raw_block = true; raw_sudo = is_sudo; raw_start_line = line_num; raw_start_off = off; },
            LineOp::RawBlockEnd   => {
                errors.push(ParseError::SyntaxError {
                    src: NamedSource::new(path, src.clone()), span, line_num,
                            advice: "Nieoczekiwany ']' bez pasującego '['".to_string(),
                });
            },
            LineOp::CloseBrace => {},
            LineOp::SysDep(dep) => result.deps.push(dep),
            LineOp::Lib(lr)     => handle_lib(lr, path, &src, span, resolve_libs, verbose, seen_libs, &mut result, &mut errors),
            other => {
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, other) {
                    push_node(&mut result, &scopes, node);
                }
            },
        }
    }
    if errors.is_empty() { Ok(result) } else { Err(errors) }
}

fn try_parse_line(parse_src: &str) -> Option<LineOp> {
    if let Some(a) = is_recur(parse_src)    { return Some(LineOp::Recur(a)); }
    if let Some(e) = is_defer(parse_src)    { return Some(LineOp::Defer(e)); }
    if let Some(s) = is_pipe_line(parse_src) { return Some(LineOp::PipeLine(s)); }
    if let Some((v, m, a)) = is_collection_mut(parse_src) { return Some(LineOp::CollectionMut(v, m, a)); }
    if parse_src.contains(" ?! ") && !parse_src.contains('=') {
        if let Some((e, msg)) = is_result_unwrap(parse_src) { return Some(LineOp::ResultUnwrap(e, msg)); }
    }
    if parse_src.starts_with("==interface ") {
        if let Some((n, ms)) = is_interface_oneline(parse_src) { return Some(LineOp::InterfaceDef(n, ms)); }
    }
    if parse_src.starts_with("==type ") {
        if let Some((n, vs)) = is_adt_oneline(parse_src) { return Some(LineOp::AdtDef(n, vs)); }
    }
    if parse_src.starts_with(";;") && parse_src.contains(" impl ") {
        if let Some((c, i)) = is_impl_def(parse_src) { return Some(LineOp::ImplDef(c, i)); }
    }
    if parse_src.starts_with("match ") {
        if let Some(cond) = is_match_stmt(parse_src) { return Some(LineOp::Match(cond)); }
    }
    if let Some((k, v)) = is_percent(parse_src)           { return Some(LineOp::Percent(k, v)); }
    if let Some((k, r)) = is_spawn_assign(parse_src)      { return Some(LineOp::AssignSpawn(k, r)); }
    if let Some((k, r)) = is_await_assign(parse_src)      { return Some(LineOp::AssignAwait(k, r)); }
    if let Some((k, p, b, r, g)) = is_lambda_assign(parse_src) { return Some(LineOp::AssignLambda(k, p, b, r, g)); }
    if let Some((k, e, r, g)) = is_expr_assign(parse_src) { return Some(LineOp::AssignExpr(k, e, r, g)); }
    if parse_src.starts_with('@') {
        if let Some((k, v)) = is_global_dollar_assign(parse_src) { return Some(LineOp::GlobalVar(k, v)); }
    }
    if let Some((k, v, r, g)) = is_assignment(parse_src) {
        return Some(if g { LineOp::GlobalVar(k, v) } else { LineOp::LocalVar(k, v, r) });
    }
    if let Some((val, cmd)) = is_match_arm(parse_src) { return Some(LineOp::MatchArm(val, cmd)); }
    if let Ok(mut pairs) = HlParser::parse(Rule::line, parse_src) {
        if let Some(pair) = pairs.next() { return line_to_op(pair); }
    }
    None
}

fn qualified(scopes: &[Scope], name: &str) -> String {
    for s in scopes.iter().rev() {
        if let Scope::Class(cls) = s { return format!("{}.{}", cls, name); }
    }
    name.to_string()
}

fn push_err(errors: &mut Vec<ParseError>, path: &str, src: &str, span: SourceSpan, line_num: usize, line_src: &str) {
    errors.push(ParseError::SyntaxError {
        src: NamedSource::new(path, src.to_string()), span, line_num, advice: suggest(line_src),
    });
}

fn handle_lib(
    lib_ref: LibRef, path: &str, src: &str, span: SourceSpan,
    resolve_libs: bool, verbose: bool,
    seen_libs: &mut HashSet<String>,
    result: &mut AnalysisResult, errors: &mut Vec<ParseError>,
) {
    result.libs.push(lib_ref.clone());
    match lib_ref.lib_type {
        LibType::Vira | LibType::Virus => {
            if verbose { eprintln!("[lib] {}: {}", lib_ref.lib_type.as_str(), lib_ref.name); }
        },
        LibType::Bytes => {
            if verbose { if let Some(p) = lib_path(&lib_ref) { eprintln!("[lib] bin: {}", p.display()); } }
        },
        LibType::Core => {
            if !resolve_libs { return; }
            let key = lib_ref.cache_key();
            if !seen_libs.insert(key.clone()) {
                if verbose { eprintln!("[lib] już widziany: {}", key); }
                return;
            }
            let fp = match lib_path(&lib_ref) { Some(p) => p, None => return };
            if verbose { eprintln!("[lib] parsowanie: {}", fp.display()); }
            if let Some(p) = fp.to_str() {
                match parse_file(p, resolve_libs, verbose, seen_libs) {
                    Ok(lr) => {
                        result.deps.extend(lr.deps);
                        result.libs.extend(lr.libs);
                        result.functions.extend(lr.functions);
                        result.main_body.extend(lr.main_body);
                        result.is_potentially_unsafe |= lr.is_potentially_unsafe;
                        result.safety_warnings.extend(lr.safety_warnings);
                    },
                    Err(mut e) => errors.append(&mut e),
                }
            } else {
                errors.push(ParseError::SyntaxError {
                    src: NamedSource::new(path, src.to_string()), span, line_num: 0,
                            advice: format!("Nieprawidłowa ścieżka lib: {}", lib_ref.name),
                });
            }
        },
    }
}

fn push_node(result: &mut AnalysisResult, scopes: &[Scope], node: ProgramNode) {
    for scope in scopes.iter().rev() {
        if let Scope::Func(name) = scope {
            if let Some(f) = result.functions.get_mut(name) { f.2.push(node); return; }
        }
    }
    result.main_body.push(node);
}

fn build_node(line_num: usize, is_sudo: bool, off: usize, src: &str, op: LineOp) -> Option<ProgramNode> {
    let cmd = match op {
        LineOp::SepCmd(c)              => CommandType::Isolated(c),
        LineOp::RawCmd(c)              => CommandType::RawNoSub(c),
        LineOp::ExplCmd(c)             => CommandType::RawSub(c),
        LineOp::GlobalVar(k, v)        => CommandType::AssignEnv   { key: k, val: v },
        LineOp::LocalVar(k, v, r)      => CommandType::AssignLocal { key: k, val: v, is_raw: r },
        LineOp::Loop(n, c)             => CommandType::Loop        { count: n, cmd: c },
        LineOp::If(co, c)              => CommandType::If          { cond: co, cmd: c },
        LineOp::Elif(co, c)            => CommandType::Elif        { cond: co, cmd: c },
        LineOp::Else(c)                => CommandType::Else        { cmd: c },
        LineOp::While(co, c)           => CommandType::While       { cond: co, cmd: c },
        LineOp::For(v, i, c)           => CommandType::For         { var: v, in_: i, cmd: c },
        LineOp::Bg(c)                  => CommandType::Background(c),
        LineOp::Call(p, a)             => CommandType::Call        { path: p, args: a },
        LineOp::Plugin(n, a)           => CommandType::Plugin      { name: n, args: a, is_super: is_sudo },
        LineOp::Log(m)                 => CommandType::Log(m),
        LineOp::Lock(k, v)             => CommandType::Lock        { key: k, val: v },
        LineOp::Unlock(k)              => CommandType::Unlock      { key: k },
        LineOp::Extern(p, sl)          => CommandType::Extern      { path: p, static_link: sl },
        LineOp::Import(r, ns)          => CommandType::Import      { resource: r, namespace: ns },
        LineOp::Enum(n, vars)          => CommandType::Enum        { name: n, variants: vars },
        LineOp::Struct(n, flds)        => CommandType::Struct      { name: n, fields: flds },
        LineOp::Try(t, c)              => CommandType::Try         { try_cmd: t, catch_cmd: c },
        LineOp::End(code)              => CommandType::End         { code },
        LineOp::Out(v)                 => CommandType::Out(v),
        LineOp::Percent(k, v)          => CommandType::Const       { key: k, val: v },
        LineOp::Spawn(r)               => CommandType::Spawn(r),
        LineOp::Await(r)               => CommandType::Await(r),
        LineOp::AssignSpawn(k, r)      => CommandType::AssignSpawn { key: k, task: r },
        LineOp::AssignAwait(k, r)      => CommandType::AssignAwait { key: k, expr: r },
        LineOp::Assert(c, m)           => CommandType::Assert      { cond: c, msg: m },
        LineOp::Match(c)               => CommandType::Match       { cond: c },
        LineOp::MatchArm(v, c)         => CommandType::MatchArm   { val: v, cmd: c },
        LineOp::Pipe(steps)            => CommandType::Pipe(steps),
        LineOp::AssignExpr(k, e, r, g) => if g {
            CommandType::AssignEnv  { key: k, val: e }
        } else {
            CommandType::AssignExpr { key: k, expr: e, is_raw: r, is_global: false }
        },
        LineOp::AssignList(k, items, r, g) => {
            let expr = format!("[{}]", items.join(", "));
            if g { CommandType::AssignEnv { key: k, val: expr } }
            else { CommandType::AssignExpr { key: k, expr, is_raw: r, is_global: false } }
        },
        LineOp::CollectionMut(v, m, a)       => CommandType::CollectionMut { var: v, method: m, args: a },
        LineOp::InterfaceDef(n, ms)          => CommandType::Interface     { name: n, methods: ms },
        LineOp::ImplDef(c, i)               => CommandType::ImplDef       { class: c, interface: i },
        LineOp::ArenaDef(n, s)              => CommandType::ArenaDef      { name: n, size: s },
        LineOp::ResultUnwrap(e, m)          => CommandType::ResultUnwrap  { expr: e, msg: m },
        LineOp::ModuleCall(p, a)            => CommandType::ModuleCall    { path: p, args: a },
        LineOp::Lambda(params, body)        => CommandType::Lambda        { params, body },
        LineOp::AssignLambda(k, p, b, r, g) => CommandType::AssignLambda { key: k, params: p, body: b, is_raw: r, is_global: g },
        LineOp::Recur(args)                 => CommandType::Recur         { args },
        LineOp::DestructList(h, t, s)       => CommandType::DestructList  { head: h, tail: t, source: s },
        LineOp::DestructMap(flds, s)        => CommandType::DestructMap   { fields: flds, source: s },
        LineOp::ScopeDef                    => CommandType::ScopeDef,
        LineOp::AdtDef(n, vs)               => CommandType::AdtDef        { name: n, variants: vs },
        LineOp::DoBlock                     => return None,
        LineOp::AssignDo(k)                 => CommandType::DoBlock       { key: k, body: Vec::new() },
        LineOp::PipeLine(step)              => CommandType::PipeLine      { step },
        LineOp::TestDef(desc)               => CommandType::TestBlock     { desc, body: Vec::new() },
        LineOp::Defer(expr)                 => CommandType::Defer         { expr },
        LineOp::FuncDefGeneric(n, s)        => CommandType::FuncDefGeneric { name: n, sig: s },
        LineOp::CloseBrace                  => return None,
        _                                   => return None,
    };
    Some(ProgramNode { line_num, is_sudo, content: cmd, original_text: src.to_string(), span: (off, src.len()) })
}
