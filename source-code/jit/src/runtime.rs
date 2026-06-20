use rustc_hash::FxHashMap;

// ── NaN-boxing ────────────────────────────────────────────────────────────────

const NAN_BASE:      u64 = 0x7FF8_0000_0000_0000;
const TAG_MASK:      u64 = 0x0000_0000_0000_FFFF;
const PAYLOAD_SHIFT: u32 = 16;

const TAG_NIL:  u64 = 0x0000;
const TAG_BOOL: u64 = 0x0001;
const TAG_STR:  u64 = 0x0002;
const TAG_INT:  u64 = 0x0003;

/// Wartość jako NaN-boxed u64 — 8 bajtów, zero alokacji dla liczb/boolów/intów
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct NanVal(pub u64);

impl NanVal {
    #[inline(always)] pub fn nil()  -> Self { NanVal(NAN_BASE | TAG_NIL) }

    #[inline(always)]
    pub fn bool(b: bool) -> Self {
        NanVal(NAN_BASE | TAG_BOOL | ((b as u64) << PAYLOAD_SHIFT))
    }

    #[inline(always)]
    pub fn num(n: f64) -> Self {
        let bits = n.to_bits();
        // Unikaj kolizji z NaN sentinel
        if (bits & NAN_BASE) == NAN_BASE {
            NanVal(0.0f64.to_bits())
        } else {
            NanVal(bits)
        }
    }

    #[inline(always)]
    pub fn str_interned(idx: u32) -> Self {
        NanVal(NAN_BASE | TAG_STR | ((idx as u64) << PAYLOAD_SHIFT))
    }

    #[inline(always)]
    pub fn int(n: i32) -> Self {
        NanVal(NAN_BASE | TAG_INT | ((n as u32 as u64) << PAYLOAD_SHIFT))
    }

    #[inline(always)] fn is_nan_tagged(&self) -> bool { (self.0 & NAN_BASE) == NAN_BASE }
    #[inline(always)] fn tag(&self) -> u64             { self.0 & TAG_MASK }
    #[inline(always)] fn payload(&self) -> u64         { (self.0 >> PAYLOAD_SHIFT) & 0xFFFF_FFFF }

    #[inline(always)] pub fn is_nil(&self)  -> bool { self.is_nan_tagged() && self.tag() == TAG_NIL }
    #[inline(always)] pub fn is_bool(&self) -> bool { self.is_nan_tagged() && self.tag() == TAG_BOOL }
    #[inline(always)] pub fn is_num(&self)  -> bool { !self.is_nan_tagged() }
    #[inline(always)] pub fn is_str(&self)  -> bool { self.is_nan_tagged() && self.tag() == TAG_STR }
    #[inline(always)] pub fn is_int(&self)  -> bool { self.is_nan_tagged() && self.tag() == TAG_INT }

    #[inline(always)]
    pub fn as_f64(&self) -> f64 {
        if self.is_num()  { return f64::from_bits(self.0); }
        if self.is_int()  { return self.payload() as i32 as f64; }
        if self.is_bool() { return if self.payload() != 0 { 1.0 } else { 0.0 }; }
        0.0
    }

    #[inline(always)]
    pub fn as_str_idx(&self) -> Option<u32> {
        if self.is_str() { Some(self.payload() as u32) } else { None }
    }

    /// Truthy check — zgodny z HL semantyką
    #[inline]
    pub fn is_truthy(&self, interner: &StringInterner) -> bool {
        if self.is_nil()  { return false; }
        if self.is_bool() { return self.payload() != 0; }
        if self.is_num()  { return self.as_f64() != 0.0; }
        if self.is_int()  { return self.payload() != 0; }
        if self.is_str()  {
            let s = interner.get(self.payload() as u32);
            return !s.is_empty() && s != "false" && s != "0";
        }
        false
    }

    /// Konwertuj do String
    pub fn to_str_val(&self, interner: &StringInterner) -> String {
        if self.is_nil()  { return String::new(); }
        if self.is_bool() { return if self.payload() != 0 { "true".into() } else { "false".into() }; }
        if self.is_num()  {
            let n = self.as_f64();
            return if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{}", n as i64)
            } else {
                format!("{}", n)
            };
        }
        if self.is_int()  { return format!("{}", self.payload() as i32); }
        if self.is_str()  { return interner.get(self.payload() as u32).to_string(); }
        String::new()
    }

    /// Równość — fast path dla stringów przez idx
    #[inline]
    pub fn eq_val(&self, other: &NanVal, interner: &StringInterner) -> bool {
        if self.is_num() && other.is_num() {
            return self.as_f64() == other.as_f64();
        }
        if self.is_str() && other.is_str() {
            return self.payload() == other.payload(); // u32 porównanie!
        }
        self.to_str_val(interner) == other.to_str_val(interner)
    }
}

impl std::fmt::Debug for NanVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_nil()  { write!(f, "Nil") }
        else if self.is_bool() { write!(f, "Bool({})", self.payload() != 0) }
        else if self.is_num()  { write!(f, "Num({})", self.as_f64()) }
        else if self.is_int()  { write!(f, "Int({})", self.payload() as i32) }
        else if self.is_str()  { write!(f, "Str(idx={})", self.payload()) }
        else { write!(f, "NanVal(0x{:016x})", self.0) }
    }
}

// ── String Interner ───────────────────────────────────────────────────────────

/// Interner stringów: String → u32 idx, Vec<String> dla lookup przez idx
/// Porównanie stringów = porównanie u32 — eliminuje strcmp w hot paths
pub struct StringInterner {
    map:     FxHashMap<String, u32>,
    pub strings: Vec<String>,
}

impl StringInterner {
    pub fn new() -> Self {
        let mut s = Self {
            map:     FxHashMap::default(),
            strings: Vec::with_capacity(512),
        };
        // Idx 0 = pusty string
        s.intern("");
        // Pre-intern często używane stringi HL
        for lit in &[
            "true", "false", "0", "1", "nil",
            "_last_exit_code", "_arena_args", "_arena_name",
            "HL_VERSION", "HL_GEN", "HL_OS",
        ] {
            s.intern(lit);
        }
        s
    }

    #[inline]
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.map.get(s) { return idx; }
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.map.insert(s.to_string(), idx);
        idx
    }

    #[inline]
    pub fn intern_owned(&mut self, s: String) -> u32 {
        if let Some(&idx) = self.map.get(&s) { return idx; }
        let idx = self.strings.len() as u32;
        self.map.insert(s.clone(), idx);
        self.strings.push(s);
        idx
    }

    #[inline(always)]
    pub fn get(&self, idx: u32) -> &str {
        self.strings.get(idx as usize).map(|s| s.as_str()).unwrap_or("")
    }

    #[inline]
    pub fn lookup(&self, s: &str) -> Option<u32> { self.map.get(s).copied() }
}

impl Default for StringInterner { fn default() -> Self { Self::new() } }

// ── Inline Cache ──────────────────────────────────────────────────────────────

/// Płaska tablica: name_idx → slot w vars_flat
/// Zamiast FxHashMap::get (9-15 ns) → array index (1-2 ns)
pub struct VarCache {
    slots: Vec<u32>,
}

impl VarCache {
    pub fn new(capacity: usize) -> Self {
        Self { slots: vec![u32::MAX; capacity] }
    }

    #[inline(always)]
    pub fn get(&self, name_idx: u32) -> Option<u32> {
        let i = name_idx as usize;
        if i < self.slots.len() {
            let s = self.slots[i];
            if s != u32::MAX { return Some(s); }
        }
        None
    }

    #[inline(always)]
    pub fn set(&mut self, name_idx: u32, slot: u32) {
        let i = name_idx as usize;
        if i >= self.slots.len() { self.slots.resize(i + 64, u32::MAX); }
        self.slots[i] = slot;
    }

    #[inline(always)]
    pub fn invalidate(&mut self, name_idx: u32) {
        let i = name_idx as usize;
        if i < self.slots.len() { self.slots[i] = u32::MAX; }
    }
}

// ── RuntimeState ─────────────────────────────────────────────────────────────

pub struct RuntimeState {
    /// Rejestry: 8 B/rejestr (NaN-boxed)
    pub regs:      Vec<NanVal>,
    /// Płaska tablica zmiennych (indexed przez slot)
    pub vars_flat: Vec<NanVal>,
    /// name_idx → slot mapping
    pub var_slots: FxHashMap<u32, u32>,
    /// Inline cache: name_idx → slot
    pub var_cache: VarCache,
    /// String interner
    pub interner:  StringInterner,
    /// Ostatni exit code
    pub last_exit: i32,
    /// Głębokość wywołań
    pub call_depth: u32,
    /// Iterator state: iter_reg → (interned word idxs, current pos)
    pub iters: FxHashMap<u32, (Vec<u32>, usize)>,
}

const MAX_CALL_DEPTH: u32 = 512;

impl RuntimeState {
    pub fn new(num_regs: usize) -> Self {
        Self {
            regs:      vec![NanVal::nil(); num_regs.max(64)],
            vars_flat: vec![NanVal::nil(); 128],
            var_slots: FxHashMap::default(),
            var_cache: VarCache::new(256),
            interner:  StringInterner::new(),
            last_exit: 0,
            call_depth: 0,
            iters:     FxHashMap::default(),
        }
    }

    #[inline(always)]
    pub fn get_reg(&self, r: u32) -> NanVal {
        if (r as usize) < self.regs.len() { self.regs[r as usize] } else { NanVal::nil() }
    }

    #[inline(always)]
    pub fn set_reg(&mut self, r: u32, val: NanVal) {
        let i = r as usize;
        if i >= self.regs.len() { self.regs.resize(i + 1, NanVal::nil()); }
        self.regs[i] = val;
    }

    /// GetVar z inline cache — O(1) hot path
    #[inline]
    pub fn get_var(&mut self, name_idx: u32) -> NanVal {
        // 1. Inline cache
        if let Some(slot) = self.var_cache.get(name_idx) {
            return self.vars_flat[slot as usize];
        }
        // 2. FxHashMap
        if let Some(&slot) = self.var_slots.get(&name_idx) {
            self.var_cache.set(name_idx, slot);
            return self.vars_flat[slot as usize];
        }
        // 3. Fallback: std::env
        let name = self.interner.get(name_idx).to_string();
        if let Ok(val) = std::env::var(&name) {
            let idx = self.interner.intern_owned(val);
            return NanVal::str_interned(idx);
        }
        NanVal::nil()
    }

    /// SetVar z inline cache
    #[inline]
    pub fn set_var(&mut self, name_idx: u32, val: NanVal) {
        // Inline cache hit
        if let Some(slot) = self.var_cache.get(name_idx) {
            self.vars_flat[slot as usize] = val;
            return;
        }
        // FxHashMap hit
        if let Some(&slot) = self.var_slots.get(&name_idx) {
            self.var_cache.set(name_idx, slot);
            self.vars_flat[slot as usize] = val;
            return;
        }
        // Nowy slot
        let slot = self.var_slots.len() as u32;
        let i    = slot as usize;
        if i >= self.vars_flat.len() { self.vars_flat.resize(i + 64, NanVal::nil()); }
        self.vars_flat[i] = val;
        self.var_slots.insert(name_idx, slot);
        self.var_cache.set(name_idx, slot);
    }

    /// Export do std::env
    pub fn export_var(&mut self, name_idx: u32, val: NanVal) {
        let name    = self.interner.get(name_idx).to_string();
        let val_str = val.to_str_val(&self.interner);
        std::env::set_var(&name, &val_str);
        self.set_var(name_idx, val);
    }

    pub fn val_to_str(&self, val: NanVal) -> String { val.to_str_val(&self.interner) }

    #[inline]
    pub fn intern_str(&mut self, s: &str) -> NanVal {
        let idx = self.interner.intern(s);
        NanVal::str_interned(idx)
    }

    #[inline]
    pub fn intern_str_owned(&mut self, s: String) -> NanVal {
        let idx = self.interner.intern_owned(s);
        NanVal::str_interned(idx)
    }

    pub fn check_call_depth(&self) -> anyhow::Result<()> {
        if self.call_depth >= MAX_CALL_DEPTH {
            anyhow::bail!("Przekroczono maksymalną głębokość wywołań ({})", MAX_CALL_DEPTH);
        }
        Ok(())
    }
}
