use clap::Parser;
use colored::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

const CACHE_DIR: &str = "/tmp/Hacker-Lang/cache";
const PLSA_BIN_NAME: &str = "hl-plsa";

// ─────────────────────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────────────────────
#[derive(Parser, Debug)]
#[command(author, version, about = "hacker-lang runtime v1.6.3")]
struct Args {
    /// Plik .hl do uruchomienia
    file: String,
    /// Szczegółowe wyjście
    #[arg(long, short)]
    verbose: bool,
    /// Nie używaj cache bytecode
    #[arg(long)]
    no_cache: bool,
}

// ─────────────────────────────────────────────────────────────
// Typy AST — muszą być IDENTYCZNE z hl-plsa
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType {
    Source,
    Core,
    Bytes,
    Github,
    Virus,
    Vira,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibRef {
    pub lib_type: LibType,
    pub name: String,
    pub version: Option<String>,
}

/// KLUCZOWA POPRAWKA: tag = "type", content = "data" — identycznie jak w hl-plsa
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommandType {
    RawNoSub(String),
    RawSub(String),
    Isolated(String),
    AssignEnv {
        key: String,
        val: String,
    },
    AssignLocal {
        key: String,
        val: String,
        is_raw: bool,
    },
    Loop {
        count: u64,
        cmd: String,
    },
    If {
        cond: String,
        cmd: String,
    },
    Elif {
        cond: String,
        cmd: String,
    },
    Else {
        cmd: String,
    },
    While {
        cond: String,
        cmd: String,
    },
    For {
        var: String,
        in_: String,
        cmd: String,
    },
    Background(String),
    Call(String),
    /// KLUCZOWA POPRAWKA: dodano brakujące pole args
    Plugin {
        name: String,
        args: String,
        is_super: bool,
    },
    Log(String),
    Lock {
        key: String,
        val: String,
    },
    Unlock {
        key: String,
    },
    Extern {
        path: String,
        static_link: bool,
    },
    Enum {
        name: String,
        variants: Vec<String>,
    },
    Import {
        resource: String,
    },
    Struct {
        name: String,
        fields: Vec<(String, String)>,
    },
    Try {
        try_cmd: String,
        catch_cmd: String,
    },
    End {
        code: i32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramNode {
    pub line_num: usize,
    pub is_sudo: bool,
    pub content: CommandType,
    pub original_text: String,
    pub span: (usize, usize),
}

/// KLUCZOWA POPRAWKA: libs to Vec<LibRef>, nie Vec<String>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub deps: Vec<String>,
    pub libs: Vec<LibRef>,
    pub functions: HashMap<String, (bool, Vec<ProgramNode>)>,
    pub main_body: Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings: Vec<String>,
}

// ─────────────────────────────────────────────────────────────
// Bytecode VM
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
enum OpCode {
    /// Wykonaj polecenie powłoki
    Exec { cmd: String, sudo: bool },
    /// Ustaw zmienną środowiskową
    SetEnv { key: String, val: String },
    /// Ustaw zmienną lokalną
    SetLocal { key: String, val: String, is_raw: bool },
    /// Wywołaj funkcję
    CallFunc { func_name: String },
    /// Uruchom plugin
    Plugin { name: String, args: String, sudo: bool },
    /// Zablokuj bufor na stercie GC
    Lock { key: String, val: String },
    /// Zwolnij bufor ze sterty GC
    Unlock { key: String },
    /// Powrót z funkcji
    Return,
    /// Zakończ program
    Exit(i32),
}

#[derive(Serialize, Deserialize)]
struct BytecodeProgram {
    ops: Vec<OpCode>,
    /// Nazwa funkcji → indeks pierwszej instrukcji
    functions: HashMap<String, usize>,
}

// ─────────────────────────────────────────────────────────────
// GC (linkowany z gc.c)
// ─────────────────────────────────────────────────────────────
extern "C" {
    fn gc_malloc(size: usize) -> *mut c_void;
    fn gc_mark(ptr: *mut c_void);
    fn gc_unmark_all();
    fn gc_sweep();
}

// ─────────────────────────────────────────────────────────────
// Ścieżki
// ─────────────────────────────────────────────────────────────
fn get_plsa_path() -> PathBuf {
    let home = dirs::home_dir().expect("HOME not set");
    let path = home
    .join(".hackeros/hacker-lang/bin")
    .join(PLSA_BIN_NAME);
    if !path.exists() {
        eprintln!(
            "{} Krytyczny błąd: {} nie znaleziony pod {:?}",
            "[x]".red(),
                  PLSA_BIN_NAME,
                  path
        );
        std::process::exit(127);
    }
    path
}

fn get_plugins_root() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/plugins")
}

// ─────────────────────────────────────────────────────────────
// Kompilacja AST → Bytecode
// ─────────────────────────────────────────────────────────────
fn compile_to_bytecode(ast: &AnalysisResult) -> BytecodeProgram {
    let mut ops: Vec<OpCode> = Vec::new();
    let mut functions: HashMap<String, usize> = HashMap::new();

    // Kompiluj ciało główne
    compile_body(&ast.main_body, &mut ops);
    ops.push(OpCode::Exit(0));

    // Kompiluj funkcje
    for (name, (_is_unsafe, nodes)) in &ast.functions {
        functions.insert(name.clone(), ops.len());
        compile_body(nodes, &mut ops);
        ops.push(OpCode::Return);
    }

    BytecodeProgram { ops, functions }
}

fn compile_body(nodes: &[ProgramNode], ops: &mut Vec<OpCode>) {
    let mut i = 0;
    while i < nodes.len() {
        let node = &nodes[i];
        match &node.content {
            // Scalaj If/Elif/Else w jedno polecenie powłoki
            CommandType::If { cond, cmd } => {
                let mut shell_cmd = format!("if {}; then {}; ", cond, cmd);
                let mut sudo = node.is_sudo;
                i += 1;
                loop {
                    if i >= nodes.len() {
                        break;
                    }
                    match &nodes[i].content {
                        CommandType::Elif { cond, cmd } => {
                            shell_cmd += &format!("elif {}; then {}; ", cond, cmd);
                            sudo = sudo || nodes[i].is_sudo;
                            i += 1;
                        }
                        CommandType::Else { cmd } => {
                            shell_cmd += &format!("else {}; ", cmd);
                            sudo = sudo || nodes[i].is_sudo;
                            i += 1;
                            break;
                        }
                        _ => break,
                    }
                }
                shell_cmd += "fi";
                ops.push(OpCode::Exec {
                    cmd: shell_cmd,
                    sudo,
                });
            }
            _ => {
                compile_node(node, ops);
                i += 1;
            }
        }
    }
}

fn compile_node(node: &ProgramNode, ops: &mut Vec<OpCode>) {
    let sudo = node.is_sudo;
    match &node.content {
        CommandType::RawNoSub(s) => {
            ops.push(OpCode::Exec {
                cmd: s.clone(),
                     sudo,
            });
        }
        CommandType::RawSub(s) => {
            ops.push(OpCode::Exec {
                cmd: s.clone(),
                     sudo,
            });
        }
        CommandType::Isolated(s) => {
            ops.push(OpCode::Exec {
                cmd: format!("( {} )", s),
                     sudo,
            });
        }
        CommandType::AssignEnv { key, val } => {
            ops.push(OpCode::SetEnv {
                key: key.clone(),
                     val: val.clone(),
            });
        }
        CommandType::AssignLocal { key, val, is_raw } => {
            ops.push(OpCode::SetLocal {
                key: key.clone(),
                     val: val.clone(),
                     is_raw: *is_raw,
            });
        }
        CommandType::Loop { count, cmd } => {
            ops.push(OpCode::Exec {
                cmd: format!("for _hl_i in $(seq 1 {}); do {}; done", count, cmd),
                     sudo,
            });
        }
        CommandType::While { cond, cmd } => {
            ops.push(OpCode::Exec {
                cmd: format!("while {}; do {}; done", cond, cmd),
                     sudo,
            });
        }
        CommandType::For { var, in_, cmd } => {
            ops.push(OpCode::Exec {
                cmd: format!("for {} in {}; do {}; done", var, in_, cmd),
                     sudo,
            });
        }
        CommandType::Background(s) => {
            ops.push(OpCode::Exec {
                cmd: format!("{} &", s),
                     sudo,
            });
        }
        CommandType::Call(name) => {
            ops.push(OpCode::CallFunc {
                func_name: name.trim_start_matches('.').to_string(),
            });
        }
        CommandType::Plugin { name, args, is_super } => {
            ops.push(OpCode::Plugin {
                name: name.clone(),
                     args: args.clone(),
                     sudo: *is_super,
            });
        }
        CommandType::Log(msg) => {
            ops.push(OpCode::Exec {
                cmd: format!("echo {}", msg),
                     sudo,
            });
        }
        CommandType::Lock { key, val } => {
            ops.push(OpCode::Lock {
                key: key.clone(),
                     val: val.clone(),
            });
        }
        CommandType::Unlock { key } => {
            ops.push(OpCode::Unlock { key: key.clone() });
        }
        CommandType::Try { try_cmd, catch_cmd } => {
            ops.push(OpCode::Exec {
                cmd: format!("( {} ) || ( {} )", try_cmd, catch_cmd),
                     sudo,
            });
        }
        CommandType::End { code } => {
            ops.push(OpCode::Exit(*code));
        }
        CommandType::Extern { path, static_link } => {
            // Extern jest metadaną linkowania — tylko logujemy w verbose, nie wykonujemy
            let _ = (path, static_link);
        }
        CommandType::Enum { name, variants } => {
            // Enum jest metadaną typów — tylko logujemy
            let _ = (name, variants);
        }
        CommandType::Struct { name, fields } => {
            // Struct jest metadaną typów — tylko logujemy
            let _ = (name, fields);
        }
        CommandType::Import { resource } => {
            // Import obsługiwany przez PLSA (resolve_libs) — runtime ignoruje
            let _ = resource;
        }
        // If/Elif/Else obsługiwane w compile_body()
        CommandType::If { .. } | CommandType::Elif { .. } | CommandType::Else { .. } => {}
    }
}

// ─────────────────────────────────────────────────────────────
// VM
// ─────────────────────────────────────────────────────────────
#[derive(Debug)]
enum LocalVal {
    Raw(String),
    Managed(*mut c_char),
}

// SAFETY: VM jest jednowątkowa
unsafe impl Send for LocalVal {}
unsafe impl Sync for LocalVal {}

struct VM {
    env: HashMap<String, String>,
    locals: HashMap<String, LocalVal>,
    /// Bufory zaalokowane przez lock
    heap: HashMap<String, Vec<u8>>,
    verbose: bool,
}

impl VM {
    fn new(verbose: bool) -> Self {
        Self {
            env: std::env::vars().collect(),
            locals: HashMap::new(),
            heap: HashMap::new(),
            verbose,
        }
    }

    /// Podmienia $VAR i ${VAR} na wartości ze zmiennych lokalnych i środowiskowych
    fn substitute(&self, text: &str) -> String {
        let mut res = text.to_string();
        // Lokalne zmienne — najpierw (wyższy priorytet)
        for (k, val) in &self.locals {
            let v_str = match val {
                LocalVal::Raw(s) => s.clone(),
                LocalVal::Managed(p) => unsafe {
                    CStr::from_ptr(*p)
                    .to_str()
                    .unwrap_or("")
                    .to_string()
                },
            };
            res = res.replace(&format!("${{{}}}", k), &v_str);
            res = res.replace(&format!("${}", k), &v_str);
        }
        // Zmienne środowiskowe
        for (k, v) in &self.env {
            res = res.replace(&format!("${{{}}}", k), v);
            res = res.replace(&format!("${}", k), v);
        }
        res
    }

    /// Mark-and-sweep GC dla zarządzanych wartości lokalnych
    fn collect(&mut self) {
        unsafe {
            gc_unmark_all();
            for (_, val) in &self.locals {
                if let LocalVal::Managed(p) = val {
                    gc_mark(*p as *mut c_void);
                }
            }
            gc_sweep();
        }
    }

    fn run(&mut self, prog: &BytecodeProgram) -> i32 {
        let mut ip: usize = 0;
        let mut call_stack: Vec<usize> = Vec::new();

        while ip < prog.ops.len() {
            match &prog.ops[ip] {
                OpCode::Exec { cmd, sudo } => {
                    let final_cmd = self.substitute(cmd);
                    if self.verbose {
                        eprintln!("{} [{}] {}", "[>]".cyan(), ip, final_cmd.dimmed());
                    }
                    let status = if *sudo {
                        Command::new("sudo")
                        .arg("sh")
                        .arg("-c")
                        .arg(&final_cmd)
                        .status()
                    } else {
                        Command::new("sh")
                        .arg("-c")
                        .arg(&final_cmd)
                        .status()
                    };
                    match status {
                        Ok(s) if !s.success() => {
                            if self.verbose {
                                eprintln!(
                                    "{} Polecenie zakończone kodem: {}",
                                    "[!]".yellow(),
                                          s.code().unwrap_or(-1)
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("{} Błąd wykonania: {}", "[x]".red(), e);
                        }
                        _ => {}
                    }
                }

                OpCode::CallFunc { func_name } => {
                    // Obsłuż wywołania kwalifikowane: Storage.init → "Storage.init"
                    // oraz niekwalifikowane: "init"
                    let resolved = self.resolve_func(func_name, &prog.functions);
                    match resolved {
                        Some(addr) => {
                            if self.verbose {
                                eprintln!(
                                    "{} Call: {} → ip {}",
                                    "[f]".green(),
                                          func_name,
                                          addr
                                );
                            }
                            call_stack.push(ip + 1);
                            ip = addr;
                            continue;
                        }
                        None => {
                            eprintln!(
                                "{} Błąd runtime: funkcja '{}' nie znaleziona",
                                "[x]".red(),
                                      func_name
                            );
                            // Nie przerywamy — kontynuujemy
                        }
                    }
                }

                OpCode::Return => {
                    if let Some(ret_addr) = call_stack.pop() {
                        ip = ret_addr;
                        continue;
                    }
                    // Return na poziomie głównym == exit 0
                    return 0;
                }

                OpCode::Exit(code) => {
                    self.collect();
                    return *code;
                }

                OpCode::SetEnv { key, val } => {
                    let v = self.substitute(val);
                    if self.verbose {
                        eprintln!("{} SetEnv: {}={}", "[e]".blue(), key, v);
                    }
                    self.env.insert(key.clone(), v.clone());
                    // Propaguj do środowiska procesu
                    std::env::set_var(key, &v);
                }

                OpCode::SetLocal { key, val, is_raw } => {
                    let v = self.substitute(val);
                    if self.verbose {
                        eprintln!(
                            "{} SetLocal: {}={} (raw={})",
                                  "[l]".blue(),
                                  key,
                                  v,
                                  is_raw
                        );
                    }
                    if *is_raw {
                        self.locals.insert(key.clone(), LocalVal::Raw(v));
                    } else {
                        // Wartość zarządzana przez GC
                        match CString::new(v) {
                            Ok(cstr) => {
                                let size = cstr.as_bytes_with_nul().len();
                                let ptr = unsafe { gc_malloc(size) } as *mut c_char;
                                if ptr.is_null() {
                                    eprintln!("{} Alokacja GC nieudana dla: {}", "[x]".red(), key);
                                    return 1;
                                }
                                unsafe {
                                    std::ptr::copy_nonoverlapping(cstr.as_ptr(), ptr, size);
                                }
                                self.locals.insert(key.clone(), LocalVal::Managed(ptr));
                            }
                            Err(_) => {
                                // Wartość zawiera null — przechowaj jako Raw
                                eprintln!(
                                    "{} Ostrzeżenie: wartość '{}' zawiera bajt null — przechowuję jako Raw",
                                    "[!]".yellow(),
                                          key
                                );
                                self.locals.insert(
                                    key.clone(),
                                                   LocalVal::Raw("[invalid: null byte]".to_string()),
                                );
                            }
                        }
                    }
                }

                OpCode::Plugin { name, args, sudo } => {
                    let root = get_plugins_root();
                    let plugin_bin = root.join(name);
                    let plugin_hl = PathBuf::from(format!("{}.hl", plugin_bin.display()));

                    let final_args = self.substitute(args);

                    if self.verbose {
                        eprintln!(
                            "{} Plugin: \\\\{} {} (sudo={})",
                                  "[p]".cyan(),
                                  name,
                                  final_args,
                                  sudo
                        );
                    }

                    // Szukaj binarki, potem .hl
                    let target = if plugin_bin.exists() {
                        Some(plugin_bin.to_str().unwrap().to_string())
                    } else if plugin_hl.exists() {
                        // Uruchom .hl przez hl-runtime
                        let rt = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hl"));
                        Some(format!(
                            "{} {}",
                            rt.display(),
                                     plugin_hl.display()
                        ))
                    } else {
                        eprintln!(
                            "{} Plugin '{}' nie znaleziony pod: {}",
                            "[!]".yellow(),
                                  name,
                                  root.display()
                        );
                        None
                    };

                    if let Some(cmd_str) = target {
                        let full_cmd = if final_args.is_empty() {
                            cmd_str
                        } else {
                            format!("{} {}", cmd_str, final_args)
                        };
                        let status = if *sudo {
                            Command::new("sudo")
                            .arg("sh")
                            .arg("-c")
                            .arg(&full_cmd)
                            .status()
                        } else {
                            Command::new("sh").arg("-c").arg(&full_cmd).status()
                        };
                        if let Err(e) = status {
                            eprintln!("{} Błąd pluginu '{}': {}", "[x]".red(), name, e);
                        }
                    }
                }

                OpCode::Lock { key, val } => {
                    let k = self.substitute(key);
                    let v = self.substitute(val);
                    // val to rozmiar bufora lub string seed
                    let size = v.parse::<usize>().unwrap_or(v.len());
                    if self.verbose {
                        eprintln!("{} Lock: {} ({} bajtów)", "[m]".magenta(), k, size);
                    }
                    self.heap.insert(k, vec![0u8; size]);
                }

                OpCode::Unlock { key } => {
                    let k = self.substitute(key);
                    if self.verbose {
                        eprintln!("{} Unlock: {}", "[m]".magenta(), k);
                    }
                    self.heap.remove(&k);
                }
            }
            ip += 1;
        }

        self.collect();
        0
    }

    /// Rozwiązuje nazwę funkcji — obsługuje:
    ///   ".Storage.init"  → "Storage.init"
    ///   ".Tasks.add"     → "Tasks.add"
    ///   ".banner"        → "banner"
    fn resolve_func<'a>(
        &self,
        name: &str,
        functions: &'a HashMap<String, usize>,
    ) -> Option<usize> {
        // Usuń wiodące kropki z call_path
        let clean = name.trim_start_matches('.');
        if let Some(&addr) = functions.get(clean) {
            return Some(addr);
        }
        // Próbuj dopasowanie sufiksowe: "init" może pasować do "Storage.init"
        for (fname, &addr) in functions {
            if fname.ends_with(&format!(".{}", clean)) || fname == clean {
                return Some(addr);
            }
        }
        None
    }
}

// ─────────────────────────────────────────────────────────────
// Cache & generowanie bytecode
// ─────────────────────────────────────────────────────────────
fn get_file_hash(path: &str) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn generate_bytecode(file_path: &str, verbose: bool) -> BytecodeProgram {
    if verbose {
        eprintln!("{} Cache miss — analizuję źródło: {}", "[*]".yellow(), file_path);
    }

    let plsa_path = get_plsa_path();

    let output = Command::new(&plsa_path)
    .arg(file_path)
    .arg("--json")
    .arg("--resolve-libs")
    .output()
    .unwrap_or_else(|e| {
        eprintln!(
            "{} Nie można uruchomić hl-plsa ({:?}): {}",
                  "[x]".red(),
                  plsa_path,
                  e
        );
        std::process::exit(1);
    });

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{} hl-plsa zakończył z błędem:\n{}", "[x]".red(), stderr);
        std::process::exit(1);
    }

    let ast: AnalysisResult = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        eprintln!(
            "{} Nieprawidłowy JSON z PLSA: {}\n--- stdout ---\n{}",
            "[x]".red(),
                  e,
                  String::from_utf8_lossy(&output.stdout)
        );
        std::process::exit(1);
    });

    if verbose {
        eprintln!(
            "{} AST: {} funkcji, {} węzłów main, {} zależności",
            "[i]".blue(),
                  ast.functions.len(),
                  ast.main_body.len(),
                  ast.deps.len()
        );
        if ast.is_potentially_unsafe {
            eprintln!(
                "{} Ostrzeżenie: skrypt zawiera komendy uprzywilejowane (^/sudo):",
                      "[!]".yellow()
            );
            for w in &ast.safety_warnings {
                eprintln!("    {}", w.yellow());
            }
        }
    }

    compile_to_bytecode(&ast)
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────
fn main() {
    let args = Args::parse();

    // Utwórz katalog cache
    if let Err(e) = fs::create_dir_all(CACHE_DIR) {
        if args.verbose {
            eprintln!("{} Nie można utworzyć cache dir: {}", "[!]".yellow(), e);
        }
    }

    let hash = get_file_hash(&args.file);
    let bc_path = PathBuf::from(CACHE_DIR).join(format!("{}.bc", hash));

    let program: BytecodeProgram = if !args.no_cache && bc_path.exists() {
        if args.verbose {
            eprintln!("{} Cache hit: {}", "[*]".green(), bc_path.display());
        }
        match fs::read(&bc_path) {
            Ok(data) => match bincode::deserialize::<BytecodeProgram>(&data) {
                Ok(p) => p,
                Err(e) => {
                    if args.verbose {
                        eprintln!("{} Błąd odczytu cache ({}), regeneruję.", "[!]".yellow(), e);
                    }
                    let p = generate_bytecode(&args.file, args.verbose);
                    save_cache(&bc_path, &p, args.verbose);
                    p
                }
            },
            Err(_) => {
                let p = generate_bytecode(&args.file, args.verbose);
                save_cache(&bc_path, &p, args.verbose);
                p
            }
        }
    } else {
        let p = generate_bytecode(&args.file, args.verbose);
        if !args.no_cache {
            save_cache(&bc_path, &p, args.verbose);
        }
        p
    };

    let mut vm = VM::new(args.verbose);
    let start = Instant::now();
    let exit_code = vm.run(&program);

    if args.verbose {
        eprintln!(
            "{} Czas wykonania: {:?}",
            "[INFO]".blue(),
                  start.elapsed()
        );
    }

    std::process::exit(exit_code);
}

fn save_cache(path: &PathBuf, prog: &BytecodeProgram, verbose: bool) {
    match bincode::serialize(prog) {
        Ok(data) => {
            if let Err(e) = fs::write(path, data) {
                if verbose {
                    eprintln!("{} Nie można zapisać cache: {}", "[!]".yellow(), e);
                }
            } else if verbose {
                eprintln!("{} Cache zapisany: {}", "[*]".green(), path.display());
            }
        }
        Err(e) => {
            if verbose {
                eprintln!("{} Błąd serializacji cache: {}", "[!]".yellow(), e);
            }
        }
    }
}
