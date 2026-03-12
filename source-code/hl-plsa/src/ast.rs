use miette::{Diagnostic, NamedSource};
pub use miette::SourceSpan;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ─────────────────────────────────────────────────────────────
// LibType / LibRef
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType {
    /// vira  → ~/.hackeros/hacker-lang/libs/.virus/
    Vira,
    /// virus → ~/.hackeros/hacker-lang/libs/.virus/  (alias vira)
    Virus,
    /// bytes → ~/.hackeros/hacker-lang/libs/bytes/
    Bytes,
    /// core  → ~/.hackeros/hacker-lang/libs/core/
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
    pub lib_type:    LibType,
    pub name:        String,
    pub version:     Option<String>,
    /// symbole zaimportowane przez `use [a, b]`; None = wszystkie
    pub use_symbols: Option<Vec<String>>,
}
impl LibRef {
    pub fn cache_key(&self) -> String {
        match &self.version {
            Some(v) => format!("{}/{}/{}", self.lib_type.as_str(), self.name, v),
            None    => format!("{}/{}", self.lib_type.as_str(), self.name),
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Atrybuty funkcji
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FuncAttr {
    pub name: String,
    pub arg:  Option<String>,
}

// ─────────────────────────────────────────────────────────────
// Widoczność
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Visibility { Public, Private }
impl Default for Visibility { fn default() -> Self { Visibility::Private } }

// ─────────────────────────────────────────────────────────────
// CommandType
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommandType {
    // ── Istniejące ────────────────────────────────────────────
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
    Plugin      { name: String, args: String, is_super: bool },
    Log(String),
    Lock        { key: String, val: String },
    Unlock      { key: String },
    Extern      { path: String, static_link: bool, use_symbols: Option<Vec<String>> },
    Import      { resource: String, namespace: Option<String> },
    Enum        { name: String, variants: Vec<String> },
    Struct      { name: String, fields: Vec<(String, String)> },
    Try         { try_cmd: String, catch_cmd: String },
    End         { code: i32 },
    Out(String),
    Const       { key: String, val: String },
    Spawn(String),
    Await(String),
    AssignSpawn { key: String, task: String },
    AssignAwait { key: String, expr: String },
    Assert      { cond: String, msg: Option<String> },
    Match       { cond: String },
    MatchArm    { val: String, cmd: String },
    Pipe(Vec<String>),
    AssignExpr  { key: String, expr: String, is_raw: bool, is_global: bool },
    CollectionMut { var: String, method: String, args: String },
    Interface   { name: String, methods: Vec<String> },
    ImplDef     { class: String, interface: String },
    ArenaDef    { name: String, size: String },
    ResultUnwrap { expr: String, msg: String },
    ModuleCall  { path: String, args: String },
    Lambda      { params: Vec<String>, body: String },
    AssignLambda { key: String, params: Vec<String>, body: String, is_raw: bool, is_global: bool },
    Recur       { args: String },
    DestructList { head: String, tail: String, source: String },
    DestructMap  { fields: Vec<String>, source: String },
    ScopeDef,
    AdtDef      { name: String, variants: Vec<(String, Vec<(String, String)>)> },
    DoBlock     { key: String, body: Vec<ProgramNode> },
    PipeLine    { step: String },
    TestBlock   { desc: String, body: Vec<ProgramNode> },
    Defer       { expr: String },
    FuncDefGeneric { name: String, sig: String },

    // ── NOWE ─────────────────────────────────────────────────

    /// =; name def — otwarcie modułu
    ModuleDef   { name: String, visibility: Visibility },

    /// =; done — zamknięcie modułu
    ModuleDone,

    /// =: ... =: — anonimowy blok scoped (zmienne nie wyciekają)
    ScopeBlock  { body: Vec<ProgramNode> },

    /// # <lib> use [sym1, sym2] — import z wybranymi symbolami
    LibImport   { lib: LibRef },

    /// x = 5 [i32] — przypisanie z adnotacją typu numerycznego
    AssignTyped { key: String, expr: String, type_ann: String, is_raw: bool, is_global: bool },

    /// match $x => ... (fat arrow matching na ADT)
    MatchFat    { cond: String },

    /// Circle [r] => .draw $r — ramię fat-arrow match
    MatchArmFat { variant: String, fields: Vec<String>, cmd: String },

    /// {name, age} => .print $name — destrukturyzacja w match arm
    MatchArmDestructFat { fields: Vec<String>, cmd: String },

    /// *{debug} / *{arch: x86_64} — kompilacja warunkowa otwierająca
    CondComp    { flag: String },

    /// *{end} — zamknięcie bloku kompilacji warunkowej
    CondCompEnd,

    /// |] inline / |] deprecated "msg" — atrybut funkcji
    FuncAttrDecl { attr: FuncAttr },

    /// :func def z atrybutami (scalenie |] + def)
    FuncDefAttrs { name: String, sig: Option<String>, attrs: Vec<FuncAttr>, visibility: Visibility },
}

// ─────────────────────────────────────────────────────────────
// ProgramNode
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
// FunctionMeta — rozszerzone metadane funkcji
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FunctionMeta {
    pub is_arena:    bool,
    pub sig:         Option<String>,
    pub body:        Vec<ProgramNode>,
    pub attrs:       Vec<FuncAttr>,
    pub visibility:  Visibility,
    pub module_path: Vec<String>,   // ścieżka modułu np. ["net", "http"]
}

// ─────────────────────────────────────────────────────────────
// ModuleMeta
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModuleMeta {
    pub name:        String,
    pub visibility:  Visibility,
    pub submodules:  Vec<String>,
    pub functions:   Vec<String>,
}

// ─────────────────────────────────────────────────────────────
// AnalysisResult
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    pub deps:                  Vec<String>,
    pub libs:                  Vec<LibRef>,
    /// klucz: kwalifikowana nazwa funkcji (module::class.name)
    pub functions:             HashMap<String, FunctionMeta>,
    /// stary klucz kompatybilny: (is_arena, sig, nodes)
    #[serde(skip)]
    pub functions_compat:      HashMap<String, (bool, Option<String>, Vec<ProgramNode>)>,
    pub modules:               HashMap<String, ModuleMeta>,
    pub main_body:             Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings:       Vec<String>,
    /// atrybuty oczekujące na następną definicję funkcji
    #[serde(skip)]
    pub pending_attrs:         Vec<FuncAttr>,
    /// flagi kompilacji warunkowej aktualnie aktywne
    #[serde(skip)]
    pub active_cond_flags:     Vec<String>,
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
