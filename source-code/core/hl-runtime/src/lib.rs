use clap::Parser;
use colored::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;
use std::hash::{Hash, Hasher};
use dynasmrt::{dynasm, DynasmApi, DynasmLabelApi, AssemblyOffset};
use std::ptr;
use std::mem;
use std::alloc::{alloc, dealloc, Layout};
use hex;

const CACHE_DIR: &str = "/tmp/Hacker-Lang/cache";
const PLSA_BIN_NAME: &str = "hl-plsa";

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    file: String,
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    jit: bool, // New flag to enable JIT (desktop only)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandType {
    Raw(String),
    AssignEnv { key: String, val: String },
    AssignLocal { key: String, val: String },
    Loop { count: u64, cmd: String },
    If { cond: String, cmd: String },
    Background(String),
    Plugin { name: String, is_super: bool },
    // Expanded for new features (parsed as strings, evaluated in VM)
    AssignTyped { key: String, ty: String, val: String },
    Function { name: String, params: Vec<(String, String)>, ret_ty: Option<String>, body: Vec<ProgramNode> },
    Return { expr: String },
    Object { name: String, fields: Vec<(bool, String, String, Option<String>)>, methods: HashMap<String, Vec<ProgramNode>> },
    Try { body: Vec<ProgramNode>, catches: Vec<(String, String, Vec<ProgramNode>)>, finally: Option<Vec<ProgramNode>> },
    Match { expr: String, arms: Vec<(String, String)> },
    For { var: String, iter: String, body: Vec<ProgramNode> },
    While { cond: String, body: Vec<ProgramNode> },
    Break,
    Continue,
    Pipe { left: String, right: String },
    Import { prefix: String, name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramNode {
    pub line_num: usize,
    pub is_sudo: bool,
    pub content: CommandType,
    pub original_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub deps: Vec<String>,
    pub libs: Vec<String>,
    pub functions: HashMap<String, Vec<ProgramNode>>,
    pub main_body: Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings: Vec<String>,
    // New: objects and modules
    pub objects: HashMap<String, Vec<ProgramNode>>,
    pub modules: HashMap<String, Vec<ProgramNode>>,
}

// --- NaN-Boxed Value ---
#[repr(transparent)]
#[derive(Copy, Clone)]
struct Value(u64);

const NAN_BITS: u64 = 0x7FF8_0000_0000_0000;
const TAG_INT: u64 = NAN_BITS | (1 << 48);
const TAG_PTR: u64 = NAN_BITS | (2 << 48);
const TAG_BOOL: u64 = NAN_BITS | (3 << 48);
const TAG_NIL: u64 = NAN_BITS | (4 << 48);
const TAG_STR: u64 = NAN_BITS | (5 << 48);
const TAG_LIST: u64 = NAN_BITS | (6 << 48);
const TAG_MAP: u64 = NAN_BITS | (7 << 48);
const TAG_OBJ: u64 = NAN_BITS | (8 << 48);
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

impl Value {
    fn from_f64(f: f64) -> Self {
        let bits = f.to_bits();
        if (bits & NAN_BITS) == NAN_BITS {
            Value(bits | NAN_BITS)
        } else {
            Value(bits)
        }
    }

    fn from_i32(i: i32) -> Self {
        Value(TAG_INT | ((i as u64) & PAYLOAD_MASK))
    }

    fn from_bool(b: bool) -> Self {
        Value(TAG_BOOL | (if b { 1 } else { 0 }))
    }

    fn from_nil() -> Self {
        Value(TAG_NIL)
    }

    fn from_gc_ptr(p: *mut GcHeader) -> Self {
        Value(TAG_PTR | ((p as u64) & PAYLOAD_MASK))
    }

    fn is_f64(&self) -> bool {
        (self.0 & NAN_BITS) != NAN_BITS
    }

    fn is_int(&self) -> bool {
        (self.0 & !PAYLOAD_MASK) == TAG_INT
    }

    fn is_bool(&self) -> bool {
        (self.0 & !PAYLOAD_MASK) == TAG_BOOL
    }

    fn is_nil(&self) -> bool {
        (self.0 & !PAYLOAD_MASK) == TAG_NIL
    }

    fn is_gc_ptr(&self) -> bool {
        (self.0 & !PAYLOAD_MASK) == TAG_PTR
    }

    fn as_f64(&self) -> f64 {
        f64::from_bits(self.0)
    }

    fn as_i32(&self) -> i32 {
        ((self.0 & PAYLOAD_MASK) as i64) as i32 // Sign extend if needed
    }

    fn as_bool(&self) -> bool {
        (self.0 & 1) == 1
    }

    fn as_gc_ptr(&self) -> *mut GcHeader {
        (self.0 & PAYLOAD_MASK) as *mut GcHeader
    }

    fn points_to_young(&self) -> bool {
        if self.is_gc_ptr() {
            unsafe { (*self.as_gc_ptr()).gen == 0 }
        } else {
            false
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if self.is_int() && other.is_int() {
            self.as_i32() == other.as_i32()
        } else if self.is_f64() && other.is_f64() {
            self.as_f64() == other.as_f64()
        } else if self.is_bool() && other.is_bool() {
            self.as_bool() == other.as_bool()
        } else if self.is_nil() && other.is_nil() {
            true
        } else if self.is_gc_ptr() && other.is_gc_ptr() {
            let a = self.as_gc_ptr();
            let b = other.as_gc_ptr();
            unsafe {
                if (*a).kind != (*b).kind {
                    return false;
                }
                match (*a).kind {
                    0 => { // String
                        let sa = (a as *mut u8).add(mem::size_of::<GcHeader>()) as *mut GcString;
                        let sb = (b as *mut u8).add(mem::size_of::<GcHeader>()) as *mut GcString;
                        let stra = std::slice::from_raw_parts((*sa).data, (*sa).len);
                        let strb = std::slice::from_raw_parts((*sb).data, (*sb).len);
                        stra == strb
                    }
                    1 => { // List
                        let la = (a as *mut u8).add(mem::size_of::<GcHeader>()) as *mut GcList;
                        let lb = (b as *mut u8).add(mem::size_of::<GcHeader>()) as *mut GcList;
                        (*la).items == (*lb).items
                    }
                    // Similarly for Map, Obj
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
    fn hash<H: Hasher>(&self, state: &mut H) {
        if self.is_int() {
            self.as_i32().hash(state)
        } else if self.is_gc_ptr() {
            unsafe {
                if (*self.as_gc_ptr()).kind == 0 { // String
                    let s = (self.as_gc_ptr() as *mut u8).add(mem::size_of::<GcHeader>()) as *mut GcString;
                    let slice = std::slice::from_raw_parts((*s).data, (*s).len);
                    slice.hash(state);
                }
            }
        } else if self.is_bool() {
            self.as_bool().hash(state);
        } else {
            panic!("Type not hashable")
        }
    }
}

// --- GC Structures ---
const BLOCK_SIZE: usize = 32 * 1024;

struct Block {
    data: Box<[u8; BLOCK_SIZE]>,
    offset: usize,
}

struct BumpArena {
    blocks: Vec<Box<Block>>,
    current: usize,
}

impl BumpArena {
    fn new() -> Self {
        let mut arena = BumpArena {
            blocks: Vec::new(),
            current: 0,
        };
        arena.add_block();
        arena
    }

    fn add_block(&mut self) {
        let block = Box::new(Block {
            data: Box::new([0u8; BLOCK_SIZE]),
                             offset: 0,
        });
        self.blocks.push(block);
    }

    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        let mut offset = (self.blocks[self.current].offset + align - 1) & !(align - 1);
        if offset + size > BLOCK_SIZE {
            self.current += 1;
            if self.current >= self.blocks.len() {
                self.add_block();
            }
            offset = 0;
            self.blocks[self.current].offset = offset + size;
        } else {
            self.blocks[self.current].offset = offset + size;
        }
        // Fix: unsafe add
        unsafe { self.blocks[self.current].data.as_mut_ptr().add(offset) }
    }

    fn reset(&mut self) {
        for block in &mut self.blocks {
            block.offset = 0;
        }
        self.current = 0;
    }
}

struct FreeListAllocator {
    free_list: *mut GcHeader,
}

impl FreeListAllocator {
    fn new() -> Self {
        FreeListAllocator { free_list: ptr::null_mut() }
    }

    fn alloc(&mut self, size: usize) -> *mut u8 {
        // For now, use global alloc, implement free list later
        unsafe { alloc(Layout::from_size_align_unchecked(size, 8)) }
    }

    fn free(&mut self, ptr: *mut GcHeader) {
        unsafe {
            let layout = Layout::from_size_align_unchecked((*ptr).size as usize + mem::size_of::<GcHeader>(), 8);
            dealloc(ptr as *mut u8, layout);
        }
    }
}

#[repr(C)]
struct GcHeader {
    mark: u8,        // 0=white, 1=gray, 2=black
    gen: u8,         // 0=young, 1=old
    kind: u8,        // 0=String, 1=List, 2=Map, 3=Object
    size: u32,       // payload size
    next: *mut GcHeader,
}

#[repr(C)]
struct GcString {
    header: GcHeader,
    len: usize,
    data: *mut u8, // Allocated separately or inlined
}

#[repr(C)]
struct GcList {
    header: GcHeader,
    items: Vec<Value>,
}

#[repr(C)]
struct GcMap {
    header: GcHeader,
    map: HashMap<Value, Value>,
}

#[repr(C)]
struct GcObject {
    header: GcHeader,
    obj: Object,
}

struct Gc {
    // Young generation — bump arena
    young: BumpArena,
    // Old generation — free list allocator
    old: FreeListAllocator,
    // Wszystkie obiekty (linked list przez GcHeader.next)
    all_objects: *mut GcHeader,
    // Gray stack dla incremental mark
    gray_stack: Vec<*mut GcHeader>,
    // Write barrier log (card table)
    write_barriers: Vec<*mut GcHeader>,
    // Liczniki
    bytes_allocated: usize,
    gc_threshold: usize,  // trigger GC co N bajtów
}

impl Gc {
    fn new(threshold: usize) -> Self {
        Gc {
            young: BumpArena::new(),
            old: FreeListAllocator::new(),
            all_objects: ptr::null_mut(),
            gray_stack: Vec::new(),
            write_barriers: Vec::new(),
            bytes_allocated: 0,
            gc_threshold: threshold,
        }
    }

    fn alloc(&mut self, size: usize, kind: u8, gen: u8) -> *mut GcHeader {
        if self.bytes_allocated > self.gc_threshold {
            // Cannot call collect(vm) here due to borrow rules.
            // For now, we just grow. In a real impl, we would need to restructure VM/GC.
            self.gc_threshold *= 2;
        }
        let total_size = size + mem::size_of::<GcHeader>();
        let ptr = if gen == 0 {
            self.young.alloc(unsafe { Layout::from_size_align_unchecked(total_size, 8) }) as *mut GcHeader
        } else {
            self.old.alloc(total_size) as *mut GcHeader
        };
        unsafe {
            (*ptr).mark = 0;
            (*ptr).gen = gen;
            (*ptr).kind = kind;
            (*ptr).size = size as u32;
            (*ptr).next = self.all_objects;
        }
        self.all_objects = ptr;
        self.bytes_allocated += total_size;
        ptr
    }

    fn alloc_string(&mut self, s: String, gen: u8) -> Value {
        let len = s.len();
        let size = mem::size_of::<GcString>() + len - mem::size_of::<*mut u8>(); // Since data is pointer
        let header = self.alloc(size, 0, gen);
        let gcs = unsafe { &mut *(header as *mut GcString) };
        gcs.len = len;
        gcs.data = unsafe { alloc(Layout::from_size_align_unchecked(len, 1)) };
        unsafe { ptr::copy_nonoverlapping(s.as_ptr(), gcs.data, len); }
        Value::from_gc_ptr(header)
    }

    fn alloc_list(&mut self, list: Vec<Value>, gen: u8) -> Value {
        let size = mem::size_of::<GcList>();
        let header = self.alloc(size, 1, gen);
        let gcl = unsafe { &mut *(header as *mut GcList) };
        gcl.items = list;
        Value::from_gc_ptr(header)
    }

    fn alloc_map(&mut self, map: HashMap<Value, Value>, gen: u8) -> Value {
        let size = mem::size_of::<GcMap>();
        let header = self.alloc(size, 2, gen);
        let gcm = unsafe { &mut *(header as *mut GcMap) };
        gcm.map = map;
        Value::from_gc_ptr(header)
    }

    fn alloc_object(&mut self, obj: Object, gen: u8) -> Value {
        let size = mem::size_of::<GcObject>();
        let header = self.alloc(size, 3, gen);
        let gco = unsafe { &mut *(header as *mut GcObject) };
        gco.obj = obj;
        Value::from_gc_ptr(header)
    }

    fn mark_roots(&mut self, vm: &VM) {
        // Stack
        for val in &vm.stack {
            if val.is_gc_ptr() {
                self.mark_gray(val.as_gc_ptr());
            }
        }
        // Globals
        for (_, val) in &vm.globals {
            if val.is_gc_ptr() {
                self.mark_gray(val.as_gc_ptr());
            }
        }
        // Call stack locals
        for frame in &vm.call_stack {
            for &val in &frame.locals {
                if val.is_gc_ptr() {
                    self.mark_gray(val.as_gc_ptr());
                }
            }
        }
        // Write barriers for generational
        let barriers = self.write_barriers.clone();
        self.write_barriers.clear();
        for obj in barriers {
            self.mark_gray(obj);
        }
    }

    fn mark_gray(&mut self, obj: *mut GcHeader) {
        unsafe {
            if (*obj).mark == 0 {
                (*obj).mark = 1;
                self.gray_stack.push(obj);
            }
        }
    }

    fn trace_children(&mut self, obj: *mut GcHeader) {
        unsafe {
            match (*obj).kind {
                0 => {} // String has no children
                1 => { // List
                    let list = obj as *mut GcList;
                    for val in &(*list).items {
                        if val.is_gc_ptr() {
                            self.mark_gray(val.as_gc_ptr());
                        }
                    }
                }
                2 => { // Map
                    let map = obj as *mut GcMap;
                    for (k, v) in &(*map).map {
                        if k.is_gc_ptr() {
                            self.mark_gray(k.as_gc_ptr());
                        }
                        if v.is_gc_ptr() {
                            self.mark_gray(v.as_gc_ptr());
                        }
                    }
                }
                3 => { // Object
                    let o = obj as *mut GcObject;
                    for (_, v) in &(*o).obj.fields {
                        if v.is_gc_ptr() {
                            self.mark_gray(v.as_gc_ptr());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn mark_phase(&mut self) {
        while let Some(obj) = self.gray_stack.pop() {
            unsafe {
                (*obj).mark = 2; // black
                self.trace_children(obj);
            }
        }
    }

    fn sweep_phase(&mut self) {
        let mut prev: *mut *mut GcHeader = &mut self.all_objects;
        let mut current = self.all_objects;
        while !current.is_null() {
            unsafe {
                let next = (*current).next;
                if (*current).mark == 0 {
                    // Białe = nieosiągalne, zwolnij
                    *prev = next;
                    self.free(current);
                } else {
                    (*current).mark = 0; // reset na biały dla następnego cyklu
                    prev = &mut (*current).next;
                }
                current = next;
            }
        }
    }

    fn free(&mut self, obj: *mut GcHeader) {
        unsafe {
            if (*obj).kind == 0 {
                let s = obj as *mut GcString;
                dealloc((*s).data, Layout::from_size_align_unchecked((*s).len, 1));
            }
            // Drop Vec, HashMap etc.
            if (*obj).kind == 1 {
                let l = obj as *mut GcList;
                ptr::drop_in_place(&mut (*l).items);
            } else if (*obj).kind == 2 {
                let m = obj as *mut GcMap;
                ptr::drop_in_place(&mut (*m).map);
            } else if (*obj).kind == 3 {
                let o = obj as *mut GcObject;
                ptr::drop_in_place(&mut (*o).obj);
            }
            let size = (*obj).size as usize + mem::size_of::<GcHeader>();
            if (*obj).gen == 1 {
                self.old.free(obj);
            } // Young freed by reset
            self.bytes_allocated -= size;
        }
    }

    fn collect(&mut self, vm: &VM) {
        self.mark_roots(vm);
        self.mark_phase();
        self.sweep_phase();
        // Optional compact
        self.young.reset();
    }

    fn write_barrier(&mut self, obj: *mut GcHeader, val: Value) {
        unsafe {
            if (*obj).gen == 1 && val.points_to_young() {
                self.write_barriers.push(obj);
            }
        }
    }
}

// --- Object ---
#[derive(Clone)]
struct Object {
    name: String,
    fields: HashMap<String, Value>,
    methods: HashMap<String, usize>, // method name to func index
}

// --- Constant Pool ---
// Fix: Add Serialize, Deserialize
#[derive(Serialize, Deserialize)]
struct ConstPool {
    strings: Vec<String>,
    numbers_i32: Vec<i32>,
    numbers_f64: Vec<f64>,
}

impl ConstPool {
    fn new() -> Self {
        ConstPool {
            strings: Vec::new(),
            numbers_i32: Vec::new(),
            numbers_f64: Vec::new(),
        }
    }

    fn add_string(&mut self, s: String) -> u32 {
        if let Some(idx) = self.strings.iter().position(|existing| *existing == s) {
            idx as u32
        } else {
            let idx = self.strings.len() as u32;
            self.strings.push(s);
            idx
        }
    }

    fn add_i32(&mut self, i: i32) -> u32 {
        if let Some(idx) = self.numbers_i32.iter().position(|existing| *existing == i) {
            idx as u32
        } else {
            let idx = self.numbers_i32.len() as u32;
            self.numbers_i32.push(i);
            idx
        }
    }

    fn add_f64(&mut self, f: f64) -> u32 {
        if let Some(idx) = self.numbers_f64.iter().position(|existing| *existing == f) {
            idx as u32
        } else {
            let idx = self.numbers_f64.len() as u32;
            self.numbers_f64.push(f);
            idx
        }
    }
}

// --- Bytecode ---
#[derive(Debug, Clone, Serialize, Deserialize)]
enum OpCode {
    LoadConst(u32),              // indeks do ConstPool
    LoadVar(u16),                // indeks do tablicy zmiennych
    StoreVar(u16),
    BinAdd,                      // osobne opkody
    BinSub,
    BinMul,
    BinDiv,
    BinEq,
    BinLt,
    Call(u16, u8),               // func_idx, num_args
    Jump(i32),                   // względny offset
    JumpIfFalse(i32),
    Return,
    Exit,
    Exec(u32, bool),             // cmd_idx w ConstPool, sudo
}

#[derive(Serialize, Deserialize)]
struct BytecodeProgram {
    ops: Vec<OpCode>,
    const_pool: ConstPool,
    functions: HashMap<u16, usize>, // func idx -> op index
    // For variables, assume per frame indices assigned by compiler
}

// --- IR for Tracing JIT ---
#[derive(Clone)]
enum IROp {
    ConstI32(i32),
    ConstF64(f64),
    ConstBool(bool),
    ConstNil,
    Add(IrRef, IrRef),
    Sub(IrRef, IrRef),
    Mul(IrRef, IrRef),
    Guard(IrRef, GuardKind, usize), // value, kind, deopt ip
    LoadVar(u16),
    StoreVar(u16, IrRef),
    // More ops
}

type IrRef = usize;

#[derive(Clone)]
enum GuardKind {
    IsInt,
    IsF64,
    IsBool,
    // etc
}

struct TraceRecorder {
    ir: Vec<IROp>,
    var_map: HashMap<u16, IrRef>,
    loop_start_ip: usize,
    is_recording: bool,
    hot_count: HashMap<usize, u32>, // ip -> count
}

impl TraceRecorder {
    fn new() -> Self {
        TraceRecorder {
            ir: Vec::new(),
            var_map: HashMap::new(),
            loop_start_ip: 0,
            is_recording: false,
            hot_count: HashMap::new(),
        }
    }
}

// --- Register Allocation ---
#[derive(Clone, Copy, PartialEq, Eq)]
enum X64Reg {
    Rax = 0, Rbx, Rcx, Rdx, Rsi, Rdi, Rbp, Rsp,
    R8, R9, R10, R11, R12, R13, R14, R15,
}

struct RegAlloc {
    live_intervals: Vec<(IrRef, usize, usize)>, // ref, start, end
    reg_map: HashMap<IrRef, X64Reg>,
    free_regs: Vec<X64Reg>,
}

impl RegAlloc {
    fn new() -> Self {
        let free_regs = vec![X64Reg::Rbx, X64Reg::Rcx, X64Reg::Rdx, X64Reg::Rsi, X64Reg::Rdi,
        X64Reg::R8, X64Reg::R9, X64Reg::R10, X64Reg::R11, X64Reg::R12, X64Reg::R13, X64Reg::R14, X64Reg::R15];
        RegAlloc {
            live_intervals: Vec::new(),
            reg_map: HashMap::new(),
            free_regs,
        }
    }

    fn allocate(&mut self, ir: &[IROp]) {
        // Simplistic linear scan
        // Collect intervals (dummy)
        for i in 0..ir.len() {
            self.live_intervals.push((i, i, ir.len()));
        }
        self.live_intervals.sort_by_key(|&(_, start, _)| start);
        let mut active = Vec::new();
        for (iref, start, end) in self.live_intervals.clone() {
            active.retain(|&(_, a_end)| a_end > start);
            if let Some(reg) = self.free_regs.pop() {
                self.reg_map.insert(iref, reg);
                active.push((iref, end));
            } // Else spill, ignore
        }
    }
}

// --- VmState ---
#[repr(C)]
struct VmState {
    stack_ptr: *mut Value,
    stack_base: *mut Value,
    locals_ptr: *mut Value,
    globals_ptr: *mut HashMap<u16, Value>, // Update to u16 indices if needed
}

// --- CompiledTrace ---
struct CompiledTrace {
    _buf: dynasmrt::ExecutableBuffer,  // owner
    fn_ptr: unsafe fn(*mut VmState),   // przekazuje wskaźnik na VM state
    entry_ip: usize,
    exit_ip: usize,
}

// --- JitCache ---
struct JitCache {
    compiled: HashMap<usize, CompiledTrace>,
}

// --- CallFrame ---
struct CallFrame {
    locals: [Value; 256],        // fixed array
    base_ip: usize,
    return_ip: usize,
}

// --- VM ---
struct VM {
    stack: Vec<Value>,
    call_stack: Vec<CallFrame>,
    globals: HashMap<String, Value>,
    env: HashMap<String, String>,
    gc: Gc,
    recorder: TraceRecorder,
    jit_cache: JitCache,
}

impl VM {
    fn new() -> Self {
        VM {
            stack: Vec::new(),
            call_stack: Vec::new(),
            globals: HashMap::new(),
            env: std::env::vars().collect(),
            gc: Gc::new(1024 * 1024),
            recorder: TraceRecorder::new(),
            jit_cache: JitCache { compiled: HashMap::new() },
        }
    }

    fn start_recording(&mut self, ip: usize) {
        self.recorder.is_recording = true;
        self.recorder.loop_start_ip = ip;
        self.recorder.ir.clear();
    }

    fn stop_recording(&mut self) -> Vec<IROp> {
        self.recorder.is_recording = false;
        self.recorder.ir.clone()
    }

    fn optimize_ir(&mut self, ir: &mut Vec<IROp>) {
        // Placeholder for const folding, DCE, etc.
    }

    fn codegen(&mut self, ir: Vec<IROp>, start_ip: usize) {
        let mut regalloc = RegAlloc::new();
        regalloc.allocate(&ir);
        let mut ops = dynasmrt::x64::Assembler::new().unwrap();
        // Generate assembly using dynasm
        // Example placeholder
        dynasm!(ops
        ; .arch x64
        ; mov rax, 42
        ; ret
        );
        let buf = ops.finalize().unwrap();
        let fn_ptr = unsafe { std::mem::transmute::<_, unsafe fn(*mut VmState)>(buf.ptr(AssemblyOffset(0))) };
        self.jit_cache.compiled.insert(start_ip, CompiledTrace {
            _buf: buf,
            fn_ptr,
            entry_ip: start_ip,
            exit_ip: start_ip + 1, // dummy
        });
    }

    fn run_interpreted(&mut self, prog: &BytecodeProgram, verbose: bool) {
        let mut ip = 0;
        while ip < prog.ops.len() {
            // Hot count for loops
            // Placeholder
            match &prog.ops[ip] {
                OpCode::LoadConst(idx) => {
                    // Assume it's string for example
                    let s = prog.const_pool.strings[*idx as usize].clone();
                    let val = self.gc.alloc_string(s, 0);
                    self.stack.push(val);
                }
                OpCode::Exec(idx, sudo) => {
                    let cmd = prog.const_pool.strings[*idx as usize].clone();
                    let final_cmd = self.substitute(&cmd);
                    let status = if *sudo {
                        Command::new("sudo").arg("sh").arg("-c").arg(&final_cmd).status()
                    } else {
                        Command::new("sh").arg("-c").arg(&final_cmd).status()
                    };
                    if let Err(e) = status {
                        eprintln!("Command failed: {}", e);
                    }
                }
                OpCode::BinAdd => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    let result = if a.is_int() && b.is_int() {
                        Value::from_i32(a.as_i32() + b.as_i32())
                    } else if a.is_f64() && b.is_f64() {
                        Value::from_f64(a.as_f64() + b.as_f64())
                    } else {
                        panic!("Type error");
                    };
                    self.stack.push(result);
                }
                // Implement other ops
                _ => {}
            }
            ip += 1;
        }
    }

    fn run(&mut self, prog: BytecodeProgram, verbose: bool, use_jit: bool) {
        self.run_interpreted(&prog, verbose);
        // Add JIT logic
    }

    fn substitute(&self, text: &str) -> String {
        let mut res = text.to_string();
        for (k, v) in &self.globals {
            if v.is_gc_ptr() && unsafe { (*v.as_gc_ptr()).kind == 0 } {
                let s = unsafe { &*(v.as_gc_ptr() as *const GcString) };
                let str_slice = unsafe { std::slice::from_raw_parts(s.data, s.len) };
                let str_val = std::string::String::from_utf8_lossy(str_slice).to_string();
                res = res.replace(&format!("@{}", k), &str_val);
            }
            // Add for other types
        }
        for (k, v) in &self.env {
            res = res.replace(&format!("${}", k), v);
        }
        res
    }
}

// --- Compiler ---
fn compile_to_bytecode(ast: &AnalysisResult) -> BytecodeProgram {
    let mut ops = Vec::new();
    let mut const_pool = ConstPool::new();
    let mut functions = HashMap::new();
    // Compile main body
    for node in &ast.main_body {
        compile_node(node, &mut ops, &mut const_pool, &mut functions);
    }
    ops.push(OpCode::Exit);
    // Compile functions
    let mut func_idx = 0u16;
    for (name, nodes) in &ast.functions {
        functions.insert(func_idx, ops.len());
        func_idx += 1;
        for node in nodes {
            compile_node(node, &mut ops, &mut const_pool, &mut functions);
        }
        ops.push(OpCode::Return);
    }
    // Similar for objects
    BytecodeProgram {
        ops,
        const_pool,
        functions,
    }
}

fn compile_node(node: &ProgramNode, ops: &mut Vec<OpCode>, const_pool: &mut ConstPool, functions: &mut HashMap<u16, usize>) {
    match &node.content {
        CommandType::Raw(s) => {
            if s.starts_with("call:") {
                let fname = s.strip_prefix("call:").unwrap().to_string();
                // Find func idx
                // Placeholder
                ops.push(OpCode::Call(0, 0));
            } else {
                let idx = const_pool.add_string(s.clone());
                ops.push(OpCode::Exec(idx, node.is_sudo));
            }
        }
        // Implement other command types
        _ => {}
    }
}

// --- Main functions ---
pub fn run_command(file_path: String, verbose: bool) -> bool {
    if let Err(_) = fs::create_dir_all(CACHE_DIR) {
        // Ignored
    }
    let hash = get_file_hash(&file_path);
    let bc_path = PathBuf::from(CACHE_DIR).join(format!("{}.bc", hash));
    let program: BytecodeProgram;
    if bc_path.exists() {
        if verbose { println!("{} Cache hit. Loading bytecode.", "[*]".green()); }
        let data = fs::read(&bc_path).unwrap();
        program = bincode::deserialize(&data).unwrap();
    } else {
        program = generate_bytecode(&file_path, verbose);
        let bc_data = bincode::serialize(&program).unwrap();
        fs::write(bc_path, bc_data).unwrap();
    }
    let mut vm = VM::new();
    let start = Instant::now();
    vm.run(program, verbose, false); // JIT disabled by default in run_command
    if verbose {
        println!("{} Execution time: {:?}", "[INFO]".blue(), start.elapsed());
    }
    true
}

fn get_plsa_path() -> PathBuf {
    let home = dirs::home_dir().expect("Failed to determine home directory");
    let path = home.join(".hackeros/hacker-lang/bin").join(PLSA_BIN_NAME);
    if !path.exists() {
        eprintln!("{} Critical Error: {} not found at {:?}", "[x]".red(), PLSA_BIN_NAME, path);
        std::process::exit(127);
    }
    path
}

fn get_file_hash(path: &str) -> String {
    let bytes = fs::read(path).unwrap_or(vec![]);
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn generate_bytecode(file_path: &str, verbose: bool) -> BytecodeProgram {
    if verbose { println!("{} Cache miss. Analyzing source.", "[*]".yellow()); }
    let plsa_path = get_plsa_path();
    let output = Command::new(&plsa_path)
    .arg(file_path)
    .arg("--json")
    .arg("--resolve-libs")
    .output()
    .expect(&format!("Failed to run hl-plsa at {:?}", plsa_path));
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        std::process::exit(1);
    }
    let ast: AnalysisResult = serde_json::from_slice(&output.stdout).expect("Invalid JSON from PLSA");
    if verbose && ast.is_potentially_unsafe {
        println!("{} Warning: Script contains privileged commands.", "[!]".yellow());
    }
    compile_to_bytecode(&ast)
}
