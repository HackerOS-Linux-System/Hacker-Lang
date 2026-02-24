use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use colored::Colorize;
use sha2::{Digest, Sha256};
use hex;
use indicatif::{ProgressBar, ProgressStyle};
use hl_plsa::{AnalysisResult, Expr, ProgramNode, Stmt};
use tempfile::tempdir;

const CACHE_DIR: &str = "/tmp/hl-cache";

fn make_progress_bar(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} [{bar:40.cyan/blue}] {msg}")
        .unwrap()
        .progress_chars("=>-"),
    );
    pb.set_message(msg.to_string());
    pb
}

fn make_step_bar(total: u64, msg: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:40.green/white}] {pos}/{len} {msg}")
        .unwrap()
        .progress_chars("=>-"),
    );
    pb.set_message(msg.to_string());
    pb
}

struct Transpiler {
    main_body: String,
    fn_codes: Vec<String>,
}

impl Transpiler {
    fn new() -> Self {
        Transpiler {
            main_body: String::new(),
            fn_codes: Vec::new(),
        }
    }

    fn transpile_ast(&mut self, ast: &AnalysisResult) {
        self.main_body += "let globals = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<String, Value>::new()));\n";
        self.main_body += "let env: std::collections::HashMap<String, String> = std::env::vars().collect();\n";
        for node in &ast.main_body {
            self.main_body += &self.transpile_node(node);
        }
        for (name, (params, ret, body, is_quick)) in &ast.functions {
            self.transpile_fn(name, params, ret, body, *is_quick);
        }
    }

    fn finish(self) -> String {
        let mut code = String::new();
        code += "use std::collections::HashMap;\n";
        code += "use std::process::Command;\n";
        code += "use std::env;\n";
        code += "use std::sync::{Arc, Mutex};\n";
        code += "use std::thread;\n";
        code += "use gc::{Gc, Finalize, Trace};\n";
        code += "use gc_derive::{Trace, Finalize};\n";
        code += r#"
        #[derive(Clone, Debug, Trace, Finalize)]
        enum Value {
        F64(f64),
        I32(i32),
        Bool(bool),
        Nil,
        Str(String),
        List(Vec<Value>),
        Map(HashMap<Value, Value>),
        Obj(Gc<Obj>),
    }
    impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
    match (self, other) {
    (Value::F64(a), Value::F64(b)) => a == b,
    (Value::I32(a), Value::I32(b)) => a == b,
    (Value::Bool(a), Value::Bool(b)) => a == b,
    (Value::Nil, Value::Nil) => true,
    (Value::Str(a), Value::Str(b)) => a == b,
    (Value::List(a), Value::List(b)) => a == b,
    (Value::Map(a), Value::Map(b)) => a == b,
    (Value::Obj(a), Value::Obj(b)) => *a == *b,
    _ => false,
    }
    }
    }
    impl Eq for Value {}
    impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    match self {
    Value::F64(f) => f.to_bits().hash(state),
    Value::I32(i) => i.hash(state),
    Value::Bool(b) => b.hash(state),
    Value::Nil => 0u64.hash(state),
    Value::Str(s) => s.hash(state),
    Value::List(l) => l.hash(state),
    Value::Map(m) => { for (k, v) in m { k.hash(state); v.hash(state); } },
    Value::Obj(o) => o.hash(state),
    }
    }
    }
    impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match self {
    Value::F64(n) => write!(f, "{}", n),
    Value::I32(n) => write!(f, "{}", n),
    Value::Bool(b) => write!(f, "{}", b),
    Value::Nil => write!(f, "nil"),
    Value::Str(s) => write!(f, "{}", s),
    Value::List(l) => write!(f, "{:?}", l),
    Value::Map(m) => write!(f, "{:?}", m),
    Value::Obj(o) => write!(f, "{:?}", o),
    }
    }
    }
    #[derive(Clone, PartialEq, Eq, Trace, Finalize)]
    struct Obj {
    name: String,
    fields: HashMap<String, Value>,
    methods: HashMap<String, fn(Vec<Value>) -> Value>,
    }
    impl std::hash::Hash for Obj {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.name.hash(state);
    for (k, v) in &self.fields {
        k.hash(state);
        v.hash(state);
    }
    for k in self.methods.keys() {
        k.hash(state);
    }
    }
    }
    impl std::fmt::Debug for Obj {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(f, "Obj {{ name: {}, fields: {:?} }}", self.name, self.fields)
    }
    }
    impl Value {
    fn is_i32(&self) -> bool {
    matches!(self, Value::I32(_))
    }
    fn as_i32(&self) -> i32 {
    if let Value::I32(i) = self { *i } else { 0 }
    }
    fn as_f64(&self) -> f64 {
    match self {
    Value::I32(i) => *i as f64,
    Value::F64(f) => *f,
    _ => 0.0,
    }
    }
    fn as_bool(&self) -> bool {
    match self {
    Value::Bool(b) => *b,
    Value::Nil => false,
    Value::I32(i) => *i != 0,
    Value::F64(f) => *f != 0.0,
    Value::Str(s) => !s.is_empty(),
    _ => true,
    }
    }
    }
    fn substitute(text: &str, globals: &Arc<Mutex<HashMap<String, Value>>>, env: &HashMap<String, String>) -> String {
    let g = globals.lock().unwrap();
    let mut r = text.to_string();
    for (k, v) in g.iter() {
        r = r.replace(&format!("@{}", k), &v.to_string());
    }
    for (k, v) in env {
        r = r.replace(&format!("${}", k), v);
    }
    r
    }
    fn bin_op(a: &Value, op: &str, b: &Value) -> Value {
    match op {
    "+" => if a.is_i32() && b.is_i32() {
    Value::I32(a.as_i32().wrapping_add(b.as_i32()))
    } else {
        Value::F64(a.as_f64() + b.as_f64())
    },
    "-" => if a.is_i32() && b.is_i32() {
    Value::I32(a.as_i32().wrapping_sub(b.as_i32()))
    } else {
        Value::F64(a.as_f64() - b.as_f64())
    },
    "*" => if a.is_i32() && b.is_i32() {
    Value::I32(a.as_i32().wrapping_mul(b.as_i32()))
    } else {
        Value::F64(a.as_f64() * b.as_f64())
    },
    "/" => Value::F64(a.as_f64() / b.as_f64()),
    "==" => Value::Bool(a == b),
    "!=" => Value::Bool(a != b),
    "<" => Value::Bool(if a.is_i32() && b.is_i32() { a.as_i32() < b.as_i32() } else { a.as_f64() < b.as_f64() }),
    ">" => Value::Bool(if a.is_i32() && b.is_i32() { a.as_i32() > b.as_i32() } else { a.as_f64() > b.as_f64() }),
    "<=" => Value::Bool(if a.is_i32() && b.is_i32() { a.as_i32() <= b.as_i32() } else { a.as_f64() <= b.as_f64() }),
    ">=" => Value::Bool(if a.is_i32() && b.is_i32() { a.as_i32() >= b.as_i32() } else { a.as_f64() >= b.as_f64() }),
    _ => Value::Nil,
    }
    }
    fn to_iter(v: &Value) -> Vec<Value> {
    if let Value::List(l) = v {
        l.clone()
    } else {
        vec![]
    }
    }
    "#;
    code += &self.fn_codes.join("\n");
    code += "fn main() {\n";
    code += &self.main_body;
    code += "}\n";
    code
    }

    fn transpile_node(&self, node: &ProgramNode) -> String {
        self.transpile_stmt(&node.content, node.is_sudo)
    }

    fn transpile_stmt(&self, stmt: &Stmt, sudo: bool) -> String {
        match stmt {
            Stmt::Raw { mode, cmd } => {
                let mut s = String::new();
                s += &format!("let cmd = \"{}\";\n", cmd.replace('\\', "\\\\").replace('"', "\\\""));
                s += "let full = substitute(&cmd, &globals, &env);\n";
                let shell_prefix = if sudo {
                    r#"Command::new("sudo").arg("sh")"#
                } else {
                    r#"Command::new("sh")"#
                };
                s += &format!("let mut shell_cmd = {};\n", shell_prefix);
                s += "shell_cmd.arg(\"-c\").arg(&full);\n";
                match mode.as_str() {
                    ">" => s += "shell_cmd.status().unwrap();\n",
                    ">>" => s += "let _ = shell_cmd.output().unwrap();\n",
                    ">>>" => s += "shell_cmd.spawn().unwrap();\n",
                    _ => s += "shell_cmd.status().unwrap();\n",
                }
                s
            }
            Stmt::AssignGlobal { key, ty: _, val } | Stmt::AssignLocal { key, ty: _, val } => {
                let v = self.transpile_expr(val);
                format!("globals.lock().unwrap().insert(\"{}\".to_string(), {});\n", key, v)
            }
            Stmt::If { cond, body, else_ifs, else_body } => {
                let mut s = String::new();
                let c = self.transpile_expr(cond);
                s += &format!("if {}.as_bool() {{\n", c);
                for st in body {
                    s += &self.transpile_stmt(st, false);
                }
                s += "}\n";
                for (ec, eb) in else_ifs {
                    s += "else ";
                    let ec = self.transpile_expr(ec);
                    s += &format!("if {}.as_bool() {{\n", ec);
                    for st in eb {
                        s += &self.transpile_stmt(st, false);
                    }
                    s += "}\n";
                }
                if let Some(eb) = else_body {
                    s += "else {\n";
                    for st in eb {
                        s += &self.transpile_stmt(st, false);
                    }
                    s += "}\n";
                }
                s
            }
            Stmt::While { cond, body } => {
                let mut s = String::new();
                let c = self.transpile_expr(cond);
                s += &format!("while {}.as_bool() {{\n", c);
                for st in body {
                    s += &self.transpile_stmt(st, false);
                }
                s += "}\n";
                s
            }
            Stmt::For { var, iter, body } => {
                let mut s = String::new();
                let i = self.transpile_expr(iter);
                s += &format!("for v in to_iter(&{}) {{\n", i);
                s += &format!("globals.lock().unwrap().insert(\"{}\".to_string(), v);\n", var);
                for st in body {
                    s += &self.transpile_stmt(st, false);
                }
                s += "}\n";
                s
            }
            Stmt::Return { expr } => {
                let e = self.transpile_expr(expr);
                format!("return {};\n", e)
            }
            Stmt::Repeat { count, body } => {
                let mut s = String::new();
                s += &format!("for _ in 0..{} {{\n", count);
                for st in body {
                    s += &self.transpile_stmt(st, false);
                }
                s += "}\n";
                s
            }
            Stmt::Background(stmts) => {
                let mut body = String::new();
                for st in stmts {
                    body += &self.transpile_stmt(st, false);
                }
                let mut s = String::new();
                s += "let globals_clone = globals.clone();\n";
                s += "let env_clone = env.clone();\n";
                s += "thread::spawn(move || {\n";
                s += "let globals = globals_clone;\n";
                s += "let env = env_clone;\n";
                s += &body;
                s += "});\n";
                s
            }
            _ => String::new(),
        }
    }

    fn transpile_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Lit(hl_plsa::Value::I32(n)) => format!("Value::I32({})", n),
            Expr::Lit(hl_plsa::Value::F64(f)) => format!("Value::F64({})", f),
            Expr::Lit(hl_plsa::Value::Bool(b)) => format!("Value::Bool({})", b),
            Expr::Lit(hl_plsa::Value::Str(s)) => format!("Value::Str(\"{}\".to_string())", s.replace("\"", "\\\"")),
            Expr::Lit(_) => "Value::Nil".to_string(),
            Expr::Var(name) => format!("globals.lock().unwrap().get(\"{}\").cloned().unwrap_or(Value::Nil)", name),
            Expr::BinOp { op, left, right } => {
                let l = self.transpile_expr(left);
                let r = self.transpile_expr(right);
                format!("bin_op(&{}, \"{}\", &{})", l, op, r)
            }
            Expr::Call { name, args } => {
                let args_s = args.iter().map(|a| self.transpile_expr(a)).collect::<Vec<_>>().join(", ");
                format!("{}(vec![{}])", name, args_s)
            }
            _ => "Value::Nil".to_string(),
        }
    }

    fn transpile_fn(&mut self, name: &str, params: &[(String, String)], _ret_ty: &Option<String>, body: &Vec<ProgramNode>, _is_quick: bool) {
        let mut f_code = String::new();
        f_code += &format!("fn {}(args: Vec<Value>) -> Value {{\n", name);
        for (i, (pname, _)) in params.iter().enumerate() {
            f_code += &format!("let {} = args.get({}).cloned().unwrap_or(Value::Nil);\n", pname, i);
        }
        for node in body {
            f_code += &self.transpile_stmt(&node.content, node.is_sudo);
        }
        f_code += "Value::Nil\n";
        f_code += "}\n";
        self.fn_codes.push(f_code);
    }
}

pub fn run_command(file: String, verbose: bool) -> bool {
    let _ = fs::create_dir_all(CACHE_DIR);
    let hash = hash_file(&file);
    let bin_path = PathBuf::from(CACHE_DIR).join(&hash);
    if bin_path.exists() {
        if verbose {
            println!("{} Cache hit — skipping compilation", "[*]".green());
        }
    } else {
        let pb = make_step_bar(3, "Starting...");
        pb.set_position(0);
        pb.set_message(format!("Parsing {}", file));
        let mut seen = HashSet::new();
        let ast = match hl_plsa::parse_file(&file, true, verbose, &mut seen) {
            Ok(a) => a,
            Err(e) => {
                pb.finish_with_message(format!("{} Parse failed", "[ERROR]".red()));
                for err in e {
                    eprintln!("{:?}", err);
                }
                return false;
            }
        };
        if verbose && ast.is_potentially_unsafe {
            pb.println(format!("{} Script has privileged commands", "[!]".yellow()));
        }
        pb.set_position(1);
        pb.set_message("Transpiling to Rust...");
        let mut transpiler = Transpiler::new();
        transpiler.transpile_ast(&ast);
        let rs_code = transpiler.finish();
        let rs_path = PathBuf::from(CACHE_DIR).join(format!("{}.rs", hash));
        if let Err(e) = fs::write(&rs_path, &rs_code) {
            pb.finish_with_message(format!("{} Failed to write .rs file", "[ERROR]".red()));
            eprintln!("Failed to write RS file: {}", e);
            return false;
        }
        pb.set_position(2);
        pb.set_message("Compiling with cargo...");
        let temp_dir = tempdir().unwrap();
        let cargo_path = temp_dir.path().join("Cargo.toml");
        if let Err(e) = fs::write(&cargo_path, format!(r#"
            [package]
            name = "hl_{}"
            version = "0.1.0"
            edition = "2021"

            [dependencies]
            gc = "0.5"
            gc_derive = "0.5"
            "#, hash)) {
            pb.finish_with_message(format!("{} Failed to write Cargo.toml", "[ERROR]".red()));
        eprintln!("Failed to write Cargo.toml: {}", e);
        return false;
            }
            let src_dir = temp_dir.path().join("src");
            if let Err(e) = fs::create_dir(&src_dir) {
                pb.finish_with_message(format!("{} Failed to create src dir", "[ERROR]".red()));
                eprintln!("Failed to create src dir: {}", e);
                return false;
            }
            let main_path = src_dir.join("main.rs");
            if let Err(e) = fs::write(&main_path, &rs_code) {
                pb.finish_with_message(format!("{} Failed to write main.rs", "[ERROR]".red()));
                eprintln!("Failed to write main.rs: {}", e);
                return false;
            }
            let output = Command::new("cargo")
            .current_dir(temp_dir.path())
            .arg("build")
            .arg("--release")
            .output()
            .unwrap();
            if !output.status.success() {
                pb.finish_with_message(format!("{} Cargo compilation failed", "[ERROR]".red()));
                eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
                return false;
            }
            let compiled_bin = temp_dir.path().join("target/release").join(format!("hl_{}", hash));
            if let Err(e) = fs::copy(&compiled_bin, &bin_path) {
                pb.finish_with_message(format!("{} Failed to cache binary", "[ERROR]".red()));
                eprintln!("Failed to cache binary: {}", e);
                return false;
            }
            pb.set_position(3);
            pb.finish_with_message(format!("{} Compiled successfully", "[OK]".green()));
    }
    let run_pb = make_progress_bar("Running...");
    run_pb.enable_steady_tick(std::time::Duration::from_millis(80));
    let status = Command::new(&bin_path).status();
    match status {
        Ok(s) if s.success() => {
            run_pb.finish_with_message(format!("{} Done", "[OK]".green()));
            true
        }
        _ => {
            run_pb.finish_with_message(format!("{} Program exited with error", "[ERROR]".red()));
            false
        }
    }
}

pub fn compile_command(file: String, output: String, verbose: bool) -> bool {
    let _ = fs::create_dir_all(CACHE_DIR);
    let hash = hash_file(&file);
    let bin_path = PathBuf::from(CACHE_DIR).join(&hash);
    if bin_path.exists() {
        if verbose {
            println!("{} Cache hit — skipping compilation", "[*]".green());
        }
    } else {
        let pb = make_step_bar(3, "Starting...");
        pb.set_position(0);
        pb.set_message(format!("Parsing {}", file));
        let mut seen = HashSet::new();
        let ast = match hl_plsa::parse_file(&file, true, verbose, &mut seen) {
            Ok(a) => a,
            Err(e) => {
                pb.finish_with_message(format!("{} Parse failed", "[ERROR]".red()));
                for err in e {
                    eprintln!("{:?}", err);
                }
                return false;
            }
        };
        if verbose && ast.is_potentially_unsafe {
            pb.println(format!("{} Script has privileged commands", "[!]".yellow()));
        }
        pb.set_position(1);
        pb.set_message("Transpiling to Rust...");
        let mut transpiler = Transpiler::new();
        transpiler.transpile_ast(&ast);
        let rs_code = transpiler.finish();
        let rs_path = PathBuf::from(CACHE_DIR).join(format!("{}.rs", hash));
        if let Err(e) = fs::write(&rs_path, &rs_code) {
            pb.finish_with_message(format!("{} Failed to write .rs file", "[ERROR]".red()));
            eprintln!("Failed to write RS file: {}", e);
            return false;
        }
        pb.set_position(2);
        pb.set_message("Compiling with cargo...");
        let temp_dir = tempdir().unwrap();
        let cargo_path = temp_dir.path().join("Cargo.toml");
        if let Err(e) = fs::write(&cargo_path, format!(r#"
            [package]
            name = "hl_{}"
            version = "0.1.0"
            edition = "2021"

            [dependencies]
            gc = "0.5"
            gc_derive = "0.5"
            "#, hash)) {
            pb.finish_with_message(format!("{} Failed to write Cargo.toml", "[ERROR]".red()));
        eprintln!("Failed to write Cargo.toml: {}", e);
        return false;
            }
            let src_dir = temp_dir.path().join("src");
            if let Err(e) = fs::create_dir(&src_dir) {
                pb.finish_with_message(format!("{} Failed to create src dir", "[ERROR]".red()));
                eprintln!("Failed to create src dir: {}", e);
                return false;
            }
            let main_path = src_dir.join("main.rs");
            if let Err(e) = fs::write(&main_path, &rs_code) {
                pb.finish_with_message(format!("{} Failed to write main.rs", "[ERROR]".red()));
                eprintln!("Failed to write main.rs: {}", e);
                return false;
            }
            let output = Command::new("cargo")
            .current_dir(temp_dir.path())
            .arg("build")
            .arg("--release")
            .output()
            .unwrap();
            if !output.status.success() {
                pb.finish_with_message(format!("{} Cargo compilation failed", "[ERROR]".red()));
                eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
                return false;
            }
            let compiled_bin = temp_dir.path().join("target/release").join(format!("hl_{}", hash));
            if let Err(e) = fs::copy(&compiled_bin, &bin_path) {
                pb.finish_with_message(format!("{} Failed to cache binary", "[ERROR]".red()));
                eprintln!("Failed to cache binary: {}", e);
                return false;
            }
            pb.set_position(3);
            pb.finish_with_message(format!("{} Compiled successfully", "[OK]".green()));
    }
    let out_path = if let Some(pos) = file.rfind('.') { &file[..pos] } else { &file };
    let out = if output.is_empty() { out_path.to_string() } else { output };
    let copy_pb = make_progress_bar(format!("Copying binary to {}...", out).as_str());
    copy_pb.enable_steady_tick(std::time::Duration::from_millis(80));
    if let Err(e) = fs::copy(&bin_path, &out) {
        copy_pb.finish_with_message(format!("{} Failed to copy binary", "[ERROR]".red()));
        eprintln!("Failed to copy binary: {}", e);
        return false;
    }
    copy_pb.finish_with_message(format!("{} Binary written to {}", "[OK]".green(), out));
    true
}

fn hash_file(path: &str) -> String {
    let b = fs::read(path).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(&b);
    hex::encode(h.finalize())
}
