use std::sync::Arc;
use rustc_hash::FxHashMap;
use hl_parser::ast::{Node, StringPart};

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(f64),
    Bool(bool),
    Nil,
}

impl Value {
    #[inline]
    pub fn to_string_val(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Number(n) => if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{}", n) },
            Value::Bool(b)   => b.to_string(),
            Value::Nil       => String::new(),
        }
    }
    #[inline]
    pub fn as_str(&self) -> &str {
        match self { Value::String(s) => s.as_str(), _ => "" }
    }
}

pub type FuncBody = Arc<Vec<Node>>;

pub struct Env {
    pub vars:      FxHashMap<String, Value>,
    pub functions: FxHashMap<String, FuncBody>,
    pub last_exit: i32,
    interp_buf: String,
}

impl Default for Env {
    fn default() -> Self { Self::new() }
}

impl Env {
    pub fn new() -> Self {
        let mut vars: FxHashMap<String, Value> = FxHashMap::default();
        vars.insert("HL_VERSION".into(), Value::String("gen 1".into()));
        vars.insert("HL_OS".into(),      Value::String("HackerOS/Debian".into()));
        Self { vars, functions: FxHashMap::default(), last_exit: 0, interp_buf: String::with_capacity(256) }
    }

    #[inline] pub fn set_var(&mut self, name: &str, val: Value) { self.vars.insert(name.to_string(), val); }
    #[inline] pub fn get_var(&self, name: &str) -> &Value {
        static NIL: Value = Value::Nil;
        self.vars.get(name).unwrap_or(&NIL)
    }

    pub fn resolve_string_parts(&mut self, parts: &[StringPart]) -> String {
        self.interp_buf.clear();
        for part in parts {
            match part {
                StringPart::Literal(s) => self.interp_buf.push_str(s),
                StringPart::Var(v) => {
                    match self.vars.get(v.as_str()) {
                        Some(Value::String(s)) => self.interp_buf.push_str(s),
                        Some(v) => self.interp_buf.push_str(&v.to_string_val()),
                        None => {}
                    }
                }
            }
        }
        self.interp_buf.clone()
    }

    pub fn interpolate(&mut self, raw: &str) -> String {
        if !raw.contains('@') { return raw.to_string(); }
        let parts = hl_parser::ast::parse_string_parts(raw);
        self.resolve_string_parts(&parts)
    }

    #[inline] pub fn define_function(&mut self, name: String, body: Vec<Node>) {
        self.functions.insert(name, Arc::new(body));
    }
    #[inline] pub fn get_function(&self, name: &str) -> Option<FuncBody> {
        self.functions.get(name).cloned()
    }
}
