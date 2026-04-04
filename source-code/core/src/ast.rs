use serde::{Deserialize, Serialize};

/// Hacker Lang AST Node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    /// ~> message — wypisz tekst na stdout (odpowiednik echo)
    Print       { parts: Vec<StringPart> },
    /// :: name [args] — wywołanie quick-funkcji (:upper, :lower, :len, itd.)
    QuickCall   { name: String, args: Vec<StringPart> },
    /// > / ^> / -> / etc. — komenda systemowa
    Command     { raw: String, mode: CommandMode, interpolate: bool },
    /// % name = value
    VarDecl     { name: String, value: VarValue },
    /// @name (standalone)
    VarRef      (String),
    /// // package
    Dependency  { name: String },
    /// # lib lub # lib <- detail
    Import      { lib: String, detail: Option<String> },
    /// : name def ... done
    FuncDef     { name: String, body: Vec<Node> },
    /// -- name
    FuncCall    { name: String },
    /// ? ok ... done  |  ? err ... done
    Conditional { condition: ConditionKind, body: Vec<Node> },
    // Comments (preserved in AST for tooling)
    BlockComment(String),
    DocComment  (String),
    LineComment  (String),
    Block       (Vec<Node>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommandMode {
    Plain,
    Sudo,
    Isolated,
    IsolatedSudo,
    WithVars,
    WithVarsSudo,
    WithVarsIsolated,
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
}

impl Node {
    pub fn is_comment(&self) -> bool {
        matches!(self, Node::LineComment(_) | Node::DocComment(_) | Node::BlockComment(_))
    }
}

/// Parse a raw string into StringParts splitting on @varname
/// Optimised: single pass, pre-allocated
pub fn parse_string_parts(s: &str) -> Vec<StringPart> {
    let mut parts = Vec::with_capacity(4);
    let mut lit   = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'@' && i + 1 < bytes.len()
            && (bytes[i+1].is_ascii_alphabetic() || bytes[i+1] == b'_')
            {
                if !lit.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut lit)));
                }
                i += 1;
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                parts.push(StringPart::Var(s[start..i].to_string()));
            } else {
                lit.push(s.as_bytes()[i] as char);
                i += 1;
            }
    }

    if !lit.is_empty() { parts.push(StringPart::Literal(lit)); }
    parts
}
