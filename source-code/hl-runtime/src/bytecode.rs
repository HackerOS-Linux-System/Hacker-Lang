use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CACHE_SCHEMA_VERSION: u32 = 6;

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
// OpCode v6
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
    /// Martwa instrukcja wstawiana przez optimizer — usuwana przez nop_strip()
    Nop,

    // ── NOWE v6 ───────────────────────────────────────────────

    /// % KEY = val — stała
    /// VM może opcjonalnie egzekwować niezmienność (warn przy nadpisaniu).
    /// Kompilacja: SetConst → setenv() jak SetEnv, ale VM zapamiętuje klucz.
    SetConst    { key_id: u32, val_id: u32 },

    /// out val — zwróć wartość z funkcji przez zmienną _HL_OUT
    SetOut      { val_id: u32 },

    /// spawn rest — uruchom zadanie w tle, PID do _HL_SPAWN_PID
    /// (fire & forget bez przypisania)
    SpawnBg     { cmd_id: u32, sudo: bool },

    /// key = spawn rest — uruchom w tle i przypisz PID do zmiennej
    SpawnAssign { key_id: u32, cmd_id: u32, sudo: bool },

    /// await $var — wait na PID bez przypisania
    AwaitPid    { expr_id: u32 },

    /// key = await $var / key = await .func — czekaj i przypisz wynik
    AwaitAssign { key_id: u32, expr_id: u32 },

    /// assert cond [msg] — walidacja w miejscu
    /// VM: jeśli cond false → eprintln! + Exit(1) bez fork/exec
    Assert      { cond_id: u32, msg_id: Option<u32> },

    /// Całe match..case..esac kompiluje się do jednego Exec (shell case)
    /// Ten opcod NIE jest używany przez vm.rs — compiler.rs emituje Exec.
    /// Zachowany dla ewentualnej przyszłej optymalizacji VM-native match.
    MatchExec   { case_cmd_id: u32, sudo: bool },

    /// Cały pipe chain kompiluje się do Exec (shell pipe) lub sekwencji CallFunc.
    /// Ten opcod NIE jest używany przez vm.rs — compiler.rs decyduje.
    PipeExec    { cmd_id: u32, sudo: bool },
}

// ─────────────────────────────────────────────────────────────
// BytecodeProgram
// ─────────────────────────────────────────────────────────────
#[derive(Serialize, Deserialize)]
pub struct BytecodeProgram {
    pub schema_version: u32,
    pub ops:            Vec<OpCode>,
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
