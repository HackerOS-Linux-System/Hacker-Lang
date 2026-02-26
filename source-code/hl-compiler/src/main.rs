use clap::Parser;
use colored::*;
use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::builder::Builder;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::OptimizationLevel;
use inkwell::values::FunctionValue;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, exit};
use std::sync::atomic::{AtomicU64, Ordering};

const PLSA_BIN_NAME: &str = "hl-plsa";

// ─────────────────────────────────────────────────────────────
// Globalny licznik unikalnych nazw symboli LLVM
// ─────────────────────────────────────────────────────────────
static GLOBAL_CTR: AtomicU64 = AtomicU64::new(0);

#[inline]
fn uid(prefix: &str) -> String {
    format!("{}_{}", prefix, GLOBAL_CTR.fetch_add(1, Ordering::Relaxed))
}

// ─────────────────────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────────────────────
#[derive(Parser, Debug)]
#[command(
author  = "HackerOS",
version = "2.2.0",
about   = "hacker-lang compiler — .hl → native binary via LLVM + gcc"
)]
struct Args {
    /// Plik .hl do kompilacji
    file: String,

    /// Plik wyjściowy (domyślnie: nazwa pliku bez rozszerzenia)
    #[arg(short, long)]
    output: Option<String>,

    /// Szczegółowe wyjście
    #[arg(long, short)]
    verbose: bool,

    /// Emituj tylko plik obiektowy .o (bez linkowania)
    #[arg(long)]
    emit_obj: bool,

    /// Emituj LLVM IR jako .ll (do debugowania)
    #[arg(long)]
    emit_ir: bool,

    /// Poziom optymalizacji: 0=brak 1=mało 2=domyślny 3=agresywny
    #[arg(long, default_value = "2")]
    opt: u8,

    /// Wymuś PIE (Position Independent Executable) — domyślnie wyłączone
    #[arg(long)]
    pie: bool,
}

// ─────────────────────────────────────────────────────────────
// AST — identyczne z hl-plsa
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType {
    Source,
    Core,
    Bytes,
    Github,
    Virus,
    Vira,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibRef {
    pub lib_type: LibType,
    pub name:     String,
    pub version:  Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct ProgramNode {
    pub line_num:      usize,
    pub is_sudo:       bool,
    pub content:       CommandType,
    pub original_text: String,
    pub span:          (usize, usize),
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisResult {
    pub deps:                  Vec<String>,
    pub libs:                  Vec<LibRef>,
    pub functions:             HashMap<String, (bool, Vec<ProgramNode>)>,
    pub main_body:             Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings:       Vec<String>,
}

// ─────────────────────────────────────────────────────────────
// Ścieżki
// ─────────────────────────────────────────────────────────────
fn get_plsa_path() -> PathBuf {
    let home = dirs::home_dir().expect("HOME not set");
    let path = home.join(".hackeros/hacker-lang/bin").join(PLSA_BIN_NAME);
    if !path.exists() {
        eprintln!(
            "{} Krytyczny błąd: {} nie znaleziony pod {:?}",
            "[x]".red(), PLSA_BIN_NAME, path
        );
        exit(127);
    }
    path
}

fn get_plugins_root() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/plugins")
}

fn get_libs_base() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/libs")
}

/// Domyślna nazwa wyjściowa: plik bez rozszerzenia, w tym samym katalogu
fn default_output(input: &str) -> String {
    let path = PathBuf::from(input);
    path.with_extension("")
    .to_str()
    .unwrap_or("a.out")
    .to_string()
}

// ─────────────────────────────────────────────────────────────
// Codegen
// ─────────────────────────────────────────────────────────────
struct Codegen<'ctx> {
    ctx:          &'ctx Context,
    module:       Module<'ctx>,
    builder:      Builder<'ctx>,
    verbose:      bool,

    system_fn:    FunctionValue<'ctx>,
    setenv_fn:    FunctionValue<'ctx>,
    gc_malloc_fn: FunctionValue<'ctx>,
    gc_unmark_fn: FunctionValue<'ctx>,
    gc_sweep_fn:  FunctionValue<'ctx>,
    gc_full_fn:   FunctionValue<'ctx>,
    exit_fn:      FunctionValue<'ctx>,

    hl_functions: HashMap<String, FunctionValue<'ctx>>,
    pub extern_libs: Vec<(String, bool)>,
}

impl<'ctx> Codegen<'ctx> {
    fn new(ctx: &'ctx Context, verbose: bool) -> Self {
        let module  = ctx.create_module("hacker_module");
        let builder = ctx.create_builder();

        let i32_t  = ctx.i32_type();
        let i64_t  = ctx.i64_type();
        let void_t = ctx.void_type();
        let ptr_t  = ctx.ptr_type(AddressSpace::default());

        let system_fn = module.add_function(
            "system",
            i32_t.fn_type(&[ptr_t.into()], false),
                                            Some(Linkage::External),
        );
        let setenv_fn = module.add_function(
            "setenv",
            i32_t.fn_type(&[ptr_t.into(), ptr_t.into(), i32_t.into()], false),
                                            Some(Linkage::External),
        );
        let gc_malloc_fn = module.add_function(
            "gc_malloc",
            ptr_t.fn_type(&[i64_t.into()], false),
                                               Some(Linkage::External),
        );
        let gc_unmark_fn = module.add_function(
            "gc_unmark_all",
            void_t.fn_type(&[], false),
                                               Some(Linkage::External),
        );
        let gc_sweep_fn = module.add_function(
            "gc_sweep",
            void_t.fn_type(&[], false),
                                              Some(Linkage::External),
        );
        let gc_full_fn = module.add_function(
            "gc_collect_full",
            void_t.fn_type(&[], false),
                                             Some(Linkage::External),
        );
        let exit_fn = module.add_function(
            "exit",
            void_t.fn_type(&[i32_t.into()], false),
                                          Some(Linkage::External),
        );

        Codegen {
            ctx, module, builder, verbose,
            system_fn, setenv_fn, gc_malloc_fn,
            gc_unmark_fn, gc_sweep_fn, gc_full_fn, exit_fn,
            hl_functions: HashMap::new(),
            extern_libs:  Vec::new(),
        }
    }

    // ── Helpers ───────────────────────────────────────────────

    fn str_ptr(&self, s: &str, prefix: &str) -> inkwell::values::PointerValue<'ctx> {
        let name   = uid(prefix);
        let cs     = self.ctx.const_string(s.as_bytes(), true);
        let arr_t  = cs.get_type();
        let global = self.module.add_global(arr_t, None, &name);
        global.set_initializer(&cs);
        global.set_linkage(Linkage::Internal);
        global.set_constant(true);

        let zero = self.ctx.i64_type().const_int(0, false);
        unsafe {
            self.builder
            .build_gep(arr_t, global.as_pointer_value(), &[zero, zero], &uid("gep"))
            .unwrap()
        }
    }

    fn emit_system(&self, cmd: &str) {
        if self.verbose {
            eprintln!("    {} {}", "→".dimmed(), cmd.dimmed());
        }
        let ptr = self.str_ptr(cmd, "cmd");
        self.builder
        .build_call(self.system_fn, &[ptr.into()], &uid("sys"))
        .unwrap();
    }

    fn wrap_sudo(sudo: bool, cmd: &str) -> String {
        if sudo {
            format!("sudo sh -c '{}'", cmd.replace('\'', "'\\''"))
        } else {
            cmd.to_string()
        }
    }

    // ── Przeddeklaracja funkcji ───────────────────────────────
    fn predeclare_functions(&mut self, ast: &AnalysisResult) {
        let fn_t = self.ctx.i32_type().fn_type(&[], false);
        let mut names: Vec<&String> = ast.functions.keys().collect();
        names.sort();
        for name in names {
            let llvm_name = format!(
                "hl_{}",
                name.replace('.', "_").replace('-', "_")
            );
            let func = self.module.add_function(&llvm_name, fn_t, None);
            self.hl_functions.insert(name.clone(), func);
            if self.verbose {
                eprintln!("{} Predeclare: {} → {}()", "[f]".blue(), name, llvm_name);
            }
        }
    }

    // ── Kompilacja ciała ──────────────────────────────────────
    fn compile_body(&mut self, nodes: &[ProgramNode]) -> bool {
        let mut i = 0;
        while i < nodes.len() {
            let node = nodes[i].clone();
            let sudo = node.is_sudo;

            match &node.content {
                CommandType::RawNoSub(cmd) | CommandType::RawSub(cmd) => {
                    self.emit_system(&Self::wrap_sudo(sudo, cmd));
                }
                CommandType::Isolated(cmd) => {
                    self.emit_system(&Self::wrap_sudo(sudo, &format!("( {} )", cmd)));
                }
                CommandType::Background(cmd) => {
                    self.emit_system(&Self::wrap_sudo(sudo, &format!("{} &", cmd)));
                }

                CommandType::If { cond, cmd } => {
                    let mut sh = format!("if {}; then {}; ", cond, cmd);
                    i += 1;
                    loop {
                        if i >= nodes.len() { break; }
                        match &nodes[i].content {
                            CommandType::Elif { cond, cmd } => {
                                sh += &format!("elif {}; then {}; ", cond, cmd);
                                i  += 1;
                            }
                            CommandType::Else { cmd } => {
                                sh += &format!("else {}; ", cmd);
                                i  += 1;
                                break;
                            }
                            _ => break,
                        }
                    }
                    sh += "fi";
                    self.emit_system(&Self::wrap_sudo(sudo, &sh));
                    continue;
                }

                CommandType::Loop { count, cmd } => {
                    let sh = format!("for _hl_i in $(seq 1 {}); do {}; done", count, cmd);
                    self.emit_system(&Self::wrap_sudo(sudo, &sh));
                }
                CommandType::While { cond, cmd } => {
                    self.emit_system(&Self::wrap_sudo(sudo,
                                                      &format!("while {}; do {}; done", cond, cmd)));
                }
                CommandType::For { var, in_, cmd } => {
                    self.emit_system(&Self::wrap_sudo(sudo,
                                                      &format!("for {} in {}; do {}; done", var, in_, cmd)));
                }

                CommandType::Try { try_cmd, catch_cmd } => {
                    self.emit_system(&Self::wrap_sudo(sudo,
                                                      &format!("( {} ) || ( {} )", try_cmd, catch_cmd)));
                }

                CommandType::Log(msg) => {
                    self.emit_system(&format!("echo {}", msg));
                }

                CommandType::AssignEnv { key, val } => {
                    let kp = self.str_ptr(key, "ekey");
                    let vp = self.str_ptr(val, "eval");
                    let ow = self.ctx.i32_type().const_int(1, false);
                    self.builder
                    .build_call(self.setenv_fn, &[kp.into(), vp.into(), ow.into()], &uid("setenv"))
                    .unwrap();
                }

                CommandType::AssignLocal { key, val, is_raw: _ } => {
                    let is_dynamic = val.contains('$')
                    || val.contains('`')
                    || val.contains("$(");

                    if !is_dynamic {
                        let kp = self.str_ptr(key, "lkey");
                        let vp = self.str_ptr(val, "lval");
                        let ow = self.ctx.i32_type().const_int(1, false);
                        self.builder
                        .build_call(self.setenv_fn, &[kp.into(), vp.into(), ow.into()], &uid("setenv_l"))
                        .unwrap();
                    } else {
                        self.emit_system(&format!("export {}={}", key, val));
                    }
                }

                CommandType::Call(name) => {
                    let clean = name.trim_start_matches('.');
                    let func  = self.hl_functions.get(clean).copied().or_else(|| {
                        self.hl_functions
                        .iter()
                        .find(|(k, _)| k.ends_with(&format!(".{}", clean)))
                        .map(|(_, v)| *v)
                    });
                    match func {
                        Some(f) => {
                            self.builder
                            .build_call(f, &[], &uid("call"))
                            .unwrap();
                        }
                        None => {
                            if self.verbose {
                                eprintln!("{} Call '{}' nie znaleziony — pomijam", "[!]".yellow(), clean);
                            }
                        }
                    }
                }

                CommandType::Plugin { name, args, is_super } => {
                    let root     = get_plugins_root();
                    let bin_path = root.join(name);
                    let hl_path  = root.join(format!("{}.hl", name));
                    let base = if bin_path.exists() {
                        bin_path.to_str().unwrap().to_string()
                    } else if hl_path.exists() {
                        format!("hl {}", hl_path.display())
                    } else {
                        if self.verbose {
                            eprintln!("{} Plugin '{}' nie znaleziony", "[!]".yellow(), name);
                        }
                        i += 1;
                        continue;
                    };
                    let cmd = if args.is_empty() { base } else { format!("{} {}", base, args) };
                    self.emit_system(&Self::wrap_sudo(*is_super, &cmd));
                }

                CommandType::Lock { key: _, val } => {
                    let size = val.parse::<u64>().unwrap_or(64);
                    let sz   = self.ctx.i64_type().const_int(size, false);
                    self.builder
                    .build_call(self.gc_malloc_fn, &[sz.into()], &uid("lock"))
                    .unwrap();
                }

                CommandType::Unlock { key: _ } => {
                    self.builder.build_call(self.gc_unmark_fn, &[], &uid("unmark")).unwrap();
                    self.builder.build_call(self.gc_sweep_fn,  &[], &uid("sweep")).unwrap();
                }

                CommandType::End { code } => {
                    self.builder.build_call(self.gc_full_fn, &[], &uid("gcfull")).unwrap();
                    let cv = self.ctx.i32_type().const_int(*code as u64, true);
                    self.builder.build_call(self.exit_fn, &[cv.into()], &uid("exit")).unwrap();
                    self.builder.build_unreachable().unwrap();
                    return true;
                }

                CommandType::Extern { path, static_link } => {
                    self.extern_libs.push((path.clone(), *static_link));
                }

                // Metadane — brak kodu
                CommandType::Enum   { .. } => {}
                CommandType::Struct { .. } => {}
                CommandType::Import { .. } => {}
                CommandType::Elif   { .. } => {}
                CommandType::Else   { .. } => {}
            }

            i += 1;
        }
        false
    }

    fn compile_functions(&mut self, ast: &AnalysisResult) {
        let mut names: Vec<&String> = ast.functions.keys().collect();
        names.sort();
        for name in names {
            let (is_unsafe, nodes) = &ast.functions[name];
            let func               = self.hl_functions[name];
            if self.verbose {
                eprintln!("{} Kompilacja: {} (unsafe={})", "[f]".green(), name, is_unsafe);
            }
            let entry = self.ctx.append_basic_block(func, "entry");
            self.builder.position_at_end(entry);
            let nodes_c = nodes.clone();
            if !self.compile_body(&nodes_c) {
                let zero = self.ctx.i32_type().const_int(0, false);
                self.builder.build_return(Some(&zero)).unwrap();
            }
        }
    }

    fn compile_main(&mut self, ast: &AnalysisResult) {
        let i32_t   = self.ctx.i32_type();
        let main_fn = self.module.add_function("main", i32_t.fn_type(&[], false), None);
        let entry   = self.ctx.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);
        let nodes = ast.main_body.clone();
        if !self.compile_body(&nodes) {
            self.builder.build_call(self.gc_full_fn, &[], "gc_final").unwrap();
            self.builder.build_return(Some(&i32_t.const_int(0, false))).unwrap();
        }
    }
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────
fn main() {
    let args = Args::parse();

    let output_name = args.output.clone()
    .unwrap_or_else(|| default_output(&args.file));

    // ── 1. hl-plsa ────────────────────────────────────────────
    let plsa = get_plsa_path();

    if args.verbose {
        eprintln!("{} Analizuję: {}", "[*]".green(), args.file);
    }

    let plsa_out = Command::new(&plsa)
    .arg(&args.file)
    .arg("--json")
    .output()
    .unwrap_or_else(|e| {
        eprintln!("{} Nie można uruchomić hl-plsa: {}", "[x]".red(), e);
        exit(1);
    });

    if !plsa_out.status.success() {
        eprintln!("{} hl-plsa błąd:\n{}", "[x]".red(),
                  String::from_utf8_lossy(&plsa_out.stderr));
        exit(1);
    }

    let ast: AnalysisResult = serde_json::from_slice(&plsa_out.stdout)
    .unwrap_or_else(|e| {
        let preview = &plsa_out.stdout[..plsa_out.stdout.len().min(512)];
        eprintln!("{} JSON z PLSA nieprawidłowy: {}\n{}",
                  "[x]".red(), e, String::from_utf8_lossy(preview));
        exit(1);
    });

    if args.verbose {
        eprintln!("{} AST: {} funkcji  {} węzłów  {} libs",
                  "[i]".blue(), ast.functions.len(), ast.main_body.len(), ast.libs.len());
        if ast.is_potentially_unsafe {
            eprintln!("{} Skrypt zawiera sudo (^):", "[!]".yellow());
            for w in &ast.safety_warnings { eprintln!("    {}", w.yellow()); }
        }
    }

    // ── 2. LLVM init ──────────────────────────────────────────
    Target::initialize_native(&InitializationConfig::default())
    .unwrap_or_else(|e| {
        eprintln!("{} LLVM init nieudana: {}", "[x]".red(), e);
        exit(1);
    });

    let context = Context::create();
    let mut cg  = Codegen::new(&context, args.verbose);

    // ── 3. Codegen ───────────────────────────────────────────
    if args.verbose { eprintln!("{} Generuję LLVM IR...", "[*]".green()); }

    cg.predeclare_functions(&ast);
    cg.compile_functions(&ast);
    cg.compile_main(&ast);

    // Zbierz Extern z ciał funkcji
    for (_, (_, nodes)) in &ast.functions {
        for node in nodes {
            if let CommandType::Extern { path, static_link } = &node.content {
                cg.extern_libs.push((path.clone(), *static_link));
            }
        }
    }

    // ── 4. Weryfikacja ────────────────────────────────────────
    if let Err(e) = cg.module.verify() {
        eprintln!("{} Błąd weryfikacji IR:\n{}", "[x]".red(), e);
        if args.verbose || args.emit_ir { cg.module.print_to_stderr(); }
        exit(1);
    }

    // ── 5. Emituj .ll (opcjonalnie) ───────────────────────────
    if args.emit_ir {
        let ll = format!("{}.ll", output_name);
        cg.module.print_to_file(std::path::Path::new(&ll)).ok();
        eprintln!("{} IR: {}", "[*]".green(), ll);
    }

    // ── 6. TargetMachine ─────────────────────────────────────
    let opt_level = match args.opt {
        0 => OptimizationLevel::None,
        1 => OptimizationLevel::Less,
        3 => OptimizationLevel::Aggressive,
        _ => OptimizationLevel::Default,
    };

    let triple = TargetMachine::get_default_triple();

    if args.verbose {
        eprintln!("{} Triple: {}", "[i]".blue(),
                  triple.as_str().to_str().unwrap_or("unknown"));
    }

    let target = Target::from_triple(&triple).unwrap_or_else(|e| {
        eprintln!("{} Target error: {}", "[x]".red(), e);
        exit(1);
    });

    // UWAGA dot. RelocMode na Debianie:
    //   Debian/Ubuntu domyślnie kompiluje z PIE włączonym w gcc (hardening).
    //   Gdy używamy RelocMode::Default, LLVM emituje relokacje absolutne
    //   (R_X86_64_32), które są niezgodne z PIE.
    //
    //   Rozwiązania (wybieramy jedno z dwóch):
    //     A) RelocMode::PIC   — .o jest PIC-compatible, gcc linkuje jako PIE  ← alternatywa
    //     B) RelocMode::Default + gcc -no-pie  ← WYBRANE (prostsze, mniejszy kod)
    //
    //   Używamy opcji B: RelocMode::Default w LLVM + "-no-pie" w gcc poniżej.
    //   Jeśli użytkownik jawnie chce PIE (--pie), używamy RelocMode::PIC.
    let reloc_mode = if args.pie {
        RelocMode::PIC
    } else {
        RelocMode::Default
    };

    let tm = target
    .create_target_machine(
        &triple,
        "",               // CPU: pusty string = domyślny dla triple
        "",               // features: pusty
        opt_level,
        reloc_mode,
        CodeModel::Default,
    )
    .unwrap_or_else(|| {
        eprintln!("{} Nie można utworzyć TargetMachine", "[x]".red());
        exit(1);
    });

    // ── 7. Emituj .o ─────────────────────────────────────────
    let obj_path = format!("{}.o", output_name);

    tm.write_to_file(&cg.module, FileType::Object, std::path::Path::new(&obj_path))
    .unwrap_or_else(|e| {
        eprintln!("{} Błąd zapisu .o: {}", "[x]".red(), e);
        exit(1);
    });

    if args.verbose { eprintln!("{} Obiekt: {}", "[*]".green(), obj_path); }
    if args.emit_obj {
        eprintln!("{} Gotowy: {}", "[+]".green(), obj_path);
        return;
    }

    // ── 8. Linkowanie przez gcc ───────────────────────────────
    if args.verbose { eprintln!("{} Linkuję...", "[*]".green()); }

    let mut linker = Command::new("gcc");
    linker.arg(&obj_path).arg("-o").arg(&output_name);

    // ── KLUCZOWA POPRAWKA DLA DEBIANA ────────────────────────
    // Debian domyślnie włącza PIE w gcc (opcja -pie w specs).
    // Nasze .o jest skompilowane z RelocMode::Default (relokacje absolutne),
    // co jest NIEZGODNE z PIE → błąd R_X86_64_32.
    // Rozwiązanie: przekaż -no-pie do gcc żeby wyłączyć PIE podczas linkowania.
    // Jeśli użytkownik chce PIE (--pie flag), pomijamy -no-pie.
    if !args.pie {
        linker.arg("-no-pie");
    }

    // Szukaj libgc.a
    let gc_search_paths = {
        let mut v: Vec<PathBuf> = vec![];
        if let Some(home) = dirs::home_dir() {
            v.push(home.join(".hackeros/hacker-lang/lib"));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(d) = exe.parent() { v.push(d.to_path_buf()); }
        }
        v.push(PathBuf::from("/usr/local/lib/hacker-lang"));
        v
    };

    let mut gc_found = false;
    for p in &gc_search_paths {
        let libgc = p.join("libgc.a");
        if libgc.exists() {
            linker.arg(&libgc);
            gc_found = true;
            if args.verbose { eprintln!("{} GC: {}", "[+]".blue(), libgc.display()); }
            break;
        }
    }
    if !gc_found {
        eprintln!(
            "{} libgc.a nie znaleziona.\n  \
Uruchom raz: cargo build --release  (w katalogu hl-compiler)\n  \
lub skopiuj ręcznie do ~/.hackeros/hacker-lang/lib/libgc.a",
"[!]".yellow()
        );
        linker.arg("-lgc");
    }

    // Biblioteki z AST
    let libs_base = get_libs_base();
    for lib in &ast.libs {
        match lib.lib_type {
            LibType::Bytes | LibType::Virus => {
                let lib_dir = libs_base.join("bytes").join(&lib.name);
                let so = lib_dir.join(format!("{}.so", lib.name));
                let a  = lib_dir.join(format!("{}.a",  lib.name));
                if so.exists() {
                    linker.arg(format!("-L{}", lib_dir.display()));
                    linker.arg(format!("-Wl,-rpath,{}", lib_dir.display()));
                    linker.arg(format!("-l:{}.so", lib.name));
                } else if a.exists() {
                    linker.arg(a.to_str().unwrap());
                } else if args.verbose {
                    eprintln!("{} Lib '{}' nie znaleziona", "[!]".yellow(), lib.name);
                }
            }
            LibType::Github => {
                let lib_dir = libs_base.join("github").join(&lib.name);
                if lib_dir.exists() {
                    linker.arg(format!("-L{}", lib_dir.display()));
                    linker.arg(format!("-l:{}.so", lib.name));
                }
            }
            LibType::Source | LibType::Core | LibType::Vira => {}
        }
    }

    // Extern
    for (path, is_static) in &cg.extern_libs {
        let clean = path.trim_matches('"');
        if *is_static {
            linker.arg(format!("-l:{}.a", clean));
        } else {
            linker.arg(format!("-l:{}.so", clean));
        }
    }

    linker.arg("-lm").arg("-ldl");

    if args.verbose { eprintln!("  {:?}", linker); }

    let status = linker.status().unwrap_or_else(|e| {
        eprintln!("{} Nie można uruchomić gcc: {}", "[x]".red(), e);
        exit(1);
    });

    let _ = std::fs::remove_file(&obj_path);

    if status.success() {
        eprintln!("{} Skompilowano: {}", "[+]".green(), output_name);
    } else {
        eprintln!("{} Linkowanie nieudane", "[x]".red());
        exit(1);
    }
}
