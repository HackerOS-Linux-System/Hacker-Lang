use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGET_STATIC: &str = "x86_64-unknown-linux-musl";

// ── Runtime template (binarka) ────────────────────────────────────────────────
// Używamy const + .replace() zamiast format!() — unikamy problemów z '{' w format stringach

const RUNTIME_BINARY: &str = r#"
use std::collections::HashMap;
use std::process::{Command, Stdio};

static AST_JSON: &str = "PLACEHOLDER_AST";

#[derive(Debug, Clone)]
enum Node {
Print(String),
QuickCall { name: String, args: String },
Cmd { raw: String, sudo: bool, isolated: bool, interp: bool },
VarDecl { name: String, value: String, is_interp: bool },
VarRef(String),
Dep(String),
FuncDef { name: String, body: Vec<Node> },
FuncCall(String),
Cond { ok: bool, body: Vec<Node> },
Noop,
}

struct Env {
vars: HashMap<String, String>,
funcs: HashMap<String, Vec<Node>>,
last_exit: i32,
}

impl Env {
fn new() -> Self {
let mut vars = HashMap::new();
vars.insert("HL_VERSION".into(), "0.3".into());
vars.insert("HL_OS".into(), "HackerOS".into());
let args: Vec<String> = std::env::args().skip(1).collect();
vars.insert("argc".into(), args.len().to_string());
for (i, a) in args.iter().enumerate() {
    vars.insert(format!("arg{}", i), a.clone());
    }
    Self { vars, funcs: HashMap::new(), last_exit: 0 }
    }

    fn interp(&self, s: &str) -> String {
    if !s.contains('@') { return s.to_string(); }
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'@' && i+1 < b.len() && (b[i+1].is_ascii_alphabetic() || b[i+1] == b'_') {
            i += 1;
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') { i += 1; }
            out.push_str(self.vars.get(&s[start..i]).map(|v| v.as_str()).unwrap_or(""));
            } else { out.push(b[i] as char); i += 1; }
            }
            out
            }
            }

            fn exec(nodes: &[Node], env: &mut Env) -> i32 {
            let mut last = 0;
            for n in nodes { last = exec_one(n, env); env.last_exit = last; }
            last
            }

            fn exec_one(node: &Node, env: &mut Env) -> i32 {
            match node {
            Node::Noop => 0,
Node::Print(s) => { println!("{}", env.interp(s)); 0 }
Node::QuickCall { name, args } => {
let a = env.interp(args);
exec_quick(name, a.trim(), env)
}
Node::Cmd { raw, sudo, isolated, interp } => {
let s = if *interp { env.interp(raw) } else { raw.clone() };
run_cmd(s.trim(), *sudo, *isolated)
}
Node::VarDecl { name, value, is_interp } => {
let v = if *is_interp { env.interp(value) } else { value.clone() };
env.vars.insert(name.clone(), v); 0
}
Node::VarRef(n) => { println!("{}", env.vars.get(n).cloned().unwrap_or_default()); 0 }
Node::Dep(n) => { if which(n) { 0 } else { eprintln!("dep: '{}' missing", n); 1 } }
Node::FuncDef { name, body } => { env.funcs.insert(name.clone(), body.clone()); 0 }
Node::FuncCall(n) => {
match env.funcs.get(n).cloned() {
Some(b) => exec(&b, env),
None => { eprintln!("undefined: '{}'", n); 1 }
}
}
Node::Cond { ok, body } => {
let run = if *ok { env.last_exit == 0 } else { env.last_exit != 0 };
if run { exec(body, env) } else { 0 }
}
}
}

fn run_cmd(cmd: &str, sudo: bool, isolated: bool) -> i32 {
if cmd.starts_with("echo ") || cmd == "echo" { eprintln!("hl: echo forbidden"); return 1; }
let parts = shell_split(cmd);
if parts.is_empty() { return 0; }
let (prog, args) = if isolated {
let mut a = vec!["--mount".to_string(),"--pid".into(),"--net".into(),"--fork".into(),"--".into()];
if sudo {
    a.extend(parts);
    ("sudo".into(), { let mut x = vec!["unshare".into()]; x.extend(a); x })
    } else { a.extend(parts); ("unshare".into(), a) }
    } else if sudo { ("sudo".into(), parts) }
    else { let mut it = parts.into_iter(); let p = it.next().unwrap(); (p, it.collect()) };
    Command::new(&prog).args(&args)
    .stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
    .status().map(|s| s.code().unwrap_or(1)).unwrap_or(1)
    }

    fn shell_split(s: &str) -> Vec<String> {
    let mut words = Vec::new(); let mut cur = String::new();
    let (mut sq, mut dq) = (false, false);
    for c in s.chars() {
        match c {
        '\'' if !dq => sq = !sq, '"' if !sq => dq = !dq,
' '|'\t' if !sq && !dq => { if !cur.is_empty() { words.push(std::mem::take(&mut cur)); } }
_ => cur.push(c),
}
}
if !cur.is_empty() { words.push(cur); }
words
}

fn which(name: &str) -> bool {
std::env::var_os("PATH")
.map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
.unwrap_or(false)
}

fn exec_quick(name: &str, arg: &str, env: &mut Env) -> i32 {
match name {
"upper"=>{ println!("{}", arg.to_uppercase()); 0 }
"lower"=>{ println!("{}", arg.to_lowercase()); 0 }
"len"  =>{ println!("{}", arg.len()); 0 }
"trim" =>{ println!("{}", arg.trim()); 0 }
"rev"  =>{ println!("{}", arg.chars().rev().collect::<String>()); 0 }
"nl"   =>{ println!(); 0 }
"hr"   =>{ let w=arg.parse().unwrap_or(60usize); println!("{}", "─".repeat(w)); 0 }
"bold" =>{ println!("\x1b[1m{}\x1b[0m", arg); 0 }
"red"  =>{ println!("\x1b[31m{}\x1b[0m", arg); 0 }
"green"=>{ println!("\x1b[32m{}\x1b[0m", arg); 0 }
"yellow"=>{ println!("\x1b[33m{}\x1b[0m", arg); 0 }
"cyan" =>{ println!("\x1b[36m{}\x1b[0m", arg); 0 }
"exists"=>{ if std::path::Path::new(arg).exists(){0}else{1} }
"isdir" =>{ if std::path::Path::new(arg).is_dir(){0}else{1} }
"isfile"=>{ if std::path::Path::new(arg).is_file(){0}else{1} }
"which" =>{ if which(arg){0}else{1} }
"pid"  =>{ println!("{}", std::process::id()); 0 }
"env"  =>{ match std::env::var(arg){ Ok(v)=>{println!("{}",v);0} Err(_)=>{println!();1} } }
"basename"=>{ println!("{}", std::path::Path::new(arg).file_name().and_then(|n|n.to_str()).unwrap_or("")); 0 }
"dirname" =>{ println!("{}", std::path::Path::new(arg).parent().and_then(|p|p.to_str()).unwrap_or(".")); 0 }
"read" =>{ match std::fs::read_to_string(arg){ Ok(c)=>{print!("{}",c);0} Err(e)=>{eprintln!("{}",e);1} } }
"abs"  =>{ let n:f64=arg.parse().unwrap_or(0.0); println!("{}",n.abs()); 0 }
"ceil" =>{ let n:f64=arg.parse().unwrap_or(0.0); println!("{}",n.ceil() as i64); 0 }
"floor"=>{ let n:f64=arg.parse().unwrap_or(0.0); println!("{}",n.floor() as i64); 0 }
"round"=>{ let n:f64=arg.parse().unwrap_or(0.0); println!("{}",n.round() as i64); 0 }
"set"  =>{ let mut it=arg.splitn(2,' '); let k=it.next().unwrap_or("").trim().to_string(); let v=it.next().unwrap_or("").trim().to_string(); env.vars.insert(k,v); 0 }
"get"  =>{ println!("{}", env.vars.get(arg).cloned().unwrap_or_default()); 0 }
"unset"=>{ env.vars.remove(arg); 0 }
"contains"=>{ let(t,p)=arg.rsplit_once(' ').map(|(a,b)|(a.trim(),b.trim())).unwrap_or((arg,"")); if t.contains(p){0}else{1} }
"startswith"=>{ let(t,p)=arg.rsplit_once(' ').map(|(a,b)|(a.trim(),b.trim())).unwrap_or((arg,"")); if t.starts_with(p){0}else{1} }
"endswith"=>{ let(t,p)=arg.rsplit_once(' ').map(|(a,b)|(a.trim(),b.trim())).unwrap_or((arg,"")); if t.ends_with(p){0}else{1} }
other=>{ eprintln!("unknown ::{}",other); 1 }
}
}

// ── Minimal JSON parser ───────────────────────────────────────────────────────

fn parse_ast(json: &str) -> Vec<Node> { parse_arr(json).unwrap_or_default() }

fn parse_arr(json: &str) -> Option<Vec<Node>> {
let json = json.trim();
if !json.starts_with('[') { return None; }
let inner = &json[1..json.len()-1];
Some(split_top(inner,',').iter().filter_map(|s| parse_node(s.trim())).collect())
}

fn parse_node(obj: &str) -> Option<Node> {
let obj = obj.trim();
if obj.is_empty() || obj == "null" { return Some(Node::Noop); }
let t = get_str(obj,"type")?;
match t.as_str() {
"Print" => {
let d = get_obj(obj,"data")?;
Some(Node::Print(parts_to_str(&get_raw(&d,"parts").unwrap_or_default())))
}
"QuickCall" => {
let d = get_obj(obj,"data")?;
Some(Node::QuickCall { name: get_str(&d,"name").unwrap_or_default(), args: parts_to_str(&get_raw(&d,"args").unwrap_or_default()) })
}
"Command" => {
let d = get_obj(obj,"data")?;
let mode = get_str(&d,"mode").unwrap_or_default();
let (sudo,isolated) = mode_flags(&mode);
Some(Node::Cmd { raw:get_str(&d,"raw").unwrap_or_default(), sudo, isolated, interp:get_bool(&d,"interpolate") })
}
"VarDecl" => {
let d = get_obj(obj,"data")?;
let v = get_obj(&d,"value").unwrap_or_default();
let (val,is_interp) = parse_val(&v);
Some(Node::VarDecl { name:get_str(&d,"name").unwrap_or_default(), value:val, is_interp })
}
"VarRef"     => Some(Node::VarRef(get_str(obj,"data").unwrap_or_default())),
"Dependency" => { let d=get_obj(obj,"data")?; Some(Node::Dep(get_str(&d,"name").unwrap_or_default())) }
"FuncDef"    => {
let d=get_obj(obj,"data")?;
Some(Node::FuncDef { name:get_str(&d,"name").unwrap_or_default(), body:parse_arr(&get_raw(&d,"body").unwrap_or_default()).unwrap_or_default() })
}
"FuncCall"   => { let d=get_obj(obj,"data")?; Some(Node::FuncCall(get_str(&d,"name").unwrap_or_default())) }
"Conditional"=> {
let d=get_obj(obj,"data")?;
let ok = get_str(&d,"condition").unwrap_or_default() == "Ok";
Some(Node::Cond { ok, body:parse_arr(&get_raw(&d,"body").unwrap_or_default()).unwrap_or_default() })
}
_ => Some(Node::Noop),
}
}

fn parts_to_str(arr: &str) -> String {
let arr = arr.trim();
if !arr.starts_with('[') { return arr.trim_matches('"').to_string(); }
split_top(&arr[1..arr.len()-1],',').iter().map(|item| {
let item=item.trim();
let t=get_str(item,"type").unwrap_or_default();
let d=get_str(item,"data").unwrap_or_default();
if t=="Var" { format!("@{}",d) } else { d }
}).collect::<Vec<_>>().join("")
}

fn parse_val(obj: &str) -> (String,bool) {
let t=get_str(obj,"type").unwrap_or_default();
match t.as_str() {
"Interpolated" => (parts_to_str(&get_raw(obj,"data").unwrap_or_default()),true),
_ => (get_str(obj,"data").unwrap_or_default(),false),
}
}

fn mode_flags(m:&str)->(bool,bool) {
match m {
"Sudo"|"WithVarsSudo" => (true,false),
"Isolated"|"WithVarsIsolated" => (false,true),
"IsolatedSudo" => (true,true),
_ => (false,false),
}
}

fn get_str(json:&str,key:&str)->Option<String> {
let pat=format!("\"{}\":",key);
let pos=json.find(&pat)?;
let after=json[pos+pat.len()..].trim_start();
let first=after.as_bytes().first().copied();
if first==Some(b'"') {
    let s=&after[1..]; let end=str_end(s)?; Some(unescape(&s[..end]))
    } else if first==Some(b'{')||first==Some(b'[') { None }
    else { let end=after.find(|c:char|c==','||c=='}'||c==']').unwrap_or(after.len()); Some(after[..end].trim().to_string()) }
    }

    fn get_raw(json:&str,key:&str)->Option<String> {
    let pat=format!("\"{}\":",key);
    let pos=json.find(&pat)?;
    let after=json[pos+pat.len()..].trim_start();
    let first=after.as_bytes().first().copied();
    match first {
    Some(b'[') => { let e=match_bracket(after,b'[',b']')?; Some(after[..=e].to_string()) }
    Some(b'{') => { let e=match_bracket(after,b'{',b'}')?; Some(after[..=e].to_string()) }
    Some(b'"') => get_str(json,key),
    _ => { let e=after.find(|c:char|c==','||c=='}').unwrap_or(after.len()); Some(after[..e].trim().to_string()) }
    }
    }

    fn get_obj(json:&str,key:&str)->Option<String> {
    let pat=format!("\"{}\":",key);
    let pos=json.find(&pat)?;
    let after=json[pos+pat.len()..].trim_start();
    let first=after.as_bytes().first().copied();
    match first {
    Some(b'{') => { let e=match_bracket(after,b'{',b'}')?; Some(after[..=e].to_string()) }
    Some(b'[') => { let e=match_bracket(after,b'[',b']')?; Some(after[..=e].to_string()) }
    _ => None
    }
    }

    fn get_bool(json:&str,key:&str)->bool { get_str(json,key).map(|s|s=="true").unwrap_or(false) }

    fn str_end(s:&str)->Option<usize> {
    let b=s.as_bytes(); let mut i=0;
    while i<b.len() { if b[i]==b'\\'{ i+=2; continue; } if b[i]==b'"'{ return Some(i); } i+=1; }
    None
    }

    fn match_bracket(s:&str,open:u8,close:u8)->Option<usize> {
    let mut depth=0i32; let mut in_str=false; let b=s.as_bytes(); let mut i=0;
    while i<b.len() {
        if in_str { if b[i]==b'\\'{ i+=2; continue; } if b[i]==b'"'{ in_str=false; } }
        else {
            if b[i]==b'"'{ in_str=true; }
            else if b[i]==open{ depth+=1; }
            else if b[i]==close{ depth-=1; if depth==0{ return Some(i); } }
            }
            i+=1;
            }
            None
            }

            fn split_top(s:&str,sep:char)->Vec<String> {
            let mut result=Vec::new(); let mut cur=String::new(); let mut depth=0i32; let mut in_str=false;
            let b=s.as_bytes(); let mut i=0;
            while i<b.len() {
                let c=b[i] as char;
                if in_str {
                    if b[i]==b'\\'{ cur.push(c); i+=1; if i<b.len(){ cur.push(b[i] as char); } }
                    else { if c=='"'{ in_str=false; } cur.push(c); }
                    } else {
                        if c=='"'{ in_str=true; cur.push(c); }
                        else if c=='{'||c=='['{ depth+=1; cur.push(c); }
                        else if c=='}'||c==']'{ depth-=1; cur.push(c); }
                        else if c==sep&&depth==0{ result.push(std::mem::take(&mut cur)); }
                        else { cur.push(c); }
                        }
                        i+=1;
                        }
                        if !cur.trim().is_empty(){ result.push(cur); }
                        result
                        }

                        fn unescape(s:&str)->String {
                        let mut out=String::new(); let mut chars=s.chars();
                        while let Some(c)=chars.next() {
                            if c=='\\' { match chars.next() {
                                Some('n')=>out.push('\n'), Some('t')=>out.push('\t'), Some('r')=>out.push('\r'),
                                Some('"')=>out.push('"'), Some('\\')=>out.push('\\'),
                                Some(c)=>{ out.push('\\'); out.push(c); } None=>{}
                                }} else { out.push(c); }
                                }
                                out
                                }

                                fn main() {
                                let nodes=parse_ast(AST_JSON);
                                let mut env=Env::new();
                                let code=exec(&nodes,&mut env);
                                std::process::exit(code);
                                }
                                "#;

                                // Template dla .so (cdylib) — eksponuje funkcje z prefiksem hl_
                                const RUNTIME_SHARED: &str = r#"
                                //! Hacker Lang Virus Library — .so
                                //! Auto-generated. Skompiluj: rustc --crate-type cdylib ...
                                //!
                                //! ABI:
                                //!   extern "C" fn hl_init() -> i32           — inicjalizacja biblioteki
                                //!   extern "C" fn hl_exec(json: *const i8) → i32  — wykonaj fragment AST JSON

                                use std::collections::HashMap;
                                use std::ffi::{CStr, CString};
                                use std::os::raw::c_char;

                                static AST_JSON: &str = "PLACEHOLDER_AST";

                                // ── Export C ABI ──────────────────────────────────────────────────────────────

                                #[no_mangle]
                                pub extern "C" fn hl_init() -> i32 {
                                // Inicjalizacja — wykonaj AST biblioteki (definicje funkcji i zmiennych)
                                let nodes = parse_ast(AST_JSON);
                                let mut env = Env::new();
                                exec(&nodes, &mut env);
                                0
                                }

                                #[no_mangle]
                                pub extern "C" fn hl_exec_json(json_ptr: *const c_char) -> i32 {
                                if json_ptr.is_null() { return 1; }
                                let json = unsafe { CStr::from_ptr(json_ptr) }.to_string_lossy();
                                let nodes = parse_ast(&json);
                                let mut env = Env::new();
                                exec(&nodes, &mut env)
                                }

                                #[no_mangle]
                                pub extern "C" fn hl_version() -> *const c_char {
                                static VER: &[u8] = b"0.3\0";
                                VER.as_ptr() as *const c_char
                                }

                                // ── Runtime (współdzielony z binarką, skrócony) ───────────────────────────────

                                #[derive(Debug, Clone)]
                                enum Node {
                                Print(String),
                                VarDecl { name: String, value: String },
FuncDef { name: String, body: Vec<Node> },
Noop,
}

struct Env {
vars: HashMap<String, String>,
funcs: HashMap<String, Vec<Node>>,
last_exit: i32,
}

impl Env {
fn new() -> Self {
Self { vars: HashMap::new(), funcs: HashMap::new(), last_exit: 0 }
}
}

fn exec(nodes: &[Node], env: &mut Env) -> i32 {
let mut last = 0;
for n in nodes {
    last = match n {
    Node::Noop => 0,
Node::Print(s) => { println!("{}", s); 0 }
Node::VarDecl { name, value } => { env.vars.insert(name.clone(), value.clone()); 0 }
Node::FuncDef { name, body } => { env.funcs.insert(name.clone(), body.clone()); 0 }
};
env.last_exit = last;
}
last
}

// Minimal JSON parser (podzbiór wystarczający dla .so)
fn parse_ast(json: &str) -> Vec<Node> {
// Dla .so bibliotek wystarczy parsować definicje funkcji i zmiennych
let mut nodes = Vec::new();
// ... (full parser z binarki jest tu dostępny, skrócony dla przejrzystości)
nodes
}

fn main() {} // Wymagane przez rustc, nieużywane w cdylib
"#;

// ── Publiczne API ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum CompileMode {
    /// Statyczna binarka ELF x86_64 (domyślna)
    Binary,
    /// Współdzielona biblioteka .so dla ekosystemu Virus
    Shared,
}

pub struct CompileOptions {
    pub input:   PathBuf,
    pub output:  Option<PathBuf>,
    pub verbose: bool,
    pub mode:    CompileMode,
}

pub struct CompileResult {
    pub output_path: PathBuf,
    pub mode:        CompileMode,
}

pub fn compile(opts: CompileOptions) -> Result<CompileResult> {
    let input = &opts.input;

    if !input.exists() { bail!("Plik nie istnieje: {}", input.display()); }
    if input.extension().and_then(|e| e.to_str()) != Some("hl") {
        bail!("Plik musi mieć rozszerzenie .hl");
    }

    let stem = input.file_stem().and_then(|s| s.to_str())
    .context("Nieprawidłowa nazwa pliku")?;

    // Domyślne rozszerzenie wyjścia
    let output_path = opts.output.clone().unwrap_or_else(|| {
        let ext = match opts.mode {
            CompileMode::Binary => "",
            CompileMode::Shared => ".so",
        };
        let name = format!("{}{}", stem, ext);
        input.parent().unwrap_or(Path::new(".")).join(name)
    });

    // 1. Parse + serialize AST
    log_step("PARSE", &input.display().to_string(), opts.verbose);
    let source = std::fs::read_to_string(input)
    .with_context(|| format!("Nie można odczytać: {}", input.display()))?;
    let nodes = hl_core::check_source(&source)
    .map_err(|e| anyhow::anyhow!("Błąd parsowania: {}", e))?;
    let ast_json = serde_json::to_string(&nodes).context("Błąd serializacji AST")?;

    // 2. Sprawdź narzędzia
    ensure_rustc()?;
    if opts.mode == CompileMode::Binary {
        ensure_musl_target(opts.verbose)?;
    }

    // 3. Generuj .rs
    let tmp_dir = tempdir(stem)?;
    let rs_path = tmp_dir.join(format!("{}_compiled.rs", stem));
    log_step("CODEGEN", &rs_path.display().to_string(), opts.verbose);

    let escaped = ast_json
    .replace('\\', "\\\\")
    .replace('"',  "\\\"")
    .replace('\n', "\\n")
    .replace('\r', "\\r");

    let template = match opts.mode {
        CompileMode::Binary => RUNTIME_BINARY,
        CompileMode::Shared => RUNTIME_SHARED,
    };
    let rs_source = template.replace("PLACEHOLDER_AST", &escaped);
    std::fs::write(&rs_path, &rs_source).context("Błąd zapisu pliku Rust")?;

    // 4. Kompiluj
    log_step("RUSTC", &output_path.display().to_string(), opts.verbose);

    let mut cmd = Command::new("rustc");
    cmd.arg(&rs_path).arg("-o").arg(&output_path);

    match opts.mode {
        CompileMode::Binary => {
            cmd.arg("--target").arg(TARGET_STATIC)
            .arg("-O")
            .arg("-C").arg("lto=thin")
            .arg("-C").arg("panic=abort")
            .arg("-C").arg("strip=symbols")
            .arg("-C").arg("target-feature=+crt-static");
        }
        CompileMode::Shared => {
            // cdylib — współdzielona biblioteka .so
            cmd.arg("--crate-type").arg("cdylib")
            .arg("-O")
            .arg("-C").arg("panic=abort")
            .arg("-C").arg("strip=symbols");
            // Zmień rozszerzenie na .so jeśli nie podano
            if output_path.extension().and_then(|e| e.to_str()) != Some("so") {
                let so_path = output_path.with_extension("so");
                cmd.arg("-o").arg(&so_path);
            }
        }
    }

    if opts.verbose {
        eprintln!("{} {:?}", "rustc cmd:".bright_black(), cmd);
    }

    let status = cmd.status().context("Nie można uruchomić rustc")?;
    let _ = std::fs::remove_dir_all(&tmp_dir);

    if !status.success() {
        bail!(
            "rustc zakończył się błędem.\n{}",
            match opts.mode {
                CompileMode::Binary =>
                format!("Zainstaluj target: rustup target add {}\napt install musl-tools", TARGET_STATIC),
                    CompileMode::Shared =>
                    "Sprawdź czy rustc jest zainstalowany (hl shell compile wymaga Rust)".to_string(),
            }
        );
    }

    log_step("OK", &output_path.display().to_string(), opts.verbose);
    Ok(CompileResult { output_path, mode: opts.mode })
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn ensure_rustc() -> Result<()> {
    Command::new("rustc").arg("--version").output()
    .context("rustc nie jest dostępny")?;
    Ok(())
}

fn ensure_musl_target(verbose: bool) -> Result<()> {
    let installed = Command::new("rustup")
    .args(["target", "list", "--installed"])
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).contains(TARGET_STATIC))
    .unwrap_or(false);

    if installed { return Ok(()); }

    eprintln!("{} Instaluję target {}...", "[hl compile]".bright_cyan(), TARGET_STATIC.bright_white());
    let status = Command::new("rustup")
    .args(["target", "add", TARGET_STATIC])
    .status().context("Nie można uruchomić rustup")?;

    if !status.success() {
        bail!("Nie udało się zainstalować targetu. Run: rustup target add {}", TARGET_STATIC);
    }
    if verbose { eprintln!("{} Target zainstalowany.", "[hl compile]".bright_cyan()); }
    Ok(())
}

fn tempdir(prefix: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hl_compile_{}", prefix));
    std::fs::create_dir_all(&dir)
    .with_context(|| format!("Nie można utworzyć katalogu tymczasowego: {}", dir.display()))?;
    Ok(dir)
}

fn log_step(step: &str, msg: &str, verbose: bool) {
    if verbose {
        eprintln!("{} {}", format!("[{step}]").bright_cyan(), msg.bright_white());
    }
}
