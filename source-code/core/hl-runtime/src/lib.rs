#![allow(dead_code, unused_variables, unused_assignments)]
use std::alloc::{alloc, dealloc, Layout};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::mem;
use std::path::PathBuf;
use std::process::Command;
use std::ptr;
use std::time::Instant;
use colored::Colorize;
use dynasmrt::{dynasm, DynasmApi, AssemblyOffset};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use hl_plsa::{AnalysisResult, ProgramNode, Stmt};
const CACHE_DIR: &str = "/tmp/hl_cache";
const PLSA_BIN_NAME: &str = "hl-plsa";
// ═══════════════════════════════════════════════════════════
// NaN-boxed Value
// ═══════════════════════════════════════════════════════════
#[repr(transparent)]
#[derive(Copy, Clone)]
struct Value(u64);
const NAN_BITS: u64 = 0x7FF8_0000_0000_0000;
const TAG_INT: u64 = NAN_BITS | (1u64 << 48);
const TAG_PTR: u64 = NAN_BITS | (2u64 << 48);
const TAG_BOOL: u64 = NAN_BITS | (3u64 << 48);
const TAG_NIL: u64 = NAN_BITS | (4u64 << 48);
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
impl Value {
    fn from_f64(f: f64) -> Self { Value(f.to_bits()) }
    fn from_i32(i: i32) -> Self { Value(TAG_INT | ((i as u64) & PAYLOAD_MASK)) }
    fn from_bool(b: bool) -> Self { Value(TAG_BOOL | (b as u64)) }
    fn nil() -> Self { Value(TAG_NIL) }
    fn from_gc(p: *mut GcHdr) -> Self { Value(TAG_PTR | ((p as u64) & PAYLOAD_MASK)) }
    fn is_f64(self) -> bool { (self.0 & NAN_BITS) != NAN_BITS }
    fn is_int(self) -> bool { (self.0 & !PAYLOAD_MASK) == TAG_INT }
    fn is_bool(self) -> bool { (self.0 & !PAYLOAD_MASK) == TAG_BOOL }
    fn is_nil(self) -> bool { (self.0 & !PAYLOAD_MASK) == TAG_NIL }
    fn is_gc(self) -> bool { (self.0 & !PAYLOAD_MASK) == TAG_PTR }
    fn as_f64(self) -> f64 { f64::from_bits(self.0) }
    fn as_i32(self) -> i32 { ((self.0 & PAYLOAD_MASK) as i64) as i32 }
    fn as_bool(self) -> bool { (self.0 & 1) != 0 }
    fn as_gc(self) -> *mut GcHdr { (self.0 & PAYLOAD_MASK) as *mut GcHdr }
    fn points_young(self) -> bool {
        self.is_gc() && unsafe { (*self.as_gc()).gen == 0 }
    }
}
impl PartialEq for Value {
    fn eq(&self, o: &Self) -> bool {
        if self.is_int() && o.is_int() { return self.as_i32() == o.as_i32(); }
        if self.is_f64() && o.is_f64() { return self.as_f64() == o.as_f64(); }
        if self.is_bool() && o.is_bool() { return self.as_bool() == o.as_bool(); }
        if self.is_nil() && o.is_nil() { return true; }
        if self.is_gc() && o.is_gc() {
            let a = self.as_gc();
            let b = o.as_gc();
            unsafe {
                if (*a).kind != (*b).kind { return false; }
                match (*a).kind {
                    0 => { // string
                        let sa = &*(a as *mut GcStr);
                        let sb = &*(b as *mut GcStr);
                        if sa.len != sb.len { return false; }
                        std::slice::from_raw_parts(sa.data, sa.len)
                        == std::slice::from_raw_parts(sb.data, sb.len)
                    }
                    1 => { // list
                        (*(a as *mut GcList)).items == (*(b as *mut GcList)).items
                    }
                    _ => false,
                }
            }
        } else {
            false
        }
    }
}
impl Eq for Value {}
impl Hash for Value {
    fn hash<H: Hasher>(&self, st: &mut H) {
        if self.is_int() { self.as_i32().hash(st); }
        else if self.is_bool() { self.as_bool().hash(st); }
        else if self.is_nil() { 0u8.hash(st); }
        else if self.is_gc() {
            unsafe {
                let h = self.as_gc();
                if (*h).kind == 0 {
                    let s = &*(h as *mut GcStr);
                    std::slice::from_raw_parts(s.data, s.len).hash(st);
                } else {
                    (h as usize).hash(st);
                }
            }
        } else {
            self.0.hash(st);
        }
    }
}
// ═══════════════════════════════════════════════════════════
// GC
// ═══════════════════════════════════════════════════════════
const BLOCK: usize = 32 * 1024;
struct BumpBlock { data: Box<[u8; BLOCK]>, off: usize }
struct BumpArena { blocks: Vec<BumpBlock>, cur: usize }
impl BumpArena {
    fn new() -> Self {
        let mut a = BumpArena { blocks: Vec::new(), cur: 0 };
        a.add(); a
    }
    fn add(&mut self) {
        self.blocks.push(BumpBlock { data: Box::new([0; BLOCK]), off: 0 });
    }
    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let sz = layout.size();
        let al = layout.align();
        let off = (self.blocks[self.cur].off + al - 1) & !(al - 1);
        if off + sz > BLOCK {
            self.cur += 1;
            if self.cur >= self.blocks.len() { self.add(); }
            self.blocks[self.cur].off = sz;
            self.blocks[self.cur].data.as_mut_ptr()
        } else {
            self.blocks[self.cur].off = off + sz;
            unsafe { self.blocks[self.cur].data.as_mut_ptr().add(off) }
        }
    }
    fn reset(&mut self) {
        for b in &mut self.blocks { b.off = 0; }
        self.cur = 0;
    }
}
struct FreeAlloc;
impl FreeAlloc {
    fn alloc(sz: usize) -> *mut u8 {
        unsafe { alloc(Layout::from_size_align_unchecked(sz, 8)) }
    }
    fn free(p: *mut u8, sz: usize) {
        unsafe { dealloc(p, Layout::from_size_align_unchecked(sz, 8)); }
    }
}
#[repr(C)]
struct GcHdr { mark: u8, gen: u8, kind: u8, size: u32, next: *mut GcHdr }
#[repr(C)]
struct GcStr { hdr: GcHdr, len: usize, data: *mut u8 }
#[repr(C)]
struct GcList { hdr: GcHdr, items: Vec<Value> }
#[repr(C)]
struct GcMap { hdr: GcHdr, map: HashMap<Value, Value> }
#[repr(C)]
struct GcObj { hdr: GcHdr, obj: Obj }
#[derive(Clone)]
struct Obj {
    name: String,
    fields: HashMap<String, Value>,
    methods: HashMap<String, usize>,
}
struct Gc {
    young: BumpArena,
    all: *mut GcHdr,
    gray: Vec<*mut GcHdr>,
    barriers: Vec<*mut GcHdr>,
    allocated: usize,
    threshold: usize,
}
impl Gc {
    fn new() -> Self {
        Gc { young: BumpArena::new(), all: ptr::null_mut(),
            gray: Vec::new(), barriers: Vec::new(),
            allocated: 0, threshold: 1 << 20 }
    }
    fn raw_alloc(&mut self, payload: usize, kind: u8, gen: u8) -> *mut GcHdr {
        if self.allocated > self.threshold { self.threshold *= 2; }
        let total = payload + mem::size_of::<GcHdr>();
        let ptr = if gen == 0 {
            self.young.alloc(unsafe { Layout::from_size_align_unchecked(total, 8) })
        } else {
            FreeAlloc::alloc(total)
        } as *mut GcHdr;
        unsafe {
            (*ptr) = GcHdr { mark: 0, gen, kind, size: payload as u32, next: self.all };
        }
        self.all = ptr;
        self.allocated += total;
        ptr
    }
    fn alloc_str(&mut self, s: String, gen: u8) -> Value {
        let len = s.len();
        let pay = mem::size_of::<GcStr>() - mem::size_of::<GcHdr>();
        let h = self.raw_alloc(pay, 0, gen);
        unsafe {
            let gs = &mut *(h as *mut GcStr);
            gs.len = len;
            gs.data = if len > 0 {
                let d = alloc(Layout::from_size_align_unchecked(len, 1));
                ptr::copy_nonoverlapping(s.as_ptr(), d, len);
                d
            } else { ptr::null_mut() };
        }
        Value::from_gc(h)
    }
    fn alloc_list(&mut self, items: Vec<Value>, gen: u8) -> Value {
        let pay = mem::size_of::<GcList>() - mem::size_of::<GcHdr>();
        let h = self.raw_alloc(pay, 1, gen);
        unsafe { (*(h as *mut GcList)).items = items; }
        Value::from_gc(h)
    }
    fn alloc_map(&mut self, map: HashMap<Value, Value>, gen: u8) -> Value {
        let pay = mem::size_of::<GcMap>() - mem::size_of::<GcHdr>();
        let h = self.raw_alloc(pay, 2, gen);
        unsafe { (*(h as *mut GcMap)).map = map; }
        Value::from_gc(h)
    }
    fn alloc_obj(&mut self, obj: Obj, gen: u8) -> Value {
        let pay = mem::size_of::<GcObj>() - mem::size_of::<GcHdr>();
        let h = self.raw_alloc(pay, 3, gen);
        unsafe { (*(h as *mut GcObj)).obj = obj; }
        Value::from_gc(h)
    }
    fn mark_gray(&mut self, p: *mut GcHdr) {
        unsafe { if (*p).mark == 0 { (*p).mark = 1; self.gray.push(p); } }
    }
    fn mark_roots(&mut self, vm: &VM) {
        for v in &vm.stack { if v.is_gc() { let p = v.as_gc(); self.mark_gray(p); } }
        for (_, v) in &vm.globals { if v.is_gc() { let p = v.as_gc(); self.mark_gray(p); } }
        for fr in &vm.frames {
            for &v in &fr.locals[..fr.live] {
                if v.is_gc() { let p = v.as_gc(); self.mark_gray(p); }
            }
        }
        let bs = mem::take(&mut self.barriers);
        for p in bs { self.mark_gray(p); }
    }
    fn trace(&mut self, p: *mut GcHdr) {
        unsafe {
            match (*p).kind {
                0 => {}
                1 => for v in &(*(p as *mut GcList)).items {
                    if v.is_gc() { let pp = v.as_gc(); self.mark_gray(pp); }
                },
                2 => for (k, v) in &(*(p as *mut GcMap)).map {
                    if k.is_gc() { let pp = k.as_gc(); self.mark_gray(pp); }
                    if v.is_gc() { let pp = v.as_gc(); self.mark_gray(pp); }
                },
                3 => for (_, v) in &(*(p as *mut GcObj)).obj.fields {
                    if v.is_gc() { let pp = v.as_gc(); self.mark_gray(pp); }
                },
                _ => {}
            }
        }
    }
    fn mark_phase(&mut self) {
        while let Some(p) = self.gray.pop() {
            unsafe { (*p).mark = 2; }
            self.trace(p);
        }
    }
    fn sweep_phase(&mut self) {
        let mut prev: *mut *mut GcHdr = &mut self.all;
        let mut cur = self.all;
        while !cur.is_null() {
            unsafe {
                let nxt = (*cur).next;
                if (*cur).mark == 0 {
                    *prev = nxt;
                    self.free_obj(cur);
                } else {
                    (*cur).mark = 0;
                    prev = &mut (*cur).next;
                }
                cur = nxt;
            }
        }
    }
    fn free_obj(&mut self, p: *mut GcHdr) {
        let total = unsafe { (*p).size as usize } + mem::size_of::<GcHdr>();
        unsafe {
            match (*p).kind {
                0 => {
                    let s = &*(p as *mut GcStr);
                    if !s.data.is_null() && s.len > 0 {
                        dealloc(s.data, Layout::from_size_align_unchecked(s.len, 1));
                    }
                }
                1 => ptr::drop_in_place(&mut (*(p as *mut GcList)).items),
                2 => ptr::drop_in_place(&mut (*(p as *mut GcMap)).map),
                3 => ptr::drop_in_place(&mut (*(p as *mut GcObj)).obj),
                _ => {}
            }
            if (*p).gen == 1 { FreeAlloc::free(p as *mut u8, total); }
        }
        self.allocated = self.allocated.saturating_sub(total);
    }
    fn collect(&mut self, vm: &VM) {
        self.mark_roots(vm);
        self.mark_phase();
        self.sweep_phase();
        self.young.reset();
    }
}
// ═══════════════════════════════════════════════════════════
// Constant pool
// ═══════════════════════════════════════════════════════════
#[derive(Serialize, Deserialize)]
struct Pool {
    strings: Vec<String>,
    ints: Vec<i32>,
    floats: Vec<f64>,
}
impl Pool {
    fn new() -> Self { Pool { strings: Vec::new(), ints: Vec::new(), floats: Vec::new() } }
    fn str_idx(&mut self, s: String) -> u32 {
        if let Some(i) = self.strings.iter().position(|x| *x == s) { return i as u32; }
        let i = self.strings.len() as u32; self.strings.push(s); i
    }
    fn int_idx(&mut self, v: i32) -> u32 {
        if let Some(i) = self.ints.iter().position(|x| *x == v) { return i as u32; }
        let i = self.ints.len() as u32; self.ints.push(v); i
    }
    fn flt_idx(&mut self, v: f64) -> u32 {
        if let Some(i) = self.floats.iter().position(|x| *x == v) { return i as u32; }
        let i = self.floats.len() as u32; self.floats.push(v); i
    }
}
// ═══════════════════════════════════════════════════════════
// Bytecode
// ═══════════════════════════════════════════════════════════
#[derive(Debug, Clone, Serialize, Deserialize)]
enum Op {
    LoadStr(u32),
    LoadInt(u32),
    LoadFlt(u32),
    LoadBool(bool),
    LoadNil,
    LoadVar(u16),
    StoreVar(u16),
    BinAdd, BinSub, BinMul, BinDiv,
    BinEq, BinNe, BinLt, BinGt, BinLe, BinGe,
    Jump(i32),
    JumpFalse(i32),
    Call(u16, u8),
    Return,
    Exec(u32, bool), // (str_idx, is_sudo)
    Exit,
}
#[derive(Serialize, Deserialize)]
struct Program {
    ops: Vec<Op>,
    pool: Pool,
    fns: HashMap<u16, usize>, // fn_idx → first op index
}
// ═══════════════════════════════════════════════════════════
// JIT stubs
// ═══════════════════════════════════════════════════════════
#[repr(C)]
struct VmState { sp: *mut Value, bp: *mut Value }
struct Trace {
    _buf: dynasmrt::ExecutableBuffer,
    fptr: unsafe fn(*mut VmState),
    start: usize,
}
struct Jit { cache: HashMap<usize, Trace> }
impl Jit {
    fn new() -> Self { Jit { cache: HashMap::new() } }
    fn codegen(&mut self, start_ip: usize) {
        let mut ops = dynasmrt::x64::Assembler::new().unwrap();
        dynasm!(ops
        ; .arch x64
        ; mov rax, 0
        ; ret
        );
        let buf = ops.finalize().unwrap();
        let fptr = unsafe {
            std::mem::transmute::<_, unsafe fn(*mut VmState)>(buf.ptr(AssemblyOffset(0)))
        };
        self.cache.insert(start_ip, Trace { _buf: buf, fptr, start: start_ip });
    }
}
// ═══════════════════════════════════════════════════════════
// Call frame
// ═══════════════════════════════════════════════════════════
struct Frame {
    locals: [Value; 256],
    live: usize, // how many locals are actually used
    ret_ip: usize,
}
// ═══════════════════════════════════════════════════════════
// VM
// ═══════════════════════════════════════════════════════════
struct VM {
    stack: Vec<Value>,
    frames: Vec<Frame>,
    globals: HashMap<String, Value>,
    env: HashMap<String, String>,
    gc: Gc,
    jit: Jit,
    hot: HashMap<usize, u32>,
}
impl VM {
    fn new() -> Self {
        VM {
            stack: Vec::new(),
            frames: Vec::new(),
            globals: HashMap::new(),
            env: std::env::vars().collect(),
            gc: Gc::new(),
            jit: Jit::new(),
            hot: HashMap::new(),
        }
    }
    fn substitute(&self, text: &str) -> String {
        let mut r = text.to_string();
        for (k, v) in &self.globals {
            if v.is_gc() {
                unsafe {
                    let h = v.as_gc();
                    if (*h).kind == 0 {
                        let s = &*(h as *mut GcStr);
                        let sv = String::from_utf8_lossy(
                            std::slice::from_raw_parts(s.data, s.len)
                        );
                        r = r.replace(&format!("@{}", k), sv.as_ref());
                    }
                }
            }
        }
        for (k, v) in &self.env {
            r = r.replace(&format!("${}", k), v);
        }
        r
    }
    fn run(&mut self, prog: &Program) {
        let mut ip = 0usize;
        while ip < prog.ops.len() {
            match &prog.ops[ip] {
                Op::LoadStr(i) => {
                    let s = prog.pool.strings[*i as usize].clone();
                    let val = self.gc.alloc_str(s, 0);
                    self.stack.push(val);
                }
                Op::LoadInt(i) => {
                    self.stack.push(Value::from_i32(prog.pool.ints[*i as usize]));
                }
                Op::LoadFlt(i) => {
                    self.stack.push(Value::from_f64(prog.pool.floats[*i as usize]));
                }
                Op::LoadBool(b) => self.stack.push(Value::from_bool(*b)),
                Op::LoadNil => self.stack.push(Value::nil()),
                Op::LoadVar(s) => {}, // not implemented in provided code
                Op::StoreVar(s) => {}, // not implemented fully
                Op::BinAdd => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    let v = if a.is_int() && b.is_int() {
                        Value::from_i32(a.as_i32().wrapping_add(b.as_i32()))
                    } else { Value::from_f64(a.as_f64() + b.as_f64()) };
                    self.stack.push(v);
                }
                Op::BinSub => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    let v = if a.is_int() && b.is_int() {
                        Value::from_i32(a.as_i32().wrapping_sub(b.as_i32()))
                    } else { Value::from_f64(a.as_f64() - b.as_f64()) };
                    self.stack.push(v);
                }
                Op::BinMul => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    let v = if a.is_int() && b.is_int() {
                        Value::from_i32(a.as_i32().wrapping_mul(b.as_i32()))
                    } else { Value::from_f64(a.as_f64() * b.as_f64()) };
                    self.stack.push(v);
                }
                Op::BinDiv => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    self.stack.push(Value::from_f64(a.as_f64() / b.as_f64()));
                }
                Op::BinEq => { let b = self.stack.pop().unwrap(); let a = self.stack.pop().unwrap(); self.stack.push(Value::from_bool(a == b)); }
                Op::BinNe => { let b = self.stack.pop().unwrap(); let a = self.stack.pop().unwrap(); self.stack.push(Value::from_bool(a != b)); }
                Op::BinLt => {
                    let b = self.stack.pop().unwrap(); let a = self.stack.pop().unwrap();
                    let r = if a.is_int() && b.is_int() { a.as_i32() < b.as_i32() } else { a.as_f64() < b.as_f64() };
                    self.stack.push(Value::from_bool(r));
                }
                Op::BinGt => {
                    let b = self.stack.pop().unwrap(); let a = self.stack.pop().unwrap();
                    let r = if a.is_int() && b.is_int() { a.as_i32() > b.as_i32() } else { a.as_f64() > b.as_f64() };
                    self.stack.push(Value::from_bool(r));
                }
                Op::BinLe => {
                    let b = self.stack.pop().unwrap(); let a = self.stack.pop().unwrap();
                    let r = if a.is_int() && b.is_int() { a.as_i32() <= b.as_i32() } else { a.as_f64() <= b.as_f64() };
                    self.stack.push(Value::from_bool(r));
                }
                Op::BinGe => {
                    let b = self.stack.pop().unwrap(); let a = self.stack.pop().unwrap();
                    let r = if a.is_int() && b.is_int() { a.as_i32() >= b.as_i32() } else { a.as_f64() >= b.as_f64() };
                    self.stack.push(Value::from_bool(r));
                }
                Op::Jump(off) => {
                    ip = (ip as i64 + *off as i64) as usize;
                    continue;
                }
                Op::JumpFalse(off) => {
                    let top = self.stack.pop().unwrap();
                    if !top.as_bool() {
                        ip = (ip as i64 + *off as i64) as usize;
                        continue;
                    }
                }
                Op::Exec(i, sudo) => {
                    let cmd = prog.pool.strings[*i as usize].clone();
                    let full = self.substitute(&cmd);
                    let status = if *sudo {
                        Command::new("sudo").arg("sh").arg("-c").arg(&full).status()
                    } else {
                        Command::new("sh").arg("-c").arg(&full).status()
                    };
                    if let Err(e) = status {
                        eprintln!("{} {}", "[x]".red(), e);
                    }
                }
                Op::Return | Op::Exit => break,
                Op::Call(_ , _) => {}, // not fully implemented in provided code
            }
            ip += 1;
            if self.gc.allocated > self.gc.threshold {
                // Cannot call collect here due to borrow rules — just bump threshold
                self.gc.threshold *= 2;
            }
        }
    }
}
// ═══════════════════════════════════════════════════════════
// Bytecode compiler
// ═══════════════════════════════════════════════════════════
struct Compiler {
    ops: Vec<Op>,
    pool: Pool,
    fns: HashMap<u16, usize>,
    fidx: u16,
}
impl Compiler {
    fn new() -> Self {
        Compiler { ops: Vec::new(), pool: Pool::new(), fns: HashMap::new(), fidx: 0 }
    }
    fn emit_nodes(&mut self, nodes: &[ProgramNode]) {
        for n in nodes { self.emit_node(n); }
    }
    fn emit_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.emit_stmt(stmt);
        }
    }
    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Raw { mode: _, cmd: s } => {
                let i = self.pool.str_idx(s.clone());
                self.ops.push(Op::Exec(i, false));
            }
            Stmt::AssignGlobal { key, val, .. } | Stmt::AssignLocal { key, val, .. } => {
                self.emit_expr(val);
                let slot = fnv16(key);
                self.ops.push(Op::StoreVar(slot));
            }
            Stmt::If { cond, body, else_ifs, else_body } => {
                self.emit_expr(cond);
                let mut patches: Vec<usize> = vec![];
                let patch = self.ops.len();
                self.ops.push(Op::JumpFalse(0));
                self.emit_stmts(body);
                let jump_out = self.ops.len();
                self.ops.push(Op::Jump(0));
                patches.push(jump_out);
                let target = self.ops.len() as i32 - patch as i32;
                if let Op::JumpFalse(ref mut off) = self.ops[patch] { *off = target; }
                for (cond, body) in else_ifs {
                    self.emit_expr(cond);
                    let patch = self.ops.len();
                    self.ops.push(Op::JumpFalse(0));
                    self.emit_stmts(body);
                    let jump_out = self.ops.len();
                    self.ops.push(Op::Jump(0));
                    patches.push(jump_out);
                    let target = self.ops.len() as i32 - patch as i32;
                    if let Op::JumpFalse(ref mut off) = self.ops[patch] { *off = target; }
                }
                if let Some(body) = else_body {
                    self.emit_stmts(body);
                }
                let out = self.ops.len() as i32;
                for p in patches {
                    let dist = out - p as i32 - 1;
                    if let Op::Jump(ref mut off) = self.ops[p] { *off = dist; }
                }
            }
            Stmt::While { cond, body } => {
                let loop_start = self.ops.len();
                self.emit_expr(cond);
                let patch = self.ops.len();
                self.ops.push(Op::JumpFalse(0));
                self.emit_stmts(body);
                let back = loop_start as i32 - self.ops.len() as i32;
                self.ops.push(Op::Jump(back));
                let exit = self.ops.len() as i32 - patch as i32;
                if let Op::JumpFalse(ref mut off) = self.ops[patch] { *off = exit; }
            }
            Stmt::For { var, iter, body } => {
                self.emit_expr(iter);
                let slot = fnv16(var);
                self.ops.push(Op::StoreVar(slot));
                self.emit_stmts(body);
            }
            Stmt::Function { name, body, .. } => {
                let fidx = self.fidx;
                self.fidx += 1;
                let skip_patch = self.ops.len();
                self.ops.push(Op::Jump(0));
                self.fns.insert(fidx, self.ops.len());
                self.emit_stmts(body);
                self.ops.push(Op::Return);
                let skip_off = self.ops.len() as i32 - skip_patch as i32;
                if let Op::Jump(ref mut off) = self.ops[skip_patch] { *off = skip_off; }
            }
            Stmt::Return { expr } => {
                self.emit_expr(expr);
                self.ops.push(Op::Return);
            }
            Stmt::Repeat { count, body } => {
                for _ in 0..*count { self.emit_stmts(body); }
            }
            Stmt::Background(nodes) => {
                self.emit_stmts(nodes);
            }
            _ => {}
        }
    }
    fn emit_node(&mut self, node: &ProgramNode) {
        match &node.content {
            Stmt::Raw { mode: _, cmd: s } => {
                let i = self.pool.str_idx(s.clone());
                self.ops.push(Op::Exec(i, node.is_sudo));
            }
            Stmt::AssignGlobal { key, val, .. } | Stmt::AssignLocal { key, val, .. } => {
                self.emit_expr(val);
                let slot = fnv16(key);
                self.ops.push(Op::StoreVar(slot));
            }
            Stmt::If { cond, body, else_ifs, else_body } => {
                self.emit_expr(cond);
                let mut patches: Vec<usize> = vec![];
                let patch = self.ops.len();
                self.ops.push(Op::JumpFalse(0));
                self.emit_stmts(body);
                let jump_out = self.ops.len();
                self.ops.push(Op::Jump(0));
                patches.push(jump_out);
                let target = self.ops.len() as i32 - patch as i32;
                if let Op::JumpFalse(ref mut off) = self.ops[patch] { *off = target; }
                for (cond, body) in else_ifs {
                    self.emit_expr(cond);
                    let patch = self.ops.len();
                    self.ops.push(Op::JumpFalse(0));
                    self.emit_stmts(body);
                    let jump_out = self.ops.len();
                    self.ops.push(Op::Jump(0));
                    patches.push(jump_out);
                    let target = self.ops.len() as i32 - patch as i32;
                    if let Op::JumpFalse(ref mut off) = self.ops[patch] { *off = target; }
                }
                if let Some(body) = else_body {
                    self.emit_stmts(body);
                }
                let out = self.ops.len() as i32;
                for p in patches {
                    let dist = out - p as i32 - 1;
                    if let Op::Jump(ref mut off) = self.ops[p] { *off = dist; }
                }
            }
            Stmt::While { cond, body } => {
                let loop_start = self.ops.len();
                self.emit_expr(cond);
                let patch = self.ops.len();
                self.ops.push(Op::JumpFalse(0));
                self.emit_stmts(body);
                let back = loop_start as i32 - self.ops.len() as i32;
                self.ops.push(Op::Jump(back));
                let exit = self.ops.len() as i32 - patch as i32;
                if let Op::JumpFalse(ref mut off) = self.ops[patch] { *off = exit; }
            }
            Stmt::For { var, iter, body } => {
                self.emit_expr(iter);
                let slot = fnv16(var);
                self.ops.push(Op::StoreVar(slot));
                self.emit_stmts(body);
            }
            Stmt::Function { name, body, .. } => {
                let fidx = self.fidx;
                self.fidx += 1;
                let skip_patch = self.ops.len();
                self.ops.push(Op::Jump(0));
                self.fns.insert(fidx, self.ops.len());
                self.emit_stmts(body);
                self.ops.push(Op::Return);
                let skip_off = self.ops.len() as i32 - skip_patch as i32;
                if let Op::Jump(ref mut off) = self.ops[skip_patch] { *off = skip_off; }
            }
            Stmt::Return { expr } => {
                self.emit_expr(expr);
                self.ops.push(Op::Return);
            }
            Stmt::Repeat { count, body } => {
                for _ in 0..*count { self.emit_stmts(body); }
            }
            Stmt::Background(nodes) => {
                self.emit_stmts(nodes);
            }
            _ => {}
        }
    }
    fn emit_expr(&mut self, expr: &hl_plsa::Expr) {
        use hl_plsa::Expr::*;
        use hl_plsa::Value as V;
        match expr {
            Lit(V::I32(n)) => { let i = self.pool.int_idx(*n); self.ops.push(Op::LoadInt(i)); }
            Lit(V::F64(f)) => { let i = self.pool.flt_idx(*f); self.ops.push(Op::LoadFlt(i)); }
            Lit(V::Str(s)) => { let i = self.pool.str_idx(s.clone()); self.ops.push(Op::LoadStr(i)); }
            Lit(V::Bool(b)) => self.ops.push(Op::LoadBool(*b)),
            Lit(_) => self.ops.push(Op::LoadNil),
            Var(name) => { let s = fnv16(name); self.ops.push(Op::LoadVar(s)); }
            BinOp { op, left, right } => {
                self.emit_expr(left);
                self.emit_expr(right);
                match op.as_str() {
                    "+" => self.ops.push(Op::BinAdd),
                    "-" => self.ops.push(Op::BinSub),
                    "*" => self.ops.push(Op::BinMul),
                    "/" => self.ops.push(Op::BinDiv),
                    "=="=> self.ops.push(Op::BinEq),
                    "!="=> self.ops.push(Op::BinNe),
                    "<" => self.ops.push(Op::BinLt),
                    ">" => self.ops.push(Op::BinGt),
                    "<=" => self.ops.push(Op::BinLe),
                    ">=" => self.ops.push(Op::BinGe),
                    _ => {}
                }
            }
            Call { name, args } => {
                for a in args { self.emit_expr(a); }
                let slot = fnv16(name);
                self.ops.push(Op::Call(slot, args.len() as u8));
            }
            _ => self.ops.push(Op::LoadNil),
        }
    }
    fn finish(self, ast: &AnalysisResult) -> Program {
        // Functions already emitted via emit_node for Function stmts in main_body
        Program { ops: self.ops, pool: self.pool, fns: self.fns }
    }
}
fn fnv16(s: &str) -> u16 {
    let mut h: u32 = 0x811c9dc5;
    for b in s.bytes() { h ^= b as u32; h = h.wrapping_mul(0x01000193); }
    h as u16
}
fn compile_ast(ast: &AnalysisResult) -> Program {
    let mut c = Compiler::new();
    c.emit_nodes(&ast.main_body);
    c.ops.push(Op::Exit);
    // emit top-level functions
    for (_name, (_params, _ret, body, _is_quick)) in &ast.functions {
        let fidx = c.fidx; c.fidx += 1;
        let skip = c.ops.len();
        c.ops.push(Op::Jump(0));
        c.fns.insert(fidx, c.ops.len());
        c.emit_nodes(body);
        c.ops.push(Op::Return);
        let off = c.ops.len() as i32 - skip as i32;
        if let Op::Jump(ref mut o) = c.ops[skip] { *o = off; }
    }
    c.finish(ast)
}
// ═══════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════
pub fn run_command(file: String, verbose: bool) -> bool {
    let _ = fs::create_dir_all(CACHE_DIR);
    let hash = hash_file(&file);
    let bcpath = PathBuf::from(CACHE_DIR).join(format!("{}.bc", hash));
    let prog: Program = if bcpath.exists() {
        if verbose { println!("{} Cache hit", "[*]".green()); }
        match fs::read(&bcpath).ok()
        .and_then(|b| bincode::deserialize::<Program>(&b).ok())
        {
            Some(p) => p,
            None => {
                if verbose { eprintln!("{} Cache corrupt, recompiling", "[!]".yellow()); }
                build_and_cache(&file, &bcpath, verbose)
            }
        }
    } else {
        build_and_cache(&file, &bcpath, verbose)
    };
    let mut vm = VM::new();
    let start = Instant::now();
    vm.run(&prog);
    if verbose { println!("{} Done in {:?}", "[*]".blue(), start.elapsed()); }
    true
}
fn build_and_cache(file: &str, bcpath: &PathBuf, verbose: bool) -> Program {
    if verbose { println!("{} Compiling {}", "[*]".yellow(), file); }
    let mut seen = HashSet::new();
    let ast = match hl_plsa::parse_file(file, true, verbose, &mut seen) {
        Ok(a) => a,
        Err(e) => { for err in e { eprintln!("{:?}", err); } std::process::exit(1); }
    };
    if verbose && ast.is_potentially_unsafe {
        eprintln!("{} Script has privileged commands", "[!]".yellow());
    }
    let prog = compile_ast(&ast);
    if let Ok(b) = bincode::serialize(&prog) { let _ = fs::write(bcpath, b); }
    prog
}
fn hash_file(path: &str) -> String {
    let b = fs::read(path).unwrap_or_default();
    let mut h = Sha256::new(); h.update(&b);
    hex::encode(h.finalize())
}
