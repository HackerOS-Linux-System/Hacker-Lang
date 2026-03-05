use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CACHE_SCHEMA_VERSION: u32 = 7;

// ─────────────────────────────────────────────────────────────
// StringPool — intern wszystkich literałów → u32 ID
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StringPool {
    pub strings: Vec<String>,
    #[serde(skip)]
    pub index: HashMap<String, u32>,
}

impl StringPool {
    pub fn new() -> Self {
        Self {
            strings: Vec::with_capacity(256),
            index:   HashMap::with_capacity(256),
        }
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.index.get(s) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index.insert(s.to_string(), id);
        id
    }

    #[inline(always)]
    pub fn get(&self, id: u32) -> &str {
        unsafe { self.strings.get_unchecked(id as usize) }
    }

    pub fn rebuild_index(&mut self) {
        self.index.clear();
        self.index.reserve(self.strings.len());
        for (i, s) in self.strings.iter().enumerate() {
            self.index.insert(s.clone(), i as u32);
        }
    }
}

// ─────────────────────────────────────────────────────────────
// CmpOp — operator porównania
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CmpOp { Eq, Ne, Lt, Le, Gt, Ge }

impl CmpOp {
    pub fn as_str(self) -> &'static str {
        match self {
            CmpOp::Eq => "==", CmpOp::Ne => "!=",
            CmpOp::Lt => "<",  CmpOp::Le => "<=",
            CmpOp::Gt => ">",  CmpOp::Ge => ">=",
        }
    }

    /// Ewaluacja integer bez shellu
    #[inline(always)]
    pub fn eval_i(self, a: i64, b: i64) -> bool {
        match self {
            CmpOp::Eq => a == b, CmpOp::Ne => a != b,
            CmpOp::Lt => a <  b, CmpOp::Le => a <= b,
            CmpOp::Gt => a >  b, CmpOp::Ge => a >= b,
        }
    }

    /// Ewaluacja float bez shellu
    #[inline(always)]
    pub fn eval_f(self, a: f64, b: f64) -> bool {
        match self {
            CmpOp::Eq => (a - b).abs() < f64::EPSILON,
            CmpOp::Ne => (a - b).abs() >= f64::EPSILON,
            CmpOp::Lt => a <  b, CmpOp::Le => a <= b,
            CmpOp::Gt => a >  b, CmpOp::Ge => a >= b,
        }
    }
}

// ─────────────────────────────────────────────────────────────
// HlValue — natywna wartość VM (rejestr typowany)
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", content = "v")]
pub enum HlValue {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    /// Lista wartości (np. z list_lit [...])
    List(Vec<HlValue>),
    /// Mapa klucz-wartość (np. z map_lit {...})
    Map(Vec<(String, HlValue)>),
    /// Domknięcie / lambda: lista parametrów + ciało jako string do eval
    Lambda { params: Vec<String>, body: String },
    /// Wariant ADT: nazwa wariantu + opcjonalne pola
    Variant { tag: String, fields: Vec<(String, HlValue)> },
    Nil,
}

impl HlValue {
    /// Konwersja do stringa dla setenv/sync z shellem
    pub fn to_env_string(&self) -> String {
        match self {
            HlValue::Int(n)     => n.to_string(),
            HlValue::Float(f)   => format!("{:.}", f),
            HlValue::Str(s)     => s.clone(),
            HlValue::Bool(b)    => b.to_string(),
            HlValue::List(v)    => v.iter()
            .map(|x| x.to_env_string())
            .collect::<Vec<_>>()
            .join("\n"),
            HlValue::Map(kv)    => kv.iter()
            .map(|(k, v)| format!("{}={}", k, v.to_env_string()))
            .collect::<Vec<_>>()
            .join("\n"),
            HlValue::Lambda { .. }  => String::from("<lambda>"),
            HlValue::Variant { tag, .. } => tag.clone(),
            HlValue::Nil            => String::new(),
        }
    }

    #[inline(always)]
    pub fn as_int(&self) -> i64 {
        match self {
            HlValue::Int(n)   => *n,
            HlValue::Float(f) => *f as i64,
            HlValue::Bool(b)  => if *b { 1 } else { 0 },
            HlValue::Str(s)   => s.parse().unwrap_or(0),
            _                  => 0,
        }
    }

    #[inline(always)]
    pub fn as_float(&self) -> f64 {
        match self {
            HlValue::Float(f) => *f,
            HlValue::Int(n)   => *n as f64,
            HlValue::Bool(b)  => if *b { 1.0 } else { 0.0 },
            HlValue::Str(s)   => s.parse().unwrap_or(0.0),
            _                  => 0.0,
        }
    }

    #[inline(always)]
    pub fn as_bool(&self) -> bool {
        match self {
            HlValue::Bool(b)  => *b,
            HlValue::Int(n)   => *n != 0,
            HlValue::Float(f) => *f != 0.0,
            HlValue::Str(s)   => !s.is_empty() && s != "false" && s != "0",
            HlValue::Nil      => false,
            _                  => true,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            HlValue::Int(_)     => "int",
            HlValue::Float(_)   => "float",
            HlValue::Str(_)     => "str",
            HlValue::Bool(_)    => "bool",
            HlValue::List(_)    => "list",
            HlValue::Map(_)     => "map",
            HlValue::Lambda {.. }  => "lambda",
            HlValue::Variant {..}  => "variant",
            HlValue::Nil        => "null",
        }
    }
}

// ─────────────────────────────────────────────────────────────
// OpCode v7 + arena
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OpCode {

    // ── ISTNIEJĄCE — BEZ ZMIAN ───────────────────────────────

    Exec        { cmd_id: u32, sudo: bool },
    SetEnv      { key_id: u32, val_id: u32 },
    SetLocal    { key_id: u32, val_id: u32, is_raw: bool },
    CallFunc    { func_id: u32 },
    Return,
    Exit(i32),
    JumpIfFalse { cond_id: u32, target: usize },
    Jump        { target: usize },
    Lock        { key_id: u32, val_id: u32 },
    Unlock      { key_id: u32 },
    Plugin      { name_id: u32, args_id: u32, sudo: bool },
    HotLoop     { loop_ip: usize },
    /// Martwa instrukcja wstawiana przez optimizer
    Nop,

    // ── v6 — BEZ ZMIAN ───────────────────────────────────────

    SetConst    { key_id: u32, val_id: u32 },
    SetOut      { val_id: u32 },
    SpawnBg     { cmd_id: u32, sudo: bool },
    SpawnAssign { key_id: u32, cmd_id: u32, sudo: bool },
    AwaitPid    { expr_id: u32 },
    AwaitAssign { key_id: u32, expr_id: u32 },
    Assert      { cond_id: u32, msg_id: Option<u32> },
    MatchExec   { case_cmd_id: u32, sudo: bool },
    PipeExec    { cmd_id: u32, sudo: bool },

    // ── v7: REJESTRY NUMERYCZNE — BEZ ZMIAN ──────────────────

    /// Załaduj literał i64 do rejestru integer
    LoadInt     { dst: u8, val: i64 },
    /// Załaduj literał f64 do rejestru float
    LoadFloat   { dst: u8, val: f64 },
    /// Załaduj bool jako 0/1 do rejestru integer
    LoadBool    { dst: u8, val: bool },
    /// Załaduj string z pool do rejestru string (przez typed_vars)
    LoadStr     { dst: u8, str_id: u32 },
    /// Odczytaj zmienną do rejestru integer
    LoadVarI    { dst: u8, var_id: u32 },
    /// Odczytaj zmienną do rejestru float
    LoadVarF    { dst: u8, var_id: u32 },
    /// Zapisz rejestr integer do typed_vars + sync do shell env
    StoreVarI   { var_id: u32, src: u8 },
    /// Zapisz rejestr float do typed_vars + sync do shell env
    StoreVarF   { var_id: u32, src: u8 },

    /// Arytmetyka integer (wrapping)
    AddI { dst: u8, a: u8, b: u8 },
    SubI { dst: u8, a: u8, b: u8 },
    MulI { dst: u8, a: u8, b: u8 },
    /// checked_div — jeśli b==0 → dst=0 + warning
    DivI { dst: u8, a: u8, b: u8 },
    ModI { dst: u8, a: u8, b: u8 },
    NegI { dst: u8, src: u8 },

    /// Arytmetyka float
    AddF { dst: u8, a: u8, b: u8 },
    SubF { dst: u8, a: u8, b: u8 },
    MulF { dst: u8, a: u8, b: u8 },
    DivF { dst: u8, a: u8, b: u8 },
    NegF { dst: u8, src: u8 },

    /// Porównanie integer — ustawia cmp_flag
    CmpI { a: u8, b: u8, op: CmpOp },
    /// Porównanie float — ustawia cmp_flag
    CmpF { a: u8, b: u8, op: CmpOp },

    /// Warunkowy skok na podstawie cmp_flag (ustawionego przez CmpI/CmpF)
    JumpIfTrue  { target: usize },

    /// Pętla numeryczna natywna w VM — bez system() per iterację
    NumForExec  {
        var_id: u32,
        start:  i64,
        end:    i64,
        step:   i64,
        cmd_id: u32,
        sudo:   bool,
    },

    /// While z warunkiem ocenianym w VM
    WhileExprExec {
        lhs_reg: u8,
        op:      CmpOp,
        rhs_reg: u8,
        cmd_id:  u32,
        sudo:    bool,
    },

    /// Konwersje typów
    IntToFloat  { dst: u8, src: u8 },
    FloatToInt  { dst: u8, src: u8 },

    /// Konwersja integer/float → string i zapis do env (dla setenv sync)
    IntToStr    { var_id: u32, src: u8 },
    FloatToStr  { var_id: u32, src: u8 },

    /// return z wyrażenia — zapisuje wynik do _HL_OUT i kończy funkcję
    ReturnI     { src: u8 },
    ReturnF     { src: u8 },

    // ── NOWE: ARENA ALLOCATOR ─────────────────────────────────
    //
    // Używane wyłącznie wewnątrz :: name [size] def...done bloków.
    // Cały pozostały kod nadal używa GC (gc.c).
    //
    // Schemat wykonania :: cache [512kb] def...done w VM:
    //
    //   ArenaEnter { name_id, size_id }   ← hl_jit_arena_enter(&scope, name, size)
    //   ... ciało bloku ...
    //   [opcjonalnie: ArenaAlloc wewnątrz ciała dla jawnych alokacji]
    //   ArenaExit                          ← hl_jit_arena_exit(&scope) → hl_arena_free()
    //
    // HlJitArenaScope jest przechowywany w VM jako pole `arena_scope: HlJitArenaScope`.
    // Stos aren obsługuje zagnieżdżone :: bloki (do 64 poziomów per aa.c).

    /// :: name [size] def — wejdź w blok areny.
    /// Wywołuje hl_jit_arena_enter(&vm.arena_scope, name, size).
    /// name_id: intern nazwy funkcji areny (np. "cache")
    /// size_id: intern specyfikacji rozmiaru (np. "512kb")
    ArenaEnter  { name_id: u32, size_id: u32 },

    /// done (dla :: bloku) — wyjdź z bloku areny.
    /// Wywołuje hl_jit_arena_exit(&vm.arena_scope) → jednorazowy hl_arena_free().
    /// GC nie jest w to zaangażowany — pamięć areny nie przechodzi przez GC heap.
    ArenaExit,

    /// Jawna alokacja z bieżącej areny (dla wewnętrznych operacji VM).
    /// Wynik (wskaźnik) jest zapisywany jako usize w typed_vars pod kluczem var_id.
    /// n_bytes: rozmiar alokacji w bajtach.
    /// Wywołuje hl_jit_arena_alloc(&vm.arena_scope, n_bytes).
    ArenaAlloc  { var_id: u32, n_bytes: u64 },

    /// Reset bieżącej areny (bump pointer cofnięty do początku).
    /// Używany wewnątrz pętli w :: bloku aby uniknąć OOM per iterację.
    /// Wywołuje hl_jit_arena_reset(&vm.arena_scope).
    ArenaReset,
}

// ─────────────────────────────────────────────────────────────
// BytecodeProgram
// ─────────────────────────────────────────────────────────────
#[derive(Serialize, Deserialize)]
pub struct BytecodeProgram {
    pub schema_version: u32,
    pub ops:            Vec<OpCode>,
    /// Mapa: nazwa funkcji → adres IP (indeks w ops)
    pub functions:      HashMap<String, usize>,
    pub pool:           StringPool,
}

impl BytecodeProgram {
    pub fn new() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            ops:            Vec::with_capacity(128),
            functions:      HashMap::new(),
            pool:           StringPool::new(),
        }
    }

    pub fn rebuild_pool_index(&mut self) {
        self.pool.rebuild_index();
    }

    #[inline(always)]
    pub fn str(&self, id: u32) -> &str {
        self.pool.get(id)
    }
}
