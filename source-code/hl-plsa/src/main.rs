use clap::Parser as ClapParser;
use colored::*;
use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::exit;
use thiserror::Error;

mod parser;
use crate::parser::{parse_file, plugins_root};

#[derive(ClapParser, Debug)]
#[command(name="hl-plsa", author="HackerOS", version=env!("CARGO_PKG_VERSION"),
about="hacker-lang static analyser v8")]
struct Args {
    /// Plik .hl do analizy
    file: String,
    /// Szczegółowe wyjście
    #[arg(long, short)] verbose: bool,
    /// Wypisz AST jako JSON
    #[arg(long, short)] json: bool,
    /// Rekurencyjnie parsuj biblioteki source/core
    #[arg(long)] resolve_libs: bool,
    /// Sprawdź czy pliki pluginów istnieją
    #[arg(long)] check_plugins: bool,
}

// ─────────────────────────────────────────────────────────────
// Typy publiczne (używane też przez parser.rs przez crate::)
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType { Source, Core, Bytes, Github, Virus, Vira }
impl LibType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LibType::Source => "source", LibType::Core   => "core",
            LibType::Bytes  => "bytes",  LibType::Github => "github",
            LibType::Virus  => "virus",  LibType::Vira   => "vira",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibRef {
    pub lib_type: LibType,
    pub name:     String,
    pub version:  Option<String>,
}
impl LibRef {
    pub fn cache_key(&self) -> String {
        match &self.version {
            Some(v) => format!("{}/{}/{}", self.lib_type.as_str(), self.name, v),
            None    => format!("{}/{}", self.lib_type.as_str(), self.name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommandType {
    RawNoSub(String),
    RawSub(String),
    Isolated(String),
    AssignEnv   { key: String, val: String },
    AssignLocal { key: String, val: String, is_raw: bool },
    Loop   { count: u64, cmd: String },
    If     { cond: String, cmd: String },
    Elif   { cond: String, cmd: String },
    Else   { cmd: String },
    While  { cond: String, cmd: String },
    For    { var: String, in_: String, cmd: String },
    Background(String),
    Call(String),
    /// \\ plugin_name [args] — uruchamia ~/.hackeros/hacker-lang/plugins/<n>
    Plugin { name: String, args: String, is_super: bool },
    Log(String),
    Lock   { key: String, val: String },
    Unlock { key: String },
    /// -- [static] path — linkowanie zewnętrzne
    Extern { path: String, static_link: bool },
    Import { resource: String },
    Enum   { name: String, variants: Vec<String> },
    Struct { name: String, fields: Vec<(String, String)> },
    Try    { try_cmd: String, catch_cmd: String },
    End    { code: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramNode {
    pub line_num:      usize,
    pub is_sudo:       bool,
    pub content:       CommandType,
    pub original_text: String,
    pub span:          (usize, usize),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    pub deps:                  Vec<String>,
    pub libs:                  Vec<LibRef>,
    pub functions:             HashMap<String, (bool, Vec<ProgramNode>)>,
    pub main_body:             Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings:       Vec<String>,
}

// ─────────────────────────────────────────────────────────────
// Błędy
// ─────────────────────────────────────────────────────────────
#[derive(Error, Debug, Diagnostic)]
pub enum ParseError {
    #[error("Błąd składni w linii {line_num}")]
    #[diagnostic(code(hl::syntax_error), url("https://hackeros.dev/docs/hacker-lang/syntax"))]
    SyntaxError {
        #[source_code] src: NamedSource,
        #[label("tutaj")] span: SourceSpan,
        line_num: usize,
        #[help] advice: String,
    },
    #[error("Błąd struktury: {message}")]
    #[diagnostic(code(hl::structure_error))]
    StructureError {
        #[source_code] src: NamedSource,
        #[label("tu")] span: SourceSpan,
        message: String,
    },
    #[error("Nie można otworzyć '{path}': {message}")]
    #[diagnostic(code(hl::io_error))]
    IoError { path: String, message: String },
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────
fn main() {
    let args = Args::parse();
    let mut seen = HashSet::new();
    match parse_file(&args.file, args.resolve_libs, args.verbose, &mut seen) {
        Ok(res) => {
            if args.verbose       { print_summary(&res); }
            if args.check_plugins { check_plugins(&res); }
            if args.json {
                match serde_json::to_string_pretty(&res) {
                    Ok(j)  => println!("{}", j),
                    Err(e) => { eprintln!("{} {e}", "[!]".red()); exit(1); },
                }
            }
        },
        Err(errors) => { print_errors(&errors, &args.file); exit(1); },
    }
}

// ─────────────────────────────────────────────────────────────
// Sprawdzenie pluginów
// ─────────────────────────────────────────────────────────────
fn check_plugins(res: &AnalysisResult) {
    let root = plugins_root();
    let nodes: Vec<&ProgramNode> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Plugin { .. }))
    .collect();
    if nodes.is_empty() { return; }
    eprintln!("{}", "━━━ Pluginy ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".cyan());
    for node in nodes {
        if let CommandType::Plugin { name, args, is_super } = &node.content {
            let bin    = root.join(name);
            let hl     = PathBuf::from(format!("{}.hl", bin.display()));
            let exists = bin.exists() || hl.exists();
            let sym    = if exists { "✓".green() } else { "✗".red() };
            let stag   = if *is_super { " [sudo]".yellow().to_string() } else { String::new() };
            let atag   = if !args.is_empty() { format!(" {}", args.dimmed()) } else { String::new() };
            eprintln!("  {} linia {:>4} — \\\\{}{}{}", sym, node.line_num, name.cyan(), atag, stag);
            if !exists {
                eprintln!("           {} brak: {}", "→".yellow(), bin.display().to_string().yellow());
            }
        }
    }
    eprintln!();
}

// ─────────────────────────────────────────────────────────────
// Wyświetlanie błędów
// ─────────────────────────────────────────────────────────────
fn print_errors(errors: &[ParseError], file: &str) {
    let total = errors.len();
    eprintln!("\n{} {} {} {}\n",
              "✗".red().bold(),
              total.to_string().red().bold(),
              if total == 1 { "błąd składni" } else { "błędów składni" },
                  format!("w {}", file).dimmed(),
    );
    let handler = miette::GraphicalReportHandler::new()
    .with_context_lines(2).with_cause_chain();
    for e in errors.iter().take(20) {
        let mut out = String::new();
        let _ = handler.render_report(&mut out, e as &dyn Diagnostic);
        eprint!("{}", out);
    }
    if total > 20 {
        eprintln!("  {} ... i {} więcej (pokazano pierwsze 20)\n",
                  "~".yellow(), (total - 20).to_string().yellow());
    }
    // unikalne wskazówki
    let mut seen_adv = HashSet::new();
    let unique: Vec<&str> = errors.iter()
    .filter_map(|e| match e { ParseError::SyntaxError { advice, .. } => Some(advice.as_str()), _ => None })
    .filter(|a| seen_adv.insert(*a))
    .collect();
    if !unique.is_empty() {
        eprintln!("{}", "━━━ Wskazówki ━━━━━━━━━━━━━━━━━━━━━━━━━━".yellow());
        for a in unique { eprintln!("  {} {}", "→".yellow(), a); }
        eprintln!();
    }
}

// ─────────────────────────────────────────────────────────────
// Podsumowanie verbose
// ─────────────────────────────────────────────────────────────
fn print_summary(res: &AnalysisResult) {
    eprintln!("{}", "═══════════════════════════════════════════".cyan());
    eprintln!("{}", "  hacker-lang PLSA v8".cyan().bold());
    eprintln!("{}", "═══════════════════════════════════════════".cyan());
    eprintln!("  Funkcje    : {}", res.functions.len().to_string().yellow());
    eprintln!("  Main nodes : {}", res.main_body.len().to_string().yellow());
    eprintln!("  Sys deps   : {}", res.deps.len().to_string().yellow());

    let mut by_type: HashMap<&str, Vec<String>> = HashMap::new();
    for lib in &res.libs {
        let label = match &lib.version {
            Some(v) => format!("{}:{}", lib.name, v),
            None    => lib.name.clone(),
        };
        by_type.entry(lib.lib_type.as_str()).or_default().push(label);
    }
    let mut types: Vec<&str> = by_type.keys().copied().collect();
    types.sort();
    for t in types { eprintln!("  lib/{:<8}: {}", t.magenta(), by_type[t].join(", ")); }

    // pluginy
    let plugin_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Plugin { .. })).collect();
    if !plugin_nodes.is_empty() {
        let root = plugins_root();
        eprintln!("\n{} Pluginy (\\\\):", "[p]".cyan().bold());
        for node in &plugin_nodes {
            if let CommandType::Plugin { name, args, .. } = &node.content {
                let p = root.join(name);
                let e = p.exists() || PathBuf::from(format!("{}.hl", p.display())).exists();
                let a = if args.is_empty() { String::new() } else { format!(" {}", args) };
                eprintln!("    {} \\\\{}{}", if e { "✓".green() } else { "?".yellow() }, name.cyan(), a);
            }
        }
    }

    // extern
    let extern_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Extern { .. })).collect();
    if !extern_nodes.is_empty() {
        eprintln!("\n{} Extern (--):", "[e]".cyan().bold());
        for node in &extern_nodes {
            if let CommandType::Extern { path, static_link } = &node.content {
                let kind = if *static_link { "static".yellow() } else { "dynamic".blue() };
                eprintln!("    [{}] {}", kind, path);
            }
        }
    }

    // unsafe
    let mut unsafe_fns: Vec<&String> = res.functions.iter()
    .filter(|(_, (u, _))| *u).map(|(n, _)| n).collect();
    unsafe_fns.sort();
    if !unsafe_fns.is_empty() {
        eprintln!("\n{} Funkcje unsafe (::):", "[~]".magenta().bold());
        for n in unsafe_fns { eprintln!("    {}", n.magenta()); }
    }
    if res.is_potentially_unsafe {
        eprintln!("\n{} Komendy sudo (^):", "[!]".red().bold());
        for w in &res.safety_warnings { eprintln!("    {}", w.yellow()); }
    }
    eprintln!("{}", "═══════════════════════════════════════════".cyan());
}
