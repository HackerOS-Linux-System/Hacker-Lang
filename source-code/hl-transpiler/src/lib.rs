use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;
use colored::Colorize;
use sha2::{Digest, Sha256};
use hex;
use indicatif::{ProgressBar, ProgressStyle};
use hl_plsa::{AnalysisResult, Expr, ProgramNode, Stmt};
use tempfile::tempdir;

// ─── Progress bar helpers ─────────────────────────────────────────────────────

fn make_bar(label: &str) -> ProgressBar {
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::with_template(
            " {spinner:.magenta} {prefix:<12} [{bar:40.magenta/238}] {pos:>3}% {msg}",
        )
        .unwrap()
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✔"])
        .progress_chars("=>."),
    );
    pb.set_prefix(label.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(60));
    pb
}

fn bar_done(pb: &ProgressBar, msg: &str) {
    pb.set_position(100);
    pb.set_style(
        ProgressStyle::with_template(
            " {spinner:.magenta} {prefix:<12} [{bar:40.magenta/238}] {pos:>3}% {msg}",
        )
        .unwrap()
        .tick_strings(&["✔"])
        .progress_chars("=>."),
    );
    pb.finish_with_message(msg.to_string());
}

fn bar_fail(pb: &ProgressBar, msg: &str) {
    pb.set_style(
        ProgressStyle::with_template(
            " {spinner:.red} {prefix:<12} [{bar:40.red/238}] {pos:>3}% {msg}",
        )
        .unwrap()
        .tick_strings(&["✖"])
        .progress_chars("=>."),
    );
    pb.finish_with_message(msg.to_string());
}

// ─── Transpiler ───────────────────────────────────────────────────────────────

struct Transpiler {
    main_body: String,
    fn_codes: Vec<String>,
}

impl Transpiler {
    fn new() -> Self {
        Transpiler { main_body: String::new(), fn_codes: Vec::new() }
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
    for (k, v) in &self.fields { k.hash(state); v.hash(state); }
    for k in self.methods.keys() { k.hash(state); }
    }
    }
    impl std::fmt::Debug for Obj {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(f, "Obj {{ name: {}, fields: {:?} }}", self.name, self.fields)
    }
    }
    impl Value {
    fn is_i32(&self) -> bool { matches!(self, Value::I32(_)) }
    fn as_i32(&self) -> i32 { if let Value::I32(i) = self { *i } else { 0 } }
    fn as_f64(&self) -> f64 {
    match self { Value::I32(i) => *i as f64, Value::F64(f) => *f, _ => 0.0 }
    }
    fn as_bool(&self) -> bool {
    match self {
    Value::Bool(b) => *b, Value::Nil => false,
    Value::I32(i) => *i != 0, Value::F64(f) => *f != 0.0,
    Value::Str(s) => !s.is_empty(), _ => true,
    }
    }
    }
    fn substitute(text: &str, globals: &Arc<Mutex<HashMap<String, Value>>>, env: &HashMap<String, String>) -> String {
    let g = globals.lock().unwrap();
    let mut r = text.to_string();
    for (k, v) in g.iter() { r = r.replace(&format!("@{}", k), &v.to_string()); }
    for (k, v) in env { r = r.replace(&format!("${}", k), v); }
    r
    }
    fn bin_op(a: &Value, op: &str, b: &Value) -> Value {
    match op {
    "+" => if a.is_i32() && b.is_i32() { Value::I32(a.as_i32().wrapping_add(b.as_i32())) } else { Value::F64(a.as_f64() + b.as_f64()) },
    "-" => if a.is_i32() && b.is_i32() { Value::I32(a.as_i32().wrapping_sub(b.as_i32())) } else { Value::F64(a.as_f64() - b.as_f64()) },
    "*" => if a.is_i32() && b.is_i32() { Value::I32(a.as_i32().wrapping_mul(b.as_i32())) } else { Value::F64(a.as_f64() * b.as_f64()) },
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
    if let Value::List(l) = v { l.clone() } else { vec![] }
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
                let shell_prefix = if sudo { r#"Command::new("sudo").arg("sh")"# }
                else { r#"Command::new("sh")"# };
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
                for st in body.iter() { s += &self.transpile_stmt(&st.content, st.is_sudo); }
                s += "}\n";
                for (ec, eb) in else_ifs.iter() {
                    let ec = self.transpile_expr(ec);
                    s += &format!("else if {}.as_bool() {{\n", ec);
                    for st in eb.iter() { s += &self.transpile_stmt(&st.content, st.is_sudo); }
                    s += "}\n";
                }
                if let Some(eb) = else_body {
                    s += "else {\n";
                    for st in eb.iter() { s += &self.transpile_stmt(&st.content, st.is_sudo); }
                    s += "}\n";
                }
                s
            }
            Stmt::While { cond, body } => {
                let mut s = String::new();
                let c = self.transpile_expr(cond);
                s += &format!("while {}.as_bool() {{\n", c);
                for st in body.iter() { s += &self.transpile_stmt(&st.content, st.is_sudo); }
                s += "}\n";
                s
            }
            Stmt::For { var, iter, body } => {
                let mut s = String::new();
                let i = self.transpile_expr(iter);
                s += &format!("for v in to_iter(&{}) {{\n", i);
                s += &format!("globals.lock().unwrap().insert(\"{}\".to_string(), v);\n", var);
                for st in body.iter() { s += &self.transpile_stmt(&st.content, st.is_sudo); }
                s += "}\n";
                s
            }
            Stmt::Return { expr } => {
                format!("return {};\n", self.transpile_expr(expr))
            }
            Stmt::Repeat { count, body } => {
                let mut s = format!("for _ in 0..{} {{\n", count);
                for st in body.iter() { s += &self.transpile_stmt(&st.content, st.is_sudo); }
                s += "}\n";
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
            Expr::Lit(hl_plsa::Value::Str(s)) => {
                format!("Value::Str(\"{}\".to_string())", s.replace('"', "\\\""))
            }
            Expr::Lit(_) => "Value::Nil".to_string(),
            Expr::Var(name) => {
                format!("globals.lock().unwrap().get(\"{}\").cloned().unwrap_or(Value::Nil)", name)
            }
            Expr::BinOp { op, left, right } => {
                format!("bin_op(&{}, \"{}\", &{})",
                    self.transpile_expr(left), op, self.transpile_expr(right))
            }
            Expr::Call { name, args } => {
                let args_s = args.iter()
                .map(|a| self.transpile_expr(a))
                .collect::<Vec<_>>()
                .join(", ");
                format!("{}(vec![{}])", name, args_s)
            }
            _ => "Value::Nil".to_string(),
        }
    }

    fn transpile_fn(
        &mut self,
        name: &str,
        params: &[(String, String)],
                    _ret_ty: &Option<String>,
                    body: &Vec<ProgramNode>,
                    _is_quick: bool,
    ) {
        let mut f = format!("fn {}(args: Vec<Value>) -> Value {{\n", name);
        for (i, (pname, _)) in params.iter().enumerate() {
            f += &format!("let {} = args.get({}).cloned().unwrap_or(Value::Nil);\n", pname, i);
        }
        for node in body { f += &self.transpile_stmt(&node.content, node.is_sudo); }
        f += "Value::Nil\n}\n";
        self.fn_codes.push(f);
    }
}

// ─── Shared compile pipeline ──────────────────────────────────────────────────

struct CompileResult { bin_path: PathBuf }

fn do_compile(file: &str, verbose: bool) -> Result<CompileResult, ()> {
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    let mut cache_dir = PathBuf::from(home);
    cache_dir.push(".cache");
    cache_dir.push("hacker-lang");
    let _ = fs::create_dir_all(&cache_dir);
    println!();
    println!(" {} {}", "▸".magenta().bold(), file.bold());
    println!();
    let total = Instant::now();
    // 1. hash ──────────────────────────────────────────────────────────────────
    let pb = make_bar("hashing");
    pb.set_message("reading source...");
    pb.set_position(40);
    let hash = hash_file(file);
    bar_done(&pb, &format!("sha256 {}...", &hash[..12]));
    let bin_path = cache_dir.join(&hash);
    if bin_path.exists() {
        println!("\n {} cache hit — skipping compilation\n", "◆".yellow().bold());
        return Ok(CompileResult { bin_path });
    }
    // 2. parse ─────────────────────────────────────────────────────────────────
    let pb = make_bar("parsing");
    pb.set_message("building AST...");
    pb.set_position(10);
    let t = Instant::now();
    let mut seen = HashSet::new();
    let ast = match hl_plsa::parse_file(file, true, verbose, &mut seen) {
        Ok(a) => a,
        Err(errors) => {
            bar_fail(&pb, "parse error");
            println!();
            for e in errors { eprintln!(" {} {:?}", "│".red(), e); }
            return Err(());
        }
    };
    let fn_count = ast.functions.len();
    let stmt_count = ast.main_body.len();
    bar_done(&pb, &format!("{} stmts · {} fns · {}ms",
                           stmt_count, fn_count, t.elapsed().as_millis()));
    if verbose && ast.is_potentially_unsafe {
        println!(" {} script uses privileged (sudo) commands", "⚠".yellow());
    }
    // 3. transpile ─────────────────────────────────────────────────────────────
    let pb = make_bar("transpiling");
    pb.set_message("hl -> rust...");
    pb.set_position(20);
    let t = Instant::now();
    let mut transpiler = Transpiler::new();
    transpiler.transpile_ast(&ast);
    pb.set_position(70);
    pb.set_message("writing source...");
    let rs_code = transpiler.finish();
    let rs_bytes = rs_code.len();
    let rs_path = cache_dir.join(format!("{}.rs", hash));
    fs::write(&rs_path, &rs_code).map_err(|e| {
        bar_fail(&pb, "write failed");
        eprintln!(" {}", e);
    })?;
    bar_done(&pb, &format!("{} bytes · {}ms", rs_bytes, t.elapsed().as_millis()));
    // 4. cargo ─────────────────────────────────────────────────────────────────
    let pb = make_bar("compiling");
    pb.set_message("cargo build --release...");
    pb.set_position(5);
    let temp_dir = tempdir().map_err(|e| {
        bar_fail(&pb, "tempdir failed");
        eprintln!(" {}", e);
    })?;
    fs::write(temp_dir.path().join("Cargo.toml"), format!(
        "[package]\nname = \"hl_{hash}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
[dependencies]\ngc = \"0.5\"\ngc_derive = \"0.5\"\n"
    )).map_err(|e| { bar_fail(&pb, "Cargo.toml"); eprintln!(" {}", e); })?;
    let src_dir = temp_dir.path().join("src");
    fs::create_dir(&src_dir).map_err(|e| { bar_fail(&pb, "src dir"); eprintln!(" {}", e); })?;
    fs::write(src_dir.join("main.rs"), &rs_code)
    .map_err(|e| { bar_fail(&pb, "main.rs"); eprintln!(" {}", e); })?;
    pb.set_position(15);
    let t = Instant::now();
    let output = Command::new("cargo")
    .current_dir(temp_dir.path())
    .arg("build")
    .arg("--release")
    .output()
    .map_err(|e| { bar_fail(&pb, "cargo not found"); eprintln!(" {}", e); })?;
    pb.set_position(95);
    if !output.status.success() {
        bar_fail(&pb, "compilation failed");
        println!();
        eprintln!("{}", String::from_utf8_lossy(&output.stderr).red());
        return Err(());
    }
    let compiled_bin = temp_dir.path()
    .join("target/release")
    .join(format!("hl_{}", hash));
    let bin_size = fs::metadata(&compiled_bin).map(|m| m.len()).unwrap_or(0);
    fs::copy(&compiled_bin, &bin_path).map_err(|e| {
        bar_fail(&pb, "cache failed");
        eprintln!(" {}", e);
    })?;
    bar_done(&pb, &format!("{} KB · {}ms", bin_size / 1024, t.elapsed().as_millis()));
    println!();
    println!(" {} ready in {:.2}s\n", "◆".magenta().bold(), total.elapsed().as_secs_f64());
    Ok(CompileResult { bin_path })
}

// ─── Public commands ──────────────────────────────────────────────────────────

pub fn run_command(file: String, verbose: bool) -> bool {
    let result = match do_compile(&file, verbose) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let pb = make_bar("running");
    pb.set_message(file.dimmed().to_string());
    pb.set_position(5);
    let t = Instant::now();
    let status = Command::new(&result.bin_path).status();
    match status {
        Ok(s) if s.success() => {
            bar_done(&pb, &format!("exited ok · {}ms", t.elapsed().as_millis()));
            println!();
            true
        }
        Ok(s) => {
            let code = s.code().unwrap_or(-1);
            bar_fail(&pb, &format!("exit code {}", code));
            println!();
            false
        }
        Err(e) => {
            bar_fail(&pb, &format!("exec error: {}", e));
            println!();
            false
        }
    }
}

pub fn compile_command(file: String, output: String, verbose: bool) -> bool {
    let result = match do_compile(&file, verbose) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let out_stem = if let Some(p) = file.rfind('.') { &file[..p] } else { &file };
    let out = if output.is_empty() { out_stem.to_string() } else { output };
    let pb = make_bar("writing");
    pb.set_message(format!("-> {}", out.magenta()));
    pb.set_position(20);
    match fs::copy(&result.bin_path, &out) {
        Ok(bytes) => {
            bar_done(&pb, &format!("-> {} ({} KB)", out, bytes / 1024));
            println!();
            true
        }
        Err(e) => {
            bar_fail(&pb, "copy failed");
            eprintln!(" {}", e);
            println!();
            false
        }
    }
}

// ─── Utility ──────────────────────────────────────────────────────────────────

fn hash_file(path: &str) -> String {
    let b = fs::read(path).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(&b);
    hex::encode(h.finalize())
}
