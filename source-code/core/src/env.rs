use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::ast::{Node, StringPart};

/// Resolved runtime value
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(f64),
    Bool(bool),
    Nil,
}

impl Value {
    pub fn to_string_val(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Number(n) => {
                if n.fract() == 0.0 { format!("{}", *n as i64) }
                else { format!("{}", n) }
            }
            Value::Bool(b) => b.to_string(),
            Value::Nil => String::new(),
        }
    }
}

/// Execution environment: variables + defined functions
#[derive(Debug, Clone)]
pub struct Env {
    pub vars: HashMap<String, Value>,
    pub functions: HashMap<String, Vec<Node>>,
    pub last_exit: i32,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    pub fn new() -> Self {
        let mut vars = HashMap::new();
        // Inject some useful defaults
        vars.insert("HL_VERSION".to_string(), Value::String("0.1.0".to_string()));
        vars.insert("HL_OS".to_string(), Value::String("HackerOS/Debian".to_string()));
        Self {
            vars,
            functions: HashMap::new(),
            last_exit: 0,
        }
    }

    pub fn set_var(&mut self, name: &str, val: Value) {
        self.vars.insert(name.to_string(), val);
    }

    pub fn get_var(&self, name: &str) -> Value {
        self.vars.get(name).cloned().unwrap_or(Value::Nil)
    }

    pub fn resolve_string_parts(&self, parts: &[StringPart]) -> String {
        parts.iter().map(|p| match p {
            StringPart::Literal(s) => s.clone(),
                         StringPart::Var(v) => self.get_var(v).to_string_val(),
        }).collect()
    }

    /// Interpolate @varname references in a raw string
    pub fn interpolate(&self, raw: &str) -> String {
        let parts = crate::ast::parse_string_parts(raw);
        self.resolve_string_parts(&parts)
    }

    pub fn define_function(&mut self, name: String, body: Vec<Node>) {
        self.functions.insert(name, body);
    }

    pub fn get_function(&self, name: &str) -> Option<Vec<Node>> {
        self.functions.get(name).cloned()
    }
}

/// Shared environment for use across async tasks
pub type SharedEnv = Arc<RwLock<Env>>;

pub fn new_shared_env() -> SharedEnv {
    Arc::new(RwLock::new(Env::new()))
}
