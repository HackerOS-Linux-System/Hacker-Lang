use serde::{Deserialize, Serialize};

/// Typ zmiennej (gen 2 — typowane zmienne)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VarType {
    String, Int, Float, Bool, List, Map,
    Any,   // brak adnotacji — domyślne zachowanie
}

impl VarType {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "str" | "string" => VarType::String,
            "int"            => VarType::Int,
            "float"          => VarType::Float,
            "bool"           => VarType::Bool,
            "list"           => VarType::List,
            "map"            => VarType::Map,
            _                => VarType::Any,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    pub pattern: String,
    pub body:    Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HackerOsTool {
    HHash, Hco, Hacker, Hsh, Hpkg, BlueEnvironment, Hnm, Hpm, Hedit,
    Ngt, Eiq, Getit, Hdev, Anvil, A, Hbuild, Lpm, Chker, Isolator,
    HackerOsSteam, Ulb, Gameframe, Hup, HackerOsBuilder, Other(String),
}

impl HackerOsTool {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "H#"               => HackerOsTool::HHash,
            "hco"              => HackerOsTool::Hco,
            "hacker"           => HackerOsTool::Hacker,
            "hsh"              => HackerOsTool::Hsh,
            "hpkg"             => HackerOsTool::Hpkg,
            "Blue-Environment" => HackerOsTool::BlueEnvironment,
            "hnm"              => HackerOsTool::Hnm,
            "hpm"              => HackerOsTool::Hpm,
            "hedit"            => HackerOsTool::Hedit,
            "ngt"              => HackerOsTool::Ngt,
            "eiq"              => HackerOsTool::Eiq,
            "getit"            => HackerOsTool::Getit,
            "hdev"             => HackerOsTool::Hdev,
            "anvil"            => HackerOsTool::Anvil,
            "a"                => HackerOsTool::A,
            "hbuild"           => HackerOsTool::Hbuild,
            "lpm"              => HackerOsTool::Lpm,
            "chker"            => HackerOsTool::Chker,
            "isolator"         => HackerOsTool::Isolator,
            "hackeros-steam"   => HackerOsTool::HackerOsSteam,
            "ulb"              => HackerOsTool::Ulb,
            "gameframe"        => HackerOsTool::Gameframe,
            "hup"              => HackerOsTool::Hup,
            "hackeros-builder" => HackerOsTool::HackerOsBuilder,
            other              => HackerOsTool::Other(other.to_string()),
        }
    }

    pub fn binary_name(&self) -> &str {
        match self {
            HackerOsTool::HHash           => "H#",
            HackerOsTool::Hco             => "hco",
            HackerOsTool::Hacker          => "hacker",
            HackerOsTool::Hsh             => "hsh",
            HackerOsTool::Hpkg            => "hpkg",
            HackerOsTool::BlueEnvironment => "Blue-Environment",
            HackerOsTool::Hnm             => "hnm",
            HackerOsTool::Hpm             => "hpm",
            HackerOsTool::Hedit           => "hedit",
            HackerOsTool::Ngt             => "ngt",
            HackerOsTool::Eiq             => "eiq",
            HackerOsTool::Getit           => "getit",
            HackerOsTool::Hdev            => "hdev",
            HackerOsTool::Anvil           => "anvil",
            HackerOsTool::A               => "a",
            HackerOsTool::Hbuild          => "hbuild",
            HackerOsTool::Lpm             => "lpm",
            HackerOsTool::Chker           => "chker",
            HackerOsTool::Isolator        => "isolator",
            HackerOsTool::HackerOsSteam   => "hackeros-steam",
            HackerOsTool::Ulb             => "ulb",
            HackerOsTool::Gameframe       => "gameframe",
            HackerOsTool::Hup             => "hup",
            HackerOsTool::HackerOsBuilder => "hackeros-builder",
            HackerOsTool::Other(s)        => s.as_str(),
        }
    }
}

// ── Arena Function (gen 2) ────────────────────────────────────────────────────
//
// Składnia definicji:
//   :: nazwa_funkcji <rozmiar_areny> def
//       kod...
//   done
//
// Składnia wywołania:
//   :: nazwa_funkcji [args...]
//
// Arena allocator: przed wywołaniem alokowany jest jeden ciągły blok pamięci
// o zadanym rozmiarze. Wszystkie alokacje wewnątrz funkcji pobierają z tej areny
// przez bump pointer — zero syscalls podczas działania funkcji.
// Po powrocie z funkcji cała arena jest zwalniana jednym free().
//
// Rozmiar areny: liczba bajtów lub skróty (4k = 4096, 1m = 1048576)
// Domyślny rozmiar gdy pominięty: 4096 B

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArenaSize(pub usize);

impl ArenaSize {
    pub fn default_size() -> Self { ArenaSize(4096) }

    /// Parsuj rozmiar areny ze stringa: "4096", "4k", "64k", "1m", "256"
    pub fn parse(s: &str) -> Self {
        let s = s.trim().to_lowercase();
        if s.is_empty() { return Self::default_size(); }
        if let Some(num) = s.strip_suffix('k') {
            if let Ok(n) = num.trim().parse::<usize>() { return ArenaSize(n * 1024); }
        }
        if let Some(num) = s.strip_suffix('m') {
            if let Ok(n) = num.trim().parse::<usize>() { return ArenaSize(n * 1024 * 1024); }
        }
        if let Ok(n) = s.parse::<usize>() { return ArenaSize(n); }
        Self::default_size()
    }

    pub fn bytes(&self) -> usize { self.0 }
}

impl std::fmt::Display for ArenaSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = self.0;
        if b >= 1024*1024 && b%(1024*1024)==0 { write!(f, "{}m", b/(1024*1024)) }
        else if b >= 1024 && b%1024==0         { write!(f, "{}k", b/1024) }
        else                                   { write!(f, "{}", b) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    Print { parts: Vec<StringPart> },

    // ── :: operator ───────────────────────────────────────────────────────────
    //
    // gen 1: :: nazwa args
    //   → wywołanie wbudowanej quick-function (upper/lower/exists/red/green itd.)
    //   → zachowane dla kompatybilności wstecznej
    //
    // gen 2: :: nazwa args
    //   → jeśli nazwa jest zdefiniowaną arena function → ArenaFuncCall (wydajne)
    //   → jeśli nie → fallback do wbudowanych quick-calls (kompatybilność)
    //   Definicja: :: nazwa <rozmiar> def ... done → ArenaFuncDef
    //
    // QuickCall: wbudowane funkcje (gen 1 + fallback gen 2)
    QuickCall { name: String, args: Vec<StringPart> },
    /// :: name args |> @var  — QuickCall z przechwyceniem stdout do zmiennej
    QuickPipeToVar { name: String, args: Vec<StringPart>, var_name: String },

    Command     { raw: String, mode: CommandMode, interpolate: bool },
    HshCommand  { raw: String },
    Background  { raw: String },
    RepeatN     { count: u64, body: Vec<Node> },
    VarDecl     { name: String, typ: VarType, value: VarValue },
    Export      { name: String, value: ExportValue },
    VarRef      (String),
    /// Deklaracja zależności narzędzia:
    ///   // curl              → name="curl", apt_package=None   (apt szuka "curl")
    ///   // ninja [ninja-build] → name="ninja", apt_package=Some("ninja-build")
    Dependency  { name: String, apt_package: Option<String> },
    Import      { lib: String, detail: Option<String> },
    FileImport  { path: String, detail: Option<String> },

    // <* katalog — import katalogu (gen 2)
    // Ładuje katalog/imports.hl (odpowiednik mod.rs w Rust)
    // imports.hl zawiera listę << plik dla każdego pliku w module
    DirImport   { path: String },

    // Zwykła funkcja (gen 1+2): : nazwa def ... done
    FuncDef     { name: String, body: Vec<Node> },
    FuncCall    { name: String },

    // Arena function (gen 2): :: nazwa <rozmiar> def ... done
    //
    // Executor alokuje arena_size bajtów jako bump-pointer arena przed wejściem.
    // Wszystkie zmienne lokalne funkcji tworzone są w tej arenie.
    // Po zakończeniu: jeden dealloc. Brak GC pressure, zero heap fragmentation.
    // Idealne dla: string processing, math loops, parsowanie, transformacje danych.
    ArenaFuncDef  { name: String, arena_size: ArenaSize, body: Vec<Node> },
    ArenaFuncCall { name: String, args: Vec<StringPart> },

    Conditional { condition: ConditionKind, body: Vec<Node> },
    ForIn       { var: String, iterable: Vec<StringPart>, body: Vec<Node> },
    WhileLoop   { condition: Vec<StringPart>, body: Vec<Node> },
    MatchExpr   { subject: Vec<StringPart>, arms: Vec<MatchArm> },
    Arithmetic  { expr: String, assign_to: Option<String> },
    PipeToVar   { command: String, mode: CommandMode, var_name: String },
    HackerOsApi { tool: HackerOsTool, args: Vec<StringPart> },
    Goroutine   { name: Option<String>, body: Vec<Node> },
    ChannelOp   { name: String, value: Option<Vec<StringPart>> },
    Channel     { name: String },
    BlockComment(String),
    DocComment  (String),
    LineComment  (String),
    Block       (Vec<Node>),

    // ── extern system (gen 2+) ────────────────────────────────────────────────
    // _> plik [runtime] def ... done
    // Uruchamia zewnętrzny runtime (shell/python/java/elf/so)
    ExternDef {
        /// Nazwa/ścieżka pliku: "myscript.sh", "tool.py", "/usr/bin/mytool", "libfoo.so"
        file:    String,
        /// Runtime: Shell | Python | Java | Elf | So
        runtime: ExternRuntime,
        /// Ciało bloku — instrukcje HL przekazywane do środowiska
        body:    Vec<Node>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportValue {
    Single(Vec<StringPart>),
    List(Vec<Vec<StringPart>>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommandMode {
    Plain, Sudo, Isolated, IsolatedSudo,
    WithVars, WithVarsSudo, WithVarsIsolated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionKind { Ok, Err }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StringPart {
    Literal(String),
    /// Prosta zmienna: @nazwa → get_var("nazwa")
    Var(String),
    /// Dynamiczna referencja: @{nazwa@_i} → get_var(resolve(nazwa@_i))
    /// Parsowana ze składni @{...}
    DynVar(Vec<StringPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VarValue {
    String(String),
    Number(f64),
    Bool(bool),
    CmdOutput(String),
    Interpolated(Vec<StringPart>),
    Int(i64),
    Float(f64),
    List(Vec<VarValue>),
    Map(Vec<(String, VarValue)>),
    Arithmetic(String),
}

impl Node {
    pub fn is_comment(&self) -> bool {
        matches!(self, Node::LineComment(_) | Node::DocComment(_) | Node::BlockComment(_))
    }
}

/// Parsuj string interpolowany ze zmiennymi (@var) i dynamicznymi referencjami (@{expr}).
///
/// Obsługuje:
///   @nazwa          → StringPart::Var("nazwa")
///   @{arg@_i}       → StringPart::DynVar([Var("arg"), Var("_i")]) → get_var("arg" + get_var("_i"))
///   @arg@_i         → StringPart::DynVar([Var("arg"), Var("_i")]) — compound ref (bez spacji/literału między)
///   "tekst @var ok" → [Literal("tekst "), Var("var"), Literal(" ok")]
pub fn parse_string_parts(s: &str) -> Vec<StringPart> {
    let mut parts = Vec::with_capacity(4);
    let mut lit   = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'@' && i + 1 < bytes.len() {
            // @{...} — dynamiczna referencja
            if bytes[i+1] == b'{' {
                if !lit.is_empty() { parts.push(StringPart::Literal(std::mem::take(&mut lit))); }
                i += 2; // skip @{
                let start = i;
                while i < bytes.len() && bytes[i] != b'}' { i += 1; }
                let inner = &s[start..i];
                if i < bytes.len() { i += 1; } // skip }
                parts.push(StringPart::DynVar(parse_string_parts(inner)));
                continue;
            }

            // @nazwa — normalny Var lub początek compound ref (@arg@_i)
            if bytes[i+1].is_ascii_alphabetic() || bytes[i+1] == b'_' {
                if !lit.is_empty() { parts.push(StringPart::Literal(std::mem::take(&mut lit))); }
                i += 1;
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
                let var_name = s[start..i].to_string();

                // Sprawdź czy zaraz po zmiennej jest kolejny @var (compound ref: @arg@_i)
                // Jeśli tak: zbierz wszystkie kolejne @var i zrób DynVar
                if i < bytes.len() && bytes[i] == b'@'
                    && i + 1 < bytes.len()
                    && (bytes[i+1].is_ascii_alphabetic() || bytes[i+1] == b'_')
                {
                    // Compound reference — zbieramy części do DynVar
                    let mut dyn_parts: Vec<StringPart> = vec![StringPart::Var(var_name)];
                    while i < bytes.len() && bytes[i] == b'@'
                        && i + 1 < bytes.len()
                        && (bytes[i+1].is_ascii_alphabetic() || bytes[i+1] == b'_')
                    {
                        i += 1;
                        let s2 = i;
                        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
                        dyn_parts.push(StringPart::Var(s[s2..i].to_string()));
                    }
                    parts.push(StringPart::DynVar(dyn_parts));
                } else {
                    parts.push(StringPart::Var(var_name));
                }
                continue;
            }
        }
        lit.push(bytes[i] as char);
        i += 1;
    }
    if !lit.is_empty() { parts.push(StringPart::Literal(lit)); }
    parts
}

// ── ExternDef (system extern — zewnętrzne runtime'y) ──────────────────────────
//
// Składnia:
//   _> plik.sh [shell] def
//     ;; ... instrukcje jak w bloku HL
//   done
//
// Plik może być: nazwą (szukamy w PATH/BIT_HOME), ścieżką względną, lub absolutną.
// Runtime: shell, python, java, elf, so
//
// Semantyka:
//   shell  → bash <plik> <args>
//   python → python3 <plik> <args>
//   java   → java -jar <plik> <args>  lub  java -cp ... <klasa> <args>
//   elf    → exec <plik> <args>  (szuka w PATH jeśli nie ma /)
//   so     → dlopen(<plik>)  + wywołanie symbolu "hl_extern_call"
pub use crate::extern_spec::ExternRuntime;
