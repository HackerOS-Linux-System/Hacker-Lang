use crate::bytecode::{BytecodeProgram, OpCode};
use crate::executor::{SessionManager, ShellKind};
use crate::gc_ffi::*;
use crate::jit::{JitCompiler, JitFunc, VmCtx};
use colored::*;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;

// ─────────────────────────────────────────────────────────────
// LocalVal
// ─────────────────────────────────────────────────────────────
pub enum LocalVal {
    Managed(*mut c_char),
    Raw(String),
}
unsafe impl Send for LocalVal {}
unsafe impl Sync for LocalVal {}

// ─────────────────────────────────────────────────────────────
// VM
// ─────────────────────────────────────────────────────────────
pub struct VM {
    pub env:        HashMap<String, String>,
    pub locals:     HashMap<String, LocalVal>,
    pub heap:       HashMap<String, Vec<u8>>,
    pub session:    SessionManager,
    pub jit:        JitCompiler,
    pub verbose:    bool,
    pub dry_run:    bool,
    // ── NOWE v6 ──────────────────────────────────────────────
    /// Klucze stałych % — VM ostrzega przy próbie nadpisania
    pub const_keys: HashSet<String>,
    /// Ostatnia wartość zwrócona przez Out/SetOut — _HL_OUT
    pub hl_out:     String,
}

impl VM {
    pub fn new(verbose: bool, dry_run: bool) -> Self {
        Self::with_shell(ShellKind::default(), verbose, dry_run)
    }

    pub fn with_shell(shell: ShellKind, verbose: bool, dry_run: bool) -> Self {
        Self {
            env:        std::env::vars().collect(),
            locals:     HashMap::new(),
            heap:       HashMap::new(),
            session:    SessionManager::with_shell(shell, verbose),
            jit:        JitCompiler::new(verbose),
            verbose,
            dry_run,
            const_keys: HashSet::new(),
            hl_out:     String::new(),
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

    // ── GC ────────────────────────────────────────────────────
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

    // ─────────────────────────────────────────────────────────
    // SetEnv helper — z walidacją stałych
    // ─────────────────────────────────────────────────────────
    fn do_set_env(&mut self, key: &str, val: &str) {
        // Ostrzeż jeśli próbujemy nadpisać stałą %
        if self.const_keys.contains(key) && self.verbose {
            eprintln!(
                "{} Ostrzeżenie: próba nadpisania stałej %{} — ignoruję",
                "[!]".yellow(), key
            );
            return;
        }
        std::env::set_var(key, val);
        self.session.set_env(key, val);
        self.env.insert(key.to_string(), val.to_string());
    }

    // ─────────────────────────────────────────────────────────
    // GŁÓWNA PĘTLA VM
    // ─────────────────────────────────────────────────────────
    pub fn run(&mut self, prog: &BytecodeProgram) -> i32 {
        let mut ip:         usize      = 0;
        let mut call_stack: Vec<usize> = Vec::with_capacity(32);

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
                                let mut ctx = VmCtx {
                                    exec_fn:      trampoline_exec,
                                    eval_cond_fn: trampoline_eval_cond,
                                    session_ptr:  session_raw,
                                    pool_ptr:     std::ptr::null(),
                                    exit_code:    0,
                                    should_exit:  0,
                                };
                                unsafe { call_jit_fn(fn_ptr, &mut ctx); }
                                if ctx.should_exit != 0 {
                                    self.gc_collect();
                                    return ctx.exit_code;
                                }
                                ip += 1;
                                continue;
                            }

                            // Interpreter fallback
                            call_stack.push(ip + 1);
                            ip = addr;
                            continue;
                        }
                    }
                }

                // ── Return ────────────────────────────────────
                OpCode::Return => {
                    match call_stack.pop() {
                        Some(ret) => { ip = ret; continue; }
                        None      => { self.gc_collect(); return 0; }
                    }
                }

                // ── Exit ──────────────────────────────────────
                OpCode::Exit(code) => {
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

                // ── SetConst — stała % ────────────────────────
                // Zapisz jako env + zapamiętaj klucz w const_keys.
                // VM nie pozwoli nadpisać przez SetEnv/SetLocal.
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

                // ── SetOut — out val ──────────────────────────
                // Zapisz wartość do _HL_OUT (przez env i self.hl_out).
                // Caller może ją przechwycić przez $(_HL_OUT) lub
                // przez vm.hl_out.
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

                // ── SpawnBg — spawn bez przypisania ──────────
                // Uruchom komendę w tle (fire & forget).
                // Strategia: session.exec("cmd &")
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

                // ── SpawnAssign — spawn z przypisaniem PID ────
                // Uruchom komendę w tle, PID zapisz do zmiennej.
                // Strategia: session.exec("export key=$( cmd & echo $! )")
                OpCode::SpawnAssign { key_id, cmd_id, sudo } => {
                    let key  = prog.str(*key_id).to_string();
                    let raw  = prog.str(*cmd_id);
                    let cmd  = self.substitute(raw);
                    let sh   = format!("export {}=$( {} & echo $! )", key, cmd);
                    if self.verbose {
                        eprintln!("{} [{}] SPAWNA {} = spawn {}", "[~]".blue(), ip, key, cmd.dimmed());
                    }
                    if !self.dry_run {
                        self.session.exec(&sh, *sudo);
                        // Zaktualizuj env cache
                        let pid = std::env::var(&key).unwrap_or_default();
                        self.env.insert(key.clone(), pid.clone());
                        self.alloc_local(&key, &pid);
                    }
                }

                // ── AwaitPid — await bez przypisania ──────────
                // expr może być: $pid_var, .func_name, dowolny string
                // Strategia:
                //   $x       → wait $x
                //   .func    → CallFunc (interpreter wywołanie)
                //   inne     → wait expr
                OpCode::AwaitPid { expr_id } => {
                    let raw   = prog.str(*expr_id);
                    let expr  = self.substitute(raw);
                    let clean = expr.trim();

                    if self.verbose {
                        eprintln!("{} [{}] AWAIT {}", "[~]".blue(), ip, clean.dimmed());
                    }

                    if !self.dry_run {
                        if clean.starts_with('.') {
                            // Wywołanie funkcji HL synchronicznie
                            let fname = clean.trim_start_matches('.');
                            if let Some(addr) = self.resolve_func(fname, &prog.functions) {
                                call_stack.push(ip + 1);
                                ip = addr;
                                continue;
                            }
                        }
                        // wait $pid lub wait expr
                        let sh = if clean.starts_with('$') {
                            format!("wait {}", clean)
                        } else {
                            format!("wait {}", clean)
                        };
                        self.session.exec(&sh, false);
                    }
                }

                // ── AwaitAssign — await z przypisaniem wyniku ─
                // expr = $pid_var    → wait $pid, wynik z $?
                // expr = .func_name  → CallFunc + przechwyt _HL_OUT
                // expr = inne        → $( expr )
                OpCode::AwaitAssign { key_id, expr_id } => {
                    let key   = prog.str(*key_id).to_string();
                    let raw   = prog.str(*expr_id);
                    let expr  = self.substitute(raw);
                    let clean = expr.trim();

                    if self.verbose {
                        eprintln!(
                            "{} [{}] AWAITA {} = await {}",
                            "[~]".blue(), ip, key, clean.dimmed()
                        );
                    }

                    if !self.dry_run {
                        if clean.starts_with('.') {
                            // Wywołanie funkcji HL → przechwyt _HL_OUT
                            let fname = clean.trim_start_matches('.');
                            if let Some(addr) = self.resolve_func(fname, &prog.functions) {
                                // Ustaw marker że po powrocie mamy przypisać _HL_OUT
                                // Implementacja: push adres return + dodatkowy adres set_out
                                // Uproszczenie: użyj specjalnej sekwencji Exec
                                call_stack.push(ip + 1);
                                ip = addr;
                                // Po Return VM wróci tutaj (+1) ale _HL_OUT już będzie ustawiony
                                // przez SetOut w funkcji. Przechwytujemy z env.
                                let out_val = self.hl_out.clone();
                                self.alloc_local(&key, &out_val);
                                self.session.invalidate_cond_cache();
                                continue;
                            }
                        }

                        if clean.starts_with('$') {
                            // wait $pid → exit code jako wynik
                            let sh = format!("wait {}; export {}=$?", clean, key);
                            self.session.exec(&sh, false);
                            let v = std::env::var(&key).unwrap_or_default();
                            self.alloc_local(&key, &v);
                        } else {
                            // Dowolne wyrażenie shell
                            let sh = format!("export {}=$( {} )", key, clean);
                            self.session.exec(&sh, false);
                            let v = std::env::var(&key).unwrap_or_default();
                            self.alloc_local(&key, &v);
                        }
                        self.session.invalidate_cond_cache();
                    }
                }

                // ── Assert — walidacja VM-native ──────────────
                // Happy path (cond true): zero fork/exec — tylko eval_cond()
                // Error path (cond false): eprintln! + Exit(1)
                // Jest to kluczowa optymalizacja — assert w pętlach
                // nie tworzy subprocesów przy normalnym działaniu.
                OpCode::Assert { cond_id, msg_id } => {
                    let raw_cond = prog.str(*cond_id);
                    let cond     = self.substitute(raw_cond);
                    let wrapped  = if cond.trim().starts_with('[')
                    || cond.trim().starts_with("((") {
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
                            self.gc_collect();
                            return 1;
                        }
                    }
                }

                // ── MatchExec / PipeExec ──────────────────────
                // Generowane przez compiler.rs jako fallback Exec.
                // Te opcody są tutaj dla kompletności — normalnie
                // compiler.rs emituje Exec(case..esac) lub CallFunc*.
                OpCode::MatchExec { case_cmd_id, sudo } => {
                    let raw = prog.str(*case_cmd_id);
                    let cmd = self.substitute(raw);
                    if self.verbose {
                        eprintln!("{} [{}] MATCH {}", "[m]".cyan(), ip, &cmd[..cmd.len().min(60)].dimmed());
                    }
                    if !self.dry_run {
                        self.session.exec(&cmd, *sudo);
                    }
                }
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

                // ── Lock ──────────────────────────────────────
                OpCode::Lock { key_id, val_id } => {
                    let k  = self.substitute(prog.str(*key_id));
                    let v  = self.substitute(prog.str(*val_id));
                    let sz = v.parse::<usize>().unwrap_or(v.len().max(1));
                    if self.verbose {
                        eprintln!("{} [{}] LOCK {} ({} B)", "[m]".magenta(), ip, k, sz);
                    }
                    self.heap.insert(k, vec![0u8; sz]);
                }

                // ── Unlock ────────────────────────────────────
                OpCode::Unlock { key_id } => {
                    let k = self.substitute(prog.str(*key_id));
                    if self.verbose {
                        eprintln!("{} [{}] ULCK {}", "[m]".magenta(), ip, k);
                    }
                    self.heap.remove(&k);
                }

                // ── HotLoop / Nop ─────────────────────────────
                OpCode::HotLoop { .. } | OpCode::Nop => {}
            }

            ip += 1;
        }

        self.gc_collect();
        0
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
}

// ─────────────────────────────────────────────────────────────
// Wolna funkcja wywoływania JIT — zero borrow na VM
// ─────────────────────────────────────────────────────────────
#[inline(always)]
unsafe fn call_jit_fn(jit_fn: *const JitFunc, ctx: *mut VmCtx) {
    (*jit_fn).call(ctx);
}

// ─────────────────────────────────────────────────────────────
// Trampolines C ABI
// ─────────────────────────────────────────────────────────────
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

// ─────────────────────────────────────────────────────────────
// JIT trampolines — #[no_mangle]
// ─────────────────────────────────────────────────────────────
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
    _ctx:    *mut JitVmCtx,
    _op_idx: i64,
) -> i32 { 0 }

/// hl_jit_assert — trampoline wywoływany przez JIT dla OpCode::Assert
///
/// Logika:
///   1. eval_cond(cond) przez istniejący trampoline
///   2. Jeśli true  → zwróć 0 (happy path — zero fork/exec)
///   3. Jeśli false → eprintln! komunikat + ustaw should_exit=1 + exit_code=1
///
/// JIT po wywołaniu sprawdza should_exit przez `test byte [rbx+36], 1`
/// i skacze do epilogu jeśli ustawiony.
#[no_mangle]
pub extern "C" fn hl_jit_assert(
    ctx:      *mut JitVmCtx,
    cond_ptr: *const u8,
    cond_len: usize,
    msg_ptr:  *const u8,
    msg_len:  usize,
) -> i32 {
    unsafe {
        let ctx  = &mut *ctx;
        let cond = std::str::from_utf8_unchecked(
            std::slice::from_raw_parts(cond_ptr, cond_len)
        );
        // Wywołaj eval_cond przez istniejący trampoline
        let ok = (ctx.eval_cond_fn)(ctx.session_ptr, cond_ptr, cond_len);
        if ok {
            return 0; // happy path — assert passed
        }
        // Assert failed
        let msg = if msg_len > 0 {
            std::str::from_utf8_unchecked(
                std::slice::from_raw_parts(msg_ptr, msg_len)
            )
        } else {
            cond
        };
        eprintln!("{} assert: {}", "\x1b[1;31m[!]\x1b[0m", msg);
        ctx.exit_code   = 1;
        ctx.should_exit = 1;
        1
    }
}

// ─────────────────────────────────────────────────────────────
// Ścieżki
// ─────────────────────────────────────────────────────────────
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
