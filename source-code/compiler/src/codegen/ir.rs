#[derive(Debug, Clone)]
pub struct HlProgram {
    pub functions: Vec<HlFunction>,
    pub string_pool: Vec<String>,
    pub deps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HlFunction {
    pub name:   String,
    pub instrs: Vec<HlInstr>,
}

#[derive(Debug, Clone)]
pub enum HlInstr {
    Print       { idx: u32 },
    PrintInterp { idx: u32 },
    RunCmd      { cmd_idx: u32, mode: CmdMode },
    /// & komenda — uruchom w tle
    RunBackground { cmd_idx: u32 },
    /// *> komenda — uruchom przez hsh
    RunHsh      { cmd_idx: u32 },
    /// _N instrukcje — powtorz N razy
    RepeatN     { count: u64, body: Vec<HlInstr> },
    SetVar      { name_idx: u32, val_idx: u32 },
    SetVarInterp { name_idx: u32, val_idx: u32 },
    ExportVar   { name_idx: u32, val_idx: u32 },
    ExportVarInterp { name_idx: u32, val_idx: u32 },
    ExportList  { name_idx: u32, items: Vec<u32> },
    QuickCall   { name_idx: u32, args_idx: u32 },
    CallFunc    { func_idx: u32 },
    CondOk      { body: Vec<HlInstr> },
    CondErr     { body: Vec<HlInstr> },
    Dep         { name_idx: u32 },
    Exit        { code: i32 },
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
    /// *> — hsh mode
    Hsh          = 10,
}

impl HlProgram {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            string_pool: Vec::new(),
            deps: Vec::new(),
        }
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(pos) = self.string_pool.iter().position(|x| x == s) {
            return pos as u32;
        }
        let idx = self.string_pool.len() as u32;
        self.string_pool.push(s.to_string());
        idx
    }
}
