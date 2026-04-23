#[derive(Debug, Clone)]
pub struct HlProgram {
    /// Lista funkcji (pierwsza to `__hl_main`)
    pub functions: Vec<HlFunction>,
    /// Stringi statyczne (interned)
    pub string_pool: Vec<String>,
    /// Nazwy zaleznosci (// dep)
    pub deps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HlFunction {
    pub name:   String,
    pub instrs: Vec<HlInstr>,
}

/// Instrukcje HlIR — celowo na wysokim poziomie.
/// Cranelift emituje wywolania do runtime C dla kazdej z nich.
#[derive(Debug, Clone)]
pub enum HlInstr {
    /// Wypisz string ze string pool pod indeksem `idx`
    Print { idx: u32 },

    /// Wypisz string ze string pool + interpolacja zmiennych
    PrintInterp { idx: u32 },

    /// Uruchom komende powloki
    RunCmd {
        cmd_idx: u32,
        mode:    CmdMode,
    },

    /// Ustaw zmienna lokalna (string pool idx)
    SetVar { name_idx: u32, val_idx: u32 },

    /// Ustaw zmienna z interpolacja
    SetVarInterp { name_idx: u32, val_idx: u32 },

    /// Export do srodowiska
    ExportVar { name_idx: u32, val_idx: u32 },

    /// Export ze zmiennymi @var (interpolacja w runtime)
    ExportVarInterp { name_idx: u32, val_idx: u32 },

    /// Export lista (elementy oddzielone ':')
    ExportList { name_idx: u32, items: Vec<u32> },

    /// Wywolaj quick-funkcje
    QuickCall { name_idx: u32, args_idx: u32 },

    /// Wywolaj funkcje HL (zdefiniowana w tym programie)
    CallFunc { func_idx: u32 },

    /// Sprawdz ostatni exit code — jesli != 0 skocz do `else_pc`
    CondOk  { body: Vec<HlInstr> },

    /// Sprawdz ostatni exit code — jesli == 0 skocz do `else_pc`
    CondErr { body: Vec<HlInstr> },

    /// Deklaracja zaleznosci (// narzedzie)
    Dep { name_idx: u32 },

    /// Zakonczenie programu z kodem
    Exit { code: i32 },

    /// NOP (komentarze)
    Nop,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CmdMode {
    Plain        = 0,
    Sudo         = 1,
    Isolated     = 2,
    IsolatedSudo = 3,
    WithVars     = 4,
    WithVarsSudo = 5,
    WithVarsIso  = 6,
}

impl HlProgram {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            string_pool: Vec::new(),
            deps: Vec::new(),
        }
    }

    /// Intern string — zwraca indeks w string pool
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(pos) = self.string_pool.iter().position(|x| x == s) {
            return pos as u32;
        }
        let idx = self.string_pool.len() as u32;
        self.string_pool.push(s.to_string());
        idx
    }
}
