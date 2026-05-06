use std::sync::Arc;
use rustc_hash::FxHashMap;
use hl_parser::ast::{Node, StringPart};

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(f64),
    Bool(bool),
    List(Vec<Value>),
    Nil,
}

impl Value {
    #[inline]
    pub fn to_string_val(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Number(n) => if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{}", n) },
            Value::Bool(b)   => b.to_string(),
            Value::List(v)   => v.iter().map(|x| x.to_string_val()).collect::<Vec<_>>().join(" "),
            Value::Nil       => String::new(),
        }
    }
    #[inline]
    pub fn as_str(&self) -> &str {
        match self { Value::String(s) => s.as_str(), _ => "" }
    }
    #[inline]
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::String(s) => s.parse().unwrap_or(0.0),
            Value::Bool(b)   => if *b { 1.0 } else { 0.0 },
            _                => 0.0,
        }
    }
    #[inline]
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b)   => *b,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty() && s != "false" && s != "0",
            Value::List(v)   => !v.is_empty(),
            Value::Nil       => false,
        }
    }
}

pub type FuncBody = Arc<Vec<Node>>;

pub struct Env {
    pub vars:      FxHashMap<String, Value>,
    pub functions: FxHashMap<String, FuncBody>,
    pub last_exit: i32,
    interp_buf:    String,
}

impl Default for Env {
    fn default() -> Self { Self::new() }
}

impl Env {
    pub fn new() -> Self {
        let mut vars: FxHashMap<String, Value> = FxHashMap::default();
        vars.insert("HL_VERSION".into(), Value::String("gen 2".into()));
        vars.insert("HL_OS".into(),      Value::String("HackerOS/Debian".into()));
        vars.insert("HL_GEN".into(),     Value::String("2".into()));
        Self {
            vars,
            functions: FxHashMap::default(),
            last_exit: 0,
            interp_buf: String::with_capacity(256),
        }
    }

    #[inline]
    pub fn set_var(&mut self, name: &str, val: Value) {
        self.vars.insert(name.to_string(), val);
    }

    /// Pobierz wartosc zmiennej jako string (z fallbackiem do env)
    pub fn get_var_str(&self, name: &str) -> String {
        if let Some(v) = self.vars.get(name) {
            return v.to_string_val();
        }
        std::env::var(name).unwrap_or_default()
    }

    /// Pobierz wartosc zmiennej (zwraca Nil jesli nie istnieje)
    pub fn get_var_owned(&self, name: &str) -> Value {
        self.vars.get(name).cloned().unwrap_or(Value::Nil)
    }

    /// Pobierz referencje do wartosci (uzywaj tylko gdy NIL jest OK)
    pub fn get_var(&self, name: &str) -> &Value {
        static NIL: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
        let nil = NIL.get_or_init(|| Value::Nil);
        self.vars.get(name).unwrap_or(nil)
    }

    pub fn resolve_string_parts(&mut self, parts: &[StringPart]) -> String {
        self.interp_buf.clear();
        for part in parts {
            match part {
                StringPart::Literal(s) => self.interp_buf.push_str(s),
                StringPart::Var(v) => {
                    let val = if let Some(val) = self.vars.get(v.as_str()) {
                        val.to_string_val()
                    } else {
                        std::env::var(v).unwrap_or_default()
                    };
                    self.interp_buf.push_str(&val);
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

    #[inline]
    pub fn define_function(&mut self, name: String, body: Vec<Node>) {
        self.functions.insert(name, Arc::new(body));
    }

    #[inline]
    pub fn get_function(&self, name: &str) -> Option<FuncBody> {
        self.functions.get(name).cloned()
    }
}
