use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use std::collections::HashSet;
use std::fs;
use miette::NamedSource;

use crate::ast::{
    AnalysisResult, CommandType, FuncAttr, FunctionMeta, LibRef, LibType,
    ModuleMeta, ParseError, ProgramNode, SourceSpan, Visibility,
};
use crate::lib_resolver::handle_lib;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct HlParser;

// ─────────────────────────────────────────────────────────────
// LineOp — wewnętrzna reprezentacja sparsowanej linii
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum LineOp {
    // ── Istniejące ────────────────────────────────────────────
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
    Extern(String, bool, Option<Vec<String>>),
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

    // ── NOWE ─────────────────────────────────────────────────
    /// =; name def — otwarcie modułu (z opcjonalnym pub)
    ModuleDef(String, Visibility),
    /// =; done — zamknięcie modułu
    ModuleDone,
    /// =: — toggle bloku scoped
    ScopeBlockToggle,
    /// # <lib> use [syms] — import z wybranymi symbolami (obs. przez Lib)
    /// pub przed :func def lub ;;class def
    PubPrefix,
    /// x = 5 [i32]
    AssignTyped(String, String, String, bool, bool),
    /// match $x =>
    MatchFat(String),
    /// Variant [fields] => cmd
    MatchArmFat(String, Vec<String>, String),
    /// {fields} => cmd
    MatchArmDestructFat(Vec<String>, String),
    /// *{flag} / *{key: val}
    CondComp(String),
    /// *{end}
    CondCompEnd,
    /// |] attr_name ["arg"]
    AttrDecl(FuncAttr),
}

// ─────────────────────────────────────────────────────────────
// Stan wieloliniowych bloków
// ─────────────────────────────────────────────────────────────
struct AdtBlockState      { name: String, body_lines: Vec<String>, start_line: usize, start_off: usize }
struct InterfaceBlockState { name: String, methods: Vec<String>, start_line: usize, start_off: usize }
struct MapBlockState      { key: String, is_raw: bool, is_global: bool, lines: Vec<String>, start_line: usize, start_off: usize }
struct ListBlockState     { key: String, is_raw: bool, is_global: bool, items: Vec<String>, start_line: usize, start_off: usize }
struct CallListBlockState { prefix: String, items: Vec<String>, start_line: usize, start_off: usize }
struct LambdaBlockState   { prefix: String, collected: Vec<String>, start_line: usize, start_off: usize }
struct ModuleBlockState   { name: String, visibility: Visibility, start_line: usize, path: Vec<String> }
struct ScopeBlockState    { body: Vec<ProgramNode>, start_line: usize, start_off: usize }

// ─────────────────────────────────────────────────────────────
// Scope tracker
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
enum Scope { Class(String), Func(String) }

// ─────────────────────────────────────────────────────────────
// Pre-pest rozpoznawanie nowych składni
// ─────────────────────────────────────────────────────────────

fn is_module_def(line: &str) -> Option<(String, Visibility)> {
    let s = line.trim();
    let (vis, s) = if let Some(r) = s.strip_prefix("pub ") {
        (Visibility::Public, r.trim_start())
    } else {
        (Visibility::Private, s)
    };
    let s = s.strip_prefix("=;")?;
    let s = s.trim_start();
    if s == "done" { return None; }
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if ident_len == 0 { return None; }
    let name = s[..ident_len].to_string();
    let rest = s[ident_len..].trim();
    if rest != "def" { return None; }
    Some((name, vis))
}

fn is_module_done(line: &str) -> bool {
    line.trim() == "=; done"
}

fn is_scope_block_toggle(line: &str) -> bool {
    line.trim() == "=:"
}

fn is_pub_prefix(line: &str) -> bool {
    line.trim() == "pub"
}

fn is_attr_decl(line: &str) -> Option<FuncAttr> {
    let s = line.trim().strip_prefix("|]")?;
    let s = s.trim_start();
    if s.is_empty() { return None; }
    let name_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
    .map(|c| c.len_utf8()).sum::<usize>();
    if name_len == 0 { return None; }
    let name = s[..name_len].to_string();
    let rest = s[name_len..].trim_start();
    let arg = if rest.starts_with('"') {
        Some(rest.trim_matches('"').to_string())
    } else if !rest.is_empty() {
        Some(rest.to_string())
    } else {
        None
    };
    Some(FuncAttr { name, arg })
}

fn is_cond_comp(line: &str) -> Option<LineOp> {
    let s = line.trim();
    if s == "*{end}" { return Some(LineOp::CondCompEnd); }
    if !s.starts_with("*{") || !s.ends_with('}') { return None; }
    let inner = &s[2..s.len() - 1];
    if inner.is_empty() { return None; }
    Some(LineOp::CondComp(inner.to_string()))
}

fn is_match_fat(line: &str) -> Option<String> {
    let s = line.trim();
    let s = s.strip_prefix("match")?;
    if s.is_empty() || s.starts_with(|c: char| c.is_alphanumeric() || c == '_') { return None; }
    let s = s.trim_start();
    // kończy się " =>"
    let s = s.strip_suffix("=>")?;
    let cond = s.trim_end().to_string();
    if cond.is_empty() { return None; }
    Some(cond)
}

fn is_match_arm_fat(line: &str) -> Option<LineOp> {
    let s = line.trim();
    // destruct arm: {name, age} => cmd
    if s.starts_with('{') {
        if let Some(end) = s.find("} =>") {
            let fields_str = &s[1..end];
            let fields: Vec<String> = fields_str.split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
            if !fields.is_empty() {
                let cmd = s[end + 4..].trim().to_string();
                if !cmd.is_empty() {
                    return Some(LineOp::MatchArmDestructFat(fields, cmd));
                }
            }
        }
        return None;
    }
    // variant arm: Circle [r] => cmd  lub  Point => cmd
    let arrow_pos = s.find(" =>")?;
    let before = s[..arrow_pos].trim();
    let cmd = s[arrow_pos + 3..].trim().to_string();
    if cmd.is_empty() || before.is_empty() { return None; }
    // sprawdź czy nie zaczyna się od słów kluczowych
    for kw in &["match ", "if ", "while ", "for ", "done", "do", "out", "log ",
        "spawn ", "await ", "defer ", "recur ", "end", "assert "] {
            if before.starts_with(kw) { return None; }
        }
        if let Some(bracket) = before.find(" [") {
            let variant = before[..bracket].trim().to_string();
            let fields_str = &before[bracket + 2..];
            let fields_end = fields_str.find(']').unwrap_or(fields_str.len());
            let fields: Vec<String> = fields_str[..fields_end].split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
            Some(LineOp::MatchArmFat(variant, fields, cmd))
        } else {
            Some(LineOp::MatchArmFat(before.to_string(), vec![], cmd))
        }
}

/// Rozpoznaje `x = 5 [i32]` — przypisanie z adnotacją typu numerycznego
fn is_typed_assign(line: &str) -> Option<(String, String, String, bool, bool)> {
    if !line.ends_with(']') { return None; }
    let bracket = line.rfind(" [")?;
    let type_ann = line[bracket + 2..line.len() - 1].trim().to_string();
    // sprawdź czy to typ numeryczny
    let valid_types = ["i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64", "int", "float", "bool", "str"];
    if !valid_types.contains(&type_ann.as_str()) { return None; }
    let without_ann = &line[..bracket];
    let (key, expr, is_raw, is_global) = is_assignment(without_ann)?;
    Some((key, expr, type_ann, is_raw, is_global))
}

/// Pre-pest parser dla `# <typ/nazwa[:ver]> [use [sym1, sym2]]`
/// Omija pest całkowicie — bezpieczna alternatywa gdy WHITESPACE skipper
/// koliduje z explicit spacją w use_list.
fn parse_lib_use_pre_pest(line: &str) -> Option<LibRef> {
    let s = line.trim().strip_prefix('#')?.trim_start();
    if !s.starts_with('<') { return None; }
    let close = s.find('>')?;
    let inner = &s[1..close];
    // inner = "typ/nazwa" lub "typ/nazwa:ver"
    let slash = inner.find('/')?;
    let lt_str = &inner[..slash];
    let rest   = &inner[slash + 1..];
    let lib_type = match lt_str {
        "core"  => LibType::Core,
        "bytes" => LibType::Bytes,
        "virus" => LibType::Virus,
        "vira"  => LibType::Vira,
        _       => return None,
    };
    let (name, version) = if let Some(colon) = rest.find(':') {
        (rest[..colon].to_string(), Some(rest[colon + 1..].to_string()))
    } else {
        (rest.to_string(), None)
    };
    // opcjonalne: " use [sym1, sym2]"
    let after_bracket = s[close + 1..].trim_start();
    let use_symbols = if let Some(use_rest) = after_bracket.strip_prefix("use") {
        let use_rest = use_rest.trim_start();
        if let Some(bracket_rest) = use_rest.strip_prefix('[') {
            let end = bracket_rest.find(']').unwrap_or(bracket_rest.len());
            let syms: Vec<String> = bracket_rest[..end]
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
            if syms.is_empty() { None } else { Some(syms) }
        } else { None }
    } else { None };
    Some(LibRef { lib_type, name, version, use_symbols })
}

/// Rozpoznaje rozbudowany extern: -- path use [sym1, sym2]
fn parse_extern_extended(raw: &str) -> (String, bool, Option<Vec<String>>) {
    let t = raw.trim();
    let (t, is_static) = if let Some(r) = t.strip_prefix("static") {
        if r.starts_with(|c: char| c.is_whitespace()) { (r.trim(), true) } else { (t, false) }
    } else { (t, false) };
    // use [sym1, sym2]?
    if let Some(use_pos) = t.find(" use [") {
        let path = t[..use_pos].trim().to_string();
        let syms_str = &t[use_pos + 6..];
        let end = syms_str.find(']').unwrap_or(syms_str.len());
        let syms: Vec<String> = syms_str[..end].split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
        (path, is_static, Some(syms))
    } else {
        (t.to_string(), is_static, None)
    }
}

// ─────────────────────────────────────────────────────────────
// Istniejące funkcje pomocnicze (zachowane bez zmian)
// ─────────────────────────────────────────────────────────────

fn is_assignment(line: &str) -> Option<(String, String, bool, bool)> {
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
    let mut is_global = false; let mut is_raw = false;
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
    let mut is_global = false; let mut is_raw = false;
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

fn is_call_list_block_start(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.ends_with(" [") { return None; }
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

fn split_plugin(raw: &str) -> (String, String) {
    let t = raw.trim();
    match t.find(|c: char| c.is_whitespace()) {
        Some(i) => (t[..i].to_string(), t[i..].trim().to_string()),
        None    => (t.to_string(), String::new()),
    }
}

fn parse_lib_ref_with_use(pair: Pair<Rule>) -> LibRef {
    let mut inner      = pair.into_inner();
    let lib_ref_pair   = inner.next().unwrap();
    let mut lib_inner  = lib_ref_pair.into_inner();
    let lt  = lib_inner.next().unwrap().as_str();
    let nm  = lib_inner.next().unwrap().as_str().to_string();
    let ver = lib_inner.next().map(|p: Pair<Rule>| p.as_str().to_string());
    let lib_type = match lt {
        "core"  => LibType::Core,
        "bytes" => LibType::Bytes,
        "virus" => LibType::Virus,
        "vira"  => LibType::Vira,
        _       => LibType::Core,
    };
    // opcjonalna lista use
    let use_symbols = inner.next().map(|use_list_pair| {
        use_list_pair.into_inner()
        .map(|p: Pair<Rule>| p.as_str().to_string())
        .collect()
    });
    LibRef { lib_type, name: nm, version: ver, use_symbols }
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
         Rule::lib_stmt   => LineOp::Lib(parse_lib_ref_with_use(node)),
         Rule::sep_cmd    => LineOp::SepCmd(node.into_inner().next()?.as_str().to_string()),
         Rule::raw_cmd    => LineOp::RawCmd(node.into_inner().next()?.as_str().to_string()),
         Rule::expl_cmd   => { let mut fi = node.into_inner(); fi.next(); LineOp::ExplCmd(fi.next()?.as_str().to_string()) },
         Rule::plugin_stmt => { let (n, a) = split_plugin(node.into_inner().next()?.as_str()); LineOp::Plugin(n, a) },
         Rule::extern_stmt => {
             let raw = node.into_inner().next()?.as_str();
             let (p, s, u) = parse_extern_extended(raw);
             LineOp::Extern(p, s, u)
         },
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
         Rule::else_stmt  => { let mut fi = node.into_inner(); fi.next(); LineOp::Else(fi.next()?.as_str().to_string()) },
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
         Rule::end_stmt         => LineOp::End(node.into_inner().next().and_then(|p: Pair<Rule>| p.as_str().parse().ok()).unwrap_or(0)),
         Rule::out_stmt         => LineOp::Out(node.into_inner().next().map(|p| p.as_str().to_string()).unwrap_or_default()),
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
         Rule::match_stmt_fat => {
             let raw = node.as_str().trim();
             let cond = raw.strip_prefix("match").unwrap_or("").trim()
             .strip_suffix("=>").unwrap_or("").trim().to_string();
             LineOp::MatchFat(cond)
         },
         Rule::match_arm_fat => {
             let raw = node.as_str().trim();
             if let Some(op) = is_match_arm_fat(raw) { op } else { return None; }
         },
         Rule::match_arm_destruct => {
             let raw = node.as_str().trim();
             if let Some(op) = is_match_arm_fat(raw) { op } else { return None; }
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
         Rule::cond_comp_s  => {
             let flag = node.as_str().trim()
             .strip_prefix("*{").unwrap_or("")
             .strip_suffix('}').unwrap_or("").to_string();
             LineOp::CondComp(flag)
         },
         Rule::cond_comp_e  => LineOp::CondCompEnd,
         Rule::attr_stmt    => {
             let raw  = node.as_str().trim().strip_prefix("|]").unwrap_or("").trim();
             let nlen = raw.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').map(|c| c.len_utf8()).sum::<usize>();
             let name = raw[..nlen].to_string();
             let arg  = raw[nlen..].trim_start().to_string();
             let arg  = if arg.is_empty() { None } else { Some(arg.trim_matches('"').to_string()) };
             LineOp::AttrDecl(FuncAttr { name, arg })
         },
         Rule::match_stmt => {
             let mut fi = node.into_inner();
             let c = fi.next()?.as_str().to_string();
             LineOp::Match(c)
         },
         _ => return None,
    })
}

// ─────────────────────────────────────────────────────────────
// suggest() — podpowiedź dla błędów składni
// ─────────────────────────────────────────────────────────────
fn suggest(line: &str) -> String {
    let t = line.trim();
    // moduł bez przypisania: ident.ident ...
    if is_module_call_standalone(t).is_some() {
        return "Wywołanie modułu — poprawna składnia: ident.metoda [args]".to_string();
    }
    // lib z use
    if t.starts_with("# <") && t.contains("> use [") {
        return "Import biblioteki — poprawna składnia: # <typ/nazwa> use [sym1, sym2]".to_string();
    }
    for cmd in &["echo ", "mkdir ", "rm ", "cp ", "mv ", "cat ", "jq ",
        "curl ", "find ", "ls ", "touch ", "chmod ", "chown ",
        "git ", "date ", "printf ", "grep ", "sed ", "awk ",
        "tar ", "df ", "ps ", "free "] {
            if t.starts_with(cmd) {
                return format!("Brakuje prefiksu komendy — użyj: >> {}", t);
            }
        }
        "Nieznana składnia".to_string()
}

/// Rozpoznaje standalone wywołanie modułu: ident.ident [args]
/// np. sqlite3.exec "query"  /  redis.set $k $v  /  http.get $url
fn is_module_call_standalone(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    // nie może zaczynać się od $ . : ; = # > < \\ & | ^ ? ! * %
    let first = t.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' { return None; }
    // znajdź pierwszą kropkę
    let dot = t.find('.')?;
    let prefix = &t[..dot];
    // prefix musi być czystym identem (no spaces)
    if prefix.contains(' ') || prefix.contains('\t') { return None; }
    if !prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') { return None; }
    let after_dot = &t[dot + 1..];
    // po kropce musi być ident (nazwa metody)
    let method_len = after_dot.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8()).sum::<usize>();
    if method_len == 0 { return None; }
    let path = format!("{}.{}", prefix, &after_dot[..method_len]);
    let args = after_dot[method_len..].trim().to_string();
    Some((path, args))
}

fn strip_sudo(trim: &str) -> (&str, bool) {
    if let Some(r) = trim.strip_prefix('^') { (r.trim_start(), true) } else { (trim, false) }
}

fn strip_pub(trim: &str) -> (&str, Visibility) {
    if let Some(r) = trim.strip_prefix("pub ") {
        (r.trim_start(), Visibility::Public)
    } else {
        (trim, Visibility::Private)
    }
}

fn emit(result: &mut AnalysisResult, scopes: &[Scope], node: Option<ProgramNode>) {
    if let Some(n) = node { push_node(result, scopes, n); }
}

fn push_err(
    errors: &mut Vec<ParseError>, path: &str, src: &str,
    span: SourceSpan, line_num: usize, line_src: &str,
) {
    errors.push(ParseError::SyntaxError {
        src: NamedSource::new(path, src.to_string()), span, line_num, advice: suggest(line_src),
    });
}

// ─────────────────────────────────────────────────────────────
// register_function — wspólna logika rejestracji funkcji
// ─────────────────────────────────────────────────────────────
fn register_function(
    name:          String,
    sig:           Option<String>,
    is_arena:      bool,
    scopes:        &mut Vec<Scope>,
    result:        &mut AnalysisResult,
    pending_attrs: &mut Vec<FuncAttr>,
    next_vis:      &mut Visibility,
    module_stack:  &[ModuleBlockState],
) {
    let full  = qualified(scopes, &name);
    let vis   = std::mem::replace(next_vis, Visibility::Private);
    let attrs = pending_attrs.drain(..).collect();
    scopes.push(Scope::Func(full.clone()));
    if let Some(mb) = module_stack.last() {
        let mut mpath = mb.path.clone();
        mpath.push(mb.name.clone());
        let mk = mpath.join("::");
        result.modules.entry(mk).or_default().functions.push(full.clone());
    }
    result.functions.insert(full.clone(), FunctionMeta {
        is_arena,
        sig,
        body:        Vec::new(),
                            attrs,
                            visibility:  vis,
                            module_path: module_stack.iter().map(|m| m.name.clone()).collect(),
    });
    result.functions_compat.insert(full, (is_arena, None, Vec::new()));
}

// ─────────────────────────────────────────────────────────────
// Główna funkcja parsowania
// ─────────────────────────────────────────────────────────────
pub fn parse_file(
    path:         &str,
    resolve_libs: bool,
    verbose:      bool,
    seen_libs:    &mut HashSet<String>,
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

    let mut errors             = Vec::<ParseError>::new();
    let mut in_blk_comment     = false;
    let mut in_raw_block       = false;
    let mut raw_buf            = String::new();
    let mut raw_sudo           = false;
    let mut raw_start_line     = 0usize;
    let mut raw_start_off      = 0usize;
    let mut scopes             = Vec::<Scope>::new();
    let mut in_do_block        = false;
    let mut do_key             = String::new();
    let mut do_buf             = Vec::<ProgramNode>::new();
    let mut do_start_line      = 0usize;
    let mut in_test_block      = false;
    let mut test_desc          = String::new();
    let mut test_buf           = Vec::<ProgramNode>::new();
    let mut adt_block:       Option<AdtBlockState>       = None;
    let mut iface_block:     Option<InterfaceBlockState> = None;
    let mut map_block:       Option<MapBlockState>        = None;
    let mut list_block:      Option<ListBlockState>       = None;
    let mut call_list_block: Option<CallListBlockState>   = None;
    let mut lambda_block:    Option<LambdaBlockState>    = None;

    // ── NOWE stany ────────────────────────────────────────────
    let mut module_stack:    Vec<ModuleBlockState>       = Vec::new();
    let mut scope_block:     Option<ScopeBlockState>     = None;
    // oczekujący atrybut dla następnej funkcji
    let mut pending_attrs:   Vec<FuncAttr>               = Vec::new();
    // pub oczekujący na następną definicję
    let mut next_vis:        Visibility                  = Visibility::Private;

    for (idx, raw_line) in src.lines().enumerate() {
        let off  = offsets[idx];
        let trim = raw_line.trim();
        if trim.is_empty() { continue; }
        if trim == "!!"   { in_blk_comment = !in_blk_comment; continue; }
        if in_blk_comment { continue; }
        if trim.starts_with('!') { continue; }

        // ── BLOK: wieloliniowa lambda ──────────────────────────
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
                        dispatch_node(&mut result, &scopes, node,
                                      &mut in_do_block, &mut do_buf,
                                      &mut in_test_block, &mut test_buf,
                                      scope_block.as_mut());
                    }
                }
            } else { lb.collected.push(trim.to_string()); }
            continue;
        }

        // ── BLOK: call_list ────────────────────────────────────
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
                        dispatch_node(&mut result, &scopes, node,
                                      &mut in_do_block, &mut do_buf,
                                      &mut in_test_block, &mut test_buf,
                                      scope_block.as_mut());
                    }
                }
            } else {
                let item = trim.trim_end_matches(',').trim().to_string();
                if !item.is_empty() { clb.items.push(item); }
            }
            continue;
        }

        // ── BLOK: lista ────────────────────────────────────────
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
                    dispatch_node(&mut result, &scopes, node,
                                  &mut in_do_block, &mut do_buf,
                                  &mut in_test_block, &mut test_buf,
                                  scope_block.as_mut());
                }
            } else {
                let item = trim.trim_end_matches(',').trim().to_string();
                if !item.is_empty() { lb.items.push(item); }
            }
            continue;
        }

        // ── BLOK: mapa ─────────────────────────────────────────
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
                    dispatch_node(&mut result, &scopes, node,
                                  &mut in_do_block, &mut do_buf,
                                  &mut in_test_block, &mut test_buf,
                                  scope_block.as_mut());
                }
            } else if let Some((k, v)) = parse_map_line(trim) {
                mb.lines.push(format!("{}: {}", k, v));
            }
            continue;
        }

        // ── BLOK: ==type ───────────────────────────────────────
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

        // ── BLOK: ==interface ──────────────────────────────────
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

        // ── do...done ──────────────────────────────────────────
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
                if let Some((key, r, g)) = is_list_block_start(ps) {
                    list_block = Some(ListBlockState { key, is_raw: r, is_global: g, items: Vec::new(), start_line: idx+1, start_off: off });
                } else if let Some((key, r, g)) = is_map_block_start(ps) {
                    map_block = Some(MapBlockState { key, is_raw: r, is_global: g, lines: Vec::new(), start_line: idx+1, start_off: off });
                } else if let Some(prefix) = is_call_list_block_start(ps) {
                    call_list_block = Some(CallListBlockState { prefix, items: Vec::new(), start_line: idx+1, start_off: off });
                } else if let Some(op) = try_parse_line(ps) {
                    if let Some(node) = build_node(idx+1, sudo_i, off, ps, op) { do_buf.push(node); }
                }
            }
            continue;
        }

        // ── ==test ─────────────────────────────────────────────
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

        // ── raw block ──────────────────────────────────────────
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

        let (parse_src_raw, is_sudo) = strip_sudo(trim);
        if is_sudo {
            result.is_potentially_unsafe = true;
            result.safety_warnings.push(format!("Linia {}: sudo (^)", idx + 1));
        }

        // ── Widoczność pub przed następną definicją ────────────
        let (parse_src, vis_override) = strip_pub(parse_src_raw);
        if vis_override == Visibility::Public {
            next_vis = Visibility::Public;
            // jeśli to był standalone "pub" — poczekaj na następną linię
            if parse_src.is_empty() {
                continue;
            }
        }

        let line_num = idx + 1;
        let span     = SourceSpan::new(off.into(), parse_src.len().into());

        // ── NOWE: =; moduł ────────────────────────────────────
        if parse_src.starts_with("=;") || parse_src_raw.starts_with("pub =;") {
            if is_module_done(parse_src) || is_module_done(parse_src_raw) {
                if let Some(mb) = module_stack.pop() {
                    let node = ProgramNode {
                        line_num, is_sudo: false,
                        content:  CommandType::ModuleDone,
                        original_text: "=; done".to_string(),
                        span: (off, 0),
                    };
                    push_node(&mut result, &scopes, node);
                    scopes.pop();
                    // usuń z modułu
                    result.modules.entry(mb.name.clone())
                    .or_insert_with(|| ModuleMeta { name: mb.name.clone(), ..Default::default() });
                } else {
                    push_err(&mut errors, path, &src, span, line_num, parse_src);
                }
                next_vis = Visibility::Private;
                continue;
            }
            // Próbuj zarówno z pub jak i bez
            let try_src = if parse_src_raw.starts_with("pub ") { parse_src_raw } else { parse_src };
            if let Some((name, vis)) = is_module_def(try_src) {
                let path_now: Vec<String> = module_stack.iter().map(|m| m.name.clone()).collect();
                module_stack.push(ModuleBlockState {
                    name: name.clone(),
                                  visibility: vis.clone(),
                                  start_line: line_num,
                                  path: path_now.clone(),
                });
                let full_name = {
                    let mut p = path_now.clone();
                    p.push(name.clone());
                    p.join("::")
                };
                scopes.push(Scope::Class(full_name.clone()));
                result.modules.insert(full_name.clone(), ModuleMeta {
                    name: full_name.clone(),
                                      visibility: vis.clone(),
                                      submodules: Vec::new(),
                                      functions: Vec::new(),
                });
                // dodaj jako submodule do rodzica
                if let Some(parent) = module_stack.iter().rev().nth(1) {
                    let parent_name: Vec<String> = {
                        let mut p = parent.path.clone();
                        p.push(parent.name.clone());
                        p
                    };
                    let pk = parent_name.join("::");
                    result.modules.entry(pk).or_default().submodules.push(full_name.clone());
                }
                let node = ProgramNode {
                    line_num, is_sudo: false,
                    content:  CommandType::ModuleDef { name: name.clone(), visibility: vis },
                    original_text: try_src.to_string(),
                    span: (off, 0),
                };
                push_node(&mut result, &scopes, node);
                next_vis = Visibility::Private;
                continue;
            }
        }

        // ── NOWE: =: scope block ───────────────────────────────
        if is_scope_block_toggle(parse_src) {
            if scope_block.is_none() {
                scope_block = Some(ScopeBlockState { body: Vec::new(), start_line: line_num, start_off: off });
            } else {
                let sb = scope_block.take().unwrap();
                let node = ProgramNode {
                    line_num: sb.start_line, is_sudo: false,
                    content:  CommandType::ScopeBlock { body: sb.body },
                    original_text: "=: ... =:".to_string(),
                    span: (sb.start_off, 0),
                };
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ── Wewnątrz scope block ───────────────────────────────
        if let Some(ref mut sb) = scope_block {
            let (ps, sudo_i) = strip_sudo(trim);
            if let Some(op) = try_parse_line(ps) {
                if let Some(node) = build_node(line_num, sudo_i, off, ps, op) {
                    sb.body.push(node);
                }
            }
            continue;
        }

        // ── NOWE: atrybut |] ──────────────────────────────────
        if parse_src.starts_with("|]") {
            if let Some(attr) = is_attr_decl(parse_src) {
                pending_attrs.push(attr.clone());
                let node = ProgramNode {
                    line_num, is_sudo,
                    content:  CommandType::FuncAttrDecl { attr },
                    original_text: parse_src.to_string(),
                    span: (off, parse_src.len()),
                };
                push_node(&mut result, &scopes, node);
            } else {
                push_err(&mut errors, path, &src, span, line_num, parse_src);
            }
            continue;
        }

        // ── NOWE: kompilacja warunkowa *{} ────────────────────
        if parse_src.starts_with("*{") {
            if let Some(op) = is_cond_comp(parse_src) {
                let cmd = match &op {
                    LineOp::CondComp(f)  => CommandType::CondComp { flag: f.clone() },
                    LineOp::CondCompEnd  => CommandType::CondCompEnd,
                    _ => unreachable!(),
                };
                let node = ProgramNode {
                    line_num, is_sudo,
                    content: cmd,
                    original_text: parse_src.to_string(),
                    span: (off, parse_src.len()),
                };
                push_node(&mut result, &scopes, node);
            } else {
                push_err(&mut errors, path, &src, span, line_num, parse_src);
            }
            continue;
        }

        // ── NOWE: fat-arrow match (match $x =>) ───────────────
        if parse_src.starts_with("match ") && parse_src.ends_with("=>") {
            if let Some(cond) = is_match_fat(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::MatchFat(cond)));
                continue;
            }
        }

        // ── NOWE: fat-arrow match arm ─────────────────────────
        if parse_src.contains(" => ") {
            if let Some(op) = is_match_arm_fat(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, op));
                continue;
            }
        }

        // ── NOWE: typed assign x = 5 [i32] ───────────────────
        if let Some((k, e, t, r, g)) = is_typed_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignTyped(k, e, t, r, g)));
            continue;
        }

        // ── Pre-pest (istniejące) ──────────────────────────────
        // NOWE: # <lib> use [...] — obsługa pre-pest zanim trafi do pest
        // (globalny WHITESPACE skipper w pest może kolidować z explicit " "?)
        if parse_src.starts_with("# <") {
            if let Some(lr) = parse_lib_use_pre_pest(parse_src) {
                handle_lib(lr.clone(), path, &src, span, resolve_libs, verbose, seen_libs, &mut result, &mut errors);
                continue;
            }
        }
        if let Some((n, s)) = is_arena_def(parse_src) {
            let sig = Some(format!("[arena:{}]", s));
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::ArenaDef(n.clone(), s)));
            register_function(n, sig, true, &mut scopes, &mut result, &mut pending_attrs, &mut next_vis, &module_stack);
            continue;
        }
        if let Some((v, m, a)) = is_collection_mut(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::CollectionMut(v, m, a)));
            continue;
        }
        if parse_src.contains(" ?! ") && !parse_src.contains('=') {
            if let Some((e, msg)) = is_result_unwrap(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::ResultUnwrap(e, msg)));
                continue;
            }
        }
        if parse_src.starts_with("==interface ") {
            if let Some((n, ms)) = is_interface_oneline(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::InterfaceDef(n, ms)));
                continue;
            }
            if let Some(n) = is_interface_block_start(parse_src) {
                iface_block = Some(InterfaceBlockState { name: n, methods: Vec::new(), start_line: line_num, start_off: off });
                continue;
            }
            push_err(&mut errors, path, &src, span, line_num, parse_src);
            continue;
        }
        if parse_src.starts_with("==type ") {
            if let Some((n, vs)) = is_adt_oneline(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AdtDef(n, vs)));
                continue;
            }
            if let Some(n) = is_adt_block_start(parse_src) {
                adt_block = Some(AdtBlockState { name: n, body_lines: Vec::new(), start_line: line_num, start_off: off });
                continue;
            }
            push_err(&mut errors, path, &src, span, line_num, parse_src);
            continue;
        }
        if parse_src.starts_with(";;") && parse_src.contains(" impl ") {
            if let Some((c, i)) = is_impl_def(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::ImplDef(c.clone(), i)));
                scopes.push(Scope::Class(c));
                continue;
            }
        }
        if parse_src.starts_with("==test ") {
            if let Some(desc) = is_test_def(parse_src) {
                in_test_block = true; test_desc = desc; test_buf.clear();
                continue;
            }
        }
        if is_scope_def(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::ScopeDef));
            scopes.push(Scope::Class(format!("__scope_{}", line_num)));
            continue;
        }
        if let Some(e) = is_defer(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::Defer(e)));
            continue;
        }
        if let Some(a) = is_recur(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::Recur(a)));
            continue;
        }
        if let Some(s) = is_pipe_line(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::PipeLine(s)));
            continue;
        }
        if parse_src.starts_with("match ") {
            if let Some(cond) = is_match_stmt(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::Match(cond)));
                continue;
            }
        }
        if parse_src.starts_with('[') && parse_src.contains('|') && parse_src.contains('=') {
            if let Some((h, t, s)) = is_destruct_list(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::DestructList(h, t, s)));
                continue;
            }
        }
        if parse_src.starts_with('{') && !parse_src.contains(':') && parse_src.contains('=') {
            if let Some((f, s)) = is_destruct_map(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::DestructMap(f, s)));
                continue;
            }
        }
        if let Some((k, v)) = is_percent(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::Percent(k, v)));
            continue;
        }
        if let Some((k, r)) = is_spawn_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignSpawn(k, r)));
            continue;
        }
        if let Some((k, r)) = is_await_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignAwait(k, r)));
            continue;
        }
        if let Some(key) = is_do_assign(parse_src) {
            in_do_block = true; do_key = key; do_buf = Vec::new(); do_start_line = line_num;
            continue;
        }
        if let Some((k, p, b, r, g)) = is_lambda_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignLambda(k, p, b, r, g)));
            continue;
        }
        if let Some((key, r, g)) = is_map_block_start(parse_src) {
            map_block = Some(MapBlockState { key, is_raw: r, is_global: g, lines: Vec::new(), start_line: line_num, start_off: off });
            continue;
        }
        if let Some((key, r, g)) = is_list_block_start(parse_src) {
            list_block = Some(ListBlockState { key, is_raw: r, is_global: g, items: Vec::new(), start_line: line_num, start_off: off });
            continue;
        }
        if let Some(prefix) = is_call_list_block_start(parse_src) {
            call_list_block = Some(CallListBlockState { prefix, items: Vec::new(), start_line: line_num, start_off: off });
            continue;
        }
        if let Some((k, e, r, g)) = is_expr_assign(parse_src) {
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::AssignExpr(k, e, r, g)));
            continue;
        }
        if parse_src.starts_with('@') {
            if let Some((k, v)) = is_global_dollar_assign(parse_src) {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, LineOp::GlobalVar(k, v)));
                continue;
            }
        }
        if let Some((k, v, r, g)) = is_assignment(parse_src) {
            let op = if g { LineOp::GlobalVar(k, v) } else { LineOp::LocalVar(k, v, r) };
            emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src, op));
            continue;
        }
        if parse_src.contains('{') && parse_src.contains(" -> ") && !parse_src.contains('}') {
            if let Some((pfx, after)) = is_multiline_lambda_start(parse_src) {
                lambda_block = Some(LambdaBlockState { prefix: pfx, collected: vec![after], start_line: line_num, start_off: off });
                continue;
            }
        }

        // ── Pre-pest: standalone module call bez przypisania ──
        // np. sqlite3.exec "DELETE ...", redis.set $k $v
        // Musi być PRZED pest bo pest nie rozpoznaje tego jako module_call
        // gdy nie ma przypisania i zawiera znaki które mylą gramatykę
        if let Some((path_mc, args_mc)) = is_module_call_standalone(parse_src) {
            // dodatkowy filtr: nie obsługuj jeśli pest sam da radę
            // (pest module_call wymaga ident.ident+ — sprawdź czy jest kropka)
            if parse_src.contains('.') && !parse_src.starts_with('.') {
                emit(&mut result, &scopes, build_node(line_num, is_sudo, off, parse_src,
                                                      LineOp::ModuleCall(path_mc, args_mc)));
                continue;
            }
        }

        // ── Pest ───────────────────────────────────────────────
        let op = match HlParser::parse(Rule::line, parse_src) {
            Ok(mut pairs) => match line_to_op(pairs.next().unwrap()) {
                Some(op) => op,
                None     => { push_err(&mut errors, path, &src, span, line_num, parse_src); continue; },
            },
            Err(_) => { push_err(&mut errors, path, &src, span, line_num, parse_src); continue; },
        };

        // ── Obsługa scope/func — rozbudowana o moduły i atrybuty ──
        match op {
            LineOp::ClassDef(name) => {
                scopes.push(Scope::Class(name));
            },
            LineOp::ScopeDef => {
                scopes.push(Scope::Class(format!("__scope_{}", line_num)));
            },
            LineOp::FuncDef(fname, fsig) => {
                register_function(
                    fname, fsig, false,
                    &mut scopes, &mut result,
                    &mut pending_attrs, &mut next_vis,
                    &module_stack,
                );
            },
            LineOp::FuncDefGeneric(fname, sig_str) => {
                register_function(
                    fname, Some(sig_str), false,
                                  &mut scopes, &mut result,
                                  &mut pending_attrs, &mut next_vis,
                                  &module_stack,
                );
            },
            LineOp::FuncDone => { scopes.pop(); },
            LineOp::RawBlockStart => {
                in_raw_block = true; raw_sudo = is_sudo;
                raw_start_line = line_num; raw_start_off = off;
            },
            LineOp::RawBlockEnd => {
                errors.push(ParseError::SyntaxError {
                    src: NamedSource::new(path, src.clone()), span, line_num,
                            advice: "Nieoczekiwany ']' bez pasującego '['".to_string(),
                });
            },
            LineOp::CloseBrace => {},
            LineOp::SysDep(dep) => result.deps.push(dep),
            LineOp::Lib(lr) => handle_lib(lr, path, &src, span, resolve_libs, verbose, seen_libs, &mut result, &mut errors),
            LineOp::ModuleDef(name, vis) => {
                let path_now: Vec<String> = module_stack.iter().map(|m| m.name.clone()).collect();
                module_stack.push(ModuleBlockState {
                    name: name.clone(), visibility: vis.clone(),
                                  start_line: line_num, path: path_now.clone(),
                });
                let mut full_parts = path_now.clone();
                full_parts.push(name.clone());
                let full_name = full_parts.join("::");
                scopes.push(Scope::Class(full_name.clone()));
                result.modules.insert(full_name.clone(), ModuleMeta {
                    name: full_name, visibility: vis,
                    submodules: Vec::new(), functions: Vec::new(),
                });
            },
            LineOp::ModuleDone => {
                module_stack.pop();
                scopes.pop();
            },
            LineOp::AttrDecl(attr) => {
                pending_attrs.push(attr.clone());
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, LineOp::AttrDecl(attr)) {
                    push_node(&mut result, &scopes, node);
                }
            },
            LineOp::CondComp(f) => {
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, LineOp::CondComp(f)) {
                    push_node(&mut result, &scopes, node);
                }
            },
            LineOp::CondCompEnd => {
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, LineOp::CondCompEnd) {
                    push_node(&mut result, &scopes, node);
                }
            },
            other => {
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, other) {
                    push_node(&mut result, &scopes, node);
                }
            },
        }
    }

    if errors.is_empty() { Ok(result) } else { Err(errors) }
}

// ─────────────────────────────────────────────────────────────
// dispatch_node — routing węzła do odpowiedniego bufora
// ─────────────────────────────────────────────────────────────
fn dispatch_node(
    result:        &mut AnalysisResult,
    scopes:        &[Scope],
    node:          ProgramNode,
    in_do:         &mut bool,
    do_buf:        &mut Vec<ProgramNode>,
    in_test:       &mut bool,
    test_buf:      &mut Vec<ProgramNode>,
    scope_block:   Option<&mut ScopeBlockState>,
) {
    if *in_do        { do_buf.push(node); return; }
    if *in_test      { test_buf.push(node); return; }
    if let Some(sb) = scope_block { sb.body.push(node); return; }
    push_node(result, scopes, node);
}

// ─────────────────────────────────────────────────────────────
// try_parse_line — szybkie pre-pest parsowanie
// ─────────────────────────────────────────────────────────────
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
    if parse_src.starts_with("match ") && parse_src.ends_with("=>") {
        if let Some(cond) = is_match_fat(parse_src) { return Some(LineOp::MatchFat(cond)); }
    }
    if parse_src.contains(" => ") {
        if let Some(op) = is_match_arm_fat(parse_src) { return Some(op); }
    }
    if parse_src.starts_with("match ") {
        if let Some(cond) = is_match_stmt(parse_src) { return Some(LineOp::Match(cond)); }
    }
    if let Some((k, e, t, r, g)) = is_typed_assign(parse_src) { return Some(LineOp::AssignTyped(k, e, t, r, g)); }
    if let Some((k, v))          = is_percent(parse_src)       { return Some(LineOp::Percent(k, v)); }
    if let Some((k, r))          = is_spawn_assign(parse_src)  { return Some(LineOp::AssignSpawn(k, r)); }
    if let Some((k, r))          = is_await_assign(parse_src)  { return Some(LineOp::AssignAwait(k, r)); }
    if let Some((k, p, b, r, g)) = is_lambda_assign(parse_src) { return Some(LineOp::AssignLambda(k, p, b, r, g)); }
    if let Some((k, e, r, g))    = is_expr_assign(parse_src)   { return Some(LineOp::AssignExpr(k, e, r, g)); }
    if parse_src.starts_with('@') {
        if let Some((k, v)) = is_global_dollar_assign(parse_src) { return Some(LineOp::GlobalVar(k, v)); }
    }
    if let Some((k, v, r, g)) = is_assignment(parse_src) {
        return Some(if g { LineOp::GlobalVar(k, v) } else { LineOp::LocalVar(k, v, r) });
    }
    if let Some((val, cmd)) = is_match_arm(parse_src) { return Some(LineOp::MatchArm(val, cmd)); }
    if let Some(attr) = is_attr_decl(parse_src) { return Some(LineOp::AttrDecl(attr)); }
    if let Some(op) = is_cond_comp(parse_src) { return Some(op); }
    // standalone module call (bez przypisania): ident.metoda [args]
    if parse_src.contains('.') && !parse_src.starts_with('.') && !parse_src.starts_with('$') {
        if let Some((p, a)) = is_module_call_standalone(parse_src) {
            return Some(LineOp::ModuleCall(p, a));
        }
    }
    if let Ok(mut pairs) = HlParser::parse(Rule::line, parse_src) {
        if let Some(pair) = pairs.next() { return line_to_op(pair); }
    }
    None
}

// ─────────────────────────────────────────────────────────────
// qualified — kwalifikowana nazwa funkcji/klasy
// ─────────────────────────────────────────────────────────────
fn qualified(scopes: &[Scope], name: &str) -> String {
    for s in scopes.iter().rev() {
        if let Scope::Class(cls) = s { return format!("{}.{}", cls, name); }
    }
    name.to_string()
}

// ─────────────────────────────────────────────────────────────
// push_node — routing węzła do funkcji lub main_body
// ─────────────────────────────────────────────────────────────
pub fn push_node(result: &mut AnalysisResult, scopes: &[Scope], node: ProgramNode) {
    for scope in scopes.iter().rev() {
        if let Scope::Func(name) = scope {
            if let Some(f) = result.functions.get_mut(name) { f.body.push(node); return; }
        }
    }
    result.main_body.push(node);
}

// ─────────────────────────────────────────────────────────────
// build_node — LineOp → ProgramNode
// ─────────────────────────────────────────────────────────────
fn build_node(line_num: usize, is_sudo: bool, off: usize, src: &str, op: LineOp) -> Option<ProgramNode> {
    let cmd = match op {
        LineOp::SepCmd(c)               => CommandType::Isolated(c),
        LineOp::RawCmd(c)               => CommandType::RawNoSub(c),
        LineOp::ExplCmd(c)              => CommandType::RawSub(c),
        LineOp::GlobalVar(k, v)         => CommandType::AssignEnv   { key: k, val: v },
        LineOp::LocalVar(k, v, r)       => CommandType::AssignLocal { key: k, val: v, is_raw: r },
        LineOp::Loop(n, c)              => CommandType::Loop        { count: n, cmd: c },
        LineOp::If(co, c)               => CommandType::If          { cond: co, cmd: c },
        LineOp::Elif(co, c)             => CommandType::Elif        { cond: co, cmd: c },
        LineOp::Else(c)                 => CommandType::Else        { cmd: c },
        LineOp::While(co, c)            => CommandType::While       { cond: co, cmd: c },
        LineOp::For(v, i, c)            => CommandType::For         { var: v, in_: i, cmd: c },
        LineOp::Bg(c)                   => CommandType::Background(c),
        LineOp::Call(p, a)              => CommandType::Call        { path: p, args: a },
        LineOp::Plugin(n, a)            => CommandType::Plugin      { name: n, args: a, is_super: is_sudo },
        LineOp::Log(m)                  => CommandType::Log(m),
        LineOp::Lock(k, v)              => CommandType::Lock        { key: k, val: v },
        LineOp::Unlock(k)               => CommandType::Unlock      { key: k },
        LineOp::Extern(p, sl, u)        => CommandType::Extern      { path: p, static_link: sl, use_symbols: u },
        LineOp::Import(r, ns)           => CommandType::Import      { resource: r, namespace: ns },
        LineOp::Enum(n, vars)           => CommandType::Enum        { name: n, variants: vars },
        LineOp::Struct(n, flds)         => CommandType::Struct      { name: n, fields: flds },
        LineOp::Try(t, c)               => CommandType::Try         { try_cmd: t, catch_cmd: c },
        LineOp::End(code)               => CommandType::End         { code },
        LineOp::Out(v)                  => CommandType::Out(v),
        LineOp::Percent(k, v)           => CommandType::Const       { key: k, val: v },
        LineOp::Spawn(r)                => CommandType::Spawn(r),
        LineOp::Await(r)                => CommandType::Await(r),
        LineOp::AssignSpawn(k, r)       => CommandType::AssignSpawn { key: k, task: r },
        LineOp::AssignAwait(k, r)       => CommandType::AssignAwait { key: k, expr: r },
        LineOp::Assert(c, m)            => CommandType::Assert      { cond: c, msg: m },
        LineOp::Match(c)                => CommandType::Match       { cond: c },
        LineOp::MatchArm(v, c)          => CommandType::MatchArm    { val: v, cmd: c },
        LineOp::Pipe(steps)             => CommandType::Pipe(steps),
        LineOp::AssignExpr(k, e, r, g)  => if g {
            CommandType::AssignEnv  { key: k, val: e }
        } else {
            CommandType::AssignExpr { key: k, expr: e, is_raw: r, is_global: false }
        },
        LineOp::AssignList(k, items, r, g) => {
            let expr = format!("[{}]", items.join(", "));
            if g { CommandType::AssignEnv { key: k, val: expr } }
            else { CommandType::AssignExpr { key: k, expr, is_raw: r, is_global: false } }
        },
        LineOp::CollectionMut(v, m, a)        => CommandType::CollectionMut { var: v, method: m, args: a },
        LineOp::InterfaceDef(n, ms)            => CommandType::Interface     { name: n, methods: ms },
        LineOp::ImplDef(c, i)                  => CommandType::ImplDef       { class: c, interface: i },
        LineOp::ArenaDef(n, s)                 => CommandType::ArenaDef      { name: n, size: s },
        LineOp::ResultUnwrap(e, m)             => CommandType::ResultUnwrap  { expr: e, msg: m },
        LineOp::ModuleCall(p, a)               => CommandType::ModuleCall    { path: p, args: a },
        LineOp::Lambda(params, body)           => CommandType::Lambda        { params, body },
        LineOp::AssignLambda(k, p, b, r, g)    => CommandType::AssignLambda  { key: k, params: p, body: b, is_raw: r, is_global: g },
        LineOp::Recur(args)                    => CommandType::Recur         { args },
        LineOp::DestructList(h, t, s)          => CommandType::DestructList  { head: h, tail: t, source: s },
        LineOp::DestructMap(flds, s)           => CommandType::DestructMap   { fields: flds, source: s },
        LineOp::ScopeDef                       => CommandType::ScopeDef,
        LineOp::AdtDef(n, vs)                  => CommandType::AdtDef        { name: n, variants: vs },
        LineOp::DoBlock                        => return None,
        LineOp::AssignDo(k)                    => CommandType::DoBlock       { key: k, body: Vec::new() },
        LineOp::PipeLine(step)                 => CommandType::PipeLine      { step },
        LineOp::TestDef(desc)                  => CommandType::TestBlock     { desc, body: Vec::new() },
        LineOp::Defer(expr)                    => CommandType::Defer         { expr },
        LineOp::FuncDefGeneric(n, s)           => CommandType::FuncDefGeneric { name: n, sig: s },
        LineOp::CloseBrace                     => return None,
        // ── NOWE ──────────────────────────────────────────────
        LineOp::ModuleDef(name, vis)           => CommandType::ModuleDef    { name, visibility: vis },
        LineOp::ModuleDone                     => CommandType::ModuleDone,
        LineOp::ScopeBlockToggle               => return None, // obsługiwane przez stan
        LineOp::PubPrefix                      => return None,
        LineOp::AssignTyped(k, e, t, r, g)     => CommandType::AssignTyped  { key: k, expr: e, type_ann: t, is_raw: r, is_global: g },
        LineOp::MatchFat(c)                    => CommandType::MatchFat     { cond: c },
        LineOp::MatchArmFat(v, f, c)           => CommandType::MatchArmFat  { variant: v, fields: f, cmd: c },
        LineOp::MatchArmDestructFat(f, c)      => CommandType::MatchArmDestructFat { fields: f, cmd: c },
        LineOp::CondComp(f)                    => CommandType::CondComp     { flag: f },
        LineOp::CondCompEnd                    => CommandType::CondCompEnd,
        LineOp::AttrDecl(a)                    => CommandType::FuncAttrDecl  { attr: a },
        // FuncDef* obsługiwane wyżej
        LineOp::FuncDef(_, _)                  => return None,
        LineOp::FuncDone                       => return None,
        LineOp::ClassDef(_)                    => return None,
        LineOp::SysDep(_)                      => return None,
        LineOp::Lib(_)                         => return None,
        LineOp::RawBlockStart                  => return None,
        LineOp::RawBlockEnd                    => return None,
    };
    Some(ProgramNode { line_num, is_sudo, content: cmd, original_text: src.to_string(), span: (off, src.len()) })
}
