use crate::bytecode::{BytecodeProgram, HlValue, OpCode};
use crate::executor::{SessionManager, ShellKind};
use crate::gc_ffi::*;
use crate::jit::{JitCompiler, JitFunc, VmCtx};
use colored::*;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;

// ═════════════════════════════════════════════════════════════
// FFI — aa.c (arena allocator, tryb HL_ARENA_MODE_JIT)
//
// Bindingi do funkcji z libaa.a skompilowanej przez build.rs
// z flagą -DHL_ARENA_MODE_JIT.
//
// HlJitArenaScope jest opakowaniem na stos aren (do 64 poziomów).
// Każdy :: blok woła hl_jit_arena_enter / hl_jit_arena_exit.
// ═════════════════════════════════════════════════════════════

/// Rozmiar HlJitArenaScope z aa.c:
///   frames[64] × (ptr HlArena + char[64]) + int depth
///   = 64 × (8 + 64) + 4 = 64 × 72 + 4 = 4612 bajtów
///   Zaokrąglamy do 4624 (wyrównanie 16).
const HL_JIT_ARENA_SCOPE_SIZE: usize = 4624;

/// Nieprzezroczysty bufor dla HlJitArenaScope.
/// Alokowany inline w VM — bez malloc, bez GC.
#[repr(C, align(16))]
pub struct HlJitArenaScopeOpaque {
    _data: [u8; HL_JIT_ARENA_SCOPE_SIZE],
}

impl HlJitArenaScopeOpaque {
    pub fn new() -> Self {
        Self { _data: [0u8; HL_JIT_ARENA_SCOPE_SIZE] }
    }

    pub fn as_mut_ptr(&mut self) -> *mut c_void {
        self._data.as_mut_ptr() as *mut c_void
    }
}

extern "C" {
    /// Wejdź w :: blok — alokuje nową areną i wrzuca na stos.
    /// name: nazwa funkcji (np. "cache"), size_spec: "512kb"
    /// Zwraca 0 przy sukcesie, -1 przy błędzie.
    fn hl_jit_arena_enter(
        scope:     *mut c_void,
        name:      *const c_char,
        size_spec: *const c_char,
    ) -> i32;

    /// Wyjdź z :: bloku — zwalnia arenę jednym hl_arena_free().
    /// GC nie jest zaangażowany.
    /// Zwraca 0 przy sukcesie, -1 jeśli stos pusty.
    fn hl_jit_arena_exit(scope: *mut c_void) -> i32;

    /// Jawna alokacja z bieżącej areny.
    /// Zwraca NULL jeśli brak miejsca lub stos pusty.
    fn hl_jit_arena_alloc(scope: *mut c_void, n: usize) -> *mut c_void;

    /// Reset bieżącej areny (bump pointer → base).
    /// Używany wewnątrz pętli w :: bloku.
    fn hl_jit_arena_reset(scope: *mut c_void);

    /// Zwolnij wszystkie areny (cleanup przy panic/unwind).
    fn hl_jit_arena_cleanup(scope: *mut c_void);
}

// ═════════════════════════════════════════════════════════════
// LocalVal
// ═════════════════════════════════════════════════════════════
pub enum LocalVal {
    /// Zarządzany przez GC (gc_malloc)
    Managed(*mut c_char),
    /// Surowy string bez GC (is_raw=true lub fallback)
    Raw(String),
}
unsafe impl Send for LocalVal {}
unsafe impl Sync for LocalVal {}

// ═════════════════════════════════════════════════════════════
// DeferEntry — wpis na stosie defer (defer expr)
// ═════════════════════════════════════════════════════════════
#[derive(Debug, Clone)]
pub struct DeferEntry {
    pub expr: String,
    pub sudo: bool,
}

// ═════════════════════════════════════════════════════════════
// VM
// ═════════════════════════════════════════════════════════════
pub struct VM {
    // ── Środowisko ────────────────────────────────────────────
    pub env:        HashMap<String, String>,
    pub locals:     HashMap<String, LocalVal>,
    pub heap:       HashMap<String, Vec<u8>>,

    // ── Sesja / executor ─────────────────────────────────────
    pub session:    SessionManager,

    // ── JIT ───────────────────────────────────────────────────
    pub jit:        JitCompiler,

    // ── Flagi ─────────────────────────────────────────────────
    pub verbose:    bool,
    pub dry_run:    bool,

    // ── v6 ────────────────────────────────────────────────────
    pub const_keys: HashSet<String>,
    pub hl_out:     String,

    // ── v7: rejestry numeryczne ───────────────────────────────
    pub regs_i:     Box<[i64; 256]>,
    pub regs_f:     Box<[f64; 256]>,
    pub cmp_flag:   bool,
    pub typed_vars: HashMap<String, HlValue>,

    // ── Arena allocator (aa.c, tryb JIT) ─────────────────────
    // Nieprzezroczysty bufor HlJitArenaScope — inline, bez GC.
    // Używany WYŁĄCZNIE przez OpCode::ArenaEnter/Exit/Alloc/Reset.
    // Reszta kodu nadal używa gc_malloc przez alloc_local().
    pub arena_scope: HlJitArenaScopeOpaque,

    // ── Defer stack ──────────────────────────────────────────
    // Stos wyrażeń do wykonania przy Return/Exit.
    // Wypełniany przez Exec z konwencją "_HL_DEFER_PUSH <expr>".
    pub defer_stack: Vec<DeferEntry>,

    // ── Lambda registry ──────────────────────────────────────
    // Przechowuje zakodowane lambdy: key → (params, body)
    pub lambdas:    HashMap<String, (Vec<String>, String)>,

    // ── ADT registry ─────────────────────────────────────────
    // Przechowuje zdefiniowane typy: TypeName → Vec<VariantName>
    pub adt_types:  HashMap<String, Vec<String>>,

    // ── Test runner state ────────────────────────────────────
    pub test_passed: usize,
    pub test_failed: usize,
    pub in_test:     bool,
    pub current_test: String,
}

impl VM {
    pub fn new(verbose: bool, dry_run: bool) -> Self {
        Self::with_shell(ShellKind::default(), verbose, dry_run)
    }

    pub fn with_shell(shell: ShellKind, verbose: bool, dry_run: bool) -> Self {
        Self {
            env:          std::env::vars().collect(),
            locals:       HashMap::new(),
            heap:         HashMap::new(),
            session:      SessionManager::with_shell(shell, verbose),
            jit:          JitCompiler::new(verbose),
            verbose,
            dry_run,
            const_keys:   HashSet::new(),
            hl_out:       String::new(),
            regs_i:       Box::new([0i64; 256]),
            regs_f:       Box::new([0f64; 256]),
            cmp_flag:     false,
            typed_vars:   HashMap::new(),
            // Arena scope — zerowany bufor (scope.depth = 0 → pusty stos)
            arena_scope:  HlJitArenaScopeOpaque::new(),
            defer_stack:  Vec::new(),
            lambdas:      HashMap::new(),
            adt_types:    HashMap::new(),
            test_passed:  0,
            test_failed:  0,
            in_test:      false,
            current_test: String::new(),
        }
    }

    // ── Podstawianie zmiennych ────────────────────────────────
    #[inline]
    pub fn substitute(&self, text: &str) -> String {
        if !text.contains('$') { return text.to_string(); }
        let mut res = text.to_string();
        for (k, val) in &self.locals {
            let v = match val {
                LocalVal::Raw(s)     => s.clone(),
                LocalVal::Managed(p) => unsafe {
                    CStr::from_ptr(*p).to_str().unwrap_or("").to_string()
                },
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

    // ── GC: alokuj zmienną lokalną ───────────────────────────
    pub fn alloc_local(&mut self, key: &str, val: &str) {
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
                unsafe { std::ptr::copy_nonoverlapping(cstr.as_ptr(), ptr, size) };
                self.locals.insert(key.to_string(), LocalVal::Managed(ptr));
            }
            Err(_) => {
                if self.verbose {
                    eprintln!("{} Zmienna '{}' zawiera bajt null — Raw", "[!]".yellow(), key);
                }
                self.locals.insert(key.to_string(), LocalVal::Raw(val.to_string()));
            }
        }
    }

    // ── GC collect ───────────────────────────────────────────
    pub fn gc_collect(&mut self) {
        unsafe {
            gc_unmark_all();
            for val in self.locals.values() {
                if let LocalVal::Managed(p) = val {
                    gc_mark(*p as *mut c_void);
                }
            }
            gc_sweep();
        }
    }

    // ── Arena: cleanup przy panic / exit ─────────────────────
    pub fn arena_cleanup(&mut self) {
        unsafe {
            hl_jit_arena_cleanup(self.arena_scope.as_mut_ptr());
        }
    }

    // ── Rozwiązywanie funkcji ─────────────────────────────────
    pub fn resolve_func(&self, name: &str, fns: &HashMap<String, usize>) -> Option<usize> {
        let c = name.trim_start_matches('.');
        if let Some(&a) = fns.get(c) { return Some(a); }
        for (fname, &addr) in fns {
            if fname == c || fname.ends_with(&format!(".{}", c)) {
                return Some(addr);
            }
        }
        None
    }

    // ── SetEnv helper ─────────────────────────────────────────
    fn do_set_env(&mut self, key: &str, val: &str) {
        if self.const_keys.contains(key) {
            if self.verbose {
                eprintln!(
                    "{} Ostrzeżenie: próba nadpisania stałej %{} — ignoruję",
                    "[!]".yellow(), key
                );
            }
            return;
        }
        std::env::set_var(key, val);
        self.session.set_env(key, val);
        self.env.insert(key.to_string(), val.to_string());
    }

    // ── Sync typed_var → shell env ────────────────────────────
    fn sync_typed_to_env(&mut self, name: &str) {
        if let Some(val) = self.typed_vars.get(name) {
            let s = val.to_env_string();
            std::env::set_var(name, &s);
            self.session.set_env(name, &s);
            self.env.insert(name.to_string(), s.clone());
            self.alloc_local(name, &s);
        }
    }

    // ── LoadVarI ──────────────────────────────────────────────
    fn load_var_i(&self, name: &str) -> i64 {
        if let Some(tv) = self.typed_vars.get(name) {
            return tv.as_int();
        }
        let s = self.env.get(name)
        .map(|s| s.as_str())
        .or_else(|| match self.locals.get(name) {
            Some(LocalVal::Raw(s))     => Some(s.as_str()),
                 Some(LocalVal::Managed(p)) => unsafe {
                     CStr::from_ptr(*p).to_str().ok()
                 },
                 None => None,
        })
        .unwrap_or("0");
        s.parse().unwrap_or(0)
    }

    // ── LoadVarF ──────────────────────────────────────────────
    fn load_var_f(&self, name: &str) -> f64 {
        if let Some(tv) = self.typed_vars.get(name) {
            return tv.as_float();
        }
        let s = self.env.get(name)
        .map(|s| s.as_str())
        .or_else(|| match self.locals.get(name) {
            Some(LocalVal::Raw(s))     => Some(s.as_str()),
                 Some(LocalVal::Managed(p)) => unsafe {
                     CStr::from_ptr(*p).to_str().ok()
                 },
                 None => None,
        })
        .unwrap_or("0");
        s.parse().unwrap_or(0.0)
    }

    // ── Wykonanie defer stack ─────────────────────────────────
    // Wywoływane przed każdym Return / Exit.
    pub fn flush_defers(&mut self) {
        // Defer wykonywany w odwrotnej kolejności (LIFO)
        let entries: Vec<DeferEntry> = self.defer_stack.drain(..).rev().collect();
        for entry in entries {
            if self.verbose {
                eprintln!("{} defer: {}", "[↩]".yellow(), entry.expr.dimmed());
            }
            if !self.dry_run {
                self.session.exec(&entry.expr, entry.sudo);
            }
        }
    }

    // ── Obsługa konwencji Exec ────────────────────────────────
    // Rozpoznaje wewnętrzne komendy VM zaczynające się od _HL_
    // emitowane przez compiler.rs dla nowych konstruktów językowych.
    // Zwraca true jeśli komenda została obsłużona wewnętrznie.
    fn handle_hl_convention(
        &mut self,
        cmd:  &str,
        sudo: bool,
        prog: &BytecodeProgram,
    ) -> bool {
        let t = cmd.trim();

        // ── Kolekcje: _HL_COLL_<METHOD> var [args] ───────────
        if let Some(rest) = t.strip_prefix("_HL_COLL_") {
            return self.handle_collection(rest, sudo);
        }

        // ── Defer push: _HL_DEFER_PUSH expr ──────────────────
        if let Some(expr) = t.strip_prefix("_HL_DEFER_PUSH ") {
            let subst = self.substitute(expr.trim());
            self.defer_stack.push(DeferEntry { expr: subst, sudo });
            if self.verbose {
                eprintln!("{} defer push: {}", "[↩]".yellow(), expr.trim().dimmed());
            }
            return true;
        }

        // ── Rekurencja ogonowa: _HL_RECUR_ARGS args ───────────
        if let Some(args) = t.strip_prefix("_HL_RECUR_ARGS ") {
            let subst = self.substitute(args.trim());
            // Zapisz argumenty w _HL_RECUR_ARGS do użycia przez _HL_RECUR
            std::env::set_var("_HL_RECUR_ARGS", &subst);
            self.env.insert("_HL_RECUR_ARGS".to_string(), subst);
            return true;
        }

        // ── _HL_RECUR — sygnał do vm.run() przez flagę ────────
        // Obsługiwane bezpośrednio w pętli run() przez sprawdzenie
        // cmd == "_HL_RECUR" i ustawienie ip = current_func_base
        // (patrz część 2 vm.rs)
        if t == "_HL_RECUR" {
            return false; // przekaż do pętli run()
        }

        // ── Lambda push: _HL_LAMBDA_PUSH params : body ───────
        if let Some(rest) = t.strip_prefix("_HL_LAMBDA_PUSH ") {
            if let Some(colon) = rest.find(" : ") {
                let params_str = &rest[..colon];
                let body       = rest[colon + 3..].trim().to_string();
                let params: Vec<String> = params_str.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
                // Zapisz jako _HL_LAST_LAMBDA do odczytu przez vm
                let key = "_HL_LAST_LAMBDA".to_string();
                let encoded = format!("__hl_lambda:{}:{}", params_str, body);
                self.env.insert(key.clone(), encoded.clone());
                std::env::set_var(&key, &encoded);
            }
            if self.verbose {
                eprintln!("{} lambda push: {}", "[λ]".magenta(), rest.dimmed());
            }
            return true;
        }

        // ── ADT def: _HL_ADT_DEF TypeName Variant [fields] ───
        if let Some(rest) = t.strip_prefix("_HL_ADT_DEF ") {
            let parts: Vec<&str> = rest.splitn(3, ' ').collect();
            if parts.len() >= 2 {
                let type_name   = parts[0].to_string();
                let variant_name = parts[1].to_string();
                self.adt_types
                .entry(type_name.clone())
                .or_default()
                .push(variant_name.clone());
                if self.verbose {
                    eprintln!(
                        "{} ADT: {}::{}",
                        "[T]".magenta(), type_name, variant_name
                    );
                }
            }
            return true;
        }

        // ── Interface def: _HL_IFACE_DEF Name methods ────────
        if let Some(rest) = t.strip_prefix("_HL_IFACE_DEF ") {
            if self.verbose {
                eprintln!("{} interface def: {}", "[i]".cyan(), rest.dimmed());
            }
            return true;
        }

        // ── Impl def: _HL_IMPL_DEF Class Interface ────────────
        if let Some(rest) = t.strip_prefix("_HL_IMPL_DEF ") {
            if self.verbose {
                eprintln!("{} impl def: {}", "[I]".cyan(), rest.dimmed());
            }
            return true;
        }

        // ── Scope enter: _HL_SCOPE_ENTER ─────────────────────
        if t == "_HL_SCOPE_ENTER" {
            if self.verbose {
                eprintln!("{} scope enter", "[s]".green());
            }
            return true;
        }

        // ── Module call: hl_module_<path> [args] ─────────────
        if let Some(rest) = t.strip_prefix("hl_module_") {
            return self.handle_module_call(rest, sudo, prog);
        }

        // ── Test begin/end ────────────────────────────────────
        if let Some(desc) = t.strip_prefix("_HL_TEST_BEGIN ") {
            self.in_test      = true;
            self.current_test = desc.trim().trim_matches('"').to_string();
            if self.verbose {
                eprintln!("{} test: \"{}\"", "[✓]".green().bold(), self.current_test);
            }
            return true;
        }
        if let Some(desc) = t.strip_prefix("_HL_TEST_END ") {
            let name = desc.trim().trim_matches('"');
            if self.verbose {
                eprintln!("{} test done: \"{}\"", "[✓]".green(), name);
            }
            self.in_test = false;
            return true;
        }

        false
    }

    // ── Obsługa kolekcji ──────────────────────────────────────
    fn handle_collection(&mut self, rest: &str, _sudo: bool) -> bool {
        // rest = "PUSH var args" / "POP var" / "SET var key val" / itp.
        let parts: Vec<&str> = rest.splitn(3, ' ').collect();
        if parts.is_empty() { return true; }

        let method = parts[0];
        let var    = parts.get(1).map(|s| self.substitute(s)).unwrap_or_default();
        let args   = parts.get(2).map(|s| self.substitute(s)).unwrap_or_default();

        match method {
            "PUSH" => {
                // Bash array: $var+=("$args")
                let sh = format!("{}+=({})", var, args);
                if !self.dry_run {
                    self.session.exec(&sh, false);
                }
            }
            "POP" => {
                // Bash: unset var[-1]
                let len_cmd = format!("unset '{}'[-1]", var);
                if !self.dry_run {
                    self.session.exec(&len_cmd, false);
                }
            }
            "SET" => {
                // Bash associative: var[key]=val
                let kv: Vec<&str> = args.splitn(2, ' ').collect();
                if kv.len() == 2 {
                    let sh = format!("{}[{}]={}", var, kv[0], kv[1]);
                    if !self.dry_run {
                        self.session.exec(&sh, false);
                    }
                }
            }
            "DEL" => {
                // Bash: unset var[key]
                let sh = format!("unset '{}[{}]'", var, args);
                if !self.dry_run {
                    self.session.exec(&sh, false);
                }
            }
            "GET" => {
                // Bash: export _HL_OUT=${var[key]}
                let sh = format!("export _HL_OUT=${{{}[{}]}}", var, args);
                if !self.dry_run {
                    self.session.exec(&sh, false);
                    let v = std::env::var("_HL_OUT").unwrap_or_default();
                    self.hl_out = v;
                }
            }
            _ => {}
        }

        if self.verbose {
            eprintln!(
                "{} coll: ${}.{} {}",
                "[c]".blue(), var, method.to_lowercase(), args
            );
        }
        true
    }

    // ── Obsługa module call ───────────────────────────────────
    fn handle_module_call(
        &mut self,
        rest: &str,
        sudo: bool,
        _prog: &BytecodeProgram,
    ) -> bool {
        // rest = "http_get \"url\"" → path=http.get, args="url"
        // Szukamy binarki hl-module-http lub pluginu http
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let path_underscored  = parts[0];
        let args              = parts.get(1).map(|s| self.substitute(s)).unwrap_or_default();

        // Zamień pierwsze _ na . żeby odtworzyć "http_get" → szukaj "hl-module-http"
        let module_name = path_underscored.splitn(2, '_').next().unwrap_or(path_underscored);
        let method_name = path_underscored.splitn(2, '_').nth(1).unwrap_or("");

        let bin_name = format!("hl-module-{}", module_name);
        let bin_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".hackeros/hacker-lang/modules")
        .join(&bin_name);

        let cmd = if bin_path.exists() {
            if args.is_empty() {
                format!("{} {}", bin_path.display(), method_name)
            } else {
                format!("{} {} {}", bin_path.display(), method_name, args)
            }
        } else {
            // Fallback: szukaj pluginu
            let plugin_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".hackeros/hacker-lang/plugins")
            .join(format!("{}.hl", module_name));
            if plugin_path.exists() {
                let rt = std::env::current_exe().unwrap_or_default();
                if args.is_empty() {
                    format!("{} {} {}", rt.display(), plugin_path.display(), method_name)
                } else {
                    format!("{} {} {} {}", rt.display(), plugin_path.display(), method_name, args)
                }
            } else {
                if self.verbose {
                    eprintln!(
                        "{} module '{}' nie znaleziony",
                        "[!]".yellow(), module_name
                    );
                }
                return true;
            }
        };

        if self.verbose {
            eprintln!("{} module: {}", "[M]".cyan(), cmd.dimmed());
        }
        if !self.dry_run {
            self.session.exec(&cmd, sudo);
        }
        true
    }

    // ── Plugin runner ─────────────────────────────────────────
    fn run_plugin(&mut self, name: &str, args: &str, sudo: bool) {
        let root = get_plugins_root();
        let bin  = root.join(name);
        let hl   = PathBuf::from(format!("{}.hl", bin.display()));

        let tgt = if bin.exists() {
            Some(bin.to_str().unwrap_or("").to_string())
        } else if hl.exists() {
            let rt = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hl"));
            Some(format!("{} {}", rt.display(), hl.display()))
        } else {
            eprintln!("{} Plugin '{}' nie znaleziony: {}",
                      "[!]".yellow(), name, root.display());
            None
        };

        if let Some(base) = tgt {
            let cmd = if args.is_empty() { base } else { format!("{} {}", base, args) };
            self.session.exec(&cmd, sudo);
        }
    }

    // ─────────────────────────────────────────────────────────
    // GŁÓWNA PĘTLA VM — CZĘŚĆ 1/2
    // Opcody: Exec, JumpIfFalse, Jump, CallFunc, Return, Exit,
    //         SetEnv, SetLocal, SetConst, SetOut,
    //         SpawnBg, SpawnAssign, AwaitPid, AwaitAssign,
    //         Assert, MatchExec, PipeExec, Plugin,
    //         Lock, Unlock, HotLoop, Nop,
    //         ArenaEnter, ArenaExit, ArenaAlloc, ArenaReset
    //
    // Część 2/2 (vm_part2.rs lub dalsza część tego pliku):
    //         LoadInt..ReturnF (opcody numeryczne v7)
    // ─────────────────────────────────────────────────────────
    pub fn run(&mut self, prog: &BytecodeProgram) -> i32 {
        let mut ip:              usize      = 0;
        let mut call_stack:      Vec<usize> = Vec::with_capacity(32);
        // Baza IP bieżącej funkcji — używana przez _HL_RECUR
        let mut current_func_base: usize   = 0;

        while ip < prog.ops.len() {
            match &prog.ops[ip] {

                // ── Exec ──────────────────────────────────────
                OpCode::Exec { cmd_id, sudo } => {
                    let raw = prog.str(*cmd_id);
                    let cmd = self.substitute(raw);

                    if self.verbose {
                        eprintln!("{} [{}] {}", "[>]".cyan(), ip, cmd.dimmed());
                    }

                    if !self.dry_run {
                        // Sprawdź wewnętrzne konwencje _HL_*
                        if cmd.starts_with("_HL_") || cmd.starts_with("hl_module_") {
                            if cmd.trim() == "_HL_RECUR" {
                                // Rekurencja ogonowa → skocz na bazę bieżącej funkcji
                                if self.verbose {
                                    eprintln!(
                                        "{} [{}] RECUR → {}",
                                        "[r]".cyan(), ip, current_func_base
                                    );
                                }
                                ip = current_func_base;
                                continue;
                            }

                            if self.handle_hl_convention(&cmd, *sudo, prog) {
                                ip += 1;
                                continue;
                            }
                        }

                        let code = self.session.exec(&cmd, *sudo);
                        if code != 0 && self.verbose {
                            eprintln!("{} exit: {}", "[!]".yellow(), code);
                        }
                    }
                }

                // ── JumpIfFalse ───────────────────────────────
                OpCode::JumpIfFalse { cond_id, target } => {
                    let raw      = prog.str(*cond_id);
                    let expanded = self.substitute(raw);
                    let result   = if self.dry_run {
                        true
                    } else {
                        self.session.eval_cond(&expanded)
                    };
                    if self.verbose {
                        eprintln!(
                            "{} [{}] JIF {} → {}",
                            "[?]".cyan(), ip, expanded.dimmed(),
                                  if result { "TRUE".green().to_string() }
                                  else { format!("FALSE → {}", target).red().to_string() }
                        );
                    }
                    if !result { ip = *target; continue; }
                }

                // ── Jump ──────────────────────────────────────
                OpCode::Jump { target } => {
                    if self.verbose {
                        eprintln!("{} [{}] JMP → {}", "[j]".cyan(), ip, target);
                    }
                    ip = *target;
                    continue;
                }

                // ── CallFunc ──────────────────────────────────
                OpCode::CallFunc { func_id } => {
                    let func_id_val = *func_id;
                    let fname       = prog.str(func_id_val);

                    match self.resolve_func(fname, &prog.functions) {
                        None => {
                            eprintln!(
                                "{} Runtime: funkcja '{}' nie znaleziona",
                                "[x]".red(), fname
                            );
                        }
                        Some(addr) => {
                            if self.verbose {
                                eprintln!(
                                    "{} [{}] CALL .{} → ip={}",
                                    "[f]".green(), ip, fname, addr
                                );
                            }

                            let is_hot = self.jit.register_call(func_id_val);
                            if is_hot && !self.dry_run && !self.jit.is_compiled(func_id_val) {
                                self.jit.compile(func_id_val, addr, prog);
                            }

                            let jit_raw: Option<*const JitFunc> =
                            if is_hot && !self.dry_run {
                                self.jit.compiled.get(&func_id_val)
                                .map(|jf| jf as *const JitFunc)
                            } else { None };

                            if let Some(fn_ptr) = jit_raw {
                                let session_raw =
                                std::ptr::addr_of_mut!(self.session) as *mut c_void;
                                let regs_i_ptr   = self.regs_i.as_mut_ptr();
                                let regs_f_ptr   = self.regs_f.as_mut_ptr();
                                let cmp_flag_u8  = &mut self.cmp_flag as *mut bool as *mut u8;

                                let mut ctx = VmCtx {
                                    exec_fn:      trampoline_exec,
                                    eval_cond_fn: trampoline_eval_cond,
                                    session_ptr:  session_raw,
                                    pool_ptr:     std::ptr::null(),
                                    exit_code:    0,
                                    should_exit:  0,
                                    regs_i_ptr,
                                    regs_f_ptr,
                                    cmp_flag_ptr: cmp_flag_u8,
                                };
                                unsafe { call_jit_fn(fn_ptr, &mut ctx); }
                                if ctx.should_exit != 0 {
                                    self.flush_defers();
                                    self.arena_cleanup();
                                    self.gc_collect();
                                    return ctx.exit_code;
                                }
                                ip += 1;
                                continue;
                            }

                            // Interpreter: push return addr, skocz do funkcji
                            call_stack.push(ip + 1);
                            let prev_base    = current_func_base;
                            current_func_base = addr;
                            ip = addr;
                            // prev_base jest przywracana przez Return — zapisz na stosie
                            // jako sentinel z MSB ustawionym
                            // (uproszczenie: recur działa tylko w bezpośredniej funkcji)
                            let _ = prev_base; // używane przy Return
                            continue;
                        }
                    }
                }

                // ── Return ────────────────────────────────────
                OpCode::Return => {
                    self.flush_defers();
                    match call_stack.pop() {
                        Some(ret) => {
                            ip = ret;
                            // Przywróć bazę funkcji wywołującej
                            // (jeśli stos pusty → wracamy do main_body, base=0)
                            current_func_base = call_stack.last().copied().unwrap_or(0);
                            continue;
                        }
                        None => {
                            self.arena_cleanup();
                            self.gc_collect();
                            return 0;
                        }
                    }
                }

                // ── Exit ──────────────────────────────────────
                OpCode::Exit(code) => {
                    self.flush_defers();
                    self.arena_cleanup();
                    self.gc_collect();
                    return *code;
                }

                // ── SetEnv ────────────────────────────────────
                OpCode::SetEnv { key_id, val_id } => {
                    let key = prog.str(*key_id).to_string();
                    let val = self.substitute(prog.str(*val_id));
                    if self.verbose {
                        eprintln!("{} [{}] SENV {}={}", "[e]".blue(), ip, key, val);
                    }
                    self.do_set_env(&key, &val);
                }

                // ── SetLocal ──────────────────────────────────
                OpCode::SetLocal { key_id, val_id, is_raw } => {
                    let key = prog.str(*key_id).to_string();
                    let val = self.substitute(prog.str(*val_id));

                    // Rozpoznaj lambdę zakodowaną jako __hl_lambda:params:body
                    if val.starts_with("__hl_lambda:") {
                        let rest = &val["__hl_lambda:".len()..];
                        if let Some(colon) = rest.find(':') {
                            let params_str = &rest[..colon];
                            let body       = rest[colon + 1..].to_string();
                            let params: Vec<String> = params_str.split(',')
                            .map(|p| p.trim().to_string())
                            .collect();
                            self.lambdas.insert(key.clone(), (params, body));
                        }
                    }

                    if self.verbose {
                        eprintln!("{} [{}] SLOC ${}={}", "[l]".blue(), ip, key, val);
                    }
                    if *is_raw {
                        self.locals.insert(key, LocalVal::Raw(val));
                    } else {
                        self.alloc_local(&key.clone(), &val);
                    }
                    self.session.invalidate_cond_cache();
                }

                // ── SetConst ──────────────────────────────────
                OpCode::SetConst { key_id, val_id } => {
                    let key = prog.str(*key_id).to_string();
                    let val = self.substitute(prog.str(*val_id));
                    if self.verbose {
                        eprintln!("{} [{}] SCONST %{}={}", "[%]".yellow(), ip, key, val);
                    }
                    if !self.dry_run {
                        std::env::set_var(&key, &val);
                        self.session.set_env(&key, &val);
                        self.env.insert(key.clone(), val);
                        self.const_keys.insert(key);
                    }
                }

                // ── SetOut ────────────────────────────────────
                OpCode::SetOut { val_id } => {
                    let val = self.substitute(prog.str(*val_id));
                    if self.verbose {
                        eprintln!("{} [{}] OUT = {:?}", "[o]".cyan(), ip, val);
                    }
                    if !self.dry_run {
                        self.hl_out = val.clone();
                        std::env::set_var("_HL_OUT", &val);
                        self.session.set_env("_HL_OUT", &val);
                        self.env.insert("_HL_OUT".to_string(), val);
                    }
                }

                // ── SpawnBg ───────────────────────────────────
                OpCode::SpawnBg { cmd_id, sudo } => {
                    let raw = prog.str(*cmd_id);
                    let cmd = self.substitute(raw);
                    let bg  = format!("{} &", cmd);
                    if self.verbose {
                        eprintln!("{} [{}] SPAWN {}", "[~]".blue(), ip, bg.dimmed());
                    }
                    if !self.dry_run {
                        self.session.exec(&bg, *sudo);
                    }
                }

                // ── SpawnAssign ───────────────────────────────
                OpCode::SpawnAssign { key_id, cmd_id, sudo } => {
                    let key = prog.str(*key_id).to_string();
                    let raw = prog.str(*cmd_id);
                    let cmd = self.substitute(raw);
                    let sh  = format!("export {}=$( {} & echo $! )", key, cmd);
                    if self.verbose {
                        eprintln!("{} [{}] SPAWNA {} = spawn {}", "[~]".blue(), ip, key, cmd.dimmed());
                    }
                    if !self.dry_run {
                        self.session.exec(&sh, *sudo);
                        let pid = std::env::var(&key).unwrap_or_default();
                        self.env.insert(key.clone(), pid.clone());
                        self.alloc_local(&key, &pid);
                    }
                }

                // ── AwaitPid ──────────────────────────────────
                OpCode::AwaitPid { expr_id } => {
                    let raw   = prog.str(*expr_id);
                    let expr  = self.substitute(raw);
                    let clean = expr.trim().to_string();
                    if self.verbose {
                        eprintln!("{} [{}] AWAIT {}", "[~]".blue(), ip, clean.dimmed());
                    }
                    if !self.dry_run {
                        if clean.starts_with('.') {
                            let fname = clean.trim_start_matches('.');
                            if let Some(addr) = self.resolve_func(fname, &prog.functions) {
                                call_stack.push(ip + 1);
                                ip = addr;
                                continue;
                            }
                        }
                        let sh = format!("wait {}", clean);
                        self.session.exec(&sh, false);
                    }
                }

                // ── AwaitAssign ───────────────────────────────
                OpCode::AwaitAssign { key_id, expr_id } => {
                    let key   = prog.str(*key_id).to_string();
                    let raw   = prog.str(*expr_id);
                    let expr  = self.substitute(raw);
                    let clean = expr.trim().to_string();
                    if self.verbose {
                        eprintln!(
                            "{} [{}] AWAITA {} = await {}",
                            "[~]".blue(), ip, key, clean.dimmed()
                        );
                    }
                    if !self.dry_run {
                        if clean.starts_with('.') {
                            let fname = clean.trim_start_matches('.');
                            if let Some(addr) = self.resolve_func(fname, &prog.functions) {
                                call_stack.push(ip + 1);
                                ip = addr;
                                let out_val = self.hl_out.clone();
                                self.alloc_local(&key, &out_val);
                                self.session.invalidate_cond_cache();
                                continue;
                            }
                        }
                        if clean.starts_with('$') {
                            let sh = format!("wait {}; export {}=$?", clean, key);
                            self.session.exec(&sh, false);
                        } else {
                            let sh = format!("export {}=$( {} )", key, clean);
                            self.session.exec(&sh, false);
                        }
                        let v = std::env::var(&key).unwrap_or_default();
                        self.alloc_local(&key, &v);
                        self.session.invalidate_cond_cache();
                    }
                }

                // ── Assert ────────────────────────────────────
                OpCode::Assert { cond_id, msg_id } => {
                    let raw_cond = prog.str(*cond_id);
                    let cond     = self.substitute(raw_cond);
                    let wrapped  = if cond.trim().starts_with('[')
                    || cond.trim().starts_with("((")
                    {
                        cond.clone()
                    } else {
                        format!("[[ {} ]]", cond)
                    };
                    if self.verbose {
                        eprintln!("{} [{}] ASSERT {}", "[a]".green(), ip, cond.dimmed());
                    }
                    if !self.dry_run {
                        let ok = self.session.eval_cond(&wrapped);
                        if !ok {
                            let msg = msg_id
                            .map(|id| prog.str(id).to_string())
                            .unwrap_or_else(|| format!("Assertion failed: {}", cond));
                            eprintln!("{} assert: {}", "[!]".red().bold(), msg.red());
                            // W kontekście testu — nie exituj, licz błędy
                            if self.in_test {
                                self.test_failed += 1;
                            } else {
                                self.flush_defers();
                                self.arena_cleanup();
                                self.gc_collect();
                                return 1;
                            }
                        } else if self.in_test {
                            self.test_passed += 1;
                        }
                    }
                }

                // ── MatchExec ─────────────────────────────────
                OpCode::MatchExec { case_cmd_id, sudo } => {
                    let raw = prog.str(*case_cmd_id);
                    let cmd = self.substitute(raw);
                    if self.verbose {
                        eprintln!(
                            "{} [{}] MATCH {}",
                            "[m]".cyan(), ip, &cmd[..cmd.len().min(60)].dimmed()
                        );
                    }
                    if !self.dry_run {
                        self.session.exec(&cmd, *sudo);
                    }
                }

                // ── PipeExec ──────────────────────────────────
                OpCode::PipeExec { cmd_id, sudo } => {
                    let raw = prog.str(*cmd_id);
                    let cmd = self.substitute(raw);
                    if self.verbose {
                        eprintln!("{} [{}] PIPE {}", "[|]".magenta(), ip, cmd.dimmed());
                    }
                    if !self.dry_run {
                        self.session.exec(&cmd, *sudo);
                    }
                }

                // ── Plugin ────────────────────────────────────
                OpCode::Plugin { name_id, args_id, sudo } => {
                    let name = prog.str(*name_id).to_string();
                    let args = self.substitute(prog.str(*args_id));
                    if self.verbose {
                        eprintln!("{} [{}] PLGN \\{} {}", "[p]".cyan(), ip, name, args);
                    }
                    if !self.dry_run {
                        self.run_plugin(&name, &args, *sudo);
                    }
                }

                // ── Lock / Unlock ─────────────────────────────
                OpCode::Lock { key_id, val_id } => {
                    let k  = self.substitute(prog.str(*key_id));
                    let v  = self.substitute(prog.str(*val_id));
                    let sz = v.parse::<usize>().unwrap_or(v.len().max(1));
                    if self.verbose {
                        eprintln!("{} [{}] LOCK {} ({} B)", "[m]".magenta(), ip, k, sz);
                    }
                    self.heap.insert(k, vec![0u8; sz]);
                }

                OpCode::Unlock { key_id } => {
                    let k = self.substitute(prog.str(*key_id));
                    if self.verbose {
                        eprintln!("{} [{}] ULCK {}", "[m]".magenta(), ip, k);
                    }
                    self.heap.remove(&k);
                }

                // ── HotLoop / Nop ─────────────────────────────
                OpCode::HotLoop { .. } | OpCode::Nop => {}

                // ── ArenaEnter ────────────────────────────────
                // :: name [size] def — wejście w blok areny.
                // Wywołuje hl_jit_arena_enter(&scope, name, size).
                // Reszta kodu nadal używa GC — arena to tylko ten blok.
                OpCode::ArenaEnter { name_id, size_id } => {
                    let name = prog.str(*name_id).to_string();
                    let size = prog.str(*size_id).to_string();
                    if self.verbose {
                        eprintln!(
                            "{} [{}] ARENA ENTER :: {} [{}]",
                            "[A]".magenta().bold(), ip, name, size
                        );
                    }
                    if !self.dry_run {
                        let name_c = match CString::new(name.as_str()) {
                            Ok(s) => s,
                            Err(_) => {
                                eprintln!(
                                    "{} ArenaEnter: nieprawidłowa nazwa '{}'",
                                    "[x]".red(), name
                                );
                                ip += 1;
                                continue;
                            }
                        };
                        let size_c = match CString::new(size.as_str()) {
                            Ok(s) => s,
                            Err(_) => {
                                eprintln!(
                                    "{} ArenaEnter: nieprawidłowy rozmiar '{}'",
                                    "[x]".red(), size
                                );
                                ip += 1;
                                continue;
                            }
                        };
                        let rc = unsafe {
                            hl_jit_arena_enter(
                                self.arena_scope.as_mut_ptr(),
                                               name_c.as_ptr(),
                                               size_c.as_ptr(),
                            )
                        };
                        if rc != 0 {
                            eprintln!(
                                "{} ArenaEnter: błąd dla '{}' [{}] (rc={})",
                                      "[x]".red(), name, size, rc
                            );
                        }
                    }
                }

                // ── ArenaExit ─────────────────────────────────
                // done (dla :: bloku) — jednorazowy hl_arena_free().
                // GC nie jest zaangażowany.
                OpCode::ArenaExit => {
                    if self.verbose {
                        eprintln!("{} [{}] ARENA EXIT", "[A]".magenta().bold(), ip);
                    }
                    if !self.dry_run {
                        let rc = unsafe {
                            hl_jit_arena_exit(self.arena_scope.as_mut_ptr())
                        };
                        if rc != 0 && self.verbose {
                            eprintln!(
                                "{} ArenaExit: pusty stos aren (rc={})",
                                      "[!]".yellow(), rc
                            );
                        }
                    }
                }

                // ── ArenaAlloc ────────────────────────────────
                // Jawna alokacja z bieżącej areny.
                // Wynik (adres) zapisywany w typed_vars jako Int (usize).
                OpCode::ArenaAlloc { var_id, n_bytes } => {
                    let var_name = prog.str(*var_id).to_string();
                    if self.verbose {
                        eprintln!(
                            "{} [{}] ARENA ALLOC ${} {}B",
                            "[A]".magenta(), ip, var_name, n_bytes
                        );
                    }
                    if !self.dry_run {
                        let ptr = unsafe {
                            hl_jit_arena_alloc(
                                self.arena_scope.as_mut_ptr(),
                                               *n_bytes as usize,
                            )
                        };
                        if ptr.is_null() {
                            eprintln!(
                                "{} ArenaAlloc: OOM dla {} bajtów",
                                "[x]".red(), n_bytes
                            );
                            self.typed_vars.insert(
                                var_name.clone(),
                                                   HlValue::Int(0),
                            );
                        } else {
                            let addr = ptr as usize as i64;
                            self.typed_vars.insert(
                                var_name.clone(),
                                                   HlValue::Int(addr),
                            );
                            if self.verbose {
                                eprintln!(
                                    "{} arena ptr: ${} = 0x{:x}",
                                    "[A]".magenta(), var_name, addr
                                );
                            }
                        }
                    }
                }

                // ── ArenaReset ────────────────────────────────
                // Cofnij bump pointer do początku areny.
                // Używany wewnątrz pętli w :: bloku.
                OpCode::ArenaReset => {
                    if self.verbose {
                        eprintln!("{} [{}] ARENA RESET", "[A]".magenta(), ip);
                    }
                    if !self.dry_run {
                        unsafe {
                            hl_jit_arena_reset(self.arena_scope.as_mut_ptr());
                        }
                    }
                }

                // ── v7: NUMERYCZNE ───────────────────────────

                OpCode::LoadInt { dst, val } => {
                    self.regs_i[*dst as usize] = *val;
                    if self.verbose {
                        eprintln!("{} [{}] LDI r{} = {}", "[n]".green(), ip, dst, val);
                    }
                }

                OpCode::LoadFloat { dst, val } => {
                    self.regs_f[*dst as usize] = *val;
                    if self.verbose {
                        eprintln!("{} [{}] LDF r{} = {}", "[n]".green(), ip, dst, val);
                    }
                }

                OpCode::LoadBool { dst, val } => {
                    self.regs_i[*dst as usize] = if *val { 1 } else { 0 };
                    if self.verbose {
                        eprintln!("{} [{}] LDB r{} = {}", "[n]".green(), ip, dst, val);
                    }
                }

                OpCode::LoadStr { dst, str_id } => {
                    let s = prog.str(*str_id).to_string();
                    if self.verbose {
                        eprintln!("{} [{}] LDS r{} = {:?}", "[n]".green(), ip, dst, s);
                    }
                    self.regs_i[*dst as usize] = s.parse().unwrap_or(0);
                    self.regs_f[*dst as usize] = s.parse().unwrap_or(0.0);
                }

                OpCode::LoadVarI { dst, var_id } => {
                    let name = prog.str(*var_id);
                    let val  = self.load_var_i(name);
                    self.regs_i[*dst as usize] = val;
                    if self.verbose {
                        eprintln!("{} [{}] LDVI r{} = {} (${})", "[n]".green(), ip, dst, val, name);
                    }
                }

                OpCode::LoadVarF { dst, var_id } => {
                    let name = prog.str(*var_id);
                    let val  = self.load_var_f(name);
                    self.regs_f[*dst as usize] = val;
                    if self.verbose {
                        eprintln!("{} [{}] LDVF r{} = {} (${})", "[n]".green(), ip, dst, val, name);
                    }
                }

                OpCode::StoreVarI { var_id, src } => {
                    let name = prog.str(*var_id).to_string();
                    let val  = self.regs_i[*src as usize];
                    if self.verbose {
                        eprintln!("{} [{}] STVI ${} = {} (r{})", "[n]".green(), ip, name, val, src);
                    }
                    self.typed_vars.insert(name.clone(), HlValue::Int(val));
                    if !self.dry_run { self.sync_typed_to_env(&name); }
                }

                OpCode::StoreVarF { var_id, src } => {
                    let name = prog.str(*var_id).to_string();
                    let val  = self.regs_f[*src as usize];
                    if self.verbose {
                        eprintln!("{} [{}] STVF ${} = {} (r{})", "[n]".green(), ip, name, val, src);
                    }
                    self.typed_vars.insert(name.clone(), HlValue::Float(val));
                    if !self.dry_run { self.sync_typed_to_env(&name); }
                }

                OpCode::AddI { dst, a, b } => {
                    self.regs_i[*dst as usize] =
                    self.regs_i[*a as usize].wrapping_add(self.regs_i[*b as usize]);
                }
                OpCode::SubI { dst, a, b } => {
                    self.regs_i[*dst as usize] =
                    self.regs_i[*a as usize].wrapping_sub(self.regs_i[*b as usize]);
                }
                OpCode::MulI { dst, a, b } => {
                    self.regs_i[*dst as usize] =
                    self.regs_i[*a as usize].wrapping_mul(self.regs_i[*b as usize]);
                }
                OpCode::DivI { dst, a, b } => {
                    let divisor = self.regs_i[*b as usize];
                    self.regs_i[*dst as usize] = if divisor == 0 {
                        if self.verbose {
                            eprintln!("{} [{}] DivI: dzielenie przez 0 → 0", "[!]".yellow(), ip);
                        }
                        0
                    } else {
                        self.regs_i[*a as usize] / divisor
                    };
                }
                OpCode::ModI { dst, a, b } => {
                    let divisor = self.regs_i[*b as usize];
                    self.regs_i[*dst as usize] =
                    if divisor == 0 { 0 } else { self.regs_i[*a as usize] % divisor };
                }
                OpCode::NegI { dst, src } => {
                    self.regs_i[*dst as usize] = self.regs_i[*src as usize].wrapping_neg();
                }

                OpCode::AddF { dst, a, b } => {
                    self.regs_f[*dst as usize] =
                    self.regs_f[*a as usize] + self.regs_f[*b as usize];
                }
                OpCode::SubF { dst, a, b } => {
                    self.regs_f[*dst as usize] =
                    self.regs_f[*a as usize] - self.regs_f[*b as usize];
                }
                OpCode::MulF { dst, a, b } => {
                    self.regs_f[*dst as usize] =
                    self.regs_f[*a as usize] * self.regs_f[*b as usize];
                }
                OpCode::DivF { dst, a, b } => {
                    self.regs_f[*dst as usize] =
                    self.regs_f[*a as usize] / self.regs_f[*b as usize];
                }
                OpCode::NegF { dst, src } => {
                    self.regs_f[*dst as usize] = -self.regs_f[*src as usize];
                }

                OpCode::CmpI { a, b, op } => {
                    let va = self.regs_i[*a as usize];
                    let vb = self.regs_i[*b as usize];
                    self.cmp_flag = op.eval_i(va, vb);
                    if self.verbose {
                        eprintln!(
                            "{} [{}] CMPI r{} {} r{} ({} {} {}) → {}",
                                  "[n]".green(), ip, a, op.as_str(), b,
                                  va, op.as_str(), vb, self.cmp_flag
                        );
                    }
                }

                OpCode::CmpF { a, b, op } => {
                    let va = self.regs_f[*a as usize];
                    let vb = self.regs_f[*b as usize];
                    self.cmp_flag = op.eval_f(va, vb);
                    if self.verbose {
                        eprintln!(
                            "{} [{}] CMPF r{} {} r{} ({} {} {}) → {}",
                                  "[n]".green(), ip, a, op.as_str(), b,
                                  va, op.as_str(), vb, self.cmp_flag
                        );
                    }
                }

                OpCode::JumpIfTrue { target } => {
                    if self.verbose {
                        eprintln!(
                            "{} [{}] JIFT flag={} → {}",
                            "[n]".cyan(), ip, self.cmp_flag,
                                  if self.cmp_flag { target.to_string() } else { "fall".to_string() }
                        );
                    }
                    if self.cmp_flag { ip = *target; continue; }
                }

                OpCode::NumForExec { var_id, start, end, step, cmd_id, sudo } => {
                    let var_name = prog.str(*var_id).to_string();
                    let cmd      = prog.str(*cmd_id).to_string();
                    let step_val = if *step == 0 { 1 } else { *step };
                    let forward  = step_val > 0;

                    if self.verbose {
                        eprintln!(
                            "{} [{}] NUMFOR ${} {}..{} step {} > {}",
                            "[n]".cyan(), ip, var_name, start, end, step_val,
                                  &cmd[..cmd.len().min(40)]
                        );
                    }

                    if !self.dry_run {
                        let mut i_val = *start;
                        loop {
                            let done = if forward { i_val >= *end } else { i_val <= *end };
                            if done { break; }

                            let s = i_val.to_string();
                            std::env::set_var(&var_name, &s);
                            self.session.set_env(&var_name, &s);
                            self.env.insert(var_name.clone(), s.clone());
                            self.typed_vars.insert(var_name.clone(), HlValue::Int(i_val));

                            if crate::compiler::is_hl_call(&cmd) {
                                let fname = crate::compiler::extract_hl_func(&cmd);
                                if let Some(addr) = self.resolve_func(&fname, &prog.functions) {
                                    let mut sub_stack: Vec<usize> = vec![usize::MAX];
                                    let mut sub_ip = addr;
                                    let sub_exit   = self.run_sub(prog, &mut sub_ip, &mut sub_stack);
                                    if sub_exit != 0 {
                                        self.flush_defers();
                                        self.arena_cleanup();
                                        self.gc_collect();
                                        return sub_exit;
                                    }
                                }
                            } else {
                                let expanded = self.substitute(&cmd);
                                let code     = self.session.exec(&expanded, *sudo);
                                if code != 0 && self.verbose {
                                    eprintln!("{} NumFor body exit: {}", "[!]".yellow(), code);
                                }
                            }

                            i_val = i_val.wrapping_add(step_val);
                        }
                    }
                }

                OpCode::WhileExprExec { lhs_reg, op, rhs_reg, cmd_id, sudo } => {
                    let cmd = prog.str(*cmd_id).to_string();
                    if self.verbose {
                        eprintln!(
                            "{} [{}] WHILEE r{} {} r{} > {}",
                            "[n]".cyan(), ip, lhs_reg, op.as_str(), rhs_reg,
                                  &cmd[..cmd.len().min(40)]
                        );
                    }

                    if !self.dry_run {
                        loop {
                            let lv = self.regs_i[*lhs_reg as usize];
                            let rv = self.regs_i[*rhs_reg as usize];
                            if !op.eval_i(lv, rv) { break; }

                            if crate::compiler::is_hl_call(&cmd) {
                                let fname = crate::compiler::extract_hl_func(&cmd);
                                if let Some(addr) = self.resolve_func(&fname, &prog.functions) {
                                    let mut sub_stack: Vec<usize> = vec![usize::MAX];
                                    let mut sub_ip    = addr;
                                    let sub_exit      = self.run_sub(prog, &mut sub_ip, &mut sub_stack);
                                    if sub_exit != 0 {
                                        self.flush_defers();
                                        self.arena_cleanup();
                                        self.gc_collect();
                                        return sub_exit;
                                    }
                                }
                            } else {
                                let expanded = self.substitute(&cmd);
                                self.session.exec(&expanded, *sudo);
                            }
                        }
                    }
                }

                OpCode::IntToFloat { dst, src } => {
                    self.regs_f[*dst as usize] = self.regs_i[*src as usize] as f64;
                }
                OpCode::FloatToInt { dst, src } => {
                    self.regs_i[*dst as usize] = self.regs_f[*src as usize] as i64;
                }

                OpCode::IntToStr { var_id, src } => {
                    let name = prog.str(*var_id).to_string();
                    let val  = self.regs_i[*src as usize];
                    let s    = val.to_string();
                    if self.verbose {
                        eprintln!("{} [{}] I2S ${} = {}", "[n]".green(), ip, name, s);
                    }
                    if !self.dry_run {
                        std::env::set_var(&name, &s);
                        self.session.set_env(&name, &s);
                        self.env.insert(name.clone(), s.clone());
                        self.alloc_local(&name, &s);
                    }
                }

                OpCode::FloatToStr { var_id, src } => {
                    let name = prog.str(*var_id).to_string();
                    let val  = self.regs_f[*src as usize];
                    let s    = format!("{}", val);
                    if self.verbose {
                        eprintln!("{} [{}] F2S ${} = {}", "[n]".green(), ip, name, s);
                    }
                    if !self.dry_run {
                        std::env::set_var(&name, &s);
                        self.session.set_env(&name, &s);
                        self.env.insert(name.clone(), s.clone());
                        self.alloc_local(&name, &s);
                    }
                }

                OpCode::ReturnI { src } => {
                    let val = self.regs_i[*src as usize];
                    let s   = val.to_string();
                    if self.verbose {
                        eprintln!("{} [{}] RETI {} (r{})", "[n]".green(), ip, val, src);
                    }
                    if !self.dry_run {
                        self.hl_out = s.clone();
                        std::env::set_var("_HL_OUT", &s);
                        self.session.set_env("_HL_OUT", &s);
                        self.env.insert("_HL_OUT".to_string(), s);
                    }
                    self.flush_defers();
                    match call_stack.pop() {
                        Some(ret) => { ip = ret; continue; }
                        None      => {
                            self.arena_cleanup();
                            self.gc_collect();
                            return 0;
                        }
                    }
                }

                OpCode::ReturnF { src } => {
                    let val = self.regs_f[*src as usize];
                    let s   = format!("{}", val);
                    if self.verbose {
                        eprintln!("{} [{}] RETF {} (r{})", "[n]".green(), ip, val, src);
                    }
                    if !self.dry_run {
                        self.hl_out = s.clone();
                        std::env::set_var("_HL_OUT", &s);
                        self.session.set_env("_HL_OUT", &s);
                        self.env.insert("_HL_OUT".to_string(), s);
                    }
                    self.flush_defers();
                    match call_stack.pop() {
                        Some(ret) => { ip = ret; continue; }
                        None      => {
                            self.arena_cleanup();
                            self.gc_collect();
                            return 0;
                        }
                    }
                }
            }

            ip += 1;
        }

        self.flush_defers();
        self.arena_cleanup();
        self.gc_collect();
        0
    }

    // ─────────────────────────────────────────────────────────
    // run_sub — uproszczony interpreter dla ciał NumFor/WhileExpr
    // ─────────────────────────────────────────────────────────
    pub fn run_sub(
        &mut self,
        prog:       &BytecodeProgram,
        ip:         &mut usize,
        call_stack: &mut Vec<usize>,
    ) -> i32 {
        while *ip < prog.ops.len() {
            match &prog.ops[*ip] {
                OpCode::Return => {
                    match call_stack.pop() {
                        Some(ret) if ret == usize::MAX => return 0,
                        Some(ret) => { *ip = ret; continue; }
                        None      => return 0,
                    }
                }
                OpCode::Exit(code) => return *code,
                OpCode::Exec { cmd_id, sudo } => {
                    let raw = prog.str(*cmd_id);
                    let cmd = self.substitute(raw);
                    if cmd.starts_with("_HL_") || cmd.starts_with("hl_module_") {
                        self.handle_hl_convention(&cmd, *sudo, prog);
                    } else {
                        self.session.exec(&cmd, *sudo);
                    }
                }
                OpCode::SetEnv { key_id, val_id } => {
                    let key = prog.str(*key_id).to_string();
                    let val = self.substitute(prog.str(*val_id));
                    self.do_set_env(&key, &val);
                }
                OpCode::SetLocal { key_id, val_id, is_raw } => {
                    let key = prog.str(*key_id).to_string();
                    let val = self.substitute(prog.str(*val_id));
                    if *is_raw {
                        self.locals.insert(key, LocalVal::Raw(val));
                    } else {
                        self.alloc_local(&key.clone(), &val);
                    }
                }
                OpCode::StoreVarI { var_id, src } => {
                    let name = prog.str(*var_id).to_string();
                    let val  = self.regs_i[*src as usize];
                    self.typed_vars.insert(name.clone(), HlValue::Int(val));
                    self.sync_typed_to_env(&name);
                }
                OpCode::StoreVarF { var_id, src } => {
                    let name = prog.str(*var_id).to_string();
                    let val  = self.regs_f[*src as usize];
                    self.typed_vars.insert(name.clone(), HlValue::Float(val));
                    self.sync_typed_to_env(&name);
                }
                OpCode::AddI { dst, a, b } => {
                    self.regs_i[*dst as usize] =
                    self.regs_i[*a as usize].wrapping_add(self.regs_i[*b as usize]);
                }
                OpCode::SubI { dst, a, b } => {
                    self.regs_i[*dst as usize] =
                    self.regs_i[*a as usize].wrapping_sub(self.regs_i[*b as usize]);
                }
                OpCode::MulI { dst, a, b } => {
                    self.regs_i[*dst as usize] =
                    self.regs_i[*a as usize].wrapping_mul(self.regs_i[*b as usize]);
                }
                OpCode::IntToFloat { dst, src } => {
                    self.regs_f[*dst as usize] = self.regs_i[*src as usize] as f64;
                }
                OpCode::IntToStr { var_id, src } => {
                    let name = prog.str(*var_id).to_string();
                    let s    = self.regs_i[*src as usize].to_string();
                    std::env::set_var(&name, &s);
                    self.session.set_env(&name, &s);
                    self.env.insert(name.clone(), s.clone());
                    self.alloc_local(&name, &s);
                }
                OpCode::ArenaReset => {
                    if !self.dry_run {
                        unsafe { hl_jit_arena_reset(self.arena_scope.as_mut_ptr()); }
                    }
                }
                _ => {}
            }
            *ip += 1;
        }
        0
    }
}

// ═════════════════════════════════════════════════════════════
// JIT trampoliny i helpers — bez zmian vs oryginał
// ═════════════════════════════════════════════════════════════

#[inline(always)]
unsafe fn call_jit_fn(jit_fn: *const JitFunc, ctx: *mut VmCtx) {
    (*jit_fn).call(ctx);
}

extern "C" fn trampoline_exec(
    session_ptr: *mut c_void,
    cmd_ptr:     *const u8,
    cmd_len:     usize,
    sudo:        bool,
) -> i32 {
    unsafe {
        let s   = &mut *(session_ptr as *mut SessionManager);
        let cmd = std::str::from_utf8_unchecked(
            std::slice::from_raw_parts(cmd_ptr, cmd_len)
        );
        s.exec(cmd, sudo)
    }
}

extern "C" fn trampoline_eval_cond(
    session_ptr: *mut c_void,
    cond_ptr:    *const u8,
    cond_len:    usize,
) -> bool {
    unsafe {
        let s    = &mut *(session_ptr as *mut SessionManager);
        let cond = std::str::from_utf8_unchecked(
            std::slice::from_raw_parts(cond_ptr, cond_len)
        );
        s.eval_cond(cond)
    }
}

// ═════════════════════════════════════════════════════════════
// Ścieżki
// ═════════════════════════════════════════════════════════════
pub const PLSA_BIN_NAME: &str = "hl-plsa";

pub fn get_plsa_path() -> PathBuf {
    let path = dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/bin")
    .join(PLSA_BIN_NAME);
    if !path.exists() {
        eprintln!("{} hl-plsa nie znaleziony: {:?}", "[x]".red(), path);
        std::process::exit(127);
    }
    path
}

pub fn get_plugins_root() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/plugins")
}

// ═════════════════════════════════════════════════════════════
// #[no_mangle] JIT trampoliny — C-ABI, wywoływane z kodu JIT
// ═════════════════════════════════════════════════════════════
use crate::jit::VmCtx as JitVmCtx;

#[no_mangle]
pub extern "C" fn hl_jit_exec(
    ctx:     *mut JitVmCtx,
    cmd_ptr: *const u8,
    cmd_len: usize,
    sudo:    bool,
) -> i32 {
    unsafe {
        let ctx = &mut *ctx;
        (ctx.exec_fn)(ctx.session_ptr, cmd_ptr, cmd_len, sudo)
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_eval_cond(
    ctx:      *mut JitVmCtx,
    cond_ptr: *const u8,
    cond_len: usize,
) -> u8 {
    unsafe {
        let ctx = &mut *ctx;
        (ctx.eval_cond_fn)(ctx.session_ptr, cond_ptr, cond_len) as u8
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_call_func(
    _ctx:      *mut JitVmCtx,
    _name_ptr: *const u8,
    _name_len: usize,
) -> i32 { 0 }

#[no_mangle]
pub extern "C" fn hl_jit_set_env(
    ctx:     *mut JitVmCtx,
    key_ptr: *const u8,
    key_len: usize,
    val_ptr: *const u8,
    val_len: usize,
) {
    unsafe {
        let ctx = &mut *ctx;
        let key = std::str::from_utf8_unchecked(std::slice::from_raw_parts(key_ptr, key_len));
        let val = std::str::from_utf8_unchecked(std::slice::from_raw_parts(val_ptr, val_len));
        let s   = &mut *(ctx.session_ptr as *mut SessionManager);
        s.set_env(key, val);
        std::env::set_var(key, val);
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_set_local(
    _ctx: *mut JitVmCtx,
    _kp: *const u8, _kl: usize,
    _vp: *const u8, _vl: usize,
    _raw: i32,
) {}

#[no_mangle]
pub extern "C" fn hl_jit_fallback(
    ctx:    *mut JitVmCtx,
    op_idx: i64,
) -> i32 {
    let op_class = ((op_idx >> 24) & 0xFF) as u8;
    unsafe {
        let ctx = &mut *ctx;
        match op_class {
            // Float binop (10-14)
            10..=14 => {
                let dst = ((op_idx >> 16) & 0xFF) as usize;
                let a   = ((op_idx >>  8) & 0xFF) as usize;
                let b   =  (op_idx        & 0xFF) as usize;
                if ctx.regs_f_ptr.is_null() { return 0; }
                let fa = *ctx.regs_f_ptr.add(a);
                let fb = *ctx.regs_f_ptr.add(b);
                let result = match op_class {
                    10 => fa + fb, 11 => fa - fb,
                    12 => fa * fb, 13 => fa / fb,
                    14 => -fa,
                    _  => 0.0,
                };
                *ctx.regs_f_ptr.add(dst) = result;
            }
            // Int MUL/DIV/MOD (2-4)
            2 | 3 | 4 => {
                let dst = ((op_idx >> 16) & 0xFF) as usize;
                let a   = ((op_idx >>  8) & 0xFF) as usize;
                let b   =  (op_idx        & 0xFF) as usize;
                if ctx.regs_i_ptr.is_null() { return 0; }
                let ia = *ctx.regs_i_ptr.add(a);
                let ib = *ctx.regs_i_ptr.add(b);
                *ctx.regs_i_ptr.add(dst) = match op_class {
                    2 => ia.wrapping_mul(ib),
                    3 => if ib == 0 { 0 } else { ia / ib },
                    4 => if ib == 0 { 0 } else { ia % ib },
                    _ => 0,
                };
            }
            // CmpI (0xC0)
            0xC0 => {
                let op_byte = ((op_idx >> 16) & 0xFF) as u8;
                let a       = ((op_idx >>  8) & 0xFF) as usize;
                let b       =  (op_idx        & 0xFF) as usize;
                if ctx.regs_i_ptr.is_null() || ctx.cmp_flag_ptr.is_null() { return 0; }
                let ia  = *ctx.regs_i_ptr.add(a);
                let ib  = *ctx.regs_i_ptr.add(b);
                let res = match op_byte {
                    0 => ia == ib, 1 => ia != ib,
                    2 => ia <  ib, 3 => ia <= ib,
                    4 => ia >  ib, 5 => ia >= ib,
                    _ => false,
                };
                *ctx.cmp_flag_ptr = res as u8;
            }
            // CmpF (0xC1)
            0xC1 => {
                let op_byte = ((op_idx >> 16) & 0xFF) as u8;
                let a       = ((op_idx >>  8) & 0xFF) as usize;
                let b       =  (op_idx        & 0xFF) as usize;
                if ctx.regs_f_ptr.is_null() || ctx.cmp_flag_ptr.is_null() { return 0; }
                let fa  = *ctx.regs_f_ptr.add(a);
                let fb  = *ctx.regs_f_ptr.add(b);
                let res = match op_byte {
                    0 => (fa - fb).abs() < f64::EPSILON,
                    1 => (fa - fb).abs() >= f64::EPSILON,
                    2 => fa <  fb, 3 => fa <= fb,
                    4 => fa >  fb, 5 => fa >= fb,
                    _ => false,
                };
                *ctx.cmp_flag_ptr = res as u8;
            }
            // IntToFloat (0x20)
            0x20 => {
                let dst = ((op_idx >> 8) & 0xFF) as usize;
                let src =  (op_idx       & 0xFF) as usize;
                if !ctx.regs_i_ptr.is_null() && !ctx.regs_f_ptr.is_null() {
                    *ctx.regs_f_ptr.add(dst) = *ctx.regs_i_ptr.add(src) as f64;
                }
            }
            // FloatToInt (0x21)
            0x21 => {
                let dst = ((op_idx >> 8) & 0xFF) as usize;
                let src =  (op_idx       & 0xFF) as usize;
                if !ctx.regs_i_ptr.is_null() && !ctx.regs_f_ptr.is_null() {
                    *ctx.regs_i_ptr.add(dst) = *ctx.regs_f_ptr.add(src) as i64;
                }
            }
            _ => {}
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn hl_jit_assert(
    ctx:      *mut JitVmCtx,
    cond_ptr: *const u8,
    cond_len: usize,
    msg_ptr:  *const u8,
    msg_len:  usize,
) -> i32 {
    unsafe {
        let ctx = &mut *ctx;
        let ok  = (ctx.eval_cond_fn)(ctx.session_ptr, cond_ptr, cond_len);
        if ok { return 0; }
        let msg = if msg_len > 0 {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(msg_ptr, msg_len))
        } else {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(cond_ptr, cond_len))
        };
        eprintln!("{} assert: {}", "\x1b[1;31m[!]\x1b[0m", msg);
        ctx.exit_code   = 1;
        ctx.should_exit = 1;
        1
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_load_var_i(
    ctx:     *mut JitVmCtx,
    var_ptr: *const u8,
    var_len: usize,
    dst_reg: u8,
) {
    unsafe {
        let ctx  = &mut *ctx;
        if ctx.regs_i_ptr.is_null() { return; }
        let name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(var_ptr, var_len));
        let val: i64 = std::env::var(name).unwrap_or_default().parse().unwrap_or(0);
        *ctx.regs_i_ptr.add(dst_reg as usize) = val;
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_load_var_f(
    ctx:     *mut JitVmCtx,
    var_ptr: *const u8,
    var_len: usize,
    dst_reg: u8,
) {
    unsafe {
        let ctx  = &mut *ctx;
        if ctx.regs_f_ptr.is_null() { return; }
        let name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(var_ptr, var_len));
        let val: f64 = std::env::var(name).unwrap_or_default().parse().unwrap_or(0.0);
        *ctx.regs_f_ptr.add(dst_reg as usize) = val;
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_store_var_i(
    ctx:     *mut JitVmCtx,
    var_ptr: *const u8,
    var_len: usize,
    src_reg: u8,
) {
    unsafe {
        let ctx  = &mut *ctx;
        if ctx.regs_i_ptr.is_null() { return; }
        let name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(var_ptr, var_len));
        let val  = *ctx.regs_i_ptr.add(src_reg as usize);
        let s    = val.to_string();
        let sess = &mut *(ctx.session_ptr as *mut SessionManager);
        sess.set_env(name, &s);
        std::env::set_var(name, &s);
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_store_var_f(
    ctx:     *mut JitVmCtx,
    var_ptr: *const u8,
    var_len: usize,
    src_reg: u8,
) {
    unsafe {
        let ctx  = &mut *ctx;
        if ctx.regs_f_ptr.is_null() { return; }
        let name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(var_ptr, var_len));
        let val  = *ctx.regs_f_ptr.add(src_reg as usize);
        let s    = format!("{}", val);
        let sess = &mut *(ctx.session_ptr as *mut SessionManager);
        sess.set_env(name, &s);
        std::env::set_var(name, &s);
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_int_to_str(
    ctx:     *mut JitVmCtx,
    var_ptr: *const u8,
    var_len: usize,
    src_reg: u8,
) {
    unsafe {
        let ctx  = &mut *ctx;
        if ctx.regs_i_ptr.is_null() { return; }
        let name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(var_ptr, var_len));
        let val  = *ctx.regs_i_ptr.add(src_reg as usize);
        let s    = val.to_string();
        let sess = &mut *(ctx.session_ptr as *mut SessionManager);
        sess.set_env(name, &s);
        std::env::set_var(name, &s);
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_float_to_str(
    ctx:     *mut JitVmCtx,
    var_ptr: *const u8,
    var_len: usize,
    src_reg: u8,
) {
    unsafe {
        let ctx  = &mut *ctx;
        if ctx.regs_f_ptr.is_null() { return; }
        let name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(var_ptr, var_len));
        let val  = *ctx.regs_f_ptr.add(src_reg as usize);
        let s    = format!("{}", val);
        let sess = &mut *(ctx.session_ptr as *mut SessionManager);
        sess.set_env(name, &s);
        std::env::set_var(name, &s);
    }
}

#[no_mangle]
pub extern "C" fn hl_jit_num_for(
    ctx:     *mut JitVmCtx,
    var_ptr: *const u8,
    var_len: usize,
    start:   i64,
    end:     i64,
    step:    i64,
    cmd_ptr: *const u8,
    cmd_len: usize,
    sudo:    bool,
) {
    unsafe {
        let ctx      = &mut *ctx;
        let name     = std::str::from_utf8_unchecked(std::slice::from_raw_parts(var_ptr, var_len));
        let cmd      = std::str::from_utf8_unchecked(std::slice::from_raw_parts(cmd_ptr, cmd_len));
        let step_val = if step == 0 { 1 } else { step };
        let forward  = step_val > 0;
        let sess     = &mut *(ctx.session_ptr as *mut SessionManager);

        let mut i_val = start;
        loop {
            let done = if forward { i_val >= end } else { i_val <= end };
            if done { break; }
            let s = i_val.to_string();
            sess.set_env(name, &s);
            std::env::set_var(name, &s);
            let expanded = cmd
            .replace(&format!("${}", name), &s)
            .replace(&format!("${{{}}}", name), &s);
            (ctx.exec_fn)(ctx.session_ptr, expanded.as_ptr(), expanded.len(), sudo);
            i_val = i_val.wrapping_add(step_val);
        }
    }
}
