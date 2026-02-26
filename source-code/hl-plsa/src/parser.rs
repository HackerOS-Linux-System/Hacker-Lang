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
    UnsafeDef(String),
    FuncDef(String),
    FuncDone,
    Call(String),
    SysDep(String),
    Lib(LibRef),
    SepCmd(String),
    RawCmd(String),
    ExplCmd(String),
    GlobalVar(String, String),
    LocalVar(String, String, bool), // key, val, is_raw
    Plugin(String, String),         // name, args
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
    Extern(String, bool),           // path, is_static
    Enum(String, Vec<String>),
    Import(String),
    Try(String, String),
    Struct(String, Vec<(String, String)>),
    RawBlockStart,
    RawBlockEnd,
    End(i32),
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
        LibType::Source => Some(r.join("sources").join(&lib.name).with_extension("hl")),
        LibType::Core   => Some(r.join("core").join(&lib.name).with_extension("hl")),
        LibType::Bytes  => Some(r.join("bytes").join(&lib.name).with_extension("so")),
        LibType::Virus  => Some(r.join(".vira").join(&lib.name).with_extension("a")),
        LibType::Github | LibType::Vira => None,
    }
}

// ─────────────────────────────────────────────────────────────
// is_assignment — wykrywa przypisanie PRZED pest
//
// Zwraca Some((key, value, is_raw, is_global)) lub None.
//
// Przyjmuje linię po trim() i po usunięciu sudo (^).
//
// Akceptuje:
//   @DATA_FILE="..."     is_global=true
//   ~n = $(...)          is_raw=true
//   done_    = $(...)    zwykłe lokalne
//   title_   = $1        zwykłe lokalne
//
// Odrzuca (zwraca None):
//   done                 (brak '=')
//   ? cond > cmd         (zaczyna się od '?')
//   > echo hi            (zaczyna się od '>')
//   log "x"              (po 'log' nie ma '=')
//   ?? cond > cmd        (zaczyna się od '?')
//   x == y               (podwójne '==')
// ─────────────────────────────────────────────────────────────
fn is_assignment(line: &str) -> Option<(String, String, bool, bool)> {
    let mut s = line;
    let mut is_global = false;
    let mut is_raw    = false;

    // prefix @ lub ~
    if let Some(rest) = s.strip_prefix('@') {
        is_global = true;
        s = rest;
    } else if let Some(rest) = s.strip_prefix('~') {
        is_raw = true;
        s = rest;
    }

    // musi zaczynać się od litery lub _
    let first = s.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }

    // zbierz znaki identyfikatora
    let ident_len = s.chars()
    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
    .map(|c| c.len_utf8())
    .sum::<usize>();
    if ident_len == 0 { return None; }

    let key  = s[..ident_len].to_string();
    let rest = s[ident_len..].trim_start_matches(|c| c == ' ' || c == '\t');

    // musi być '=' ale NIE '=='
    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') { return None; }  // to jest '=='

    let value = rest.trim_start_matches(|c| c == ' ' || c == '\t').to_string();

    Some((key, value, is_raw, is_global))
}

// ─────────────────────────────────────────────────────────────
// Parsowanie przez pest
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
        "source" => LibType::Source, "core"   => LibType::Core,
        "bytes"  => LibType::Bytes,  "github" => LibType::Github,
        "virus"  => LibType::Virus,  "vira"   => LibType::Vira,
        _        => LibType::Source,
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
         Rule::unsafe_def => LineOp::UnsafeDef(node.into_inner().next()?.as_str().to_string()),
         Rule::func_def   => LineOp::FuncDef(node.into_inner().next()?.as_str().to_string()),
         Rule::func_done  => LineOp::FuncDone,
         Rule::call_stmt  => LineOp::Call(node.into_inner().next()?.as_str().to_string()),
         Rule::sys_dep    => LineOp::SysDep(node.into_inner().next()?.as_str().to_string()),
         Rule::lib_stmt   => LineOp::Lib(parse_lib_ref(node.into_inner().next()?)),

         Rule::sep_cmd  => LineOp::SepCmd(node.into_inner().next()?.as_str().to_string()),
         Rule::raw_cmd  => LineOp::RawCmd(node.into_inner().next()?.as_str().to_string()),
         Rule::expl_cmd => {
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

         Rule::bg_stmt     => LineOp::Bg(node.into_inner().next()?.as_str().to_string()),
         Rule::log_stmt    => LineOp::Log(node.into_inner().next()?.as_str().to_string()),
         Rule::lock_stmt   => {
             let mut fi = node.into_inner();
             let k = fi.next()?.as_str().to_string(); fi.next();
             LineOp::Lock(k, fi.next()?.as_str().to_string())
         },
         Rule::unlock_stmt => LineOp::Unlock(node.into_inner().next()?.as_str().to_string()),

         Rule::enum_stmt => {
             let mut fi = node.into_inner();
             let name = fi.next()?.as_str().to_string();
             LineOp::Enum(name, fi.map(|p: Pair<Rule>| p.as_str().to_string()).collect())
         },
         Rule::struct_stmt => {
             let mut fi = node.into_inner();
             let name = fi.next()?.as_str().to_string();
             let fields = fi.map(|p: Pair<Rule>| {
                 let mut f = p.into_inner();
                 (
                     f.next().map(|x| x.as_str().to_string()).unwrap_or_default(),
                  f.next().map(|x| x.as_str().to_string()).unwrap_or_default(),
                 )
             }).collect();
             LineOp::Struct(name, fields)
         },
         Rule::import_stmt => LineOp::Import(node.into_inner().next()?.as_str().to_string()),
         Rule::try_stmt => {
             let mut fi = node.into_inner();
             LineOp::Try(fi.next()?.as_str().to_string(), fi.next()?.as_str().to_string())
         },
         Rule::raw_blk_s => LineOp::RawBlockStart,
         Rule::raw_blk_e => LineOp::RawBlockEnd,
         Rule::end_stmt  => {
             let code = node.into_inner().next()
             .and_then(|p: Pair<Rule>| p.as_str().parse::<i32>().ok())
             .unwrap_or(0);
             LineOp::End(code)
         },
         _ => return None,
    })
}

fn suggest(line: &str) -> String {
    let t = line.trim();
    let cmds = ["echo ", "mkdir ", "rm ", "cp ", "mv ", "cat ", "jq ",
    "curl ", "find ", "ls ", "touch ", "chmod ", "chown ",
    "git ", "date ", "printf ", "grep ", "sed ", "awk ",
    "tar ", "df ", "ps ", "free "];
    for cmd in &cmds {
        if t.starts_with(cmd) {
            return format!("Brakuje prefiksu komendy — użyj: > {}", t);
        }
    }
    "Nieznana składnia — dokumentacja: https://hackeros.dev/docs/hacker-lang".to_string()
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

    for (idx, raw_line) in src.lines().enumerate() {
        let off  = offsets[idx];
        let trim = raw_line.trim();
        if trim.is_empty() { continue; }

        if trim == "!!" { in_blk_comment = !in_blk_comment; continue; }
        if in_blk_comment { continue; }
        if trim.starts_with('!') { continue; }

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
        // ETAP 1: Sprawdź przypisanie w Rust — PRZED pest
        // Obsługuje done_=x, total_=x, @VAR=x, ~name=x itp.
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
            continue;  // ← kluczowe: nie trafia do pest
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
            LineOp::ClassDef(name) => { scopes.push(Scope::Class(name)); },
            LineOp::FuncDef(name) => {
                let full = qualified(&scopes, &name);
                scopes.push(Scope::Func(full.clone()));
                result.functions.insert(full, (false, Vec::new()));
            },
            LineOp::UnsafeDef(name) => {
                let full = qualified(&scopes, &name);
                scopes.push(Scope::Func(full.clone()));
                result.functions.insert(full, (true, Vec::new()));
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
            LineOp::SysDep(dep) => result.deps.push(dep),
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
        src: NamedSource::new(path, src.to_string()),
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
        LibType::Github | LibType::Vira => {
            if verbose { eprintln!("[lib] {}: {}", lib_ref.lib_type.as_str(), lib_ref.name); }
        },
        LibType::Bytes | LibType::Virus => {
            if verbose {
                if let Some(p) = lib_path(&lib_ref) { eprintln!("[lib] bin: {}", p.display()); }
            }
        },
        LibType::Source | LibType::Core => {
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
                    src: NamedSource::new(path, src.to_string()),
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
            if let Some(f) = result.functions.get_mut(name) { f.1.push(node); return; }
        }
    }
    result.main_body.push(node);
}

fn build_node(line_num: usize, is_sudo: bool, off: usize, src: &str, op: LineOp) -> Option<ProgramNode> {
    let cmd = match op {
        LineOp::SepCmd(c)         => CommandType::Isolated(c),
        LineOp::RawCmd(c)         => CommandType::RawNoSub(c),
        LineOp::ExplCmd(c)        => CommandType::RawSub(c),
        LineOp::GlobalVar(k, v)   => CommandType::AssignEnv { key: k, val: v },
        LineOp::LocalVar(k, v, r) => CommandType::AssignLocal { key: k, val: v, is_raw: r },
        LineOp::Loop(n, c)        => CommandType::Loop { count: n, cmd: c },
        LineOp::If(co, c)         => CommandType::If { cond: co, cmd: c },
        LineOp::Elif(co, c)       => CommandType::Elif { cond: co, cmd: c },
        LineOp::Else(c)           => CommandType::Else { cmd: c },
        LineOp::While(co, c)      => CommandType::While { cond: co, cmd: c },
        LineOp::For(v, i, c)      => CommandType::For { var: v, in_: i, cmd: c },
        LineOp::Bg(c)             => CommandType::Background(c),
        LineOp::Call(n)           => CommandType::Call(n),
        LineOp::Plugin(n, a)      => CommandType::Plugin { name: n, args: a, is_super: is_sudo },
        LineOp::Log(m)            => CommandType::Log(m),
        LineOp::Lock(k, v)        => CommandType::Lock { key: k, val: v },
        LineOp::Unlock(k)         => CommandType::Unlock { key: k },
        LineOp::Extern(p, sl)     => CommandType::Extern { path: p, static_link: sl },
        LineOp::Import(r)         => CommandType::Import { resource: r },
        LineOp::Enum(n, vars)     => CommandType::Enum { name: n, variants: vars },
        LineOp::Struct(n, flds)   => CommandType::Struct { name: n, fields: flds },
        LineOp::Try(t, c)         => CommandType::Try { try_cmd: t, catch_cmd: c },
        LineOp::End(code)         => CommandType::End { code },
        _ => return None,
    };
    Some(ProgramNode {
        line_num, is_sudo,
         content: cmd,
         original_text: src.to_string(),
         span: (off, src.len()),
    })
}
