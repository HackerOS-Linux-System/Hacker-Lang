use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType {
    Source,
    Core,
    Bytes,
    Github,
    Virus,
    Vira,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibRef {
    pub lib_type: LibType,
    pub name:     String,
    pub version:  Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommandType {
    // ── ISTNIEJĄCE ────────────────────────────────────────────
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

    /// Zmiana: Call ma teraz path + args (poprzednio Call(String))
    Call        { path: String, args: String },

    Plugin      { name: String, args: String, is_super: bool },
    Log(String),
    Lock        { key: String, val: String },
    Unlock      { key: String },
    Extern      { path: String, static_link: bool },

    /// Zmiana: Import ma teraz opcjonalny namespace (in ident)
    Import      { resource: String, namespace: Option<String> },

    Enum        { name: String, variants: Vec<String> },
    Struct      { name: String, fields: Vec<(String, String)> },
    Try         { try_cmd: String, catch_cmd: String },
    End         { code: i32 },
    Out(String),

    // ── NOWE ─────────────────────────────────────────────────

    /// % KEY = val — stała (niezmienne przez konwencję)
    Const       { key: String, val: String },

    /// spawn rest — uruchom zadanie asynchronicznie (fire & forget)
    Spawn(String),

    /// await rest — czekaj na wynik zadania (bez przypisania)
    Await(String),

    /// key = spawn rest — uruchom i przypisz PID/handle do zmiennej
    AssignSpawn { key: String, task: String },

    /// key = await rest — czekaj i przypisz wynik do zmiennej
    AssignAwait { key: String, expr: String },

    /// assert cond [msg] — walidacja w miejscu, exit 1 przy błędzie
    Assert      { cond: String, msg: Option<String> },

    /// match cond |> — nagłówek bloku dopasowania wzorców
    Match       { cond: String },

    /// val > cmd — pojedyncze ramię match
    MatchArm    { val: String, cmd: String },

    /// .a |> .b |> .c — łańcuch wywołań (pipe)
    Pipe(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProgramNode {
    pub line_num:      usize,
    pub is_sudo:       bool,
    pub content:       CommandType,
    pub original_text: String,
    pub span:          (usize, usize),
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisResult {
    pub deps:                  Vec<String>,
    pub libs:                  Vec<LibRef>,
    /// Trójka: (is_unsafe, Option<type_sig>, Vec<ProgramNode>)
    /// type_sig — opcjonalna sygnatura typów np. "[int, int -> int]"
    pub functions:             HashMap<String, (bool, Option<String>, Vec<ProgramNode>)>,
    pub main_body:             Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings:       Vec<String>,
}
