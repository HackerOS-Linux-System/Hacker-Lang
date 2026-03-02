use crate::ast::{AnalysisResult, CommandType, ProgramNode};
use crate::paths::get_plugins_root;
use colored::*;
use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::builder::Builder;
use inkwell::values::{FunctionValue, PointerValue};
use inkwell::AddressSpace;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// ─────────────────────────────────────────────────────────────
// Globalny licznik unikalnych nazw symboli LLVM
// ─────────────────────────────────────────────────────────────
static GLOBAL_CTR: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
fn uid(prefix: &str) -> String {
    format!("{}_{}", prefix, GLOBAL_CTR.fetch_add(1, Ordering::Relaxed))
}

// ─────────────────────────────────────────────────────────────
// Codegen
// ─────────────────────────────────────────────────────────────
pub struct Codegen<'ctx> {
    ctx:          &'ctx Context,
    pub module:   Module<'ctx>,
    builder:      Builder<'ctx>,
    verbose:      bool,

    // ── Zewnętrzne funkcje C ──────────────────────────────────
    system_fn:    FunctionValue<'ctx>,
    setenv_fn:    FunctionValue<'ctx>,
    gc_malloc_fn: FunctionValue<'ctx>,
    gc_unmark_fn: FunctionValue<'ctx>,
    gc_sweep_fn:  FunctionValue<'ctx>,
    gc_full_fn:   FunctionValue<'ctx>,
    exit_fn:      FunctionValue<'ctx>,

    // ── Funkcje HL zadeklarowane w LLVM IR ───────────────────
    hl_functions: HashMap<String, FunctionValue<'ctx>>,

    // ── Biblioteki Extern zgromadzone podczas codegen ────────
    pub extern_libs: Vec<(String, bool)>,

    // ── String cache — deduplicacja identycznych stałych ─────
    // Klucz: zawartość stringa, wartość: wskaźnik na GEP.
    // Bez tego kompilator tworzy N kopii tych samych literałów.
    string_cache: HashMap<String, PointerValue<'ctx>>,

    // ── Atrybuty funkcji preloaded dla wydajności ─────────────
    nounwind_attr:  Attribute,
    noreturn_attr:  Attribute,
    cold_attr:      Attribute,
    inline_attr:    Attribute,
    noinline_attr:  Attribute,

    // ── Stan match — zbieramy ramiona dla bieżącego `case` ───
    // Klucz: cond stringa, wartość: wektor (val, cmd)
    // Używane przez compile_body do łączenia Match + MatchArm
    // w jeden system("case ... esac") call.
    match_stack: Vec<(String, Vec<(String, String)>)>,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(ctx: &'ctx Context, verbose: bool) -> Self {
        let module  = ctx.create_module("hacker_module");
        let builder = ctx.create_builder();

        // ── Typy LLVM ─────────────────────────────────────────
        let i32_t  = ctx.i32_type();
        let i64_t  = ctx.i64_type();
        let void_t = ctx.void_type();
        let ptr_t  = ctx.ptr_type(AddressSpace::default());

        // ── Zewnętrzne funkcje C ──────────────────────────────
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

        // ── Atrybuty LLVM ──────────────────────────────────────
        let nounwind_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("nounwind"), 0,
        );
        let noreturn_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("noreturn"), 0,
        );
        let cold_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("cold"), 0,
        );
        let inline_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("alwaysinline"), 0,
        );
        let noinline_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("noinline"), 0,
        );
        let noalias_attr = ctx.create_enum_attribute(
            Attribute::get_named_enum_kind_id("noalias"), 0,
        );

        // ── Atrybuty dla extern functions ────────────────────
        system_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        setenv_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        gc_malloc_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        gc_malloc_fn.add_attribute(AttributeLoc::Return,   noalias_attr);
        gc_full_fn.add_attribute(AttributeLoc::Function,   nounwind_attr);
        gc_sweep_fn.add_attribute(AttributeLoc::Function,  nounwind_attr);
        gc_unmark_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        exit_fn.add_attribute(AttributeLoc::Function, noreturn_attr);
        exit_fn.add_attribute(AttributeLoc::Function, nounwind_attr);
        exit_fn.add_attribute(AttributeLoc::Function, cold_attr);

        Codegen {
            ctx,
            module,
            builder,
            verbose,
            system_fn,
            setenv_fn,
            gc_malloc_fn,
            gc_unmark_fn,
            gc_sweep_fn,
            gc_full_fn,
            exit_fn,
            hl_functions:  HashMap::new(),
            extern_libs:   Vec::new(),
            string_cache:  HashMap::new(),
            match_stack:   Vec::new(),
            nounwind_attr,
            noreturn_attr,
            cold_attr,
            inline_attr,
            noinline_attr,
        }
    }

    // ─────────────────────────────────────────────────────────
    // String interning
    // ─────────────────────────────────────────────────────────
    fn str_ptr(&mut self, s: &str, prefix: &str) -> PointerValue<'ctx> {
        if let Some(&cached) = self.string_cache.get(s) {
            return cached;
        }
        let name   = uid(prefix);
        let cs     = self.ctx.const_string(s.as_bytes(), true);
        let arr_t  = cs.get_type();
        let global = self.module.add_global(arr_t, None, &name);
        global.set_initializer(&cs);
        global.set_linkage(Linkage::Internal);
        global.set_constant(true);
        global.set_unnamed_addr(true);

        let zero = self.ctx.i64_type().const_int(0, false);
        let gep = unsafe {
            self.builder
            .build_gep(arr_t, global.as_pointer_value(), &[zero, zero], &uid("gep"))
            .unwrap()
        };
        self.string_cache.insert(s.to_string(), gep);
        gep
    }

    // ─────────────────────────────────────────────────────────
    // emit_system — emituj call do system(3)
    // ─────────────────────────────────────────────────────────
    fn emit_system(&mut self, cmd: &str) {
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

    // ─────────────────────────────────────────────────────────
    // resolve_call — rozwiąż ścieżkę funkcji HL do FunctionValue
    // Obsługuje: .func, .Class.func, func (bez kropki)
    // ─────────────────────────────────────────────────────────
    fn resolve_call(&self, path: &str) -> Option<FunctionValue<'ctx>> {
        let clean = path.trim_start_matches('.');
        // Dokładne dopasowanie
        if let Some(&f) = self.hl_functions.get(clean) {
            return Some(f);
        }
        // Sufiks — np. "init" → "App.init"
        self.hl_functions
        .iter()
        .find(|(k, _)| k.ends_with(&format!(".{}", clean)) || k.as_str() == clean)
        .map(|(_, v)| *v)
    }

    // ─────────────────────────────────────────────────────────
    // emit_call_hl — wywołaj funkcję HL przez LLVM call
    // ─────────────────────────────────────────────────────────
    fn emit_call_hl(&mut self, path: &str, args: &str, sudo: bool) {
        match self.resolve_call(path) {
            Some(f) => {
                // Argumenty przekazujemy przez środowisko jeśli niepuste
                if !args.is_empty() {
                    self.emit_system(&format!("export _HL_ARGS='{}'", args));
                }
                self.builder
                .build_call(f, &[], &uid("call"))
                .unwrap();
            }
            None => {
                // Funkcja nie istnieje w HL — próbuj jako komendę shell
                if self.verbose {
                    eprintln!(
                        "{} Call '{}' nie znaleziony — fallback shell",
                        "[!]".yellow(),
                              path
                    );
                }
                let cmd = if args.is_empty() {
                    path.trim_start_matches('.').to_string()
                } else {
                    format!("{} {}", path.trim_start_matches('.'), args)
                };
                self.emit_system(&Self::wrap_sudo(sudo, &cmd));
            }
        }
    }

    // ─────────────────────────────────────────────────────────
    // Predeclare — zgłoś wszystkie HL-funkcje do LLVM
    // ─────────────────────────────────────────────────────────
    pub fn predeclare_functions(&mut self, ast: &AnalysisResult) {
        let fn_t = self.ctx.i32_type().fn_type(&[], false);

        let mut names: Vec<&String> = ast.functions.keys().collect();
        names.sort();

        for name in names {
            let (_, _sig, nodes) = &ast.functions[name];
            let llvm_name = format!(
                "hl_{}",
                name.replace('.', "_").replace('-', "_")
            );

            let func = self.module.add_function(&llvm_name, fn_t, None);
            func.add_attribute(AttributeLoc::Function, self.nounwind_attr);

            let n = nodes.len();
            if n <= 5 {
                func.add_attribute(AttributeLoc::Function, self.inline_attr);
            } else if n > 50 {
                func.add_attribute(AttributeLoc::Function, self.noinline_attr);
            }

            self.hl_functions.insert(name.clone(), func);

            if self.verbose {
                let attr = if n <= 5 { "alwaysinline" }
                else if n > 50 { "noinline" }
                else { "default" };
                eprintln!(
                    "{} Predeclare: {}  →  {}()  [nodes={}, {}]",
                          "[f]".blue(), name, llvm_name, n, attr
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────
    // compile_body — generuje LLVM IR dla listy węzłów
    //
    // Zwraca true jeśli ostatnia instrukcja jest terminatorem.
    //
    // Strategia dla nowych konstrukcji:
    //
    //  % Const      → setenv() — identycznie jak AssignEnv
    //  spawn        → system("cmd &") — fire and forget
    //  key=spawn    → system("export key=$( cmd & echo $! )")
    //  await        → system("wait PID_or_job")
    //  key=await    → system("export key=$(wait expr; echo $?)")
    //  assert       → system("if ! (cond); then echo msg; exit 1; fi")
    //  match        → zbierz MatchArm i emituj jeden `case...esac`
    //  MatchArm     → część case (pochłaniana przez match)
    //  pipe         → system("step1 | step2 | step3")
    //  Out          → setenv("_HL_OUT", val) — wynik funkcji
    //  Call{p,a}    → emit_call_hl(p, a)
    // ─────────────────────────────────────────────────────────
    pub fn compile_body(&mut self, nodes: &[ProgramNode]) -> bool {
        let mut i = 0;
        while i < nodes.len() {
            let node = nodes[i].clone();
            let sudo = node.is_sudo;

            match &node.content {

                // ── Proste polecenia shell ────────────────────
                CommandType::RawNoSub(cmd) | CommandType::RawSub(cmd) => {
                    self.emit_system(&Self::wrap_sudo(sudo, cmd));
                }
                CommandType::Isolated(cmd) => {
                    self.emit_system(&Self::wrap_sudo(sudo, &format!("( {} )", cmd)));
                }
                CommandType::Background(cmd) => {
                    self.emit_system(&Self::wrap_sudo(sudo, &format!("{} &", cmd)));
                }

                // ── Log ───────────────────────────────────────
                CommandType::Log(msg) => {
                    self.emit_system(&format!("echo {}", msg));
                }

                // ── Out — zwrócenie wartości z funkcji HL ────
                // Zapisujemy wynik do zmiennej środowiskowej _HL_OUT
                // żeby caller mógł go przechwycić przez $(_HL_OUT)
                CommandType::Out(val) => {
                    let kp = self.str_ptr("_HL_OUT", "out_key");
                    let vp = self.str_ptr(val,       "out_val");
                    let ow = self.ctx.i32_type().const_int(1, false);
                    self.builder
                    .build_call(
                        self.setenv_fn,
                        &[kp.into(), vp.into(), ow.into()],
                                &uid("setenv_out"),
                    )
                    .unwrap();
                }

                // ── If / Elif / Else ──────────────────────────
                // Łączymy cały łańcuch w jeden string sh → 1x system()
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

                // ── Pętle ─────────────────────────────────────
                CommandType::Loop { count, cmd } => {
                    let sh = format!(
                        "for _hl_i in $(seq 1 {}); do {}; done",
                                     count, cmd
                    );
                    self.emit_system(&Self::wrap_sudo(sudo, &sh));
                }
                CommandType::While { cond, cmd } => {
                    self.emit_system(&Self::wrap_sudo(
                        sudo,
                        &format!("while {}; do {}; done", cond, cmd),
                    ));
                }
                CommandType::For { var, in_, cmd } => {
                    self.emit_system(&Self::wrap_sudo(
                        sudo,
                        &format!("for {} in {}; do {}; done", var, in_, cmd),
                    ));
                }

                // ── Try / Catch ───────────────────────────────
                CommandType::Try { try_cmd, catch_cmd } => {
                    self.emit_system(&Self::wrap_sudo(
                        sudo,
                        &format!("( {} ) || ( {} )", try_cmd, catch_cmd),
                    ));
                }

                // ── Zmienne środowiskowe ──────────────────────
                CommandType::AssignEnv { key, val } => {
                    let kp = self.str_ptr(key, "ekey");
                    let vp = self.str_ptr(val, "eval");
                    let ow = self.ctx.i32_type().const_int(1, false);
                    self.builder
                    .build_call(
                        self.setenv_fn,
                        &[kp.into(), vp.into(), ow.into()],
                                &uid("setenv"),
                    )
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
                        .build_call(
                            self.setenv_fn,
                            &[kp.into(), vp.into(), ow.into()],
                                    &uid("setenv_l"),
                        )
                        .unwrap();
                    } else {
                        self.emit_system(&format!("export {}={}", key, val));
                    }
                }

                // ── % Const — stała ───────────────────────────
                // Semantycznie identyczna z AssignEnv — różni się
                // tylko tym że parser nie pozwala jej nadpisać.
                // Na poziomie LLVM traktujemy jak setenv().
                CommandType::Const { key, val } => {
                    let is_dynamic = val.contains('$')
                    || val.contains('`')
                    || val.contains("$(");

                    if !is_dynamic {
                        let kp = self.str_ptr(key, "ckey");
                        let vp = self.str_ptr(val, "cval");
                        let ow = self.ctx.i32_type().const_int(1, false);
                        self.builder
                        .build_call(
                            self.setenv_fn,
                            &[kp.into(), vp.into(), ow.into()],
                                    &uid("setenv_c"),
                        )
                        .unwrap();
                    } else {
                        // Dynamiczna wartość stałej (np. %TS = $(date))
                        self.emit_system(&format!("export {}={}", key, val));
                    }

                    if self.verbose {
                        eprintln!("{} Const: %{} = {}", "[%]".yellow(), key, val);
                    }
                }

                // ── Call { path, args } ───────────────────────
                // Poprzednio Call(String) — teraz ma osobne args.
                CommandType::Call { path, args } => {
                    self.emit_call_hl(path, args, sudo);
                }

                // ── Spawn — asynchroniczne zadanie ───────────
                // Strategia: shell background (&) z zapisem PID
                // system("task &") — fire and forget bez przypisania
                CommandType::Spawn(task) => {
                    let cmd = format!("{} &", task.trim_start_matches('.'));
                    self.emit_system(&Self::wrap_sudo(sudo, &cmd));
                    if self.verbose {
                        eprintln!("{} Spawn (no handle): {}", "[~]".blue(), task);
                    }
                }

                // ── AssignSpawn — spawn z przypisaniem PID ───
                // key = spawn .task args
                // → system("export key=$( .task args & echo $! )")
                // Zapisujemy PID procesu tła do zmiennej key.
                // Caller może potem: await $key → wait $key
                CommandType::AssignSpawn { key, task } => {
                    let clean_task = task.trim_start_matches('.');
                    let cmd = format!(
                        "export {}=$( {} & echo $! )",
                                      key, clean_task
                    );
                    self.emit_system(&Self::wrap_sudo(sudo, &cmd));
                    if self.verbose {
                        eprintln!("{} AssignSpawn: {} = spawn {}", "[~]".blue(), key, task);
                    }
                }

                // ── Await — czekaj na zadanie ────────────────
                // await $job → wait $job
                // await .func → wywołaj .func synchronicznie
                CommandType::Await(expr) => {
                    let clean = expr.trim();
                    let cmd = if clean.starts_with('$') {
                        // await $pid → wait $pid
                        format!("wait {}", clean)
                    } else if clean.starts_with('.') {
                        // await .func → bezpośrednie wywołanie HL
                        let path = clean.trim_start_matches('.');
                        // Próba wywołania HL function
                        if let Some(f) = self.resolve_call(path) {
                            self.builder
                            .build_call(f, &[], &uid("await_call"))
                            .unwrap();
                            i += 1;
                            continue;
                        }
                        // Fallback: shell
                        clean.trim_start_matches('.').to_string()
                    } else {
                        format!("wait {}", clean)
                    };
                    self.emit_system(&Self::wrap_sudo(sudo, &cmd));
                }

                // ── AssignAwait — await z przypisaniem wyniku ─
                // key = await $job   → export key=$(wait $job; echo $?)
                // key = await .func  → wywołaj .func, wynik z _HL_OUT
                CommandType::AssignAwait { key, expr } => {
                    let clean = expr.trim();
                    if clean.starts_with('$') {
                        // Czekamy na PID i przechwytujemy exit code
                        let cmd = format!(
                            "wait {}; export {}=$?",
                            clean, key
                        );
                        self.emit_system(&Self::wrap_sudo(sudo, &cmd));
                    } else if clean.starts_with('.') {
                        // Wywołaj funkcję HL i pobierz _HL_OUT
                        let path = clean.trim_start_matches('.');
                        if let Some(f) = self.resolve_call(path) {
                            self.builder
                            .build_call(f, &[], &uid("await_fn"))
                            .unwrap();
                            // Przechwytuj _HL_OUT → key
                            let cmd = format!("export {}=$_HL_OUT", key);
                            self.emit_system(&cmd);
                        } else {
                            // Fallback: shell subprocess
                            let cmd = format!(
                                "export {}=$( {} )",
                                              key,
                                              clean.trim_start_matches('.')
                            );
                            self.emit_system(&Self::wrap_sudo(sudo, &cmd));
                        }
                    } else {
                        // Dowolne wyrażenie shell
                        let cmd = format!("export {}=$( {} )", key, clean);
                        self.emit_system(&Self::wrap_sudo(sudo, &cmd));
                    }
                    if self.verbose {
                        eprintln!("{} AssignAwait: {} = await {}", "[~]".blue(), key, expr);
                    }
                }

                // ── Assert ───────────────────────────────────
                // assert cond "msg"
                // → system("if ! ( cond ); then echo 'msg' >&2; exit 1; fi")
                // Emituje jeden system() call — zero narzutu w happy path
                // (branch prediction: assert prawie zawsze przechodzi).
                CommandType::Assert { cond, msg } => {
                    let message = msg.as_deref().unwrap_or("Assertion failed");
                    let sh = format!(
                        "if ! ( {} ) 2>/dev/null; then echo 'assert: {}' >&2; exit 1; fi",
                                     cond, message
                    );
                    self.emit_system(&Self::wrap_sudo(sudo, &sh));
                    if self.verbose {
                        eprintln!("{} Assert: {} → \"{}\"", "[a]".green(), cond, message);
                    }
                }

                // ── Match — nagłówek bloku dopasowania ───────
                // Zbieramy wszystkie następne MatchArm i emitujemy
                // jeden `case $var in ... esac` call przez system().
                //
                // Algorytm:
                //   1. Napotkaj Match { cond }
                //   2. Pochłaniaj kolejne MatchArm dopóki są
                //   3. Zbuduj pełny case...esac string
                //   4. Emituj jeden system() call
                //
                // Dzięki temu N ramion = 1 wywołanie system()
                // zamiast N wywołań (ogromna oszczędność fork/exec).
                CommandType::Match { cond } => {
                    let mut arms: Vec<(String, String)> = Vec::new();
                    i += 1;

                    // Pochłoń wszystkie następne MatchArm
                    while i < nodes.len() {
                        if let CommandType::MatchArm { val, cmd } = &nodes[i].content {
                            arms.push((val.clone(), cmd.clone()));
                            i += 1;
                        } else {
                            break;
                        }
                    }

                    if arms.is_empty() {
                        if self.verbose {
                            eprintln!(
                                "{} Match bez ramion: {}",
                                "[!]".yellow(), cond
                            );
                        }
                        continue;
                    }

                    // Buduj case ... esac
                    let mut sh = format!("case {} in\n", cond);
                    for (val, cmd) in &arms {
                        if val == "_" {
                            // _ → wildcard *
                            sh += &format!("  *) {};;\n", cmd);
                        } else {
                            // Usuń cudzysłowy z wartości jeśli są
                            let clean_val = val.trim_matches('"').trim_matches('\'');
                            sh += &format!("  {}) {};;\n", clean_val, cmd);
                        }
                    }
                    sh += "esac";

                    self.emit_system(&Self::wrap_sudo(sudo, &sh));

                    if self.verbose {
                        eprintln!(
                            "{} Match: {} ({} ramion)",
                                  "[m]".cyan(), cond, arms.len()
                        );
                    }
                    continue; // i już zinkrementowane w pętli pochłaniania
                }

                // ── MatchArm poza Match — ignoruj ────────────
                // Normalnie pochłaniane przez obsługę Match powyżej.
                // Jeśli trafiamy tu to błąd struktury AST — pomijamy.
                CommandType::MatchArm { .. } => {
                    if self.verbose {
                        eprintln!(
                            "{} MatchArm poza match — ignoruję",
                            "[!]".yellow()
                        );
                    }
                }

                // ── Pipe — łańcuch wywołań ────────────────────
                // .fetch |> .parse |> .sort "name" |> .take 10
                //
                // Strategia:
                //   • Funkcje HL → wywoływane sekwencyjnie,
                //     wynik każdej przekazywany przez _HL_OUT
                //   • Komendy shell → łączone przez | w jeden system()
                //
                // Heurystyka: jeśli wszystkie kroki są funkcjami HL
                // emitujemy sekwencję LLVM call.
                // Jeśli mieszane → fallback do shell pipe.
                CommandType::Pipe(steps) => {
                    if steps.is_empty() {
                        i += 1;
                        continue;
                    }

                    // Sprawdź czy wszystkie kroki to funkcje HL
                    let all_hl = steps.iter().all(|s| {
                        let clean = s.trim().trim_start_matches('.');
                        // Weź tylko nazwę funkcji (bez args)
                        let fname = clean.split_whitespace().next().unwrap_or("");
                        self.resolve_call(fname).is_some()
                    });

                    if all_hl {
                        // Fast path: sekwencja LLVM call
                        for step in steps {
                            let parts: Vec<&str> = step.trim().splitn(2, ' ').collect();
                            let fname = parts[0].trim_start_matches('.');
                            let args  = parts.get(1).copied().unwrap_or("");
                            if !args.is_empty() {
                                self.emit_system(&format!("export _HL_ARGS='{}'", args));
                            }
                            if let Some(f) = self.resolve_call(fname) {
                                self.builder
                                .build_call(f, &[], &uid("pipe_call"))
                                .unwrap();
                            }
                        }
                    } else {
                        // Slow path: shell pipe
                        // Funkcje HL zastępowane przez wywołania shell
                        let parts: Vec<String> = steps.iter().map(|s| {
                            let clean = s.trim().trim_start_matches('.');
                            clean.to_string()
                        }).collect();
                        let sh = parts.join(" | ");
                        self.emit_system(&Self::wrap_sudo(sudo, &sh));
                    }

                    if self.verbose {
                        eprintln!(
                            "{} Pipe: {} kroków",
                            "[|]".magenta(), steps.len()
                        );
                    }
                }

                // ── Plugin ────────────────────────────────────
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
                            eprintln!(
                                "{} Plugin '{}' nie znaleziony",
                                "[!]".yellow(), name
                            );
                        }
                        i += 1;
                        continue;
                    };

                    let cmd = if args.is_empty() {
                        base
                    } else {
                        format!("{} {}", base, args)
                    };
                    self.emit_system(&Self::wrap_sudo(*is_super, &cmd));
                }

                // ── GC Memory ─────────────────────────────────
                CommandType::Lock { key: _, val } => {
                    let size = val.parse::<u64>().unwrap_or(64);
                    let sz   = self.ctx.i64_type().const_int(size, false);
                    self.builder
                    .build_call(self.gc_malloc_fn, &[sz.into()], &uid("lock"))
                    .unwrap();
                }
                CommandType::Unlock { key: _ } => {
                    self.builder
                    .build_call(self.gc_unmark_fn, &[], &uid("unmark"))
                    .unwrap();
                    self.builder
                    .build_call(self.gc_sweep_fn,  &[], &uid("sweep"))
                    .unwrap();
                }

                // ── End ───────────────────────────────────────
                CommandType::End { code } => {
                    self.builder
                    .build_call(self.gc_full_fn, &[], &uid("gcfull"))
                    .unwrap();
                    let cv = self.ctx.i32_type().const_int(*code as u64, true);
                    self.builder
                    .build_call(self.exit_fn, &[cv.into()], &uid("exit"))
                    .unwrap();
                    self.builder.build_unreachable().unwrap();
                    return true; // terminator
                }

                // ── Extern ────────────────────────────────────
                CommandType::Extern { path, static_link } => {
                    self.extern_libs.push((path.clone(), *static_link));
                }

                // ── Metadane — brak generowanego kodu ─────────
                CommandType::Enum   { .. } => {}
                CommandType::Struct { .. } => {}
                CommandType::Import { .. } => {}

                // Elif/Else bez If — pochłonięte przez If powyżej
                CommandType::Elif { .. } => {}
                CommandType::Else { .. } => {}
            }

            i += 1;
        }
        false // brak terminatora
    }

    // ─────────────────────────────────────────────────────────
    // compile_functions
    // ─────────────────────────────────────────────────────────
    pub fn compile_functions(&mut self, ast: &AnalysisResult) {
        let mut names: Vec<&String> = ast.functions.keys().collect();
        names.sort();

        for name in names {
            let (is_unsafe, _sig, nodes) = &ast.functions[name];
            let func = self.hl_functions[name];

            if self.verbose {
                eprintln!(
                    "{} Kompilacja: {} (unsafe={}, sig={:?})",
                          "[f]".green(),
                          name,
                          is_unsafe,
                          _sig.as_deref().unwrap_or("-")
                );
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

    // ─────────────────────────────────────────────────────────
    // compile_main
    // ─────────────────────────────────────────────────────────
    pub fn compile_main(&mut self, ast: &AnalysisResult) {
        let i32_t   = self.ctx.i32_type();
        let main_fn = self.module.add_function(
            "main",
            i32_t.fn_type(&[], false),
                                               None,
        );
        main_fn.add_attribute(AttributeLoc::Function, self.nounwind_attr);

        let entry = self.ctx.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);

        let nodes = ast.main_body.clone();
        if !self.compile_body(&nodes) {
            self.builder
            .build_call(self.gc_full_fn, &[], "gc_final")
            .unwrap();
            self.builder
            .build_return(Some(&i32_t.const_int(0, false)))
            .unwrap();
        }
    }
}
