use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────
// Biblioteki
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType {
    Source,
    Core,
    Bytes,
    Github,
    Virus,
    Vira,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibRef {
    pub lib_type: LibType,
    pub name:     String,
    pub version:  Option<String>,
}

// ─────────────────────────────────────────────────────────────
// Węzły komend — pełna składnia hacker-lang v9
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommandType {
    // ── ISTNIEJĄCE ────────────────────────────────────────────

    /// Surowa komenda bez podstawienia zmiennych (>> prefix)
    RawNoSub(String),
    /// Surowa komenda z podstawieniem zmiennych (> prefix)
    RawSub(String),
    /// Komenda izolowana w subshell'u: ( cmd )
    Isolated(String),
    /// Przypisanie zmiennej środowiskowej: @KEY = val
    AssignEnv    { key: String, val: String },
    /// Przypisanie zmiennej lokalnej: key = val
    AssignLocal  { key: String, val: String, is_raw: bool },
    /// Pętla n razy: = N > cmd
    Loop         { count: u64, cmd: String },
    /// Warunek: ? cond > cmd
    If           { cond: String, cmd: String },
    /// Warunek elif: ?? cond > cmd
    Elif         { cond: String, cmd: String },
    /// Gałąź else: ?: > cmd
    Else         { cmd: String },
    /// Pętla while: while cond |> cmd
    While        { cond: String, cmd: String },
    /// Pętla for: for var in expr |> cmd
    For          { var: String, in_: String, cmd: String },
    /// Tło: & cmd
    Background(String),

    /// Wywołanie funkcji HL: .func_name [args]
    /// Zmiana: teraz ma osobne args (poprzednio Call(String))
    Call         { path: String, args: String },

    /// Plugin z ~/.hackeros/hacker-lang/plugins/: \\ plugin [args]
    Plugin       { name: String, args: String, is_super: bool },
    /// Wypisz wiadomość: log "msg"
    Log(String),
    /// Zaalokuj blok pamięci na stercie GC: lock $key = size
    Lock         { key: String, val: String },
    /// Zwolnij blok pamięci: unlock $key
    Unlock       { key: String },
    /// Dołącz zewnętrzną bibliotekę .so/.a: -- [static] path
    Extern       { path: String, static_link: bool },
    /// Deklaracja enum: == Name [A, B, C]
    Enum         { name: String, variants: Vec<String> },
    /// Import zasobu: << "path" [in ns]
    /// Zmiana: teraz ma opcjonalny namespace
    Import       { resource: String, namespace: Option<String> },
    /// Deklaracja struct: struct Name [field: type, ...]
    Struct       { name: String, fields: Vec<(String, String)> },
    /// Try/catch: try cmd catch cmd
    Try          { try_cmd: String, catch_cmd: String },
    /// Zakończ program: end [N]
    End          { code: i32 },
    /// Zwróć wartość z funkcji: out val
    Out(String),

    // ── NOWE v9 ───────────────────────────────────────────────

    /// % KEY = val — stała (niezmienne przez konwencję)
    Const        { key: String, val: String },

    /// spawn rest — uruchom zadanie asynchronicznie (fire & forget)
    Spawn(String),

    /// await rest — czekaj na wynik zadania (bez przypisania)
    Await(String),

    /// key = spawn rest — uruchom i przypisz PID do zmiennej
    AssignSpawn  { key: String, task: String },

    /// key = await rest — czekaj i przypisz wynik do zmiennej
    AssignAwait  { key: String, expr: String },

    /// assert cond [msg] — walidacja w miejscu, exit 1 przy błędzie
    Assert       { cond: String, msg: Option<String> },

    /// match cond |> — nagłówek bloku dopasowania wzorców
    Match        { cond: String },

    /// val > cmd — pojedyncze ramię match
    MatchArm     { val: String, cmd: String },

    /// .a |> .b |> .c — łańcuch wywołań
    Pipe(Vec<String>),
}

// ─────────────────────────────────────────────────────────────
// Węzeł programu
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramNode {
    pub line_num:      usize,
    pub is_sudo:       bool,
    pub content:       CommandType,
    pub original_text: String,
    pub span:          (usize, usize),
}

// ─────────────────────────────────────────────────────────────
// Wynik analizy
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub deps:                  Vec<String>,
    pub libs:                  Vec<LibRef>,
    /// Trójka: (is_unsafe, Option<type_sig>, Vec<ProgramNode>)
    pub functions:             HashMap<String, (bool, Option<String>, Vec<ProgramNode>)>,
    pub main_body:             Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings:       Vec<String>,
}
