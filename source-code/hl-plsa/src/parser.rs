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
    // ── ISTNIEJĄCE — BEZ ZMIAN ───────────────────────────────────
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
    LocalVar(String, String, bool),   // key, val, is_raw
    Plugin(String, String),           // name, args
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
    Extern(String, bool),             // path, is_static
    Enum(String, Vec<String>),
    Import(String, Option<String>),
    Try(String, String),
    Struct(String, Vec<(String, String)>),
    RawBlockStart,
    RawBlockEnd,
    End(i32),
    Out(String),
    // ── NOWE (spawn/await/assert/match/pipe/const) ────────────────
    Percent(String, String),
    Spawn(String),
    Await(String),
    Assert(String, Option<String>),
    Match(String),
    MatchArm(String, String),
    Pipe(Vec<String>),
    AssignSpawn(String, String),
    AssignAwait(String, String),
    // ── NOWE: wyrażenia ──────────────────────────────────────────
    AssignExpr(String, String, bool, bool),  // key, expr, is_raw, is_global
    // ── NOWE: kolekcje ───────────────────────────────────────────
    CollectionMut(String, String, String),   // var, method, args
    // ── NOWE: interfejsy / impl ───────────────────────────────────
    InterfaceDef(String, Vec<String>),       // name, methods
    ImplDef(String, String),                 // class, interface
    // ── NOWE: arena allocator ─────────────────────────────────────
    ArenaDef(String, String),                // name, size
    // ── NOWE: result unwrap ───────────────────────────────────────
    ResultUnwrap(String, String),            // expr, msg
    // ── NOWE: wywołanie metody modułu ────────────────────────────
    ModuleCall(String, String),              // path, args
    // ── NOWE: domknięcia / lambdy ─────────────────────────────────
    Lambda(Vec<String>, String),             // params, body
    AssignLambda(String, Vec<String>, String, bool, bool), // key, params, body, is_raw, is_global
    // ── NOWE: rekurencja ogonowa ──────────────────────────────────
    Recur(String),                           // args
    // ── NOWE: destrukturyzacja ────────────────────────────────────
    DestructList(String, String, String),    // head, tail, source
    DestructMap(Vec<String>, String),        // fields, source
    // ── NOWE: zasięg leksykalny ──────────────────────────────────
    ScopeDef,
    // ── NOWE: typy algebraiczne (ADT) ────────────────────────────
    AdtDef(String, Vec<(String, Vec<(String, String)>)>), // name, [(variant, [(field, type)])]
    // ── NOWE: do-notacja ─────────────────────────────────────────
    DoBlock,
    AssignDo(String),                        // key — wartość wypełniana przez blok do...done
    PipeLine(String),                        // krok potoku (| .fetch $url)
    // ── NOWE: testy jednostkowe ──────────────────────────────────
    TestDef(String),                         // opis testu
    // ── NOWE: defer ──────────────────────────────────────────────
    Defer(String),                           // wyrażenie do wykonania przy wyjściu ze scope
    // ── NOWE: generics z constraints ─────────────────────────────
    FuncDefGeneric(String, String),          // name, generic_sig np. "[T impl Serializable -> str]"
}

#[derive(Debug, Clone)]
enum Scope { Class(String), Func(String) }

fn libs_root() -> PathBuf {
    dirs::home_dir().expect("HOME not set")
    .join(".hackeros/hacker-lang/libs")
}
pub fn plugins_root() -> PathBuf {
    dirs::home_dir().expect("HOME not set")
    .join(".hackeros/hacker-lang/plugins")
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

// ─────────────────────────────────────────────────────────────
// is_assignment — wykrywa przypisanie PRZED pest
// ─────────────────────────────────────────────────────────────
fn is_assignment(line: &str) -> Option<(String, String, bool, bool)> {
    let mut s = line;
    let mut is_global = false;
    let mut is_raw    = false;

    if let Some(rest) = s.strip_prefix('@') {
        is_global = true;
        s = rest;
    } else if let Some(rest) = s.strip_prefix('~') {
        is_raw = true;
        s = rest;
    }

    let first = s.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }

    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8())
    .sum::<usize>();
    if ident_len == 0 { return None; }

    let key  = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start_matches(|c| c == ' ' || c == '\t');

    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') { return None; }

    let value = rest.trim_start_matches(|c| c == ' ' || c == '\t').to_string();

    Some((key, value, is_raw, is_global))
}

// ─────────────────────────────────────────────────────────────
// is_percent — wykrywa stałą % PRZED pest
// ─────────────────────────────────────────────────────────────
fn is_percent(line: &str) -> Option<(String, String)> {
    let s = line.strip_prefix('%')?;
    let first = s.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' { return None; }
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8())
    .sum::<usize>();
    if ident_len == 0 { return None; }
    let key  = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start_matches(|c| c == ' ' || c == '\t');
    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') { return None; }
    let value = rest.trim_start_matches(|c| c == ' ' || c == '\t').to_string();
    Some((key, value))
}

// ─────────────────────────────────────────────────────────────
// is_spawn_assign — `key = spawn rest`
// ─────────────────────────────────────────────────────────────
fn is_spawn_assign(line: &str) -> Option<(String, String)> {
    let (key, val, _, _) = is_assignment(line)?;
    let rest = val.strip_prefix("spawn")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric()) { return None; }
    Some((key, rest.trim_start().to_string()))
}

// ─────────────────────────────────────────────────────────────
// is_await_assign — `key = await rest`
// ─────────────────────────────────────────────────────────────
fn is_await_assign(line: &str) -> Option<(String, String)> {
    let (key, val, _, _) = is_assignment(line)?;
    let rest = val.strip_prefix("await")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric()) { return None; }
    Some((key, rest.trim_start().to_string()))
}

// ─────────────────────────────────────────────────────────────
// is_do_assign — `key = do`
// ─────────────────────────────────────────────────────────────
fn is_do_assign(line: &str) -> Option<String> {
    let (key, val, _, _) = is_assignment(line)?;
    if val.trim() == "do" { Some(key) } else { None }
}

// ─────────────────────────────────────────────────────────────
// is_lambda_assign — `key = { $x -> expr }`
// ─────────────────────────────────────────────────────────────
fn is_lambda_assign(line: &str) -> Option<(String, Vec<String>, String, bool, bool)> {
    let (key, val, is_raw, is_global) = is_assignment(line)?;
    let (params, body) = parse_lambda_literal(val.trim())?;
    Some((key, params, body, is_raw, is_global))
}

// ─────────────────────────────────────────────────────────────
// parse_lambda_literal — `{ $x -> body }` lub `{ $x, $y -> body }`
// ─────────────────────────────────────────────────────────────
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

// ─────────────────────────────────────────────────────────────
// is_expr_assign — `key = expr` gdzie expr zawiera operator
// lub jest listą [...] / mapą {...} / dostępem $x.y
//
// Odróżnia "zwykłe" przypisanie (key = $val lub key = "str")
// od wyrażenia (key = 2 + 3  |  x = [$a, $b]  |  m = {k: 1}).
//
// Zwraca Some((key, expr, is_raw, is_global)) jeśli value
// wygląda jak wyrażenie złożone.
// ─────────────────────────────────────────────────────────────
fn is_expr_assign(line: &str) -> Option<(String, String, bool, bool)> {
    let (key, val, is_raw, is_global) = is_assignment(line)?;

    // Nie parsuj lambd tutaj — obsługiwane przez is_lambda_assign
    if val.trim().starts_with('{') && val.contains(" -> ") {
        return None;
    }

    // Wyrażenie jeśli:
    // 1. zaczyna się od '[' (lista)
    // 2. zaczyna się od '{' (mapa)
    // 3. zawiera operator arytmetyczny/logiczny otoczony spacjami
    // 4. zawiera interpolację wyrażeń $(...)
    let is_list = val.starts_with('[');
    let is_map  = val.starts_with('{');
    let has_op  = val.contains(" + ")  || val.contains(" - ")  ||
    val.contains(" * ")  || val.contains(" / ")  ||
    val.contains(" % ")  || val.contains(" == ") ||
    val.contains(" != ") || val.contains(" >= ") ||
    val.contains(" <= ") || val.contains(" > ")  ||
    val.contains(" < ")  || val.contains(" && ") ||
    val.contains(" || ") || val.contains(" ?! ");
    let has_interp = val.contains("$(");

    if is_list || is_map || has_op || has_interp {
        Some((key, val, is_raw, is_global))
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────
// is_collection_mut — `$var.method args`
// $list.push 42   $map.set "key" "val"   $list.pop
// ─────────────────────────────────────────────────────────────
fn is_collection_mut(line: &str) -> Option<(String, String, String)> {
    let s = line.strip_prefix('$')?;
    let dot_pos = s.find('.')?;
    let var = s[..dot_pos].to_string();
    let rest = &s[dot_pos + 1..];
    let methods = ["push", "pop", "set", "del", "get"];
    for method in &methods {
        if let Some(after) = rest.strip_prefix(method) {
            if after.is_empty() || after.starts_with(' ') || after.starts_with('\t') {
                let args = after.trim().to_string();
                return Some((var, method.to_string(), args));
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────
// is_result_unwrap — `expr ?! "msg"` (jako standalone stmt)
// Używane gdy wynik jest już przypisany przez is_expr_assign
// ale też gdy standalone: .fetch $url ?! "błąd"
// ─────────────────────────────────────────────────────────────
fn is_result_unwrap(line: &str) -> Option<(String, String)> {
    let pos = line.find(" ?! ")?;
    let expr = line[..pos].trim().to_string();
    let after = line[pos + 4..].trim();
    let msg = after.trim_matches('"').to_string();
    Some((expr, msg))
}

// ─────────────────────────────────────────────────────────────
// is_arena_def — `:: name [size] def`
// :: cache [512kb] def
// ─────────────────────────────────────────────────────────────
fn is_arena_def(line: &str) -> Option<(String, String)> {
    let s = line.strip_prefix("::")?;
    let s = s.trim_start();
    // zbierz ident
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8())
    .sum::<usize>();
    if ident_len == 0 { return None; }
    let name = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start();
    // musi być [size] def
    let rest = rest.strip_prefix('[')?;
    let end  = rest.find(']')?;
    let size = rest[..end].trim().to_string();
    let after = rest[end + 1..].trim();
    if after != "def" { return None; }
    Some((name, size))
}

// ─────────────────────────────────────────────────────────────
// is_recur — `recur args`
// ─────────────────────────────────────────────────────────────
fn is_recur(line: &str) -> Option<String> {
    let rest = line.strip_prefix("recur")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(rest.trim().to_string())
}

// ─────────────────────────────────────────────────────────────
// is_destruct_list — `[head | tail] = $source`
// ─────────────────────────────────────────────────────────────
fn is_destruct_list(line: &str) -> Option<(String, String, String)> {
    let s = line.trim();
    if !s.starts_with('[') { return None; }
    let end = s.find(']')?;
    let inside = &s[1..end];
    let pipe_pos = inside.find('|')?;
    let head = inside[..pipe_pos].trim().to_string();
    let tail = inside[pipe_pos + 1..].trim().to_string();
    if head.is_empty() || tail.is_empty() { return None; }
    let after = s[end + 1..].trim();
    let after = after.strip_prefix('=')?;
    if after.starts_with('=') { return None; }
    let source = after.trim().to_string();
    Some((head, tail, source))
}

// ─────────────────────────────────────────────────────────────
// is_destruct_map — `{name, age} = $source`
// ─────────────────────────────────────────────────────────────
fn is_destruct_map(line: &str) -> Option<(Vec<String>, String)> {
    let s = line.trim();
    if !s.starts_with('{') { return None; }
    let end = s.find('}')?;
    let inside = &s[1..end];
    // Sprawdź czy to destrukturyzacja (bez ':') a nie mapa
    if inside.contains(':') { return None; }
    // Sprawdź czy to destrukturyzacja a nie lambda
    if inside.contains("->") { return None; }
    let fields: Vec<String> = inside.split(',')
    .map(|f| f.trim().to_string())
    .filter(|f| !f.is_empty())
    .collect();
    if fields.is_empty() { return None; }
    let after = s[end + 1..].trim();
    let after = after.strip_prefix('=')?;
    if after.starts_with('=') { return None; }
    let source = after.trim().to_string();
    Some((fields, source))
}

// ─────────────────────────────────────────────────────────────
// is_defer — `defer expr`
// ─────────────────────────────────────────────────────────────
fn is_defer(line: &str) -> Option<String> {
    let rest = line.strip_prefix("defer")?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(rest.trim().to_string())
}

// ─────────────────────────────────────────────────────────────
// is_scope_def — `;;scope def`
// ─────────────────────────────────────────────────────────────
fn is_scope_def(line: &str) -> bool {
    line.trim() == ";;scope def"
}

// ─────────────────────────────────────────────────────────────
// is_test_def — `==test "opis" [`
// ─────────────────────────────────────────────────────────────
fn is_test_def(line: &str) -> Option<String> {
    let s = line.trim().strip_prefix("==test")?;
    let s = s.trim_start();
    if !s.starts_with('"') { return None; }
    let end = s[1..].find('"')?;
    let desc = s[1..end + 1].to_string();
    Some(desc)
}

// ─────────────────────────────────────────────────────────────
// is_adt_def — `==type Name [ Variant1 [...], Variant2 ]`
// ─────────────────────────────────────────────────────────────
fn is_adt_def(line: &str) -> Option<(String, Vec<(String, Vec<(String, String)>)>)> {
    let s = line.trim().strip_prefix("==type")?;
    let s = s.trim_start();
    // pobierz nazwę
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
    .map(|c| c.len_utf8())
    .sum::<usize>();
    if ident_len == 0 { return None; }
    let name = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start();
    // musi zaczynać się od '['
    let rest = rest.strip_prefix('[')?;
    let end  = rest.rfind(']')?;
    let body = &rest[..end];
    // podziel na warianty po ','
    let mut variants = Vec::new();
    for part in body.split(',') {
        let part = part.trim();
        if part.is_empty() { continue; }
        if let Some(bracket) = part.find('[') {
            let vname = part[..bracket].trim().to_string();
            let fields_str = &part[bracket + 1..];
            let fend = fields_str.rfind(']').unwrap_or(fields_str.len());
            let fields: Vec<(String, String)> = fields_str[..fend].split(',')
            .filter_map(|f| {
                let f = f.trim();
                let colon = f.find(':')?;
                let fname = f[..colon].trim().to_string();
                let ftype = f[colon + 1..].trim().to_string();
                Some((fname, ftype))
            })
            .collect();
            variants.push((vname, fields));
        } else {
            variants.push((part.to_string(), Vec::new()));
        }
    }
    if variants.is_empty() { return None; }
    Some((name, variants))
}

// ─────────────────────────────────────────────────────────────
// is_pipe_line — `| .step args` (wieloliniowy pipe)
// ─────────────────────────────────────────────────────────────
fn is_pipe_line(line: &str) -> Option<String> {
    let s = line.trim();
    if !s.starts_with("| ") { return None; }
    // Nie mylić z |> (pipe operator)
    if s.starts_with("|>") { return None; }
    Some(s[2..].trim().to_string())
}

// ─────────────────────────────────────────────────────────────
// Parsowanie przez pest — funkcje pomocnicze
// ─────────────────────────────────────────────────────────────
fn parse_extern(raw: &str) -> (String, bool) {
    let t = raw.trim();
    if let Some(r) = t.strip_prefix("static") {
        if r.starts_with(|c: char| c.is_whitespace()) {
            return (r.trim().to_string(), true);
        }
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

// ─────────────────────────────────────────────────────────────
// line_to_op — główna funkcja konwersji pest → LineOp
// ─────────────────────────────────────────────────────────────
fn line_to_op(line_pair: Pair<Rule>) -> Option<LineOp> {
    let mut inner = line_pair.into_inner();
    if inner.peek().map(|p| p.as_rule()) == Some(Rule::sudo) { inner.next(); }
    let stmt = inner.next()?;
    let node = stmt.into_inner().next()?;

    Some(match node.as_rule() {

        // ── ISTNIEJĄCE — BEZ ZMIAN ────────────────────────────────
        Rule::class_def  => LineOp::ClassDef(node.into_inner().next()?.as_str().to_string()),
         Rule::func_def => {
             let mut fi = node.into_inner();
             let name = fi.next()?.as_str().to_string();
             let sig  = fi.next().map(|p| p.as_str().to_string());
             // Sprawdź czy to generic sig z constraint
             if let Some(ref s) = sig {
                 if s.contains(" impl ") {
                     return Some(LineOp::FuncDefGeneric(name, s.clone()));
                 }
             }
             LineOp::FuncDef(name, sig)
         },
         Rule::func_done  => LineOp::FuncDone,
         Rule::call_stmt  => {
             let mut fi = node.into_inner();
             let path = fi.next()?.as_str().to_string();
             let args = fi.next().map(|p| p.as_str().to_string()).unwrap_or_default();
             LineOp::Call(path, args)
         },
         Rule::sys_dep    => LineOp::SysDep(node.into_inner().next()?.as_str().to_string()),
         Rule::lib_stmt   => LineOp::Lib(parse_lib_ref(node.into_inner().next()?)),
         Rule::sep_cmd    => LineOp::SepCmd(node.into_inner().next()?.as_str().to_string()),
         Rule::raw_cmd    => LineOp::RawCmd(node.into_inner().next()?.as_str().to_string()),
         Rule::expl_cmd   => {
             let mut fi = node.into_inner();
             fi.next(); // cpfx
             LineOp::ExplCmd(fi.next()?.as_str().to_string())
         },
         Rule::plugin_stmt => {
             let (name, args) = split_plugin(node.into_inner().next()?.as_str());
             LineOp::Plugin(name, args)
         },
         Rule::extern_stmt => {
             let (path, is_static) = parse_extern(node.into_inner().next()?.as_str());
             LineOp::Extern(path, is_static)
         },
         Rule::loop_stmt => {
             let mut fi = node.into_inner();
             let n: u64 = fi.next()?.as_str().parse().unwrap_or(0);
             fi.next(); // cpfx
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
         Rule::else_stmt => {
             let mut fi = node.into_inner();
             fi.next();
             LineOp::Else(fi.next()?.as_str().to_string())
         },
         Rule::while_stmt => {
             let mut fi = node.into_inner();
             let c = fi.next()?.as_str().to_string(); fi.next();
             LineOp::While(c, fi.next()?.as_str().to_string())
         },
         Rule::for_stmt => {
             let mut fi = node.into_inner();
             let v = fi.next()?.as_str().to_string();
             let i = fi.next()?.as_str().to_string();
             fi.next();
             LineOp::For(v, i, fi.next()?.as_str().to_string())
         },
         Rule::bg_stmt    => LineOp::Bg(node.into_inner().next()?.as_str().to_string()),
         Rule::log_stmt   => LineOp::Log(node.into_inner().next()?.as_str().to_string()),
         Rule::lock_stmt  => {
             let mut fi   = node.into_inner();
             let lock_key = fi.next()?;
             let k = lock_key.into_inner().next()?.as_str().to_string();
             let v = fi.next()?.as_str().to_string();
             LineOp::Lock(k, v)
         },
         Rule::unlock_stmt => {
             let lock_key = node.into_inner().next()?;
             let k = lock_key.into_inner().next()?.as_str().to_string();
             LineOp::Unlock(k)
         },
         Rule::enum_stmt => {
             let mut fi = node.into_inner();
             let name = fi.next()?.as_str().to_string();
             LineOp::Enum(name, fi.map(|p: Pair<Rule>| p.as_str().to_string()).collect())
         },
         Rule::struct_stmt => {
             let mut fi   = node.into_inner();
             let name     = fi.next()?.as_str().to_string();
             let fields   = fi.map(|p: Pair<Rule>| {
                 let mut f = p.into_inner();
                 (
                     f.next().map(|x| x.as_str().to_string()).unwrap_or_default(),
                  f.next().map(|x| x.as_str().to_string()).unwrap_or_default(),
                 )
             }).collect();
             LineOp::Struct(name, fields)
         },
         Rule::import_stmt => {
             let mut fi    = node.into_inner();
             let resource  = fi.next()?.as_str().trim_matches('"').to_string();
             let namespace = fi.next().map(|p| p.as_str().to_string());
             LineOp::Import(resource, namespace)
         },
         Rule::try_stmt => {
             let mut fi = node.into_inner();
             LineOp::Try(fi.next()?.as_str().to_string(), fi.next()?.as_str().to_string())
         },
         Rule::raw_blk_s  => LineOp::RawBlockStart,
         Rule::raw_blk_e  => LineOp::RawBlockEnd,
         Rule::end_stmt   => {
             let code = node.into_inner().next()
             .and_then(|p: Pair<Rule>| p.as_str().parse::<i32>().ok())
             .unwrap_or(0);
             LineOp::End(code)
         },
         Rule::out_stmt => {
             let val = node.into_inner().next()
             .map(|p| p.as_str().to_string())
             .unwrap_or_default();
             LineOp::Out(val)
         },

         // ── NOWE reguły ───────────────────────────────────────────

         // % key = val — stała
         Rule::percent_stmt => {
             let mut fi = node.into_inner();
             let key = fi.next()?.as_str().to_string();
             let val = fi.next()?.as_str().to_string();
             LineOp::Percent(key, val)
         },

         // spawn rest
         Rule::spawn_stmt => {
             let rest = node.into_inner().next()?.as_str().to_string();
             LineOp::Spawn(rest)
         },

         // await rest
         Rule::await_stmt => {
             let rest = node.into_inner().next()?.as_str().to_string();
             LineOp::Await(rest)
         },

         // assert assert_body str_lit?
         Rule::assert_stmt => {
             let mut fi = node.into_inner();
             let cond   = fi.next()?.as_str().trim().to_string();
             let msg    = fi.next().map(|p| p.as_str().trim_matches('"').to_string());
             LineOp::Assert(cond, msg)
         },

         // match cond cpfx
         Rule::match_stmt => {
             let mut fi = node.into_inner();
             let cond   = fi.next()?.as_str().to_string();
             LineOp::Match(cond)
         },

         // match_val cpfx rest
         Rule::match_arm => {
             let mut fi = node.into_inner();
             let val    = fi.next()?.as_str().trim().to_string();
             fi.next(); // cpfx
             let cmd    = fi.next()?.as_str().to_string();
             LineOp::MatchArm(val, cmd)
         },

         // pipe_stmt = { call_path ~ rest? ~ (pipe_item_sep ~ call_path ~ rest?)+ }
         Rule::pipe_stmt => {
             let steps: Vec<String> = node.into_inner()
             .map(|p| p.as_str().to_string())
             .collect();
             LineOp::Pipe(steps)
         },

         // | step — wieloliniowy pipe
         Rule::pipe_line_stmt => {
             let rest = node.into_inner().next()?.as_str().to_string();
             LineOp::PipeLine(rest)
         },

         // ==interface Name [method1, method2]
         Rule::interface_def => {
             let mut fi   = node.into_inner();
             let name     = fi.next()?.as_str().to_string();
             let methods  = fi.map(|p| p.as_str().to_string()).collect();
             LineOp::InterfaceDef(name, methods)
         },

         // ;;Class impl Interface def
         Rule::impl_def => {
             let mut fi    = node.into_inner();
             let class     = fi.next()?.as_str().to_string();
             let interface = fi.next()?.as_str().to_string();
             LineOp::ImplDef(class, interface)
         },

         // :: name [size] def — obsługiwane przez is_arena_def w Rust
         // ale jeśli pest złapie arena_def:
         Rule::arena_def => {
             let mut fi = node.into_inner();
             let name   = fi.next()?.as_str().to_string();
             let size   = fi.next()?.as_str().to_string();
             LineOp::ArenaDef(name, size)
         },

         // $var.method args — kolekcja
         Rule::collection_mut => {
             let raw = node.as_str();
             // parse: $var.method args
             let s = raw.strip_prefix('$').unwrap_or(raw);
             let dot = s.find('.').unwrap_or(s.len());
             let var  = s[..dot].to_string();
             let rest = &s[dot + 1..];
             let sp   = rest.find(' ').unwrap_or(rest.len());
             let method = rest[..sp].to_string();
             let args   = rest[sp..].trim().to_string();
             LineOp::CollectionMut(var, method, args)
         },

         // expr ?! "msg"
         Rule::result_unwrap => {
             let raw = node.as_str();
             if let Some((expr, msg)) = is_result_unwrap(raw) {
                 LineOp::ResultUnwrap(expr, msg)
             } else {
                 return None;
             }
         },

         // module.method args
         Rule::module_call => {
             let mut fi = node.into_inner();
             let path   = fi.next()?.as_str().to_string();
             let args   = fi.next().map(|p| p.as_str().trim().to_string()).unwrap_or_default();
             LineOp::ModuleCall(path, args)
         },

         // recur args
         Rule::recur_stmt => {
             let rest = node.into_inner().next()?.as_str().to_string();
             LineOp::Recur(rest)
         },

         // defer expr
         Rule::defer_stmt => {
             let rest = node.into_inner().next()?.as_str().to_string();
             LineOp::Defer(rest)
         },

         // do
         Rule::do_stmt => {
             LineOp::DoBlock
         },

         // ;;scope def
         Rule::scope_def => {
             LineOp::ScopeDef
         },

         // ==type Name [...]
         Rule::adt_def => {
             let raw = node.as_str();
             if let Some((name, variants)) = is_adt_def(raw) {
                 LineOp::AdtDef(name, variants)
             } else {
                 return None;
             }
         },

         // ==test "opis" [
         Rule::test_def => {
             let mut fi  = node.into_inner();
             let desc    = fi.next()?.as_str().trim_matches('"').to_string();
             LineOp::TestDef(desc)
         },

         // lambda_lit { $x -> body } — jako standalone (np. jako arg inline)
         Rule::lambda_lit => {
             let raw = node.as_str();
             if let Some((params, body)) = parse_lambda_literal(raw) {
                 LineOp::Lambda(params, body)
             } else {
                 return None;
             }
         },

         // destruct_stmt
         Rule::destruct_stmt => {
             let raw = node.as_str();
             if let Some((head, tail, src)) = is_destruct_list(raw) {
                 LineOp::DestructList(head, tail, src)
             } else if let Some((fields, src)) = is_destruct_map(raw) {
                 LineOp::DestructMap(fields, src)
             } else {
                 return None;
             }
         },

         _ => return None,
    })
}

fn suggest(line: &str) -> String {
    let t = line.trim();
    let cmds = [
        "echo ", "mkdir ", "rm ", "cp ", "mv ", "cat ", "jq ",
        "curl ", "find ", "ls ", "touch ", "chmod ", "chown ",
        "git ", "date ", "printf ", "grep ", "sed ", "awk ",
        "tar ", "df ", "ps ", "free ",
    ];
    for cmd in &cmds {
        if t.starts_with(cmd) {
            return format!("Brakuje prefiksu komendy — użyj: > {}", t);
        }
    }
    "Nieznana składnia — dokumentacja: https://hackeros-linux-system.github.io/HackerOS-Website/hacker-lang/docs.html".to_string()
}

// ─────────────────────────────────────────────────────────────
// parse_file — główna funkcja
// ─────────────────────────────────────────────────────────────
pub fn parse_file(
    path: &str,
    resolve_libs: bool,
    verbose: bool,
    seen_libs: &mut HashSet<String>,
) -> Result<AnalysisResult, Vec<ParseError>> {
    let mut result = AnalysisResult::default();

    let src = match fs::read_to_string(path) {
        Ok(s)  => s,
        Err(e) => return Err(vec![ParseError::IoError {
            path: path.to_string(), message: e.to_string(),
        }]),
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

    // Stan do-notacji
    let mut in_do_block    = false;
    let mut do_key         = String::new();
    let mut do_buf         = Vec::<ProgramNode>::new();
    let mut do_start_line  = 0usize;

    // Stan testu jednostkowego
    let mut in_test_block  = false;
    let mut test_desc      = String::new();
    let mut test_buf       = Vec::<ProgramNode>::new();

    for (idx, raw_line) in src.lines().enumerate() {
        let off  = offsets[idx];
        let trim = raw_line.trim();
        if trim.is_empty() { continue; }

        if trim == "!!" { in_blk_comment = !in_blk_comment; continue; }
        if in_blk_comment { continue; }
        if trim.starts_with('!') { continue; }

        // ── Zakończenie bloku do...done ─────────────────────────────────────
        if in_do_block {
            if trim == "done" {
                // Zamknij blok do
                let node = ProgramNode {
                    line_num: do_start_line, is_sudo: false,
                    content:  CommandType::DoBlock { key: do_key.clone(), body: do_buf.clone() },
                    original_text: format!("{} = do", do_key),
                    span: (offsets[do_start_line.saturating_sub(1)], 0),
                };
                push_node(&mut result, &scopes, node);
                in_do_block = false;
                do_key.clear();
                do_buf.clear();
            } else {
                // Parsuj linię wewnątrz do-bloku jako normalny węzeł
                let (parse_src, is_sudo) = if trim.starts_with('^') {
                    (trim[1..].trim(), true)
                } else {
                    (trim, false)
                };
                let span = SourceSpan::new(off.into(), parse_src.len().into());
                let line_num = idx + 1;
                if let Some(op) = try_parse_line(parse_src) {
                    if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                        do_buf.push(node);
                    }
                }
            }
            continue;
        }

        // ── Zakończenie bloku testu ==test ──────────────────────────────────
        if in_test_block {
            if trim == "]" {
                let node = ProgramNode {
                    line_num: idx + 1, is_sudo: false,
                    content:  CommandType::TestBlock { desc: test_desc.clone(), body: test_buf.clone() },
                    original_text: format!("==test \"{}\" [...]", test_desc),
                    span: (off, 0),
                };
                push_node(&mut result, &scopes, node);
                in_test_block = false;
                test_desc.clear();
                test_buf.clear();
            } else {
                // Parsuj assert i inne linie testu
                let (parse_src, is_sudo) = if trim.starts_with('^') {
                    (trim[1..].trim(), true)
                } else {
                    (trim, false)
                };
                let line_num = idx + 1;
                if let Some(op) = try_parse_line(parse_src) {
                    if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                        test_buf.push(node);
                    }
                }
            }
            continue;
        }

        if in_raw_block {
            if trim == "]" {
                let node = ProgramNode {
                    line_num: raw_start_line, is_sudo: raw_sudo,
                    content:  CommandType::RawNoSub(raw_buf.trim().to_string()),
                    original_text: "[ ... ]".to_string(),
                    span: (raw_start_off, raw_buf.len()),
                };
                push_node(&mut result, &scopes, node);
                in_raw_block = false;
                raw_buf.clear();
            } else {
                raw_buf.push_str(raw_line);
                raw_buf.push('\n');
            }
            continue;
        }

        let (parse_src, is_sudo) = if trim.starts_with('^') {
            (trim[1..].trim(), true)
        } else {
            (trim, false)
        };
        if is_sudo {
            result.is_potentially_unsafe = true;
            result.safety_warnings.push(format!("Linia {}: sudo (^)", idx + 1));
        }

        let line_num = idx + 1;
        let span     = SourceSpan::new(off.into(), parse_src.len().into());

        // ══════════════════════════════════════════════════════
        // ETAP 0a: arena allocator `:: name [size] def`
        // PRZED sprawdzaniem :: jako unsafe_def
        // ══════════════════════════════════════════════════════
        if let Some((name, size)) = is_arena_def(parse_src) {
            let op = LineOp::ArenaDef(name, size);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0b: kolekcja `$var.method args`
        // ══════════════════════════════════════════════════════
        if let Some((var, method, args)) = is_collection_mut(parse_src) {
            let op = LineOp::CollectionMut(var, method, args);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0c: result unwrap standalone `expr ?! "msg"`
        // (bez przypisania)
        // ══════════════════════════════════════════════════════
        if parse_src.contains(" ?! ") && !parse_src.contains('=') {
            if let Some((expr, msg)) = is_result_unwrap(parse_src) {
                let op = LineOp::ResultUnwrap(expr, msg);
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                    push_node(&mut result, &scopes, node);
                }
                continue;
            }
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0d: ADT `==type Name [...]`
        // ══════════════════════════════════════════════════════
        if parse_src.starts_with("==type ") {
            if let Some((name, variants)) = is_adt_def(parse_src) {
                let op = LineOp::AdtDef(name, variants);
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                    push_node(&mut result, &scopes, node);
                }
                continue;
            }
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0e: test jednostkowy `==test "opis" [`
        // ══════════════════════════════════════════════════════
        if parse_src.starts_with("==test ") {
            if let Some(desc) = is_test_def(parse_src) {
                in_test_block = true;
                test_desc = desc;
                test_buf.clear();
                continue;
            }
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0f: ;;scope def — zasięg leksykalny
        // ══════════════════════════════════════════════════════
        if is_scope_def(parse_src) {
            let op = LineOp::ScopeDef;
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
                // ;;scope traktujemy jak nową klasę anonimową z unikalnym imieniem
                let scope_name = format!("__scope_{}", line_num);
                scopes.push(Scope::Class(scope_name));
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0g: defer `defer expr`
        // ══════════════════════════════════════════════════════
        if let Some(expr) = is_defer(parse_src) {
            let op = LineOp::Defer(expr);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0h: recur `recur args`
        // ══════════════════════════════════════════════════════
        if let Some(args) = is_recur(parse_src) {
            let op = LineOp::Recur(args);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0i: wieloliniowy pipe `| .step args`
        // ══════════════════════════════════════════════════════
        if let Some(step) = is_pipe_line(parse_src) {
            let op = LineOp::PipeLine(step);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0j: destrukturyzacja listy `[head | tail] = $src`
        // ══════════════════════════════════════════════════════
        if parse_src.starts_with('[') && parse_src.contains('|') && parse_src.contains('=') {
            if let Some((head, tail, src_v)) = is_destruct_list(parse_src) {
                let op = LineOp::DestructList(head, tail, src_v);
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                    push_node(&mut result, &scopes, node);
                }
                continue;
            }
        }

        // ══════════════════════════════════════════════════════
        // ETAP 0k: destrukturyzacja mapy `{field1, field2} = $src`
        // ══════════════════════════════════════════════════════
        if parse_src.starts_with('{') && !parse_src.contains(':') && parse_src.contains('=') {
            if let Some((fields, src_v)) = is_destruct_map(parse_src) {
                let op = LineOp::DestructMap(fields, src_v);
                if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                    push_node(&mut result, &scopes, node);
                }
                continue;
            }
        }

        // ══════════════════════════════════════════════════════
        // ETAP 1a: stała %KEY = val
        // ══════════════════════════════════════════════════════
        if let Some((key, val)) = is_percent(parse_src) {
            let op = LineOp::Percent(key, val);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 1b: `key = spawn rest`
        // ══════════════════════════════════════════════════════
        if let Some((key, rest)) = is_spawn_assign(parse_src) {
            let op = LineOp::AssignSpawn(key, rest);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 1c: `key = await rest`
        // ══════════════════════════════════════════════════════
        if let Some((key, rest)) = is_await_assign(parse_src) {
            let op = LineOp::AssignAwait(key, rest);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 1d: `key = do` — blok do-notacji
        // ══════════════════════════════════════════════════════
        if let Some(key) = is_do_assign(parse_src) {
            in_do_block   = true;
            do_key        = key;
            do_buf        = Vec::new();
            do_start_line = line_num;
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 1e: `key = { $x -> body }` — lambda
        // ══════════════════════════════════════════════════════
        if let Some((key, params, body, is_raw, is_global)) = is_lambda_assign(parse_src) {
            let op = LineOp::AssignLambda(key, params, body, is_raw, is_global);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 1f: wyrażenie `key = expr_z_operatorem / lista / mapa`
        // ══════════════════════════════════════════════════════
        if let Some((key, expr, is_raw, is_global)) = is_expr_assign(parse_src) {
            let op = LineOp::AssignExpr(key, expr, is_raw, is_global);
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 1g: zwykłe przypisanie
        // ══════════════════════════════════════════════════════
        if let Some((key, val, is_raw, is_global)) = is_assignment(parse_src) {
            let op = if is_global {
                LineOp::GlobalVar(key, val)
            } else {
                LineOp::LocalVar(key, val, is_raw)
            };
            if let Some(node) = build_node(line_num, is_sudo, off, parse_src, op) {
                push_node(&mut result, &scopes, node);
            }
            continue;
        }

        // ══════════════════════════════════════════════════════
        // ETAP 2: Parsuj przez pest
        // ══════════════════════════════════════════════════════
        let op = match HlParser::parse(Rule::line, parse_src) {
            Ok(mut pairs) => match line_to_op(pairs.next().unwrap()) {
                Some(op) => op,
                None     => { push_err(&mut errors, path, &src, span, line_num, parse_src); continue; },
            },
            Err(_) => { push_err(&mut errors, path, &src, span, line_num, parse_src); continue; },
        };

        match op {
            LineOp::ClassDef(name)       => { scopes.push(Scope::Class(name)); },
            LineOp::ScopeDef             => {
                let scope_name = format!("__scope_{}", line_num);
                scopes.push(Scope::Class(scope_name));
            },
            LineOp::FuncDef(name, sig)   => {
                let full = qualified(&scopes, &name);
                scopes.push(Scope::Func(full.clone()));
                result.functions.insert(full, (false, sig, Vec::new()));
            },
            LineOp::FuncDefGeneric(name, sig) => {
                let full = qualified(&scopes, &name);
                scopes.push(Scope::Func(full.clone()));
                result.functions.insert(full, (false, Some(sig), Vec::new()));
            },
            // ArenaDef z pest (fallback — normalnie złapane wyżej przez is_arena_def)
            LineOp::ArenaDef(name, size) => {
                let full = qualified(&scopes, &name);
                scopes.push(Scope::Func(full.clone()));
                // is_unsafe=false — arena to zwykła (wydajna) funkcja, nie unsafe
                result.functions.insert(full, (false, Some(format!("[arena:{}]", size)), Vec::new()));
            },
            LineOp::FuncDone => { scopes.pop(); },
            LineOp::RawBlockStart => {
                in_raw_block   = true;
                raw_sudo       = is_sudo;
                raw_start_line = line_num;
                raw_start_off  = off;
            },
            LineOp::RawBlockEnd => {
                errors.push(ParseError::SyntaxError {
                    src: NamedSource::new(path, src.clone()), span, line_num,
                            advice: "Nieoczekiwany ']' bez pasującego '['".to_string(),
                });
            },
            LineOp::SysDep(dep)  => result.deps.push(dep),
            LineOp::Lib(lib_ref) => handle_lib(
                lib_ref, path, &src, span,
                resolve_libs, verbose, seen_libs, &mut result, &mut errors,
            ),
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
// try_parse_line — próbuje sparsować linię przez wszystkie
// pre-pest etapy, używane wewnątrz bloków do/test
// ─────────────────────────────────────────────────────────────
fn try_parse_line(parse_src: &str) -> Option<LineOp> {
    if let Some(args) = is_recur(parse_src) { return Some(LineOp::Recur(args)); }
    if let Some(expr) = is_defer(parse_src)  { return Some(LineOp::Defer(expr)); }
    if let Some(step) = is_pipe_line(parse_src) { return Some(LineOp::PipeLine(step)); }
    if let Some((var, method, args)) = is_collection_mut(parse_src) {
        return Some(LineOp::CollectionMut(var, method, args));
    }
    if parse_src.contains(" ?! ") && !parse_src.contains('=') {
        if let Some((expr, msg)) = is_result_unwrap(parse_src) {
            return Some(LineOp::ResultUnwrap(expr, msg));
        }
    }
    if let Some((key, val)) = is_percent(parse_src) { return Some(LineOp::Percent(key, val)); }
    if let Some((key, rest)) = is_spawn_assign(parse_src) { return Some(LineOp::AssignSpawn(key, rest)); }
    if let Some((key, rest)) = is_await_assign(parse_src) { return Some(LineOp::AssignAwait(key, rest)); }
    if let Some((key, params, body, is_raw, is_global)) = is_lambda_assign(parse_src) {
        return Some(LineOp::AssignLambda(key, params, body, is_raw, is_global));
    }
    if let Some((key, expr, is_raw, is_global)) = is_expr_assign(parse_src) {
        return Some(LineOp::AssignExpr(key, expr, is_raw, is_global));
    }
    if let Some((key, val, is_raw, is_global)) = is_assignment(parse_src) {
        let op = if is_global { LineOp::GlobalVar(key, val) } else { LineOp::LocalVar(key, val, is_raw) };
        return Some(op);
    }
    if let Ok(mut pairs) = HlParser::parse(Rule::line, parse_src) {
        if let Some(pair) = pairs.next() {
            return line_to_op(pair);
        }
    }
    None
}

fn qualified(scopes: &[Scope], name: &str) -> String {
    for s in scopes.iter().rev() {
        if let Scope::Class(cls) = s { return format!("{}.{}", cls, name); }
    }
    name.to_string()
}

fn push_err(
    errors: &mut Vec<ParseError>, path: &str, src: &str,
    span: SourceSpan, line_num: usize, line_src: &str,
) {
    errors.push(ParseError::SyntaxError {
        src:     NamedSource::new(path, src.to_string()),
                span, line_num, advice: suggest(line_src),
    });
}

fn handle_lib(
    lib_ref: LibRef, path: &str, src: &str, span: SourceSpan,
    resolve_libs: bool, verbose: bool,
    seen_libs: &mut HashSet<String>,
    result: &mut AnalysisResult, errors: &mut Vec<ParseError>,
) {
    result.libs.push(lib_ref.clone());
    let key = lib_ref.cache_key();
    match lib_ref.lib_type {
        LibType::Vira | LibType::Virus => {
            // repozytoria git z ~/.hackeros/hacker-lang/libs/.virus/
            if verbose { eprintln!("[lib] {}: {}", lib_ref.lib_type.as_str(), lib_ref.name); }
        },
        LibType::Bytes => {
            if verbose {
                if let Some(p) = lib_path(&lib_ref) { eprintln!("[lib] bin: {}", p.display()); }
            }
        },
        LibType::Core => {
            if !resolve_libs { return; }
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
                    src:     NamedSource::new(path, src.to_string()),
                            span, line_num: 0,
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

fn build_node(
    line_num: usize, is_sudo: bool, off: usize, src: &str, op: LineOp,
) -> Option<ProgramNode> {
    let cmd = match op {
        // ── ISTNIEJĄCE — BEZ ZMIAN ────────────────────────────────
        LineOp::SepCmd(c)          => CommandType::Isolated(c),
        LineOp::RawCmd(c)          => CommandType::RawNoSub(c),
        LineOp::ExplCmd(c)         => CommandType::RawSub(c),
        LineOp::GlobalVar(k, v)    => CommandType::AssignEnv   { key: k, val: v },
        LineOp::LocalVar(k, v, r)  => CommandType::AssignLocal { key: k, val: v, is_raw: r },
        LineOp::Loop(n, c)         => CommandType::Loop   { count: n, cmd: c },
        LineOp::If(co, c)          => CommandType::If     { cond: co, cmd: c },
        LineOp::Elif(co, c)        => CommandType::Elif   { cond: co, cmd: c },
        LineOp::Else(c)            => CommandType::Else   { cmd: c },
        LineOp::While(co, c)       => CommandType::While  { cond: co, cmd: c },
        LineOp::For(v, i, c)       => CommandType::For    { var: v, in_: i, cmd: c },
        LineOp::Bg(c)              => CommandType::Background(c),
        LineOp::Call(p, a)         => CommandType::Call   { path: p, args: a },
        LineOp::Plugin(n, a)       => CommandType::Plugin { name: n, args: a, is_super: is_sudo },
        LineOp::Log(m)             => CommandType::Log(m),
        LineOp::Lock(k, v)         => CommandType::Lock   { key: k, val: v },
        LineOp::Unlock(k)          => CommandType::Unlock { key: k },
        LineOp::Extern(p, sl)      => CommandType::Extern { path: p, static_link: sl },
        LineOp::Import(r, ns)      => CommandType::Import { resource: r, namespace: ns },
        LineOp::Enum(n, vars)      => CommandType::Enum   { name: n, variants: vars },
        LineOp::Struct(n, flds)    => CommandType::Struct { name: n, fields: flds },
        LineOp::Try(t, c)          => CommandType::Try    { try_cmd: t, catch_cmd: c },
        LineOp::End(code)          => CommandType::End    { code },
        LineOp::Out(v)             => CommandType::Out(v),
        // ── NOWE ─────────────────────────────────────────────────
        LineOp::Percent(k, v)      => CommandType::Const  { key: k, val: v },
        LineOp::Spawn(r)           => CommandType::Spawn(r),
        LineOp::Await(r)           => CommandType::Await(r),
        LineOp::AssignSpawn(k, r)  => CommandType::AssignSpawn { key: k, task: r },
        LineOp::AssignAwait(k, r)  => CommandType::AssignAwait { key: k, expr: r },
        LineOp::Assert(c, m)       => CommandType::Assert { cond: c, msg: m },
        LineOp::Match(c)           => CommandType::Match  { cond: c },
        LineOp::MatchArm(v, c)     => CommandType::MatchArm { val: v, cmd: c },
        LineOp::Pipe(steps)        => CommandType::Pipe(steps),
        // ── NOWE: wyrażenia / kolekcje / interfejsy / arena ───────
        LineOp::AssignExpr(k, e, r, g) => if g {
            CommandType::AssignEnv   { key: k, val: e }
        } else {
            CommandType::AssignExpr  { key: k, expr: e, is_raw: r, is_global: false }
        },
        LineOp::CollectionMut(v, m, a) => CommandType::CollectionMut { var: v, method: m, args: a },
        LineOp::InterfaceDef(n, ms)    => CommandType::Interface { name: n, methods: ms },
        LineOp::ImplDef(c, i)          => CommandType::ImplDef { class: c, interface: i },
        LineOp::ArenaDef(n, s)         => CommandType::ArenaDef { name: n, size: s },
        LineOp::ResultUnwrap(e, m)     => CommandType::ResultUnwrap { expr: e, msg: m },
        LineOp::ModuleCall(p, a)       => CommandType::ModuleCall { path: p, args: a },
        // ── NOWE: lambdy / domknięcia ─────────────────────────────
        LineOp::Lambda(params, body)   => CommandType::Lambda { params, body },
        LineOp::AssignLambda(k, params, body, r, g) => CommandType::AssignLambda {
            key: k, params, body, is_raw: r, is_global: g,
        },
        // ── NOWE: rekurencja ogonowa ──────────────────────────────
        LineOp::Recur(args)            => CommandType::Recur { args },
        // ── NOWE: destrukturyzacja ────────────────────────────────
        LineOp::DestructList(h, t, s)  => CommandType::DestructList { head: h, tail: t, source: s },
        LineOp::DestructMap(flds, s)   => CommandType::DestructMap  { fields: flds, source: s },
        // ── NOWE: zasięg leksykalny ──────────────────────────────
        LineOp::ScopeDef               => CommandType::ScopeDef,
        // ── NOWE: ADT ────────────────────────────────────────────
        LineOp::AdtDef(n, vs)          => CommandType::AdtDef { name: n, variants: vs },
        // ── NOWE: do-notacja ─────────────────────────────────────
        LineOp::DoBlock                => return None, // obsługiwane przez blok stanu
        LineOp::AssignDo(k)            => CommandType::DoBlock { key: k, body: Vec::new() },
        LineOp::PipeLine(step)         => CommandType::PipeLine { step },
        // ── NOWE: testy ──────────────────────────────────────────
        LineOp::TestDef(desc)          => CommandType::TestBlock { desc, body: Vec::new() },
        // ── NOWE: defer ──────────────────────────────────────────
        LineOp::Defer(expr)            => CommandType::Defer { expr },
        // ── NOWE: generics z constraints ─────────────────────────
        LineOp::FuncDefGeneric(n, s)   => CommandType::FuncDefGeneric { name: n, sig: s },
        _ => return None,
    };
    Some(ProgramNode {
        line_num, is_sudo,
         content:       cmd,
         original_text: src.to_string(),
         span:          (off, src.len()),
    })
}
