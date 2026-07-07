use serde::{Deserialize, Serialize};

/// Identyfikator rejestru (wirtualny, nieograniczony)
pub type Reg = u32;

/// Indeks do puli stałych
pub type ConstIdx = u32;

/// Indeks funkcji w tablicy funkcji
pub type FuncIdx = u32;

/// Offset instrukcji (do skoków)
pub type InsnOff = u32;

/// Pula stałych z O(1) deduplikacją przez HashMap.
/// Poprzednia implementacja używała iter().position() = O(n) per insert → O(n²) ogółem.
/// Dla bit.hl (836 linii, ~500 unikalnych stringów) powodowało 8s+ kompilacji.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConstPool {
    pub strings:      Vec<String>,
    pub numbers:      Vec<f64>,
    pub bools:        Vec<bool>,
    // Indeksy do szybkiej deduplikacji — nie serializowane (rebuilt przy load)
    #[serde(skip)]
    str_index:    std::collections::HashMap<String, ConstIdx>,
    #[serde(skip)]
    num_index:    std::collections::HashMap<u64, ConstIdx>,
}

impl ConstPool {
    /// Dodaj lub zdeduplikuj string — O(1) amortyzowane
    pub fn add_str(&mut self, s: impl Into<String>) -> ConstIdx {
        let s = s.into();
        if let Some(&i) = self.str_index.get(&s) {
            return i;
        }
        let i = self.strings.len() as ConstIdx;
        self.str_index.insert(s.clone(), i);
        self.strings.push(s);
        i
    }

    /// Dodaj lub zdeduplikuj f64 — O(1) przez bit-pattern hash
    pub fn add_num(&mut self, n: f64) -> ConstIdx {
        let bits = n.to_bits();
        if let Some(&i) = self.num_index.get(&bits) {
            return i;
        }
        let i = self.numbers.len() as ConstIdx;
        self.num_index.insert(bits, i);
        self.numbers.push(n);
        i
    }

    pub fn add_bool(&mut self, b: bool) -> ConstIdx {
        b as ConstIdx
    }

    /// Odbuduj indeksy po deserializacji (serde skip → puste HashMap)
    pub fn rebuild_index(&mut self) {
        self.str_index.clear();
        for (i, s) in self.strings.iter().enumerate() {
            self.str_index.insert(s.clone(), i as ConstIdx);
        }
        self.num_index.clear();
        for (i, n) in self.numbers.iter().enumerate() {
            self.num_index.insert(n.to_bits(), i as ConstIdx);
        }
    }
}

/// Tabela funkcji zdefiniowanych w module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuncEntry {
    pub name:        String,
    /// Offset pierwszej instrukcji funkcji w `HlModule::instructions`
    pub start_insn:  InsnOff,
    /// Liczba instrukcji
    pub insn_count:  u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FuncTable {
    pub entries: Vec<FuncEntry>,
}

impl FuncTable {
    pub fn find(&self, name: &str) -> Option<&FuncEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

/// Główny moduł bytecode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlModule {
    /// Metadane
    pub header:       HlBcHeader,
    /// Pula stałych
    pub consts:       ConstPool,
    /// Tablica funkcji
    pub funcs:        FuncTable,
    /// Liniowa lista instrukcji (główny kod + ciała funkcji)
    pub instructions: Vec<Instruction>,
    /// Liczba rejestrów potrzebnych do wykonania głównego bloku
    pub main_regs:    u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlBcHeader {
    pub hl_gen:      u32,
    pub source_path: String,
    pub compiled_at: u64,  // unix timestamp
    pub hl_version:  String,
}

impl HlModule {
    pub fn new(source_path: &str, gen: u32) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
        Self {
            header: HlBcHeader {
                hl_gen: gen,
                source_path: source_path.to_string(),
                compiled_at: now,
                hl_version: "gen 2".to_string(),
            },
            consts:       ConstPool::default(),
            funcs:        FuncTable::default(),
            instructions: Vec::new(),
            main_regs:    0,
        }
    }
}

/// Zestaw instrukcji IR
///
/// Projektowane kryteria:
///  - Kompaktowe (enum z u32/u8 polami → ~16 bajtów/instrukcja średnio)
///  - Wystarczająco wysokopoziomowe by JIT łatwo tłumaczył
///  - Wystarczająco nisko by nie tracić informacji optymalizacyjnych
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instruction {
    // ── Ładowanie stałych ───────────────────────────────────────
    /// dst = strings[idx]
    LoadStr  { dst: Reg, idx: ConstIdx },
    /// dst = numbers[idx]
    LoadNum  { dst: Reg, idx: ConstIdx },
    /// dst = bools[0/1]
    LoadBool { dst: Reg, val: bool },
    /// dst = nil
    LoadNil  { dst: Reg },

    // ── Zmienne ─────────────────────────────────────────────────
    /// dst = env[name_idx]  (czytaj zmienną)
    GetVar    { dst: Reg, name: ConstIdx },
    /// dst = env[name_reg]  (dynamiczna nazwa zmiennej — @{arg@_i})
    GetVarDyn { dst: Reg, name: Reg },
    /// env[name_idx] = src
    SetVar   { name: ConstIdx, src: Reg },
    /// env[name_idx] = src  (export do std::env)
    SetEnv   { name: ConstIdx, src: Reg },

    // ── Arytmetyka natywna ───────────────────────────────────────
    Add  { dst: Reg, a: Reg, b: Reg },
    Sub  { dst: Reg, a: Reg, b: Reg },
    Mul  { dst: Reg, a: Reg, b: Reg },
    Div  { dst: Reg, a: Reg, b: Reg },
    Mod  { dst: Reg, a: Reg, b: Reg },
    Neg  { dst: Reg, src: Reg },

    // ── Porównania ───────────────────────────────────────────────
    CmpEq  { dst: Reg, a: Reg, b: Reg },
    CmpNe  { dst: Reg, a: Reg, b: Reg },
    CmpLt  { dst: Reg, a: Reg, b: Reg },
    CmpLe  { dst: Reg, a: Reg, b: Reg },
    CmpGt  { dst: Reg, a: Reg, b: Reg },
    CmpGe  { dst: Reg, a: Reg, b: Reg },

    // ── Konwersje ────────────────────────────────────────────────
    /// dst = to_string(src)
    ToString { dst: Reg, src: Reg },
    /// dst = to_f64(src)
    ToNumber { dst: Reg, src: Reg },
    /// dst = truthy(src)
    Truthy   { dst: Reg, src: Reg },

    // ── String interpolation ─────────────────────────────────────
    /// dst = concat(parts[0..n]) — parts to lista Reg
    Concat { dst: Reg, parts: Vec<Reg> },

    // ── Sterowanie przepływem ────────────────────────────────────
    /// if !cond goto offset
    JumpIfFalse { cond: Reg, offset: InsnOff },
    /// if cond goto offset
    JumpIfTrue  { cond: Reg, offset: InsnOff },
    /// unconditional jump
    Jump        { offset: InsnOff },
    /// koniec głównego bloku / powrót z funkcji
    Return      { src: Option<Reg> },

    // ── Wywołania ────────────────────────────────────────────────
    /// wywołaj funkcję HL zdefiniowaną w module
    CallFunc    { name: ConstIdx },
    /// wywołaj quick-function (::upper itd.)
    CallQuick   { name: ConstIdx, arg: Reg, dst: Reg },

    // ── Komendy systemowe ────────────────────────────────────────
    /// uruchom komendę; dst = exit_code (i32 jako f64)
    ExecCmd     {
        cmd:  Reg,      // rejestr ze stringiem komendy
        mode: CmdMode,
        dst:  Reg,      // exit_code
    },
    /// jak ExecCmd ale przechwytuje stdout → dst_out
    ExecCapture {
        cmd:     Reg,
        mode:    CmdMode,
        dst_ec:  Reg,
        dst_out: Reg,
    },
    /// wypisz na stdout
    Print       { src: Reg },

    // ── Pętle ────────────────────────────────────────────────────
    /// for-in: iteruj po słowach w src; iterator state w rejestrze iter_reg
    ForInStart  { iter_reg: Reg, src: Reg },
    /// for-in next: dst = następne słowo lub skocz do end_off
    ForInNext   { iter_reg: Reg, dst: Reg, end_off: InsnOff },

    // ── HackerOS API ─────────────────────────────────────────────
    /// wywołaj narzędzie HackerOS; args_reg = string argumentów
    HackerOsCall { tool: ConstIdx, args: Reg, dst: Reg },

    // ── Debugowanie / metadata ───────────────────────────────────
    /// marker źródłowy (usuwany przez optymalizator w release)
    SourceLine  { line: u32 },
    /// noop (po usuniętych instrukcjach)
    Nop,
}

/// Tryb wykonania komendy (odpowiada CommandMode z AST)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum CmdMode {
    Plain,
    Sudo,
    Isolated,
    IsolatedSudo,
    WithVars,
    WithVarsSudo,
    WithVarsIsolated,
}
