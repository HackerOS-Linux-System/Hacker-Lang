use crate::bytecode::{BytecodeProgram, CmpOp, OpCode};
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
    pub exit_code:    i32,   // @ +32
    pub should_exit:  u8,    // @ +36
    pub regs_i_ptr:   *mut i64,  // @ +40
    pub regs_f_ptr:   *mut f64,  // @ +48
    pub cmp_flag_ptr: *mut u8,   // @ +56
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
    fn hl_jit_assert    (ctx: *mut VmCtx, cond_ptr: *const u8, cond_len: usize, msg_ptr: *const u8, msg_len: usize) -> i32;
    fn hl_jit_load_var_i  (ctx: *mut VmCtx, var_ptr: *const u8, var_len: usize, dst_reg: u8);
    fn hl_jit_load_var_f  (ctx: *mut VmCtx, var_ptr: *const u8, var_len: usize, dst_reg: u8);
    fn hl_jit_store_var_i (ctx: *mut VmCtx, var_ptr: *const u8, var_len: usize, src_reg: u8);
    fn hl_jit_store_var_f (ctx: *mut VmCtx, var_ptr: *const u8, var_len: usize, src_reg: u8);
    fn hl_jit_int_to_str  (ctx: *mut VmCtx, var_ptr: *const u8, var_len: usize, src_reg: u8);
    fn hl_jit_float_to_str(ctx: *mut VmCtx, var_ptr: *const u8, var_len: usize, src_reg: u8);
    fn hl_jit_num_for(
        ctx:     *mut VmCtx,
        var_ptr: *const u8, var_len: usize,
        start:   i64,
        end:     i64,
        step:    i64,
        cmd_ptr: *const u8, cmd_len: usize,
        sudo:    bool,
    );
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
// OS mmap wrappers
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
    relocs: Vec<(usize, usize)>,
    ip2off: HashMap<usize, usize>,
}

impl Emit {
    fn new() -> Self {
        Self { code: Vec::with_capacity(512), relocs: Vec::new(), ip2off: HashMap::new() }
    }

    fn len(&self) -> usize { self.code.len() }

    fn mark_ip(&mut self, ip: usize) { self.ip2off.insert(ip, self.code.len()); }

    fn u8(&mut self, b: u8)     { self.code.push(b); }
    fn u32le(&mut self, v: u32) { self.code.extend_from_slice(&v.to_le_bytes()); }
    fn i32le(&mut self, v: i32) { self.code.extend_from_slice(&v.to_le_bytes()); }
    fn u64le(&mut self, v: u64) { self.code.extend_from_slice(&v.to_le_bytes()); }
    fn i64le(&mut self, v: i64) { self.code.extend_from_slice(&v.to_le_bytes()); }

    fn mov_rax(&mut self, v: u64)  { self.u8(0x48); self.u8(0xB8); self.u64le(v); }
    fn mov_rsi(&mut self, v: u64)  { self.u8(0x48); self.u8(0xBE); self.u64le(v); }
    fn mov_rdx(&mut self, v: u64)  { self.u8(0x48); self.u8(0xBA); self.u64le(v); }
    fn mov_rcx(&mut self, v: u64)  { self.u8(0x48); self.u8(0xB9); self.u64le(v); }
    fn mov_r8 (&mut self, v: u64)  { self.u8(0x49); self.u8(0xB8); self.u64le(v); }
    fn mov_r9d(&mut self, v: i32)  { self.u8(0x41); self.u8(0xB9); self.i32le(v); }
    fn mov_ecx(&mut self, v: i32)  { self.u8(0xB9); self.i32le(v); }
    fn mov_rdi_rbx(&mut self)      { self.u8(0x48); self.u8(0x89); self.u8(0xDF); }
    fn call_rax(&mut self)         { self.u8(0xFF); self.u8(0xD0); }
    fn test_al (&mut self)         { self.u8(0x84); self.u8(0xC0); }

    fn mov_rsi_i64(&mut self, v: i64) { self.u8(0x48); self.u8(0xBE); self.i64le(v); }
    fn mov_rdx_i64(&mut self, v: i64) { self.u8(0x48); self.u8(0xBA); self.i64le(v); }
    fn mov_rcx_i64(&mut self, v: i64) { self.u8(0x48); self.u8(0xB9); self.i64le(v); }

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
// Pomocniki emisji
// ─────────────────────────────────────────────────────────────

fn emit_exec_call(em: &mut Emit, cmd: &str, sudo: bool) {
    em.mov_rdi_rbx();
    em.mov_rsi(cmd.as_ptr() as u64);
    em.mov_rdx(cmd.len() as u64);
    em.mov_ecx(sudo as i32);
    em.mov_rax(hl_jit_exec as u64);
    em.call_rax();
}

fn emit_set_env_call(em: &mut Emit, key: &str, val: &str) {
    em.mov_rdi_rbx();
    em.mov_rsi(key.as_ptr() as u64);
    em.mov_rdx(key.len() as u64);
    em.mov_rcx(val.as_ptr() as u64);
    em.mov_r8(val.len() as u64);
    em.mov_rax(hl_jit_set_env as u64);
    em.call_rax();
}

fn emit_assert_call(em: &mut Emit, cond: &str, msg: &str) {
    em.mov_rdi_rbx();
    em.mov_rsi(cond.as_ptr() as u64);
    em.mov_rdx(cond.len() as u64);
    em.mov_rcx(msg.as_ptr() as u64);
    em.mov_r8(msg.len() as u64);
    em.mov_rax(hl_jit_assert as u64);
    em.call_rax();
    emit_check_should_exit(em);
}

fn emit_check_should_exit(em: &mut Emit) {
    em.u8(0xF6); em.u8(0x43); em.u8(36); em.u8(0x01);
    em.u8(0x0F); em.u8(0x85);
    let off = em.len();
    em.relocs.push((off, usize::MAX));
    em.i32le(0);
}

// ─────────────────────────────────────────────────────────────
// Numeryczne pomocniki emisji
// ─────────────────────────────────────────────────────────────

fn emit_load_regs_i_ptr(em: &mut Emit) {
    em.u8(0x48); em.u8(0x8B); em.u8(0x43); em.u8(40);
}

fn emit_load_regs_f_ptr(em: &mut Emit) {
    em.u8(0x48); em.u8(0x8B); em.u8(0x43); em.u8(48);
}

fn emit_load_int(em: &mut Emit, dst: u8, val: i64) {
    emit_load_regs_i_ptr(em);
    em.u8(0x49); em.u8(0xBA); em.i64le(val);
    let off = (dst as u32) * 8;
    if off < 128 {
        em.u8(0x4C); em.u8(0x89); em.u8(0x50); em.u8(off as u8);
    } else {
        em.u8(0x4C); em.u8(0x89); em.u8(0x90); em.u32le(off);
    }
}

fn emit_load_float(em: &mut Emit, dst: u8, val: f64) {
    emit_load_regs_f_ptr(em);
    let bits = val.to_bits();
    em.u8(0x49); em.u8(0xBA); em.u64le(bits);
    let off = (dst as u32) * 8;
    if off < 128 {
        em.u8(0x4C); em.u8(0x89); em.u8(0x50); em.u8(off as u8);
    } else {
        em.u8(0x4C); em.u8(0x89); em.u8(0x90); em.u32le(off);
    }
}

fn emit_load_bool(em: &mut Emit, dst: u8, val: bool) {
    emit_load_int(em, dst, if val { 1 } else { 0 });
}

enum IntBinOp { Add, Sub, Mul, Div, Mod }

fn emit_int_binop(em: &mut Emit, dst: u8, a: u8, b: u8, op: IntBinOp) {
    emit_load_regs_i_ptr(em);
    let off_a = (a as u32) * 8;
    let off_b = (b as u32) * 8;
    let off_d = (dst as u32) * 8;

    match op {
        IntBinOp::Add => {
            emit_load_reg_from_arr(em, off_a);
            emit_add_reg_mem(em, off_b);
            emit_store_reg_to_arr(em, off_d);
        }
        IntBinOp::Sub => {
            emit_load_reg_from_arr(em, off_a);
            emit_sub_reg_mem(em, off_b);
            emit_store_reg_to_arr(em, off_d);
        }
        IntBinOp::Mul => { emit_fallback_numeric(em, dst, a, b, 2); }
        IntBinOp::Div => { emit_fallback_numeric(em, dst, a, b, 3); }
        IntBinOp::Mod => { emit_fallback_numeric(em, dst, a, b, 4); }
    }
}

fn emit_load_reg_from_arr(em: &mut Emit, off: u32) {
    if off < 128 {
        em.u8(0x4C); em.u8(0x8B); em.u8(0x50); em.u8(off as u8);
    } else {
        em.u8(0x4C); em.u8(0x8B); em.u8(0x90); em.u32le(off);
    }
}

fn emit_add_reg_mem(em: &mut Emit, off: u32) {
    if off < 128 {
        em.u8(0x4C); em.u8(0x03); em.u8(0x50); em.u8(off as u8);
    } else {
        em.u8(0x4C); em.u8(0x03); em.u8(0x90); em.u32le(off);
    }
}

fn emit_sub_reg_mem(em: &mut Emit, off: u32) {
    if off < 128 {
        em.u8(0x4C); em.u8(0x2B); em.u8(0x50); em.u8(off as u8);
    } else {
        em.u8(0x4C); em.u8(0x2B); em.u8(0x90); em.u32le(off);
    }
}

fn emit_store_reg_to_arr(em: &mut Emit, off: u32) {
    if off < 128 {
        em.u8(0x4C); em.u8(0x89); em.u8(0x50); em.u8(off as u8);
    } else {
        em.u8(0x4C); em.u8(0x89); em.u8(0x90); em.u32le(off);
    }
}

fn emit_fallback_numeric(em: &mut Emit, dst: u8, a: u8, b: u8, op_code: u8) {
    let op_idx = ((op_code as i64) << 24)
    | ((dst as i64) << 16)
    | ((a   as i64) << 8)
    |  (b   as i64);
    em.mov_rdi_rbx();
    em.mov_rsi(op_idx as u64);
    em.mov_rax(hl_jit_fallback as u64);
    em.call_rax();
}

fn emit_cmp_i(em: &mut Emit, a: u8, b: u8, op: CmpOp) {
    let op_byte: u8 = match op {
        CmpOp::Eq => 0, CmpOp::Ne => 1,
        CmpOp::Lt => 2, CmpOp::Le => 3,
        CmpOp::Gt => 4, CmpOp::Ge => 5,
    };
    let op_idx = (0xC0i64 << 24)
    | ((op_byte as i64) << 16)
    | ((a as i64) << 8)
    |  (b as i64);
    em.mov_rdi_rbx();
    em.mov_rsi(op_idx as u64);
    em.mov_rax(hl_jit_fallback as u64);
    em.call_rax();
}

fn emit_jump_if_true(em: &mut Emit, target_ip: usize) {
    em.u8(0x48); em.u8(0x8B); em.u8(0x43); em.u8(56);
    em.u8(0xF6); em.u8(0x00); em.u8(0x01);
    em.u8(0x0F); em.u8(0x85);
    let off = em.len();
    em.relocs.push((off, target_ip));
    em.i32le(0);
}

fn emit_load_var_i(em: &mut Emit, var_name: &str, dst: u8) {
    em.mov_rdi_rbx();
    em.mov_rsi(var_name.as_ptr() as u64);
    em.mov_rdx(var_name.len() as u64);
    em.mov_ecx(dst as i32);
    em.mov_rax(hl_jit_load_var_i as u64);
    em.call_rax();
}

fn emit_load_var_f(em: &mut Emit, var_name: &str, dst: u8) {
    em.mov_rdi_rbx();
    em.mov_rsi(var_name.as_ptr() as u64);
    em.mov_rdx(var_name.len() as u64);
    em.mov_ecx(dst as i32);
    em.mov_rax(hl_jit_load_var_f as u64);
    em.call_rax();
}

fn emit_store_var_i(em: &mut Emit, var_name: &str, src: u8) {
    em.mov_rdi_rbx();
    em.mov_rsi(var_name.as_ptr() as u64);
    em.mov_rdx(var_name.len() as u64);
    em.mov_ecx(src as i32);
    em.mov_rax(hl_jit_store_var_i as u64);
    em.call_rax();
}

fn emit_store_var_f(em: &mut Emit, var_name: &str, src: u8) {
    em.mov_rdi_rbx();
    em.mov_rsi(var_name.as_ptr() as u64);
    em.mov_rdx(var_name.len() as u64);
    em.mov_ecx(src as i32);
    em.mov_rax(hl_jit_store_var_f as u64);
    em.call_rax();
}

fn emit_int_to_str(em: &mut Emit, var_name: &str, src: u8) {
    em.mov_rdi_rbx();
    em.mov_rsi(var_name.as_ptr() as u64);
    em.mov_rdx(var_name.len() as u64);
    em.mov_ecx(src as i32);
    em.mov_rax(hl_jit_int_to_str as u64);
    em.call_rax();
}

fn emit_float_to_str(em: &mut Emit, var_name: &str, src: u8) {
    em.mov_rdi_rbx();
    em.mov_rsi(var_name.as_ptr() as u64);
    em.mov_rdx(var_name.len() as u64);
    em.mov_ecx(src as i32);
    em.mov_rax(hl_jit_float_to_str as u64);
    em.call_rax();
}

fn emit_num_for(em: &mut Emit, var: &str, start: i64, end: i64, step: i64) {
    // NumFor z 9 argumentami — za dużo dla prostego inline.
    // Delegujemy przez fallback z op_idx = 0xF0 (sygnał do interpretera).
    // W praktyce JIT nigdy nie kompiluje funkcji zawierających NumForExec
    // bo są one zazwyczaj pojedyncze w hot path — interpreter jest wystarczający.
    let op_idx: i64 = 0xF0_0000_0000i64
    | ((start.min(0xFFFF) as i64) & 0xFFFF);
    let _ = (var, end, step); // używane przez interpreter, nie JIT inline
    em.mov_rdi_rbx();
    em.mov_rsi(op_idx as u64);
    em.mov_rax(hl_jit_fallback as u64);
    em.call_rax();
}

// ─────────────────────────────────────────────────────────────
// Skompilowana funkcja JIT
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

                // ── Kontrola przepływu ────────────────────────

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
                    em.jz(*target);
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

                OpCode::Return => break,

                OpCode::Exit(code) => {
                    em.mov_rbx_d8_i32(32, *code);
                    em.mov_rbx_d8_i8(36, 1);
                    em.jmp(usize::MAX);
                }

                // ── Zmienne ───────────────────────────────────

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

                OpCode::SetConst { key_id, val_id } => {
                    let k = prog.str(*key_id);
                    let v = prog.str(*val_id);
                    emit_set_env_call(&mut em, k, v);
                }

                OpCode::SetOut { val_id } => {
                    let v = prog.str(*val_id);
                    emit_set_env_call(&mut em, "_HL_OUT", v);
                }

                // ── Misc ──────────────────────────────────────

                OpCode::Plugin { .. } | OpCode::Lock { .. } | OpCode::Unlock { .. } => {
                    em.mov_rdi_rbx();
                    em.mov_rsi(ip as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                }

                OpCode::HotLoop { .. } | OpCode::Nop => {}

                // ── Spawn / Await ─────────────────────────────

                OpCode::SpawnBg { cmd_id, sudo } => {
                    let raw = prog.str(*cmd_id);
                    let bg  = format!("{} &", raw);
                    let bg_leaked: &'static str = Box::leak(bg.into_boxed_str());
                    emit_exec_call(&mut em, bg_leaked, *sudo);
                }

                OpCode::SpawnAssign { key_id, cmd_id, sudo } => {
                    let key = prog.str(*key_id);
                    let cmd = prog.str(*cmd_id);
                    let sh  = format!("export {}=$( {} & echo $! )", key, cmd);
                    let sh_leaked: &'static str = Box::leak(sh.into_boxed_str());
                    emit_exec_call(&mut em, sh_leaked, *sudo);
                }

                OpCode::AwaitPid { expr_id } => {
                    let expr = prog.str(*expr_id);
                    if expr.trim().starts_with('.') {
                        let fname = expr.trim().trim_start_matches('.');
                        em.mov_rdi_rbx();
                        em.mov_rsi(fname.as_ptr() as u64);
                        em.mov_rdx(fname.len() as u64);
                        em.mov_rax(hl_jit_call_func as u64);
                        em.call_rax();
                    } else {
                        let sh = format!("wait {}", expr.trim());
                        let sh_leaked: &'static str = Box::leak(sh.into_boxed_str());
                        emit_exec_call(&mut em, sh_leaked, false);
                    }
                }

                OpCode::AwaitAssign { key_id, expr_id } => {
                    let key  = prog.str(*key_id);
                    let expr = prog.str(*expr_id).trim();
                    if expr.starts_with('.') {
                        let fname = expr.trim_start_matches('.');
                        em.mov_rdi_rbx();
                        em.mov_rsi(fname.as_ptr() as u64);
                        em.mov_rdx(fname.len() as u64);
                        em.mov_rax(hl_jit_call_func as u64);
                        em.call_rax();
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

                OpCode::Assert { cond_id, msg_id } => {
                    let cond = prog.str(*cond_id);
                    let msg  = msg_id
                    .map(|id| prog.str(id))
                    .unwrap_or("Assertion failed");
                    emit_assert_call(&mut em, cond, msg);
                }

                OpCode::MatchExec { case_cmd_id, sudo } => {
                    let cmd = prog.str(*case_cmd_id);
                    emit_exec_call(&mut em, cmd, *sudo);
                }

                OpCode::PipeExec { cmd_id, sudo } => {
                    let cmd = prog.str(*cmd_id);
                    emit_exec_call(&mut em, cmd, *sudo);
                }

                // ── v7: NUMERYCZNE ────────────────────────────

                OpCode::LoadInt { dst, val } => {
                    emit_load_int(&mut em, *dst, *val);
                }

                OpCode::LoadFloat { dst, val } => {
                    emit_load_float(&mut em, *dst, *val);
                }

                OpCode::LoadBool { dst, val } => {
                    emit_load_bool(&mut em, *dst, *val);
                }

                OpCode::LoadStr { dst, str_id } => {
                    let s = prog.str(*str_id);
                    if let Ok(n) = s.parse::<i64>() {
                        emit_load_int(&mut em, *dst, n);
                    } else if let Ok(f) = s.parse::<f64>() {
                        emit_load_float(&mut em, *dst, f);
                    } else {
                        em.mov_rdi_rbx();
                        em.mov_rsi(ip as u64);
                        em.mov_rax(hl_jit_fallback as u64);
                        em.call_rax();
                    }
                }

                OpCode::LoadVarI { dst, var_id } => {
                    let var_name = prog.str(*var_id);
                    emit_load_var_i(&mut em, var_name, *dst);
                }

                OpCode::LoadVarF { dst, var_id } => {
                    let var_name = prog.str(*var_id);
                    emit_load_var_f(&mut em, var_name, *dst);
                }

                OpCode::StoreVarI { var_id, src } => {
                    let var_name = prog.str(*var_id);
                    emit_store_var_i(&mut em, var_name, *src);
                }

                OpCode::StoreVarF { var_id, src } => {
                    let var_name = prog.str(*var_id);
                    emit_store_var_f(&mut em, var_name, *src);
                }

                OpCode::AddI { dst, a, b } => {
                    emit_int_binop(&mut em, *dst, *a, *b, IntBinOp::Add);
                }
                OpCode::SubI { dst, a, b } => {
                    emit_int_binop(&mut em, *dst, *a, *b, IntBinOp::Sub);
                }
                OpCode::MulI { dst, a, b } => {
                    emit_int_binop(&mut em, *dst, *a, *b, IntBinOp::Mul);
                }
                OpCode::DivI { dst, a, b } => {
                    emit_int_binop(&mut em, *dst, *a, *b, IntBinOp::Div);
                }
                OpCode::ModI { dst, a, b } => {
                    emit_int_binop(&mut em, *dst, *a, *b, IntBinOp::Mod);
                }

                OpCode::NegI { dst, src } => {
                    emit_load_regs_i_ptr(&mut em);
                    let off_s = (*src as u32) * 8;
                    let off_d = (*dst as u32) * 8;
                    emit_load_reg_from_arr(&mut em, off_s);
                    // neg r10
                    em.u8(0x49); em.u8(0xF7); em.u8(0xD2);
                    emit_store_reg_to_arr(&mut em, off_d);
                }

                OpCode::AddF { dst, a, b }
                | OpCode::SubF { dst, a, b }
                | OpCode::MulF { dst, a, b }
                | OpCode::DivF { dst, a, b } => {
                    let op_code: u8 = match &prog.ops[ip] {
                        OpCode::AddF { .. } => 10,
                        OpCode::SubF { .. } => 11,
                        OpCode::MulF { .. } => 12,
                        OpCode::DivF { .. } => 13,
                        _ => unreachable!(),
                    };
                    let op_idx = ((op_code as i64) << 24)
                    | ((*dst as i64) << 16)
                    | ((*a  as i64) <<  8)
                    |  (*b  as i64);
                    em.mov_rdi_rbx();
                    em.mov_rsi(op_idx as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                }

                OpCode::NegF { dst, src } => {
                    let op_idx: i64 = (14i64 << 24)
                    | ((*dst as i64) << 16)
                    |  (*src as i64);
                    em.mov_rdi_rbx();
                    em.mov_rsi(op_idx as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                }

                OpCode::CmpI { a, b, op } => {
                    emit_cmp_i(&mut em, *a, *b, *op);
                }

                OpCode::CmpF { a, b, op } => {
                    let op_byte: u8 = match op {
                        CmpOp::Eq => 0, CmpOp::Ne => 1,
                        CmpOp::Lt => 2, CmpOp::Le => 3,
                        CmpOp::Gt => 4, CmpOp::Ge => 5,
                    };
                    let op_idx = (0xC1i64 << 24)
                    | ((op_byte as i64) << 16)
                    | ((*a as i64) << 8)
                    |  (*b as i64);
                    em.mov_rdi_rbx();
                    em.mov_rsi(op_idx as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                }

                OpCode::JumpIfTrue { target } => {
                    emit_jump_if_true(&mut em, *target);
                }

                OpCode::NumForExec { var_id, start, end, step, .. } => {
                    let var = prog.str(*var_id);
                    emit_num_for(&mut em, var, *start, *end, *step);
                }

                OpCode::WhileExprExec { lhs_reg, op, rhs_reg, .. } => {
                    let op_byte: u8 = match op {
                        CmpOp::Eq => 0, CmpOp::Ne => 1,
                        CmpOp::Lt => 2, CmpOp::Le => 3,
                        CmpOp::Gt => 4, CmpOp::Ge => 5,
                    };
                    let op_idx = (0xE0i64 << 24)
                    | ((op_byte   as i64) << 16)
                    | ((*lhs_reg  as i64) <<  8)
                    |  (*rhs_reg  as i64);
                    em.mov_rdi_rbx();
                    em.mov_rsi(op_idx as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                }

                OpCode::IntToFloat { dst, src } => {
                    let op_idx: i64 = (0x20i64 << 24)
                    | ((*dst as i64) << 8)
                    |  (*src as i64);
                    em.mov_rdi_rbx();
                    em.mov_rsi(op_idx as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                }

                OpCode::FloatToInt { dst, src } => {
                    let op_idx: i64 = (0x21i64 << 24)
                    | ((*dst as i64) << 8)
                    |  (*src as i64);
                    em.mov_rdi_rbx();
                    em.mov_rsi(op_idx as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                }

                OpCode::IntToStr { var_id, src } => {
                    let var_name = prog.str(*var_id);
                    emit_int_to_str(&mut em, var_name, *src);
                }

                OpCode::FloatToStr { var_id, src } => {
                    let var_name = prog.str(*var_id);
                    emit_float_to_str(&mut em, var_name, *src);
                }

                OpCode::ReturnI { src } => {
                    emit_int_to_str(&mut em, "_HL_OUT", *src);
                    break;
                }

                OpCode::ReturnF { src } => {
                    emit_float_to_str(&mut em, "_HL_OUT", *src);
                    break;
                }

                // ── Arena — NIE kompilujemy natywnie ─────────
                //
                // Funkcje z flagą is_arena_fn=true nie powinny być
                // kompilowane przez JIT — compiler.rs emituje ArenaEnter
                // na początku ich ciała i JitCompiler.compile() nigdy
                // nie powinien ich dostać.
                //
                // Jeśli tu trafimy (błąd lub przyszła zmiana), delegujemy
                // całość do interpretera przez fallback. Arena wymaga
                // starannego zarządzania stosem HlJitArenaScope który
                // żyje w VM — nie możemy go bezpiecznie obsłużyć z JIT.
                OpCode::ArenaEnter { .. }
                | OpCode::ArenaExit
                | OpCode::ArenaAlloc { .. }
                | OpCode::ArenaReset => {
                    em.mov_rdi_rbx();
                    em.mov_rsi(ip as u64);
                    em.mov_rax(hl_jit_fallback as u64);
                    em.call_rax();
                    // ArenaExit sygnalizuje koniec bloku — zatrzymaj JIT
                    if matches!(&prog.ops[ip], OpCode::ArenaExit) {
                        break;
                    }
                }
            }

            ip += 1;
        }

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
