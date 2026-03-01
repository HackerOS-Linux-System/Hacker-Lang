use crate::bytecode::{BytecodeProgram, OpCode};
use colored::*;
use std::collections::HashMap;

pub const HOT_THRESHOLD: u32 = 10;

// ─────────────────────────────────────────────────────────────
// VmCtx — C-ABI kontekst przekazywany do JIT funkcji
// ─────────────────────────────────────────────────────────────
#[repr(C)]
pub struct VmCtx {
    pub exec_fn:      extern "C" fn(*mut std::ffi::c_void, *const u8, usize, bool) -> i32,
    pub eval_cond_fn: extern "C" fn(*mut std::ffi::c_void, *const u8, usize) -> bool,
    pub session_ptr:  *mut std::ffi::c_void,
    pub pool_ptr:     *const std::ffi::c_void,
    pub exit_code:    i32,
    pub should_exit:  u8,
}

// ─────────────────────────────────────────────────────────────
// Trampolines — implementacje w vm.rs (#[no_mangle])
// ─────────────────────────────────────────────────────────────
extern "C" {
    fn hl_jit_exec      (ctx: *mut VmCtx, cmd_ptr:  *const u8, cmd_len:  usize, sudo: bool) -> i32;
    fn hl_jit_eval_cond (ctx: *mut VmCtx, cond_ptr: *const u8, cond_len: usize) -> u8;
    fn hl_jit_call_func (ctx: *mut VmCtx, name_ptr: *const u8, name_len: usize) -> i32;
    fn hl_jit_set_env   (ctx: *mut VmCtx, kp: *const u8, kl: usize, vp: *const u8, vl: usize);
    fn hl_jit_set_local (ctx: *mut VmCtx, kp: *const u8, kl: usize, vp: *const u8, vl: usize, raw: i32);
    fn hl_jit_fallback  (ctx: *mut VmCtx, op_idx: i64) -> i32;
    // NOWY: assert trampoline — eval_cond + exit 1 jeśli false
    fn hl_jit_assert    (ctx: *mut VmCtx, cond_ptr: *const u8, cond_len: usize, msg_ptr: *const u8, msg_len: usize) -> i32;
}

// ─────────────────────────────────────────────────────────────
// Executable buffer — mmap RW → fill → mprotect RX
// ─────────────────────────────────────────────────────────────
pub struct ExecBuf {
    ptr: *mut u8,
    cap: usize,
}
unsafe impl Send for ExecBuf {}
unsafe impl Sync for ExecBuf {}

impl ExecBuf {
    fn alloc(code: &[u8]) -> Option<Self> {
        let cap = (code.len() + 4095) & !4095;
        let ptr = unsafe { os_mmap_rw(cap)? };
        unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), ptr, code.len()); }
        unsafe {
            if os_mprotect_rx(ptr, cap) != 0 {
                os_munmap(ptr, cap);
                return None;
            }
        }
        Some(Self { ptr, cap })
    }

    pub fn as_fn<F: Copy>(&self) -> F {
        unsafe { std::mem::transmute_copy(&self.ptr) }
    }
}

impl Drop for ExecBuf {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { os_munmap(self.ptr, self.cap); }
        }
    }
}

// ─────────────────────────────────────────────────────────────
// OS mmap wrappers (bez libc crate)
// ─────────────────────────────────────────────────────────────
unsafe fn os_mmap_rw(size: usize) -> Option<*mut u8> {
    extern "C" {
        fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    }
    #[cfg(target_os = "linux")]
    let (prot, flags) = (0x1 | 0x2, 0x2 | 0x20);
    #[cfg(target_os = "macos")]
    let (prot, flags) = (0x1 | 0x2, 0x0002 | 0x1000);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let (prot, flags) = (0x3, 0x22);

    let p = mmap(std::ptr::null_mut(), size, prot, flags, -1, 0);
    if p as isize == -1 { None } else { Some(p) }
}

unsafe fn os_mprotect_rx(ptr: *mut u8, size: usize) -> i32 {
    extern "C" { fn mprotect(addr: *mut u8, len: usize, prot: i32) -> i32; }
    #[cfg(target_os = "linux")]    let prot = 0x1 | 0x4;
    #[cfg(target_os = "macos")]    let prot = 0x01 | 0x04;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))] let prot = 0x5;
    mprotect(ptr, size, prot)
}

unsafe fn os_munmap(ptr: *mut u8, size: usize) {
    extern "C" { fn munmap(addr: *mut u8, len: usize) -> i32; }
    munmap(ptr, size);
}

// ─────────────────────────────────────────────────────────────
// x86-64 Emitter
// ─────────────────────────────────────────────────────────────
struct Emit {
    code:   Vec<u8>,
    /// (offset_patchable_rel32, target_bytecode_ip) — usize::MAX = epilog
    relocs: Vec<(usize, usize)>,
    /// bytecode_ip → code offset
    ip2off: HashMap<usize, usize>,
}

impl Emit {
    fn new() -> Self {
        Self { code: Vec::with_capacity(512), relocs: Vec::new(), ip2off: HashMap::new() }
    }

    fn len(&self) -> usize { self.code.len() }

    fn mark_ip(&mut self, ip: usize) { self.ip2off.insert(ip, self.code.len()); }

    fn u8(&mut self, b: u8)    { self.code.push(b); }
    fn u32le(&mut self, v: u32) { self.code.extend_from_slice(&v.to_le_bytes()); }
    fn i32le(&mut self, v: i32) { self.code.extend_from_slice(&v.to_le_bytes()); }
    fn u64le(&mut self, v: u64) { self.code.extend_from_slice(&v.to_le_bytes()); }

    // ── Podstawowe instrukcje ─────────────────────────────────
    fn mov_rax(&mut self, v: u64) { self.u8(0x48); self.u8(0xB8); self.u64le(v); }
    fn mov_rsi(&mut self, v: u64) { self.u8(0x48); self.u8(0xBE); self.u64le(v); }
    fn mov_rdx(&mut self, v: u64) { self.u8(0x48); self.u8(0xBA); self.u64le(v); }
    fn mov_rcx(&mut self, v: u64) { self.u8(0x48); self.u8(0xB9); self.u64le(v); }
    fn mov_r8 (&mut self, v: u64) { self.u8(0x49); self.u8(0xB8); self.u64le(v); }
    fn mov_r9d(&mut self, v: i32) { self.u8(0x41); self.u8(0xB9); self.i32le(v); }
    fn mov_ecx(&mut self, v: i32) { self.u8(0xB9); self.i32le(v); }
    fn mov_rdi_rbx(&mut self) { self.u8(0x48); self.u8(0x89); self.u8(0xDF); }
    fn call_rax(&mut self) { self.u8(0xFF); self.u8(0xD0); }
    fn test_al (&mut self) { self.u8(0x84); self.u8(0xC0); }

    fn jz(&mut self, target_ip: usize) {
        self.u8(0x0F); self.u8(0x84);
        let off = self.len();
        self.relocs.push((off, target_ip));
        self.i32le(0);
    }
    fn jmp(&mut self, target_ip: usize) {
        self.u8(0xE9);
        let off = self.len();
        self.relocs.push((off, target_ip));
        self.i32le(0);
    }
    fn mov_rbx_d8_i32(&mut self, disp: i8, val: i32) {
        self.u8(0xC7); self.u8(0x43); self.u8(disp as u8); self.i32le(val);
    }
    fn mov_rbx_d8_i8(&mut self, disp: i8, val: i8) {
        self.u8(0xC6); self.u8(0x43); self.u8(disp as u8); self.u8(val as u8);
    }

    /// Prolog: push rbp; mov rbp,rsp; push rbx; push r12; push r13; sub rsp,8; mov rbx,rdi
    fn prolog(&mut self) {
        self.u8(0x55);
        self.u8(0x48); self.u8(0x89); self.u8(0xE5);
        self.u8(0x53);
        self.u8(0x41); self.u8(0x54);
        self.u8(0x41); self.u8(0x55);
        self.u8(0x48); self.u8(0x83); self.u8(0xEC); self.u8(0x08);
        self.u8(0x48); self.u8(0x89); self.u8(0xFB);
    }

    /// Epilog: add rsp,8; pop r13; pop r12; pop rbx; pop rbp; xor eax,eax; ret
    fn epilog(&mut self) {
        self.u8(0x48); self.u8(0x83); self.u8(0xC4); self.u8(0x08);
        self.u8(0x41); self.u8(0x5D);
        self.u8(0x41); self.u8(0x5C);
        self.u8(0x5B);
        self.u8(0x5D);
        self.u8(0x31); self.u8(0xC0);
        self.u8(0xC3);
    }

    fn resolve(&mut self) {
        let epilog_off = *self.ip2off.get(&usize::MAX).unwrap_or(&self.code.len());
        for (patch, target_ip) in self.relocs.drain(..).collect::<Vec<_>>() {
            let target_off = self.ip2off.get(&target_ip).copied().unwrap_or(epilog_off);
            let after = patch + 4;
            let rel   = target_off as i32 - after as i32;
            let bytes = rel.to_le_bytes();
            self.code[patch]     = bytes[0];
            self.code[patch + 1] = bytes[1];
            self.code[patch + 2] = bytes[2];
            self.code[patch + 3] = bytes[3];
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Pomocniki emisji dla nowych opcodes
// ─────────────────────────────────────────────────────────────

/// Emituj wywołanie hl_jit_exec(ctx, ptr, len, sudo)
fn emit_exec_call(em: &mut Emit, cmd: &str, sudo: bool) {
    em.mov_rdi_rbx();
    em.mov_rsi(cmd.as_ptr() as u64);
    em.mov_rdx(cmd.len() as u64);
    em.mov_ecx(sudo as i32);
    em.mov_rax(hl_jit_exec as u64);
    em.call_rax();
}

/// Emituj wywołanie hl_jit_set_env(ctx, key_ptr, key_len, val_ptr, val_len)
fn emit_set_env_call(em: &mut Emit, key: &str, val: &str) {
    em.mov_rdi_rbx();
    em.mov_rsi(key.as_ptr() as u64);
    em.mov_rdx(key.len() as u64);
    em.mov_rcx(val.as_ptr() as u64);
    em.mov_r8(val.len() as u64);
    em.mov_rax(hl_jit_set_env as u64);
    em.call_rax();
}

/// Emituj wywołanie hl_jit_assert(ctx, cond_ptr, cond_len, msg_ptr, msg_len)
/// Jeśli assert false → JIT ustawia should_exit=1 i jmp epilog
fn emit_assert_call(em: &mut Emit, cond: &str, msg: &str) {
    em.mov_rdi_rbx();
    em.mov_rsi(cond.as_ptr() as u64);
    em.mov_rdx(cond.len() as u64);
    em.mov_rcx(msg.as_ptr() as u64);
    em.mov_r8(msg.len() as u64);
    em.mov_rax(hl_jit_assert as u64);
    em.call_rax();
    // Sprawdź wynik — jeśli assert false → should_exit ustawiony przez trampoline
    // JIT sprawdza should_exit przez test byte [rbx+36], 1
    emit_check_should_exit(em);
}

/// Emituj sprawdzenie should_exit po każdym wywołaniu które może go ustawić.
/// Jeśli should_exit != 0 → jmp epilog (VM odczyta exit_code z ctx).
fn emit_check_should_exit(em: &mut Emit) {
    // test byte [rbx + 36], 1
    em.u8(0xF6); em.u8(0x43); em.u8(36); em.u8(0x01);
    // jnz → epilog
    em.u8(0x0F); em.u8(0x85);
    let off = em.len();
    em.relocs.push((off, usize::MAX));
    em.i32le(0);
}

// ─────────────────────────────────────────────────────────────
// Skompilowana funkcja
// ─────────────────────────────────────────────────────────────
pub struct JitFunc {
    buf:      ExecBuf,
    pub size: usize,
}

impl JitFunc {
    pub fn call(&self, ctx: *mut VmCtx) -> i32 {
        let f: extern "C" fn(*mut VmCtx) -> i32 = self.buf.as_fn();
        f(ctx)
    }
}

// ─────────────────────────────────────────────────────────────
// Licznik wywołań
// ─────────────────────────────────────────────────────────────
#[derive(Default)]
pub struct CallCounter {
    counts: HashMap<u32, u32>,
}

impl CallCounter {
    pub fn increment(&mut self, id: u32) -> u32 {
        let c = self.counts.entry(id).or_insert(0);
        *c += 1;
        *c
    }
}

// ─────────────────────────────────────────────────────────────
// JIT Compiler
// ─────────────────────────────────────────────────────────────
pub struct JitCompiler {
    pub compiled: HashMap<u32, JitFunc>,
    pub counter:  CallCounter,
    verbose:      bool,
}

impl JitCompiler {
    pub fn new(verbose: bool) -> Self {
        Self { compiled: HashMap::new(), counter: CallCounter::default(), verbose }
    }

    pub fn is_compiled(&self, id: u32) -> bool { self.compiled.contains_key(&id) }

    pub fn register_call(&mut self, id: u32) -> bool {
        self.counter.increment(id) >= HOT_THRESHOLD
    }

    pub fn compile(&mut self, func_id: u32, start_ip: usize, prog: &BytecodeProgram) {
        if self.is_compiled(func_id) { return; }

        let fname = prog.str(func_id);
        if self.verbose {
            eprintln!("{} JIT compile: .{} @ ip={}", "[jit]".magenta(), fname, start_ip);
        }

        let mut em = Emit::new();
        em.prolog();

        let mut ip = start_ip;
        while ip < prog.ops.len() {
            em.mark_ip(ip);

            match &prog.ops[ip] {

                // ── ISTNIEJĄCE OPCODY ─────────────────────────

                OpCode::Exec { cmd_id, sudo } => {
                    let cmd = prog.str(*cmd_id);
                    emit_exec_call(&mut em, cmd, *sudo);
                }

                OpCode::JumpIfFalse { cond_id, target } => {
                    let cond = prog.str(*cond_id);
                    em.mov_rdi_rbx();
                    em.mov_rsi(cond.as_ptr() as u64);
                    em.mov_rdx(cond.len() as u64);
                    em.mov_rax(hl_jit_eval_cond as u64);
                    em.call_rax();
                    em.test_al();
                    em.jz(*target); // FALSE (al==0) → skok
                }

                OpCode::Jump { target } => {
                    em.jmp(*target);
                }

                OpCode::CallFunc { func_id: callee } => {
                    let name = prog.str(*callee);
                    em.mov_rdi_rbx();
                    em.mov_rsi(name.as_ptr() as u64);
                    em.mov_rdx(name.len() as u64);
                    em.mov_rax(hl_jit_call_func as u64);
                    em.call_rax();
                }

                OpCode::SetEnv { key_id, val_id } => {
                    let k = prog.str(*key_id);
                    let v = prog.str(*val_id);
                    emit_set_env_call(&mut em, k, v);
                }

                OpCode::SetLocal { key_id, val_id, is_raw } => {
                    let k = prog.str(*key_id);
                    let v = prog.str(*val_id);
                    em.mov_rdi_rbx();
                    em.mov_rsi(k.as_ptr() as u64);
                    em.mov_rdx(k.len() as u64);
                    em.mov_rcx(v.as_ptr() as u64);
                    em.mov_r8(v.len() as u64);
                    em.mov_r9d(*is_raw as i32);
                    em.mov_rax(hl_jit_set_local as u64);
                    em.call_rax();
                }

                OpCode::Exit(code) => {
                    em.mov_rbx_d8_i32(32, *code);  // exit_code  @ rbx+32
                    em.mov_rbx_d8_i8(36, 1);        // should_exit @ rbx+36
                    em.jmp(usize::MAX);             // → epilog
                }

                OpCode::Return => break,

                // Plugin, Lock, Unlock — trampoline fallback
                OpCode::Plugin { .. } | OpCode::Lock { .. } | OpCode::Unlock { .. } => {
                    em.mov_rdi_rbx();
                    em.mov_rsi(ip as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                }

                OpCode::HotLoop { .. } | OpCode::Nop => {}

                // ── NOWE OPCODY v6 ────────────────────────────

                // SetConst — identycznie jak SetEnv
                // JIT nie egzekwuje niezmienności (to zadanie VM interpretera)
                OpCode::SetConst { key_id, val_id } => {
                    let k = prog.str(*key_id);
                    let v = prog.str(*val_id);
                    emit_set_env_call(&mut em, k, v);
                }

                // SetOut — zapisz wynik funkcji do _HL_OUT przez set_env
                OpCode::SetOut { val_id } => {
                    let v = prog.str(*val_id);
                    emit_set_env_call(&mut em, "_HL_OUT", v);
                }

                // SpawnBg — uruchom w tle (cmd &) bez przypisania PID
                OpCode::SpawnBg { cmd_id, sudo } => {
                    let raw = prog.str(*cmd_id);
                    // Tworzymy "cmd &" jako tymczasowy string
                    // WAŻNE: string musi żyć przez czas wywołania.
                    // Strategia: budujemy string ze stałym suffixem " &" w pool.
                    // Pool żyje przez cały czas life JitFunc → bezpieczne.
                    let bg = format!("{} &", raw);
                    // Leak string do stałej pamięci (żyje przez cały czas procesu)
                    let bg_leaked: &'static str = Box::leak(bg.into_boxed_str());
                    emit_exec_call(&mut em, bg_leaked, *sudo);
                }

                // SpawnAssign — uruchom w tle i przypisz PID do zmiennej
                // Strategia: exec("export key=$( cmd & echo $! )")
                OpCode::SpawnAssign { key_id, cmd_id, sudo } => {
                    let key = prog.str(*key_id);
                    let cmd = prog.str(*cmd_id);
                    let sh  = format!("export {}=$( {} & echo $! )", key, cmd);
                    let sh_leaked: &'static str = Box::leak(sh.into_boxed_str());
                    emit_exec_call(&mut em, sh_leaked, *sudo);
                }

                // AwaitPid — wait $pid lub wait expr
                // Strategia: exec("wait expr")
                OpCode::AwaitPid { expr_id } => {
                    let expr = prog.str(*expr_id);
                    let sh   = if expr.trim().starts_with('.') {
                        // .func → CallFunc przez trampoline (nie shell wait)
                        let fname = expr.trim().trim_start_matches('.');
                        em.mov_rdi_rbx();
                        em.mov_rsi(fname.as_ptr() as u64);
                        em.mov_rdx(fname.len() as u64);
                        em.mov_rax(hl_jit_call_func as u64);
                        em.call_rax();
                        ip += 1;
                        continue;
                    } else {
                        format!("wait {}", expr.trim())
                    };
                    let sh_leaked: &'static str = Box::leak(sh.into_boxed_str());
                    emit_exec_call(&mut em, sh_leaked, false);
                }

                // AwaitAssign — czekaj i przypisz wynik
                // $pid   → exec("wait $pid; export key=$?")
                // .func  → CallFunc + set_env(key, "_HL_OUT")
                // inne   → exec("export key=$( expr )")
                OpCode::AwaitAssign { key_id, expr_id } => {
                    let key  = prog.str(*key_id);
                    let expr = prog.str(*expr_id).trim();

                    if expr.starts_with('.') {
                        // Wywołaj funkcję HL
                        let fname = expr.trim_start_matches('.');
                        em.mov_rdi_rbx();
                        em.mov_rsi(fname.as_ptr() as u64);
                        em.mov_rdx(fname.len() as u64);
                        em.mov_rax(hl_jit_call_func as u64);
                        em.call_rax();
                        // Przechwyt _HL_OUT → key przez set_env
                        // Nie możemy łatwo przechwycić wartości w JIT bez dodatkowego
                        // trampolinu — używamy exec("export key=$_HL_OUT")
                        let sh = format!("export {}=$_HL_OUT", key);
                        let sh_leaked: &'static str = Box::leak(sh.into_boxed_str());
                        emit_exec_call(&mut em, sh_leaked, false);
                    } else if expr.starts_with('$') {
                        let sh = format!("wait {}; export {}=$?", expr, key);
                        let sh_leaked: &'static str = Box::leak(sh.into_boxed_str());
                        emit_exec_call(&mut em, sh_leaked, false);
                    } else {
                        let sh = format!("export {}=$( {} )", key, expr);
                        let sh_leaked: &'static str = Box::leak(sh.into_boxed_str());
                        emit_exec_call(&mut em, sh_leaked, false);
                    }
                }

                // Assert — specjalny trampoline który eval_cond + exit 1 jeśli false
                // W hot path (cond == true) — tylko eval_cond syscall, zero fork/exec
                OpCode::Assert { cond_id, msg_id } => {
                    let cond = prog.str(*cond_id);
                    let msg  = msg_id
                    .map(|id| prog.str(id))
                    .unwrap_or("Assertion failed");
                    emit_assert_call(&mut em, cond, msg);
                }

                // MatchExec — już skompilowany jako case..esac string → zwykły Exec
                OpCode::MatchExec { case_cmd_id, sudo } => {
                    let cmd = prog.str(*case_cmd_id);
                    emit_exec_call(&mut em, cmd, *sudo);
                }

                // PipeExec — już skompilowany jako pipe string → zwykły Exec
                OpCode::PipeExec { cmd_id, sudo } => {
                    let cmd = prog.str(*cmd_id);
                    emit_exec_call(&mut em, cmd, *sudo);
                }
            }

            ip += 1;
        }

        // Zapamiętaj offset epilogu
        let epilog_off = em.len();
        em.ip2off.insert(usize::MAX, epilog_off);
        em.epilog();

        em.resolve();

        let size = em.code.len();
        match ExecBuf::alloc(&em.code) {
            None => {
                eprintln!("{} JIT mmap failed: .{}", "[!]".yellow(), fname);
            }
            Some(buf) => {
                if self.verbose {
                    eprintln!("{} JIT ok: .{} → {} bytes", "[jit]".green(), fname, size);
                }
                self.compiled.insert(func_id, JitFunc { buf, size });
            }
        }
    }

    pub fn report(&self, prog: &BytecodeProgram) {
        if self.compiled.is_empty() { return; }
        eprintln!("{}", "━━━ JIT Stats ━━━━━━━━━━━━━━━━━━━━━━━━━━".magenta());
        eprintln!("  compiled : {} functions", self.compiled.len().to_string().yellow());
        for (id, f) in &self.compiled {
            eprintln!("    .{:<30} {} bytes", prog.str(*id), f.size);
        }
        eprintln!("  threshold: {} calls", HOT_THRESHOLD);
        eprintln!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".magenta());
    }
}
