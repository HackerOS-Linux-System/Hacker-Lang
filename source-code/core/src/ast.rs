use serde::{Deserialize, Serialize};

/// Represents a node in the Hacker Lang AST
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    /// :: message
    Print {
        parts: Vec<StringPart>,
    },

    /// > command [args]
    Command {
        raw: String,
        mode: CommandMode,
        interpolate: bool,
    },

    /// % name = value
    VarDecl {
        name: String,
        value: VarValue,
    },

    /// @name (used standalone as expression)
    VarRef(String),

    /// // dependency
    Dependency {
        name: String,
    },

    /// : name def ... done
    FuncDef {
        name: String,
        body: Vec<Node>,
    },

    /// -- name
    FuncCall {
        name: String,
    },

    /// ? ok ... done  /  ? err ... done
    Conditional {
        condition: ConditionKind,
        body: Vec<Node>,
    },

    /// Block comment // ... \\
    BlockComment(String),

    /// Doc comment ///
    DocComment(String),

    /// Line comment ;;
    LineComment(String),

    /// Sequence of statements
    Block(Vec<Node>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommandMode {
    /// > plain command
    Plain,
    /// ^> with sudo
    Sudo,
    /// -> isolated (unshare namespace)
    Isolated,
    /// ^-> isolated + sudo
    IsolatedSudo,
    /// >> with variable interpolation
    WithVars,
    /// ^>> with vars + sudo
    WithVarsSudo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionKind {
    Ok,
    Err,
}

/// Parts of a string that may contain variable references
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StringPart {
    Literal(String),
    Var(String),
}

/// Value stored in a variable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VarValue {
    String(String),
    Number(f64),
    Bool(bool),
    /// Value obtained by running a command (backtick-style)
    CmdOutput(String),
    /// Interpolated string with @vars
    Interpolated(Vec<StringPart>),
}

impl Node {
    pub fn is_comment(&self) -> bool {
        matches!(
            self,
            Node::LineComment(_) | Node::DocComment(_) | Node::BlockComment(_)
        )
    }
}

/// Parse a raw string into StringParts, splitting on @varname tokens
pub fn parse_string_parts(s: &str) -> Vec<StringPart> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@' && i + 1 < chars.len() && (chars[i+1].is_alphabetic() || chars[i+1] == '_') {
            if !current.is_empty() {
                parts.push(StringPart::Literal(current.clone()));
                current.clear();
            }
            i += 1;
            let mut var_name = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                var_name.push(chars[i]);
                i += 1;
            }
            parts.push(StringPart::Var(var_name));
        } else {
            current.push(chars[i]);
            i += 1;
        }
    }

    if !current.is_empty() {
        parts.push(StringPart::Literal(current));
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_string_parts() {
        let parts = parse_string_parts("Hello @target, scanning @port");
        assert_eq!(parts.len(), 4);
        match &parts[0] {
            StringPart::Literal(s) => assert_eq!(s, "Hello "),
            _ => panic!("Expected literal"),
        }
        match &parts[1] {
            StringPart::Var(v) => assert_eq!(v, "target"),
            _ => panic!("Expected var"),
        }
    }
}
