use serde::{Deserialize, Serialize};

/// Typ zmiennej (gen 2 — typowane zmienne)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VarType {
    String,
    Int,
    Float,
    Bool,
    List,
    Map,
    Any,   // brak adnotacji — domyslne zachowanie
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

/// Galaz match/case (gen 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    pub pattern: String,
    pub body:    Vec<Node>,
}

/// Narzedzia HackerOS dostepne przez || (gen 2)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HackerOsTool {
    HHash,          // H#
    Hco,
    Hacker,
    Hsh,
    Hpkg,
    BlueEnvironment,
    Hnm,
    Hpm,
    Hedit,
    Ngt,
    Eiq,
    Getit,
    Hdev,
    Anvil,
    A,
    Hbuild,
    Lpm,
    Chker,
    Isolator,
    HackerOsSteam,
    Ulb,
    Gameframe,
    Hup,
    HackerOsBuilder,
    Other(String),
}

impl HackerOsTool {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "H#"                => HackerOsTool::HHash,
            "hco"               => HackerOsTool::Hco,
            "hacker"            => HackerOsTool::Hacker,
            "hsh"               => HackerOsTool::Hsh,
            "hpkg"              => HackerOsTool::Hpkg,
            "Blue-Environment"  => HackerOsTool::BlueEnvironment,
            "hnm"               => HackerOsTool::Hnm,
            "hpm"               => HackerOsTool::Hpm,
            "hedit"             => HackerOsTool::Hedit,
            "ngt"               => HackerOsTool::Ngt,
            "eiq"               => HackerOsTool::Eiq,
            "getit"             => HackerOsTool::Getit,
            "hdev"              => HackerOsTool::Hdev,
            "anvil"             => HackerOsTool::Anvil,
            "a"                 => HackerOsTool::A,
            "hbuild"            => HackerOsTool::Hbuild,
            "lpm"               => HackerOsTool::Lpm,
            "chker"             => HackerOsTool::Chker,
            "isolator"          => HackerOsTool::Isolator,
            "hackeros-steam"    => HackerOsTool::HackerOsSteam,
            "ulb"               => HackerOsTool::Ulb,
            "gameframe"         => HackerOsTool::Gameframe,
            "hup"               => HackerOsTool::Hup,
            "hackeros-builder"  => HackerOsTool::HackerOsBuilder,
            other               => HackerOsTool::Other(other.to_string()),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    Print       { parts: Vec<StringPart> },
    QuickCall   { name: String, args: Vec<StringPart> },
    Command     { raw: String, mode: CommandMode, interpolate: bool },
    HshCommand  { raw: String },
    Background  { raw: String },
    RepeatN     { count: u64, body: Vec<Node> },

    // Gen 2 — typowana deklaracja zmiennej
    // % nazwa: typ = wartosc
    // % nazwa = wartosc  (Any, kompatybilnosc)
    VarDecl     { name: String, typ: VarType, value: VarValue },

    Export      { name: String, value: ExportValue },
    VarRef      (String),
    Dependency  { name: String },
    Import      { lib: String, detail: Option<String> },
    FileImport  { path: String, detail: Option<String> },

    FuncDef     { name: String, body: Vec<Node> },
    FuncCall    { name: String },
    Conditional { condition: ConditionKind, body: Vec<Node> },

    // Gen 2 — for loop
    // @ item in lista done
    ForIn       { var: String, iterable: Vec<StringPart>, body: Vec<Node> },

    // Gen 2 — while loop
    // ? while warunek done
    WhileLoop   { condition: Vec<StringPart>, body: Vec<Node> },

    // Gen 2 — match/case
    // ? match @var
    // | "wartosc" -> ...done
    // | * -> ...done  (wildcard)
    // done
    MatchExpr   { subject: Vec<StringPart>, arms: Vec<MatchArm> },

    // Gen 2 — arytmetyka natywna: $( wyrazenie )
    Arithmetic  { expr: String, assign_to: Option<String> },

    // Gen 2 — pipe do zmiennej: > komenda |> @nazwa
    PipeToVar   { command: String, mode: CommandMode, var_name: String },

    // Gen 2 — HackerOS API: || narzedzie argumenty
    HackerOsApi { tool: HackerOsTool, args: Vec<StringPart> },

    // Gen 1 — goroutine z nazwa: :* nazwa def ... done
    Goroutine   { name: Option<String>, body: Vec<Node> },
    // *-- nazwa — wywolaj goroutine lub kanal
    ChannelOp   { name: String, value: Option<Vec<StringPart>> },
    Channel     { name: String },

    BlockComment(String),
    DocComment  (String),
    LineComment  (String),
    Block       (Vec<Node>),
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
    Var(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VarValue {
    String(String),
    Number(f64),
    Bool(bool),
    CmdOutput(String),
    Interpolated(Vec<StringPart>),
    // Gen 2
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

pub fn parse_string_parts(s: &str) -> Vec<StringPart> {
    let mut parts = Vec::with_capacity(4);
    let mut lit   = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@'
            && i + 1 < bytes.len()
            && (bytes[i+1].is_ascii_alphabetic() || bytes[i+1] == b'_')
        {
            if !lit.is_empty() { parts.push(StringPart::Literal(std::mem::take(&mut lit))); }
            i += 1;
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            parts.push(StringPart::Var(s[start..i].to_string()));
        } else {
            lit.push(bytes[i] as char);
            i += 1;
        }
    }
    if !lit.is_empty() { parts.push(StringPart::Literal(lit)); }
    parts
}
