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
#[command(
name    = "hl-plsa",
author  = "HackerOS",
version = env!("CARGO_PKG_VERSION"),
          about   = "hacker-lang static analyser"
)]
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
pub enum LibType {
    // ── Obsługiwane typy repozytoriów ────────────────────────
    /// vira  → ~/.hackeros/hacker-lang/libs/.virus/  (repo git)
    Vira,
    /// virus → ~/.hackeros/hacker-lang/libs/.virus/  (alias vira)
    Virus,
    /// bytes → ~/.hackeros/hacker-lang/libs/bytes/  (pliki .so)
    Bytes,
    /// core  → ~/.hackeros/hacker-lang/libs/core/   (pliki .hl)
    Core,
}
impl LibType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LibType::Vira  => "vira",
            LibType::Virus => "virus",
            LibType::Bytes => "bytes",
            LibType::Core  => "core",
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
    // ── ISTNIEJĄCE — BEZ ZMIAN ───────────────────────────────────
    RawNoSub(String),
    RawSub(String),
    Isolated(String),
    AssignEnv   { key: String, val: String },
    AssignLocal { key: String, val: String, is_raw: bool },
    Loop        { count: u64, cmd: String },
    If          { cond: String, cmd: String },
    Elif        { cond: String, cmd: String },
    Else        { cmd: String },
    While       { cond: String, cmd: String },
    For         { var: String, in_: String, cmd: String },
    Background(String),
    Call        { path: String, args: String },
    /// \\ plugin_name [args] — uruchamia ~/.hackeros/hacker-lang/plugins/<n>
    Plugin      { name: String, args: String, is_super: bool },
    Log(String),
    Lock        { key: String, val: String },
    Unlock      { key: String },
    /// -- [static] path — linkowanie zewnętrzne
    Extern      { path: String, static_link: bool },
    Import      { resource: String, namespace: Option<String> },
    Enum        { name: String, variants: Vec<String> },
    Struct      { name: String, fields: Vec<(String, String)> },
    Try         { try_cmd: String, catch_cmd: String },
    End         { code: i32 },
    Out(String),

    // ── NOWE (spawn/await/assert/match/pipe/const) ────────────────

    /// % KEY = val — stała (niezmienne przez konwencję)
    Const       { key: String, val: String },

    /// spawn rest — uruchom zadanie asynchronicznie, zwróć handle
    Spawn(String),

    /// await rest — czekaj na wynik (bez przypisania)
    Await(String),

    /// key = spawn rest — uruchom i przypisz handle do zmiennej
    AssignSpawn { key: String, task: String },

    /// key = await rest — czekaj i przypisz wynik do zmiennej
    AssignAwait { key: String, expr: String },

    /// assert cond [msg] — walidacja w miejscu
    Assert      { cond: String, msg: Option<String> },

    /// match cond |> — nagłówek bloku dopasowania
    Match       { cond: String },

    /// val > cmd — ramię match
    MatchArm    { val: String, cmd: String },

    /// .a |> .b |> .c — łańcuch wywołań
    Pipe(Vec<String>),

    // ── NOWE: system typów i wyrażenia ───────────────────────────

    /// key = expr — przypisanie z wyrażeniem arytmetycznym/logicznym
    /// np. x = 2 + 3 * 4  |  y = $x > 10  |  z = [$a, $b]  |  m = {k: "v"}
    /// Obsługuje też interpolację wyrażeń: "Wynik: $(2 + 3)"
    AssignExpr  { key: String, expr: String, is_raw: bool, is_global: bool },

    /// $list.push 42  /  $map.set "key" "val"  — mutacja kolekcji
    CollectionMut { var: String, method: String, args: String },

    // ── NOWE: interfejsy / protokoły ─────────────────────────────

    /// ==interface Serializable [to_json, from_json]
    Interface   { name: String, methods: Vec<String> },

    /// ;;Config impl Serializable def
    ImplDef     { class: String, interface: String },

    // ── NOWE: arena allocator ─────────────────────────────────────

    /// :: nazwa [rozmiar] def — funkcja z arena allocatorem
    /// np.  :: cache [512kb] def
    ArenaDef    { name: String, size: String },

    // ── NOWE: error handling jako wartość ─────────────────────────

    /// expr ?! "komunikat błędu" — unwrap lub panik z komunikatem (jak Rust ?)
    ResultUnwrap { expr: String, msg: String },

    /// wywołanie metody modułu: http.get "url"
    ModuleCall  { path: String, args: String },

    // ── NOWE: domknięcia / lambdy ─────────────────────────────────

    /// { $x -> $x * 2 } — domknięcie (standalone, np. jako argument inline)
    Lambda      { params: Vec<String>, body: String },

    /// callback = { $x -> $x * 2 } — przypisanie lambdy do zmiennej
    AssignLambda { key: String, params: Vec<String>, body: String, is_raw: bool, is_global: bool },

    // ── NOWE: rekurencja ogonowa ──────────────────────────────────

    /// recur ($1 - 1) — wywołanie ogonowe bieżącej funkcji
    Recur       { args: String },

    // ── NOWE: destrukturyzacja ────────────────────────────────────

    /// [head | tail] = $lista — destrukturyzacja listy
    DestructList { head: String, tail: String, source: String },

    /// {name, age} = $user — destrukturyzacja mapy/struktury
    DestructMap  { fields: Vec<String>, source: String },

    // ── NOWE: zasięg leksykalny ──────────────────────────────────

    /// ;;scope def — anonimowy zakres leksykalny
    ScopeDef,

    // ── NOWE: typy algebraiczne (ADT) ────────────────────────────

    /// ==type Shape [ Circle [radius: float], Rect [w: float, h: float], Point ]
    AdtDef      { name: String, variants: Vec<(String, Vec<(String, String)>)> },

    // ── NOWE: do-notacja ─────────────────────────────────────────

    /// result = do ... done — blok sekwencyjny (jak do-notacja Haskell)
    DoBlock     { key: String, body: Vec<ProgramNode> },

    /// | .step args — krok wieloliniowego potoku
    PipeLine    { step: String },

    // ── NOWE: testy jednostkowe ──────────────────────────────────

    /// ==test "opis" [ assert ... ] — blok testowy jako pierwsza klasa
    TestBlock   { desc: String, body: Vec<ProgramNode> },

    // ── NOWE: defer ──────────────────────────────────────────────

    /// defer .file.close $f — sprzątanie zasobów przy wyjściu ze scope
    Defer       { expr: String },

    // ── NOWE: generics z constraints ─────────────────────────────

    /// :serialize [T impl Serializable -> str] def — funkcja z ograniczeniem generycznym
    FuncDefGeneric { name: String, sig: String },
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
    pub functions:             HashMap<String, (bool, Option<String>, Vec<ProgramNode>)>,
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
    #[diagnostic(
    code(hl::syntax_error),
                 url("https://hackeros-linux-system.github.io/HackerOS-Website/hacker-lang/docs.html")
    )]
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
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
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
    .with_context_lines(2)
    .with_cause_chain();
    for e in errors.iter().take(20) {
        let mut out = String::new();
        let _ = handler.render_report(&mut out, e as &dyn Diagnostic);
        eprint!("{}", out);
    }
    if total > 20 {
        eprintln!("  {} ... i {} więcej (pokazano pierwsze 20)\n",
                  "~".yellow(), (total - 20).to_string().yellow());
    }
    let mut seen_adv = HashSet::new();
    let unique: Vec<&str> = errors.iter()
    .filter_map(|e| match e {
        ParseError::SyntaxError { advice, .. } => Some(advice.as_str()),
                _ => None,
    })
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
    eprintln!("{}", "  hacker-lang PLSA".cyan().bold());
    eprintln!("{}", "═══════════════════════════════════════════".cyan());
    eprintln!("  Funkcje    : {}", res.functions.len().to_string().yellow());
    eprintln!("  Main nodes : {}", res.main_body.len().to_string().yellow());
    eprintln!("  Sys deps   : {}", res.deps.len().to_string().yellow());

    // biblioteki pogrupowane po typie
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

    // interfejsy
    let iface_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Interface { .. }))
    .collect();
    if !iface_nodes.is_empty() {
        eprintln!("\n{} Interfejsy (==interface):", "[i]".cyan().bold());
        for node in &iface_nodes {
            if let CommandType::Interface { name, methods } = &node.content {
                eprintln!("    {} [{}]", name.cyan(), methods.join(", ").yellow());
            }
        }
    }

    // impl
    let impl_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::ImplDef { .. }))
    .collect();
    if !impl_nodes.is_empty() {
        eprintln!("\n{} Implementacje (impl):", "[I]".cyan().bold());
        for node in &impl_nodes {
            if let CommandType::ImplDef { class, interface } = &node.content {
                eprintln!("    {} impl {}", class.cyan(), interface.yellow());
            }
        }
    }

    // arena
    let arena_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::ArenaDef { .. }))
    .collect();
    if !arena_nodes.is_empty() {
        eprintln!("\n{} Arena allocator (::):", "[A]".magenta().bold());
        for node in &arena_nodes {
            if let CommandType::ArenaDef { name, size } = &node.content {
                eprintln!("    linia {:>4} — :: {} [{}]", node.line_num, name.cyan(), size.yellow());
            }
        }
    }

    // stałe %
    let const_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Const { .. }))
    .collect();
    if !const_nodes.is_empty() {
        eprintln!("\n{} Stałe (%):", "[%]".yellow().bold());
        for node in &const_nodes {
            if let CommandType::Const { key, val } = &node.content {
                eprintln!("    %{} = {}", key.yellow(), val);
            }
        }
    }

    // wyrażenia
    let expr_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::AssignExpr { .. }))
    .collect();
    if !expr_nodes.is_empty() {
        eprintln!("\n{} Wyrażenia (=expr):", "[=]".green().bold());
        for node in &expr_nodes {
            if let CommandType::AssignExpr { key, expr, .. } = &node.content {
                eprintln!("    linia {:>4} — {} = {}", node.line_num, key.yellow(), expr.cyan());
            }
        }
    }

    // kolekcje — mutacje
    let col_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::CollectionMut { .. }))
    .collect();
    if !col_nodes.is_empty() {
        eprintln!("\n{} Mutacje kolekcji:", "[c]".blue().bold());
        for node in &col_nodes {
            if let CommandType::CollectionMut { var, method, args } = &node.content {
                eprintln!("    linia {:>4} — ${}.{} {}", node.line_num, var.cyan(), method.yellow(), args);
            }
        }
    }

    // result unwrap ?!
    let unwrap_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::ResultUnwrap { .. }))
    .collect();
    if !unwrap_nodes.is_empty() {
        eprintln!("\n{} Result unwrap (?!):", "[?]".red().bold());
        for node in &unwrap_nodes {
            if let CommandType::ResultUnwrap { expr, msg } = &node.content {
                eprintln!("    linia {:>4} — {} ?! \"{}\"", node.line_num, expr.cyan(), msg.yellow());
            }
        }
    }

    // match
    let match_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Match { .. }))
    .collect();
    if !match_nodes.is_empty() {
        eprintln!("\n{} Match statements: {}", "[m]".cyan().bold(), match_nodes.len().to_string().yellow());
    }

    // assert
    let assert_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Assert { .. }))
    .collect();
    if !assert_nodes.is_empty() {
        eprintln!("\n{} Assert statements:", "[a]".green().bold());
        for node in &assert_nodes {
            if let CommandType::Assert { cond, msg } = &node.content {
                let m = msg.as_deref().unwrap_or("(brak komunikatu)");
                eprintln!("    linia {:>4} — {} → \"{}\"", node.line_num, cond.cyan(), m);
            }
        }
    }

    // spawn/await
    let async_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content,
                         CommandType::Spawn(_)           |
                         CommandType::Await(_)           |
                         CommandType::AssignSpawn { .. } |
                         CommandType::AssignAwait { .. }
    ))
    .collect();
    if !async_nodes.is_empty() {
        eprintln!("\n{} Async (spawn/await): {}", "[~]".blue().bold(), async_nodes.len().to_string().yellow());
        for node in &async_nodes {
            match &node.content {
                CommandType::AssignSpawn { key, task } =>
                eprintln!("    linia {:>4} — {} = spawn {}", node.line_num, key.yellow(), task.cyan()),
                CommandType::AssignAwait { key, expr } =>
                eprintln!("    linia {:>4} — {} = await {}", node.line_num, key.yellow(), expr.cyan()),
                CommandType::Spawn(r) =>
                eprintln!("    linia {:>4} — spawn {}", node.line_num, r.cyan()),
                CommandType::Await(r) =>
                eprintln!("    linia {:>4} — await {}", node.line_num, r.cyan()),
                _ => {},
            }
        }
    }

    // pipe
    let pipe_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Pipe(_)))
    .collect();
    if !pipe_nodes.is_empty() {
        eprintln!("\n{} Pipe chains:", "[|]".magenta().bold());
        for node in &pipe_nodes {
            if let CommandType::Pipe(steps) = &node.content {
                eprintln!("    linia {:>4} — {} kroków: {}",
                          node.line_num,
                          steps.len().to_string().yellow(),
                          steps.join(" |> ").cyan()
                );
            }
        }
    }

    // pluginy
    let plugin_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Plugin { .. }))
    .collect();
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
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Extern { .. }))
    .collect();
    if !extern_nodes.is_empty() {
        eprintln!("\n{} Extern (--):", "[e]".cyan().bold());
        for node in &extern_nodes {
            if let CommandType::Extern { path, static_link } = &node.content {
                let kind = if *static_link { "static".yellow() } else { "dynamic".blue() };
                eprintln!("    [{}] {}", kind, path);
            }
        }
    }

    // funkcje arena (::)
    let mut arena_fns: Vec<&String> = res.functions.iter()
    .filter(|(_, (u, _, _))| *u)
    .map(|(n, _)| n)
    .collect();
    arena_fns.sort();
    if !arena_fns.is_empty() {
        eprintln!("\n{} Funkcje arena (::):", "[A]".magenta().bold());
        for n in arena_fns { eprintln!("    {}", n.magenta()); }
    }

    // funkcje z sygnaturami typów
    let typed_fns: Vec<(&String, &Option<String>)> = res.functions.iter()
    .filter(|(_, (_, sig, _))| sig.is_some())
    .map(|(n, (_, sig, _))| (n, sig))
    .collect();
    if !typed_fns.is_empty() {
        eprintln!("\n{} Funkcje z typami:", "[t]".green().bold());
        let mut sorted: Vec<_> = typed_fns;
        sorted.sort_by_key(|(n, _)| n.as_str());
        for (name, sig) in sorted {
            eprintln!("    {} {}", name.cyan(), sig.as_deref().unwrap_or("").yellow());
        }
    }

    // sudo
    if res.is_potentially_unsafe {
        eprintln!("\n{} Komendy sudo (^):", "[!]".red().bold());
        for w in &res.safety_warnings { eprintln!("    {}", w.yellow()); }
    }

    // ── NOWE sekcje verbose ────────────────────────────────────────────────────

    // lambdy / domknięcia
    let lambda_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Lambda { .. } | CommandType::AssignLambda { .. }))
    .collect();
    if !lambda_nodes.is_empty() {
        eprintln!("\n{} Lambdy / domknięcia:", "[λ]".magenta().bold());
        for node in &lambda_nodes {
            match &node.content {
                CommandType::AssignLambda { key, params, body, .. } =>
                eprintln!("    linia {:>4} — {} = {{ {} -> {} }}",
                          node.line_num, key.yellow(),
                          params.join(", ").cyan(), body.dimmed()),
                          CommandType::Lambda { params, body } =>
                          eprintln!("    linia {:>4} — {{ {} -> {} }}",
                                    node.line_num,
                                    params.join(", ").cyan(), body.dimmed()),
                                    _ => {},
            }
        }
    }

    // rekurencja ogonowa (recur)
    let recur_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Recur { .. }))
    .collect();
    if !recur_nodes.is_empty() {
        eprintln!("\n{} Rekurencja ogonowa (recur): {}", "[r]".cyan().bold(), recur_nodes.len().to_string().yellow());
        for node in &recur_nodes {
            if let CommandType::Recur { args } = &node.content {
                eprintln!("    linia {:>4} — recur {}", node.line_num, args.cyan());
            }
        }
    }

    // destrukturyzacja
    let destruct_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::DestructList { .. } | CommandType::DestructMap { .. }))
    .collect();
    if !destruct_nodes.is_empty() {
        eprintln!("\n{} Destrukturyzacja:", "[d]".blue().bold());
        for node in &destruct_nodes {
            match &node.content {
                CommandType::DestructList { head, tail, source } =>
                eprintln!("    linia {:>4} — [{} | {}] = {}",
                          node.line_num, head.yellow(), tail.yellow(), source.cyan()),
                          CommandType::DestructMap { fields, source } =>
                          eprintln!("    linia {:>4} — {{{}}} = {}",
                                    node.line_num, fields.join(", ").yellow(), source.cyan()),
                                    _ => {},
            }
        }
    }

    // zasięg leksykalny (;;scope)
    let scope_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::ScopeDef))
    .collect();
    if !scope_nodes.is_empty() {
        eprintln!("\n{} Zasięgi leksykalne (;;scope): {}", "[s]".green().bold(), scope_nodes.len().to_string().yellow());
    }

    // ADT (==type)
    let adt_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::AdtDef { .. }))
    .collect();
    if !adt_nodes.is_empty() {
        eprintln!("\n{} Typy algebraiczne (==type):", "[T]".magenta().bold());
        for node in &adt_nodes {
            if let CommandType::AdtDef { name, variants } = &node.content {
                let vs: Vec<String> = variants.iter().map(|(vn, fields)| {
                    if fields.is_empty() {
                        vn.clone()
                    } else {
                        let fs: Vec<String> = fields.iter().map(|(f, t)| format!("{}: {}", f, t)).collect();
                        format!("{}[{}]", vn, fs.join(", "))
                    }
                }).collect();
                eprintln!("    {} → {}", name.cyan().bold(), vs.join(" | ").yellow());
            }
        }
    }

    // do-bloki
    let do_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::DoBlock { .. }))
    .collect();
    if !do_nodes.is_empty() {
        eprintln!("\n{} Do-bloki:", "[do]".cyan().bold());
        for node in &do_nodes {
            if let CommandType::DoBlock { key, body } = &node.content {
                eprintln!("    linia {:>4} — {} = do ({} kroków)",
                          node.line_num, key.yellow(), body.len().to_string().cyan());
            }
        }
    }

    // wieloliniowe pipe (|)
    let pline_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::PipeLine { .. }))
    .collect();
    if !pline_nodes.is_empty() {
        eprintln!("\n{} Kroki potoku (|):", "[»]".blue().bold());
        for node in &pline_nodes {
            if let CommandType::PipeLine { step } = &node.content {
                eprintln!("    linia {:>4} — | {}", node.line_num, step.cyan());
            }
        }
    }

    // testy jednostkowe
    let test_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::TestBlock { .. }))
    .collect();
    if !test_nodes.is_empty() {
        eprintln!("\n{} Testy jednostkowe (==test):", "[✓]".green().bold());
        for node in &test_nodes {
            if let CommandType::TestBlock { desc, body } = &node.content {
                let asserts = body.iter().filter(|n| matches!(&n.content, CommandType::Assert { .. })).count();
                eprintln!("    linia {:>4} — \"{}\" ({} assert{})",
                          node.line_num, desc.yellow(), asserts.to_string().cyan(),
                          if asserts == 1 { "" } else { "y" });
            }
        }
    }

    // defer
    let defer_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::Defer { .. }))
    .collect();
    if !defer_nodes.is_empty() {
        eprintln!("\n{} Defer (cleanup):", "[↩]".yellow().bold());
        for node in &defer_nodes {
            if let CommandType::Defer { expr } = &node.content {
                eprintln!("    linia {:>4} — defer {}", node.line_num, expr.cyan());
            }
        }
    }

    // generics z constraints
    let generic_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|(_, _, n)| n.iter()))
    .filter(|n| matches!(&n.content, CommandType::FuncDefGeneric { .. }))
    .collect();
    if !generic_nodes.is_empty() {
        eprintln!("\n{} Generics z constraints:", "[G]".cyan().bold());
        for node in &generic_nodes {
            if let CommandType::FuncDefGeneric { name, sig } = &node.content {
                eprintln!("    linia {:>4} — :{} {}", node.line_num, name.cyan(), sig.yellow());
            }
        }
    }

    eprintln!("{}", "═══════════════════════════════════════════".cyan());
}
