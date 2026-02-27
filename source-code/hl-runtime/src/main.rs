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
use std::time::{Instant, UNIX_EPOCH};

const PLSA_BIN_NAME: &str = "hl-plsa";
/// Wersja 4 — nowy format OpCode z JumpIfFalse/Jump
const CACHE_SCHEMA_VERSION: u32 = 4;

// ─────────────────────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────────────────────
#[derive(Parser, Debug)]
#[command(
author  = "HackerOS",
version = "2.2.0",
about   = "hacker-lang runtime — generacyjny GC, bytecode VM z VM-level branching"
)]
struct Args {
    file: String,
    #[arg(long, short)] verbose:  bool,
    #[arg(long)]        no_cache: bool,
    #[arg(long)]        gc_stats: bool,
    #[arg(long)]        dry_run:  bool,
}

// ─────────────────────────────────────────────────────────────
// AST — identyczne z hl-plsa/main.rs
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType { Source, Core, Bytes, Github, Virus, Vira }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibRef {
    pub lib_type: LibType,
    pub name:     String,
    pub version:  Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommandType {
    RawNoSub(String),
    RawSub(String),
    Isolated(String),
    AssignEnv    { key: String, val: String },
    AssignLocal  { key: String, val: String, is_raw: bool },
    Loop         { count: u64, cmd: String },
    If           { cond: String, cmd: String },
    Elif         { cond: String, cmd: String },
    Else         { cmd: String },
    While        { cond: String, cmd: String },
    For          { var: String, in_: String, cmd: String },
    Background(String),
    Call(String),
    Plugin       { name: String, args: String, is_super: bool },
    Log(String),
    Lock         { key: String, val: String },
    Unlock       { key: String },
    Extern       { path: String, static_link: bool },
    Enum         { name: String, variants: Vec<String> },
    Import       { resource: String },
    Struct       { name: String, fields: Vec<(String, String)> },
    Try          { try_cmd: String, catch_cmd: String },
    End          { code: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramNode {
    pub line_num:      usize,
    pub is_sudo:       bool,
    pub content:       CommandType,
    pub original_text: String,
    pub span:          (usize, usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub deps:                  Vec<String>,
    pub libs:                  Vec<LibRef>,
    pub functions:             HashMap<String, (bool, Vec<ProgramNode>)>,
    pub main_body:             Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings:       Vec<String>,
}

// ─────────────────────────────────────────────────────────────
// Bytecode — rozszerzony o JumpIfFalse i Jump
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
enum OpCode {
    /// Wykonaj komendę shell przez: bash -c CMD
    Exec        { cmd: String, sudo: bool },
    /// Ustaw zmienną środowiskową
    SetEnv      { key: String, val: String },
    /// Ustaw zmienną lokalną (zarządzaną przez GC lub Raw)
    SetLocal    { key: String, val: String, is_raw: bool },
    /// Wywołaj funkcję HL (push ip+1 na call stack, skocz do addr)
    CallFunc    { func_name: String },
    /// Uruchom plugin z ~/.hackeros/hacker-lang/plugins/
    Plugin      { name: String, args: String, sudo: bool },
    /// Zaalokuj blok pamięci heap
    Lock        { key: String, val: String },
    /// Zwolnij blok pamięci heap
    Unlock      { key: String },
    /// Powrót z funkcji HL (pop call stack)
    Return,
    /// Zakończ program z kodem wyjścia
    Exit(i32),
    /// Ewaluuj warunek COND przez bash.
    /// Jeśli FALSE (exit code != 0) → ip = target.
    /// Jeśli TRUE  (exit code == 0) → fall-through (ip + 1).
    JumpIfFalse { cond: String, target: usize },
    /// Bezwarunkowy skok do ops[target].
    /// Używany po każdej gałęzi if/elif żeby przeskoczyć pozostałe gałęzie.
    Jump        { target: usize },
}

#[derive(Serialize, Deserialize)]
struct BytecodeProgram {
    schema_version: u32,
    ops:            Vec<OpCode>,
    functions:      HashMap<String, usize>,
}

// ─────────────────────────────────────────────────────────────
// Cache metadata
// ─────────────────────────────────────────────────────────────
#[derive(Serialize, Deserialize)]
struct CacheMeta {
    sha256:         String,
    mtime:          u64,
    file_size:      u64,
    schema_version: u32,
}

// ─────────────────────────────────────────────────────────────
// GC FFI — gc.c v2
// ─────────────────────────────────────────────────────────────
extern "C" {
    fn gc_malloc(size: usize) -> *mut c_void;
    fn gc_alloc_old(size: usize) -> *mut c_void;
    fn gc_mark(ptr: *mut c_void);
    fn gc_unmark_all();
    fn gc_sweep();
    fn gc_collect_full();
    fn gc_stats_print();
    fn gc_stats_get(minor_out: *mut u64, major_out: *mut u64, promoted_out: *mut u64, total_out: *mut u64);
}

// ─────────────────────────────────────────────────────────────
// Ścieżki
// ─────────────────────────────────────────────────────────────
fn cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        let p = PathBuf::from(xdg).join("hacker-lang");
        if fs::create_dir_all(&p).is_ok() { return p; }
    }
    let p = dirs::home_dir().expect("HOME not set").join(".cache").join("hacker-lang");
    fs::create_dir_all(&p).ok();
    p
}

fn get_plsa_path() -> PathBuf {
    let path = dirs::home_dir().expect("HOME not set")
    .join(".hackeros/hacker-lang/bin")
    .join(PLSA_BIN_NAME);
    if !path.exists() {
        eprintln!("{} Krytyczny błąd: {} nie znaleziony pod {:?}", "[x]".red(), PLSA_BIN_NAME, path);
        std::process::exit(127);
    }
    path
}

fn get_plugins_root() -> PathBuf {
    dirs::home_dir().expect("HOME not set").join(".hackeros/hacker-lang/plugins")
}

// ─────────────────────────────────────────────────────────────
// Cache — SHA-256 + mtime hybryda
// ─────────────────────────────────────────────────────────────
fn file_mtime_size(path: &str) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mt   = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some((mt, meta.len()))
}

fn file_sha256(path: &str) -> String {
    let mut h = Sha256::new();
    h.update(fs::read(path).unwrap_or_default());
    format!("{:x}", h.finalize())
}

fn cache_paths(src: &str) -> (PathBuf, PathBuf) {
    let mut h = Sha256::new();
    h.update(src.as_bytes());
    let key = format!("{:x}", h.finalize());
    let d   = cache_dir();
    (d.join(format!("{}.bc", key)), d.join(format!("{}.meta", key)))
}

fn cache_load(src_path: &str, verbose: bool) -> Option<BytecodeProgram> {
    let (bc_path, meta_path) = cache_paths(src_path);
    let meta: CacheMeta = bincode::deserialize(&fs::read(&meta_path).ok()?).ok()?;

    if meta.schema_version != CACHE_SCHEMA_VERSION {
        if verbose { eprintln!("{} Cache: nieaktualna wersja schematu, regeneruję.", "[*]".yellow()); }
        return None;
    }

    if let Some((mtime, size)) = file_mtime_size(src_path) {
        if mtime == meta.mtime && size == meta.file_size {
            if verbose { eprintln!("{} Cache hit (mtime+size): {}", "[*]".green(), bc_path.display()); }
            return load_bc(&bc_path, verbose);
        }
        let sha = file_sha256(src_path);
        if sha == meta.sha256 {
            if verbose { eprintln!("{} Cache hit (sha256): {}", "[*]".green(), bc_path.display()); }
            let nm = CacheMeta { sha256: sha, mtime, file_size: size, schema_version: CACHE_SCHEMA_VERSION };
            if let Ok(d) = bincode::serialize(&nm) { let _ = fs::write(&meta_path, d); }
            return load_bc(&bc_path, verbose);
        }
        if verbose { eprintln!("{} Cache miss: {}", "[!]".yellow(), src_path); }
    }
    None
}

fn load_bc(path: &PathBuf, verbose: bool) -> Option<BytecodeProgram> {
    match bincode::deserialize::<BytecodeProgram>(&fs::read(path).ok()?) {
        Ok(p) if p.schema_version == CACHE_SCHEMA_VERSION => Some(p),
        Ok(_)  => { if verbose { eprintln!("{} Cache: niezgodna wersja.", "[!]".yellow()); } None }
        Err(e) => { if verbose { eprintln!("{} Błąd cache: {}", "[!]".yellow(), e); } None }
    }
}

fn cache_save(src_path: &str, prog: &BytecodeProgram, verbose: bool) {
    let (bc_path, meta_path) = cache_paths(src_path);
    let sha256 = file_sha256(src_path);
    let (mtime, file_size) = file_mtime_size(src_path).unwrap_or((0, 0));
    let meta = CacheMeta { sha256, mtime, file_size, schema_version: CACHE_SCHEMA_VERSION };

    if let Ok(d) = bincode::serialize(prog) {
        if let Err(e) = fs::write(&bc_path, d) {
            if verbose { eprintln!("{} Błąd zapisu .bc: {}", "[!]".yellow(), e); }
            return;
        }
    }
    if let Ok(d) = bincode::serialize(&meta) {
        if let Err(e) = fs::write(&meta_path, d) {
            if verbose { eprintln!("{} Błąd zapisu .meta: {}", "[!]".yellow(), e); }
        } else if verbose {
            eprintln!("{} Cache zapisany: {}", "[*]".green(), cache_dir().display());
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Pomocniki kompilacji
// ─────────────────────────────────────────────────────────────

/// Opakuj warunek w [[ ]] gdy zawiera operatory porównania.
/// Warunki już zaczynające się od [ lub (( zostają bez zmian.
fn wrap_cond(cond: &str) -> String {
    let t = cond.trim();
    if t.starts_with('[') || t.starts_with("((") { return t.to_string(); }
    let needs = t.contains(" == ") || t.contains(" != ")
    || t.contains(" -eq ") || t.contains(" -ne ")
    || t.contains(" -lt ") || t.contains(" -le ")
    || t.contains(" -gt ") || t.contains(" -ge ");
    if needs { format!("[[ {} ]]", t) } else { t.to_string() }
}

/// Czy cmd to wywołanie funkcji HL? (.FuncName, .Class.method, ...)
fn is_hl_call(cmd: &str) -> bool {
    let t = cmd.trim();
    if !t.starts_with('.') || t.len() < 2 { return false; }
    let c = t.chars().nth(1).unwrap_or(' ');
    c.is_ascii_alphabetic() || c == '_'
}

/// Wyciągnij nazwę funkcji z HL call cmd.
/// ".new_project $a $b" → "new_project"
/// ".Utils.danger_clean" → "Utils.danger_clean"
fn extract_hl_func(cmd: &str) -> String {
    cmd.trim()
    .trim_start_matches('.')
    .split_whitespace()
    .next()
    .unwrap_or("")
    .to_string()
}

/// Tłumaczy cmd na komendę shell gdy NIE jest wywołaniem HL.
/// Obsługuje: log "msg" → echo "msg", end N → exit N,
/// prefix > (ExplCmd) → usunięcie prefiksu.
fn shell_inline(cmd: &str) -> String {
    let t = cmd.trim();
    if let Some(r) = t.strip_prefix("log ")  { return format!("echo {}", r); }
    if let Some(r) = t.strip_prefix("end ")  { return format!("exit {}", r.trim().parse::<i32>().unwrap_or(0)); }
    if t == "end"                             { return "exit 0".to_string(); }
    if let Some(r) = t.strip_prefix("> ")    { return r.to_string(); }
    if let Some(r) = t.strip_prefix('>')     { return r.trim().to_string(); }
    t.to_string()
}

// ─────────────────────────────────────────────────────────────
// Kompilacja AST → Bytecode
// ─────────────────────────────────────────────────────────────

/// Jedna gałąź bloku if/elif/else zebrana przez compile_body.
struct Branch {
    /// Warunek (None = gałąź else)
    cond: Option<String>,
    /// Treść gałęzi
    body: BranchBody,
    sudo: bool,
}

/// Klasyfikacja body gałęzi
enum BranchBody {
    /// Zwykła komenda shell
    Shell(String),
    /// Wywołanie funkcji HL — emitowane jako CallFunc opcode
    HlCall(String),
}

fn compile_to_bytecode(ast: &AnalysisResult) -> BytecodeProgram {
    let mut ops:       Vec<OpCode>            = Vec::with_capacity(ast.main_body.len() + 32);
    let mut functions: HashMap<String, usize> = HashMap::new();

    compile_body(&ast.main_body, &mut ops);
    ops.push(OpCode::Exit(0));

    // Kompiluj funkcje HL
    for (name, (_is_unsafe, nodes)) in &ast.functions {
        functions.insert(name.clone(), ops.len());
        compile_body(nodes, &mut ops);
        ops.push(OpCode::Return);
    }

    BytecodeProgram { schema_version: CACHE_SCHEMA_VERSION, ops, functions }
}

fn compile_body(nodes: &[ProgramNode], ops: &mut Vec<OpCode>) {
    let mut i = 0;
    while i < nodes.len() {
        match &nodes[i].content {

            // ── If — zbierz cały blok (If + Elif* + Else?) i emituj jako VM branches ──
            CommandType::If { cond, cmd } => {
                let mut branches: Vec<Branch> = vec![Branch {
                    cond: Some(wrap_cond(cond)),
                    body: classify(cmd),
                    sudo: nodes[i].is_sudo,
                }];
                i += 1;

                // Zbierz kolejne Elif i opcjonalny Else
                loop {
                    if i >= nodes.len() { break; }
                    match &nodes[i].content {
                        CommandType::Elif { cond, cmd } => {
                            branches.push(Branch {
                                cond: Some(wrap_cond(cond)),
                                          body: classify(cmd),
                                          sudo: nodes[i].is_sudo,
                            });
                            i += 1;
                        }
                        CommandType::Else { cmd } => {
                            branches.push(Branch { cond: None, body: classify(cmd), sudo: nodes[i].is_sudo });
                            i += 1;
                            break;
                        }
                        _ => break,
                    }
                }

                emit_if_block(branches, ops);
                // i już zinkrementowane — continue bez dodatkowego i+=1
                continue;
            }

            _ => {
                compile_node(&nodes[i], ops);
                i += 1;
            }
        }
    }
}

fn classify(cmd: &str) -> BranchBody {
    if is_hl_call(cmd) {
        BranchBody::HlCall(extract_hl_func(cmd))
    } else {
        BranchBody::Shell(shell_inline(cmd))
    }
}

/// Emituj bytecode dla całego bloku if/elif/else używając backpatchingu.
///
/// Algorytm:
///   Dla każdej gałęzi z warunkiem:
///     1. Emituj JumpIfFalse { cond, target: PLACEHOLDER }
///     2. Emituj body (Shell/HlCall)
///     3. Emituj Jump { target: PLACEHOLDER } (skok na koniec całego bloku)
///     4. Backpatch JumpIfFalse.target = ops.len() (= start następnej gałęzi)
///   Dla gałęzi else (bez warunku):
///     1. Emituj body
///     2. Emituj Jump (dla spójności)
///   Na końcu: backpatch wszystkie Jump.target = ops.len()
fn emit_if_block(branches: Vec<Branch>, ops: &mut Vec<OpCode>) {
    let mut end_jumps: Vec<usize> = Vec::new();

    for branch in branches {
        // 1. JumpIfFalse dla If/Elif (Else go nie ma)
        let jif_idx: Option<usize> = branch.cond.map(|cond| {
            let idx = ops.len();
            ops.push(OpCode::JumpIfFalse { cond, target: 0 }); // backpatch później
            idx
        });

        // 2. Body gałęzi
        match branch.body {
            BranchBody::Shell(cmd)      => ops.push(OpCode::Exec { cmd, sudo: branch.sudo }),
            BranchBody::HlCall(fname)   => ops.push(OpCode::CallFunc { func_name: fname }),
        }

        // 3. Jump na koniec całego bloku (backpatch później)
        let jump_idx = ops.len();
        ops.push(OpCode::Jump { target: 0 });
        end_jumps.push(jump_idx);

        // 4. Backpatch JumpIfFalse → wskazuje na START następnej gałęzi
        if let Some(idx) = jif_idx {
            let next = ops.len();
            if let OpCode::JumpIfFalse { target, .. } = &mut ops[idx] {
                *target = next;
            }
        }
    }

    // 5. Backpatch wszystkich Jump → END całego bloku
    let end = ops.len();
    for idx in end_jumps {
        if let OpCode::Jump { target } = &mut ops[idx] {
            *target = end;
        }
    }
}

fn compile_node(node: &ProgramNode, ops: &mut Vec<OpCode>) {
    let sudo = node.is_sudo;
    match &node.content {
        CommandType::RawNoSub(s) | CommandType::RawSub(s) => {
            ops.push(OpCode::Exec { cmd: s.clone(), sudo });
        }
        CommandType::Isolated(s) => {
            ops.push(OpCode::Exec { cmd: format!("( {} )", s), sudo });
        }
        CommandType::AssignEnv { key, val } => {
            ops.push(OpCode::SetEnv { key: key.clone(), val: val.clone() });
        }
        CommandType::AssignLocal { key, val, is_raw } => {
            ops.push(OpCode::SetLocal { key: key.clone(), val: val.clone(), is_raw: *is_raw });
        }
        CommandType::Loop { count, cmd } => {
            ops.push(OpCode::Exec {
                cmd: format!("for _hl_i in $(seq 1 {}); do {}; done", count, cmd),
                     sudo,
            });
        }
        CommandType::While { cond, cmd } => {
            ops.push(OpCode::Exec {
                cmd: format!("while {}; do {}; done", wrap_cond(cond), cmd),
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
            ops.push(OpCode::Exec { cmd: format!("{} &", s), sudo });
        }
        CommandType::Call(name) => {
            ops.push(OpCode::CallFunc {
                func_name: name.trim_start_matches('.').to_string(),
            });
        }
        CommandType::Plugin { name, args, is_super } => {
            ops.push(OpCode::Plugin { name: name.clone(), args: args.clone(), sudo: *is_super });
        }
        CommandType::Log(msg) => {
            ops.push(OpCode::Exec { cmd: format!("echo {}", msg), sudo });
        }
        CommandType::Lock   { key, val } => {
            ops.push(OpCode::Lock   { key: key.clone(), val: val.clone() });
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
        // Metadane — PLSA obsługuje, runtime ignoruje
        CommandType::Extern  { .. }
        | CommandType::Enum  { .. }
        | CommandType::Struct { .. }
        | CommandType::Import { .. } => {}
        // If/Elif/Else — obsługiwane przez compile_body()
        CommandType::If   { .. }
        | CommandType::Elif { .. }
        | CommandType::Else { .. } => {}
    }
}

// ─────────────────────────────────────────────────────────────
// VM
// ─────────────────────────────────────────────────────────────
enum LocalVal {
    Managed(*mut c_char),
    Raw(String),
}
unsafe impl Send for LocalVal {}
unsafe impl Sync for LocalVal {}

struct VM {
    env:     HashMap<String, String>,
    locals:  HashMap<String, LocalVal>,
    heap:    HashMap<String, Vec<u8>>,
    verbose: bool,
}

impl VM {
    fn new(verbose: bool) -> Self {
        Self { env: std::env::vars().collect(), locals: HashMap::new(), heap: HashMap::new(), verbose }
    }

    #[inline]
    fn substitute(&self, text: &str) -> String {
        if !text.contains('$') { return text.to_string(); }
        let mut res = text.to_string();
        for (k, val) in &self.locals {
            let v = match val {
                LocalVal::Raw(s)     => s.clone(),
                LocalVal::Managed(p) => unsafe { CStr::from_ptr(*p).to_str().unwrap_or("").to_string() },
            };
            res = res.replace(&format!("${{{}}}", k), &v);
            res = res.replace(&format!("${}", k), &v);
        }
        for (k, v) in &self.env {
            res = res.replace(&format!("${{{}}}", k), v);
            res = res.replace(&format!("${}", k), v);
        }
        res
    }

    fn gc_collect(&mut self) {
        unsafe {
            gc_unmark_all();
            for (_, val) in &self.locals {
                if let LocalVal::Managed(p) = val { gc_mark(*p as *mut c_void); }
            }
            gc_sweep();
        }
    }

    fn alloc_local(&mut self, key: &str, val: &str) {
        match CString::new(val) {
            Ok(cstr) => {
                let size = cstr.as_bytes_with_nul().len();
                let ptr  = unsafe { gc_malloc(size) } as *mut c_char;
                let ptr  = if ptr.is_null() {
                    let p2 = unsafe { gc_alloc_old(size) } as *mut c_char;
                    if p2.is_null() {
                        eprintln!("{} GC: alokacja nieudana dla '{}'", "[x]".red(), key);
                        self.locals.insert(key.to_string(), LocalVal::Raw(val.to_string()));
                        return;
                    }
                    p2
                } else { ptr };
                unsafe { std::ptr::copy_nonoverlapping(cstr.as_ptr(), ptr, size); }
                self.locals.insert(key.to_string(), LocalVal::Managed(ptr));
            }
            Err(_) => {
                if self.verbose { eprintln!("{} Zmienna '{}' zawiera bajt null — Raw", "[!]".yellow(), key); }
                self.locals.insert(key.to_string(), LocalVal::Raw(val.to_string()));
            }
        }
    }

    /// Ewaluuj warunek przez bash.
    /// Zwraca true gdy bash exit code == 0 (warunek spełniony).
    fn eval_cond(&self, cond: &str) -> bool {
        let expanded = self.substitute(cond);
        let script   = format!("if {}; then exit 0; else exit 1; fi", expanded);
        if self.verbose {
            eprintln!("{} eval_cond: {}", "[?]".cyan(), script.dimmed());
        }
        match Command::new("bash").arg("-c").arg(&script).status() {
            Ok(s)  => s.code().unwrap_or(1) == 0,
            Err(e) => { eprintln!("{} eval_cond błąd: {}", "[x]".red(), e); false }
        }
    }

    fn run(&mut self, prog: &BytecodeProgram) -> i32 {
        let mut ip:         usize      = 0;
        let mut call_stack: Vec<usize> = Vec::with_capacity(32);

        while ip < prog.ops.len() {
            match &prog.ops[ip] {

                // ── Exec ──────────────────────────────────────────────
                OpCode::Exec { cmd, sudo } => {
                    let c = self.substitute(cmd);
                    if self.verbose { eprintln!("{} [{}] {}", "[>]".cyan(), ip, c.dimmed()); }
                    let st = if *sudo {
                        Command::new("sudo").arg("bash").arg("-c").arg(&c).status()
                    } else {
                        Command::new("bash").arg("-c").arg(&c).status()
                    };
                    match st {
                        Ok(s) if !s.success() && self.verbose =>
                        eprintln!("{} exit: {}", "[!]".yellow(), s.code().unwrap_or(-1)),
                        Err(e) => eprintln!("{} exec błąd: {}", "[x]".red(), e),
                        _ => {}
                    }
                }

                // ── JumpIfFalse ───────────────────────────────────────
                // Serce obsługi if/elif/else.
                // Ewaluuje warunek (poprzez bash exit code).
                // TRUE  → fall-through do body gałęzi (ip+1)
                // FALSE → skok do następnej gałęzi lub końca bloku
                OpCode::JumpIfFalse { cond, target } => {
                    let expanded = self.substitute(cond);
                    let result   = self.eval_cond(&expanded);
                    if self.verbose {
                        eprintln!(
                            "{} [{}] JumpIfFalse [[ {} ]] → {}",
                            "[?]".cyan(), ip, expanded.dimmed(),
                                  if result { "TRUE (fall-through)".green().to_string() }
                                  else      { format!("FALSE → jump {}", target).red().to_string() }
                        );
                    }
                    if !result {
                        ip = *target;
                        continue;
                    }
                }

                // ── Jump ──────────────────────────────────────────────
                // Bezwarunkowy skok — po wykonaniu body gałęzi
                // przeskakuje pozostałe elif/else.
                OpCode::Jump { target } => {
                    if self.verbose { eprintln!("{} [{}] Jump → {}", "[j]".cyan(), ip, target); }
                    ip = *target;
                    continue;
                }

                // ── CallFunc ──────────────────────────────────────────
                OpCode::CallFunc { func_name } => {
                    match self.resolve_func(func_name, &prog.functions) {
                        Some(addr) => {
                            if self.verbose { eprintln!("{} Call: {} → ip={}", "[f]".green(), func_name, addr); }
                            call_stack.push(ip + 1);
                            ip = addr;
                            continue;
                        }
                        None => eprintln!("{} Runtime: funkcja '{}' nie znaleziona", "[x]".red(), func_name),
                    }
                }

                // ── Return ────────────────────────────────────────────
                OpCode::Return => {
                    match call_stack.pop() {
                        Some(ret) => { ip = ret; continue; }
                        None      => { self.gc_collect(); return 0; }
                    }
                }

                // ── Exit ──────────────────────────────────────────────
                OpCode::Exit(code) => { self.gc_collect(); return *code; }

                // ── SetEnv ────────────────────────────────────────────
                OpCode::SetEnv { key, val } => {
                    let v = self.substitute(val);
                    if self.verbose { eprintln!("{} env: {}={}", "[e]".blue(), key, v); }
                    std::env::set_var(key, &v);
                    self.env.insert(key.clone(), v);
                }

                // ── SetLocal ──────────────────────────────────────────
                OpCode::SetLocal { key, val, is_raw } => {
                    let v = self.substitute(val);
                    if self.verbose { eprintln!("{} local: {}={} (raw={})", "[l]".blue(), key, v, is_raw); }
                    if *is_raw { self.locals.insert(key.clone(), LocalVal::Raw(v)); }
                    else       { self.alloc_local(key, &v); }
                }

                // ── Plugin ────────────────────────────────────────────
                OpCode::Plugin { name, args, sudo } => {
                    let root     = get_plugins_root();
                    let bin      = root.join(name);
                    let hl       = PathBuf::from(format!("{}.hl", bin.display()));
                    let fa       = self.substitute(args);
                    if self.verbose { eprintln!("{} Plugin: \\\\{} {} (sudo={})", "[p]".cyan(), name, fa, sudo); }

                    let tgt = if bin.exists() {
                        Some(bin.to_str().unwrap().to_string())
                    } else if hl.exists() {
                        let rt = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hl"));
                        Some(format!("{} {}", rt.display(), hl.display()))
                    } else {
                        eprintln!("{} Plugin '{}' nie znaleziony: {}", "[!]".yellow(), name, root.display());
                        None
                    };

                    if let Some(base) = tgt {
                        let full = if fa.is_empty() { base } else { format!("{} {}", base, fa) };
                        let st = if *sudo {
                            Command::new("sudo").arg("bash").arg("-c").arg(&full).status()
                        } else {
                            Command::new("bash").arg("-c").arg(&full).status()
                        };
                        if let Err(e) = st { eprintln!("{} Błąd pluginu '{}': {}", "[x]".red(), name, e); }
                    }
                }

                // ── Lock ──────────────────────────────────────────────
                OpCode::Lock { key, val } => {
                    let k    = self.substitute(key);
                    let v    = self.substitute(val);
                    let size = v.parse::<usize>().unwrap_or(v.len().max(1));
                    if self.verbose { eprintln!("{} Lock: {} ({} B)", "[m]".magenta(), k, size); }
                    self.heap.insert(k, vec![0u8; size]);
                }

                // ── Unlock ────────────────────────────────────────────
                OpCode::Unlock { key } => {
                    let k = self.substitute(key);
                    if self.verbose { eprintln!("{} Unlock: {}", "[m]".magenta(), k); }
                    self.heap.remove(&k);
                }
            }
            ip += 1;
        }

        self.gc_collect();
        0
    }

    fn resolve_func<'a>(&self, name: &str, fns: &'a HashMap<String, usize>) -> Option<usize> {
        let c = name.trim_start_matches('.');
        if let Some(&a) = fns.get(c) { return Some(a); }
        for (fname, &addr) in fns {
            if fname == c || fname.ends_with(&format!(".{}", c)) { return Some(addr); }
        }
        None
    }
}

// ─────────────────────────────────────────────────────────────
// Generowanie bytecode przez hl-plsa
// ─────────────────────────────────────────────────────────────
fn generate_bytecode(file_path: &str, verbose: bool) -> BytecodeProgram {
    if verbose { eprintln!("{} Cache miss — analizuję: {}", "[*]".yellow(), file_path); }

    let out = Command::new(get_plsa_path())
    .args([file_path, "--json", "--resolve-libs"])
    .output()
    .unwrap_or_else(|e| { eprintln!("{} hl-plsa: {}", "[x]".red(), e); std::process::exit(1); });

    if !out.status.success() {
        eprintln!("{} hl-plsa błąd:\n{}", "[x]".red(), String::from_utf8_lossy(&out.stderr));
        std::process::exit(1);
    }

    let ast: AnalysisResult = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        eprintln!("{} Nieprawidłowy JSON z PLSA: {}\n{}",
                  "[x]".red(), e, &String::from_utf8_lossy(&out.stdout)[..out.stdout.len().min(512)]);
        std::process::exit(1);
    });

    if verbose {
        eprintln!("{} AST: {} funkcji, {} węzłów, {} deps",
                  "[i]".blue(), ast.functions.len(), ast.main_body.len(), ast.deps.len());
        if ast.is_potentially_unsafe {
            eprintln!("{} Sudo (^):", "[!]".yellow());
            for w in &ast.safety_warnings { eprintln!("    {}", w.yellow()); }
        }
    }

    compile_to_bytecode(&ast)
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────
fn main() {
    let args = Args::parse();

    let program: BytecodeProgram = if !args.no_cache {
        match cache_load(&args.file, args.verbose) {
            Some(p) => p,
            None    => {
                let p = generate_bytecode(&args.file, args.verbose);
                cache_save(&args.file, &p, args.verbose);
                p
            }
        }
    } else {
        generate_bytecode(&args.file, args.verbose)
    };

    if args.dry_run {
        eprintln!("{} Dry run: {} ops, {} funkcji.", "[✓]".green(), program.ops.len(), program.functions.len());
        if args.verbose {
            for (i, op) in program.ops.iter().enumerate() {
                eprintln!("  {:>4}: {:?}", i, op);
            }
        }
        return;
    }

    let mut vm    = VM::new(args.verbose);
    let start     = Instant::now();
    let exit_code = vm.run(&program);
    let elapsed   = start.elapsed();

    unsafe { gc_collect_full(); }

    if args.verbose { eprintln!("{} Czas: {:?}", "[INFO]".blue(), elapsed); }

    if args.gc_stats || args.verbose {
        let (mut minor, mut major, mut promoted, mut total) = (0u64, 0u64, 0u64, 0u64);
        unsafe { gc_stats_get(&mut minor, &mut major, &mut promoted, &mut total); }
        eprintln!("{}", "━━━ GC Statistics ━━━━━━━━━━━━━━━━━━━━━━".cyan());
        eprintln!("  allocs total : {}", total.to_string().yellow());
        eprintln!("  minor GC     : {}", minor.to_string().green());
        eprintln!("  major GC     : {}", major.to_string().red());
        eprintln!("  promoted     : {}", promoted.to_string().magenta());
        eprintln!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".cyan());
        if args.verbose { unsafe { gc_stats_print(); } }
    }

    std::process::exit(exit_code);
}
