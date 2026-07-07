use thiserror::Error;
use crate::import_spec::parse_import_line;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Print(String),
    // Wbudowane quick-call (gen 1 + gen 2 fallback): :: nazwa args
    QuickCall { name: String, args: String },
    /// :: nazwa args |> @var  — QuickCall z przechwyceniem stdout do zmiennej
    QuickPipeToVar { name: String, args: String, var_name: String },
    // Arena function DEFINICJA (gen 2): :: nazwa <rozmiar> def
    ArenaFuncDef { name: String, arena_size: String },
    // Arena function WYWOŁANIE (gen 2): :: nazwa args
    // Rozróżnienie od QuickCall następuje w parserze (sprawdza czy nazwa zdefiniowana)
    // W lekserze emitujemy QuickCall — parser decyduje co to jest
    CmdIsolatedSudo(String),
    CmdWithVarsSudo(String),
    CmdWithVarsIsolated(String),
    CmdSudo(String),
    CmdIsolated(String),
    CmdWithVars(String),
    Cmd(String),
    HshCmd(String),
    Background(String),
    CmdPipeToVar { cmd: String, mode: PipeCmdMode, var_name: String },
    HackerOsApi { tool: String, args: String },
    VarDecl { name: String, typ: String, value: String },
    VarRef(String),
    ExportSingle { name: String, value: String },
    ExportListStart(String),
    ExportListItem(String),
    ExportListEnd,
    /// // narzedzie [pakiet-apt]
    /// Pole 0: nazwa binarka (np. "ninja"), pole 1: apt package (np. Some("ninja-build"))
    Dependency(String, Option<String>),
    Import { lib: String, detail: Option<String> },
    FileImport { path: String, detail: Option<String> },
    // <* katalog — import katalogu (gen 2)
    DirImport  { path: String },
    FuncDef(String),
    FuncCall(String),
    IfOk,
    IfErr,
    WhileStart(String),
    SwitchStart(String),
    SwitchArm { pattern: String },
    ForIn { var: String, iterable: String },
    Arithmetic { expr: String, assign_to: Option<String> },
    Done,
    Using(String),
    GoroutineStart { name: Option<String> },
    ChannelDecl(String),
    ChannelOp(String),
    // _> plik [runtime] — extern system
    ExternStart { file: String, runtime: String },
    RepeatN(u64),
    Comments(CommentKind, String),
    Ident(String),
    StringLit(String),
    Bool(bool),
    Number(f64),
    Newline,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipeCmdMode { Plain, Sudo, WithVars }

#[derive(Debug, Clone, PartialEq)]
pub enum CommentKind { Line, Doc, Block }

#[derive(Debug, Error)]
pub enum LexError {
    #[error("Nieoczekiwany znak '{0}' w linii {1}:{2}")]
    UnexpectedChar(char, usize, usize),
    #[error("Niezamknięty string w linii {0}")]
    UnterminatedString(usize),
    #[error("Niezamknięty komentarz blokowy")]
    UnterminatedBlockComment,
}

pub struct Lexer {
    source: Vec<char>,
    pub pos:  usize,
    pub line: usize,
    pub col:  usize,
    in_export_list: bool,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self { source: source.chars().collect(), pos: 0, line: 1, col: 1, in_export_list: false }
    }

    #[inline] pub fn peek(&self) -> Option<char> { self.source.get(self.pos).copied() }
    #[inline] pub fn peek_at(&self, n: usize) -> Option<char> { self.source.get(self.pos + n).copied() }
    #[inline] fn matches_seq(&self, seq: &[char]) -> bool {
    self.source.get(self.pos..self.pos + seq.len()) == Some(seq)
    }

    #[inline]
    pub fn advance(&mut self) -> Option<char> {
        let ch = self.source.get(self.pos).copied();
        if let Some(c) = ch {
            self.pos += 1;
            if c == '\n' { self.line += 1; self.col = 1; } else { self.col += 1; }
        }
        ch
    }

    fn skip_n(&mut self, n: usize) { for _ in 0..n { self.advance(); } }
    fn skip_ws(&mut self) { while matches!(self.peek(), Some(' ') | Some('\t')) { self.advance(); } }

    fn read_line(&mut self) -> String {
        let start = self.pos;
        let mut end = self.pos;
        while end < self.source.len() && self.source[end] != '\n' { end += 1; }
        let s: String = self.source[start..end].iter().collect();
        for _ in start..end { self.advance(); }
        s.trim_end().to_string()
    }

    fn read_string_lit(&mut self) -> Result<String, LexError> {
        let start_line = self.line;
        self.advance();
        let mut s = String::with_capacity(64);
        loop {
            match self.advance() {
                None       => return Err(LexError::UnterminatedString(start_line)),
                Some('"')  => break,
                Some('\\') => match self.advance() {
                    Some('n')  => s.push('\n'),
                    Some('t')  => s.push('\t'),
                    Some('"')  => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some(c)    => { s.push('\\'); s.push(c); }
                    None       => return Err(LexError::UnterminatedString(start_line)),
                },
                Some(c) => s.push(c),
            }
        }
        Ok(s)
    }

    fn read_ident(&mut self) -> String {
        let mut s = String::with_capacity(16);
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' { s.push(c); self.advance(); } else { break; }
        }
        s
    }

    fn read_ident_full(&mut self) -> String {
        let mut s = String::with_capacity(16);
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' { s.push(c); self.advance(); } else { break; }
        }
        while s.ends_with('-') { s.pop(); }
        s
    }

    fn read_number(&mut self) -> f64 {
        let mut s = String::with_capacity(16);
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' { s.push(c); self.advance(); } else { break; }
        }
        s.parse().unwrap_or(0.0)
    }

    fn read_cmd(&mut self) -> String { self.skip_ws(); self.read_line() }

    /// Rozdziel `args |> @var` dla QuickCall (:: name args |> @var)
    /// Zwraca (args_before_pipe, var_name) lub None jeśli brak |>
    fn split_quick_pipe(line: &str) -> Option<(String, String)> {
        let b = line.as_bytes();
        let mut in_s = false;
        let mut in_d = false;
        let mut i = 0;
        while i + 1 < b.len() {
            match b[i] {
                b'\'' if !in_d => in_s = !in_s,
                b'"'  if !in_s => in_d = !in_d,
                b'|' if !in_s && !in_d && b[i+1] == b'>' => {
                    let args_part = line[..i].trim().to_string();
                    let rest      = line[i+2..].trim();
                    let var_name  = rest.strip_prefix('@')
                        .map(|v| v.split(|c: char| !c.is_alphanumeric() && c != '_')
                            .next().unwrap_or("").to_string())
                        .unwrap_or_default();
                    if var_name.is_empty() { return None; }
                    return Some((args_part, var_name));
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn split_pipe_to_var(line: &str) -> Option<(String, String)> {
        let bytes = line.as_bytes();
        let mut in_sq = false;
        let mut in_dq = false;
        let mut i = 0;
        while i + 1 < bytes.len() {
            match bytes[i] {
                b'\'' if !in_dq => in_sq = !in_sq,
                b'"'  if !in_sq => in_dq = !in_dq,
                b'|' if !in_sq && !in_dq && bytes[i+1] == b'>' => {
                    let cmd = line[..i].trim().to_string();
                    let var = line[i+2..].trim().trim_start_matches('@').to_string();
                    if !var.is_empty() && !cmd.is_empty() { return Some((cmd, var)); }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    // ── Parsowanie rozmiaru areny: <4k>, <1m>, <4096> ────────────────────────
    fn try_read_arena_size(&mut self) -> Option<String> {
        // Spójrz czy po skip_ws jest '<'
        let saved_pos  = self.pos;
        let saved_line = self.line;
        let saved_col  = self.col;

        self.skip_ws();
        if self.peek() != Some('<') {
            // Cofnij
            self.pos  = saved_pos;
            self.line = saved_line;
            self.col  = saved_col;
            return None;
        }
        self.advance(); // '<'
        let mut size_str = String::new();
        loop {
            match self.peek() {
                Some('>') => { self.advance(); break; }
                Some('\n') | None => {
                    // Niezamknięty '<' — cofnij i traktuj jako brak rozmiaru
                    self.pos  = saved_pos;
                    self.line = saved_line;
                    self.col  = saved_col;
                    return None;
                }
                Some(c) => { size_str.push(c); self.advance(); }
            }
        }
        Some(size_str.trim().to_string())
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::with_capacity(self.source.len() / 8 + 16);

        while self.pos < self.source.len() {
            let ch = match self.peek() { None => break, Some(c) => c };

            // ── Export list mode ─────────────────────────────────────────────
            if self.in_export_list {
                match ch {
                    '\n' => { tokens.push(Token::Newline); self.advance(); }
                    ' ' | '\t' | '\r' => { self.advance(); }
                    '|' if self.peek_at(1) != Some('>') && self.peek_at(1) != Some('|') => {
                        self.advance(); self.skip_ws();
                        tokens.push(Token::ExportListItem(self.read_line()));
                    }
                    ']' => {
                        self.advance();
                        self.in_export_list = false;
                        tokens.push(Token::ExportListEnd);
                        self.read_line();
                    }
                    ';' if self.peek_at(1) == Some(';') => {
                        self.skip_n(2);
                        tokens.push(Token::Comments(CommentKind::Line, self.read_line()));
                    }
                    _ => {
                        let (l, c) = (self.line, self.col);
                        self.advance();
                        return Err(LexError::UnexpectedChar(ch, l, c));
                    }
                }
                continue;
            }

            match ch {
                '\n' => { tokens.push(Token::Newline); self.advance(); }
                ' ' | '\t' | '\r' => { self.advance(); }

                // ── ~> print ─────────────────────────────────────────────────
                '~' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2); self.skip_ws();
                    tokens.push(Token::Print(self.read_line()));
                }

                // ── $( expr ) arytmetyka ──────────────────────────────────────
                '$' if self.peek_at(1) == Some('(') => {
                    self.skip_n(2);
                    let mut expr = String::new();
                    let mut depth = 1usize;
                    loop {
                        match self.peek() {
                            Some('(')  => { depth += 1; expr.push('('); self.advance(); }
                            Some(')')  => {
                                depth -= 1; self.advance();
                                if depth == 0 { break; }
                                expr.push(')');
                            }
                            Some(c)    => { expr.push(c); self.advance(); }
                            None       => break,
                        }
                    }
                    self.skip_ws();
                    let assign_to = if self.matches_seq(&['-', '>']) {
                        self.skip_n(2); self.skip_ws();
                        if self.peek() == Some('@') { self.advance(); }
                        let v = self.read_ident_full();
                        if v.is_empty() { None } else { Some(v) }
                    } else { None };
                    tokens.push(Token::Arithmetic { expr: expr.trim().to_string(), assign_to });
                }

                // ── || HackerOS API ───────────────────────────────────────────
                '|' if self.peek_at(1) == Some('|') => {
                    self.skip_n(2); self.skip_ws();
                    let mut tool = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_alphanumeric() || c == '-' || c == '#' { tool.push(c); self.advance(); }
                        else { break; }
                    }
                    self.skip_ws();
                    tokens.push(Token::HackerOsApi { tool, args: self.read_line() });
                }

                // ── | switch arm ──────────────────────────────────────────────
                '|' if self.peek_at(1) != Some('>') && self.peek_at(1) != Some('|') => {
                    self.advance(); self.skip_ws();
                    let line = self.read_line();
                    let pattern = if let Some(p) = line.find("->") {
                        line[..p].trim().to_string()
                    } else {
                        line.trim().to_string()
                    };
                    tokens.push(Token::SwitchArm { pattern });
                }

                // ── :** channel ───────────────────────────────────────────────
                ':' if self.matches_seq(&[':', '*', '*']) => {
                    self.skip_n(3); self.skip_ws();
                    tokens.push(Token::ChannelDecl(self.read_ident_full()));
                    self.read_line();
                }

                // ── :* goroutine ──────────────────────────────────────────────
                ':' if self.matches_seq(&[':', '*']) => {
                    self.skip_n(2); self.skip_ws();
                    let rest = self.read_line();
                    let rest = rest.trim();
                    let name = if rest.ends_with("def") {
                        let n = rest[..rest.len()-3].trim().to_string();
                        if n.is_empty() { None } else { Some(n) }
                    } else if rest.is_empty() { None }
                    else { Some(rest.to_string()) };
                    tokens.push(Token::GoroutineStart { name });
                }

                // ── :: arena function DEF lub QuickCall ───────────────────────
                //
                // Reguła rozróżnienia w lekserze:
                //   :: nazwa <rozmiar> def  → ArenaFuncDef
                //   :: nazwa [args...]      → QuickCall (parser rozróżni czy to wywołanie areny)
                ':' if self.peek_at(1) == Some(':') => {
                    self.skip_n(2); self.skip_ws();
                    let name = self.read_ident_full();
                    self.skip_ws();

                    // Próbuj rozpoznać def areny: <rozmiar> def  LUB  def
                    let saved = self.pos;
                    let saved_line = self.line;
                    let saved_col = self.col;

                    // Sprawdź czy następne to <rozmiar> def lub samo def
                    let arena_size_str = self.try_read_arena_size();
                    self.skip_ws();
                    let is_def = {
                        let mut tmp_pos = self.pos;
                        let mut kw = String::new();
                        while tmp_pos < self.source.len()
                            && self.source[tmp_pos].is_alphabetic()
                            {
                                kw.push(self.source[tmp_pos]);
                                tmp_pos += 1;
                            }
                            kw == "def"
                    };

                    if is_def {
                        // Pochłoń "def" i resztę linii
                        self.read_ident(); // "def"
                        self.read_line();
                        let size = arena_size_str.unwrap_or_default();
                        tokens.push(Token::ArenaFuncDef { name, arena_size: size });
                    } else {
                        // Cofnij po read_arena_size (jeśli było) i traktuj jako QuickCall
                        if arena_size_str.is_some() {
                            self.pos  = saved;
                            self.line = saved_line;
                            self.col  = saved_col;
                            self.skip_ws();
                        }
                        // Sprawdź czy linia zawiera |> @var (QuickPipeToVar)
                        let line = self.read_line();
                        if let Some((cmd_part, var_part)) = Self::split_quick_pipe(&line) {
                            tokens.push(Token::QuickPipeToVar {
                                name,
                                args:     cmd_part,
                                var_name: var_part,
                            });
                        } else {
                            tokens.push(Token::QuickCall { name, args: line });
                        }
                    }
                }

                ':' => {
                    self.advance(); self.skip_ws();
                    let name = self.read_ident_full(); self.skip_ws();
                    let kw = self.read_ident();
                    if kw == "def" { tokens.push(Token::FuncDef(name)); self.read_line(); }
                    else { tokens.push(Token::Ident(format!(":{} {}", name, kw))); }
                }

                // ── ;; komentarz ──────────────────────────────────────────────
                ';' if self.peek_at(1) == Some(';') => {
                    self.skip_n(2);
                    tokens.push(Token::Comments(CommentKind::Line, self.read_line()));
                }

                // ── /// doc comment ───────────────────────────────────────────
                '/' if self.matches_seq(&['/', '/', '/']) => {
                    self.skip_n(3);
                    tokens.push(Token::Comments(CommentKind::Doc, self.read_line()));
                }

                // ── // zależność lub blok ─────────────────────────────────────
                '/' if self.matches_seq(&['/', '/']) => {
                    self.skip_n(2); self.skip_ws();
                    let rest: String = self.source[self.pos..].iter().collect();
                    if let Some(end) = rest.find("\\\\") {
                        let content = rest[..end].trim().to_string();
                        self.skip_n(end + 2);
                        tokens.push(Token::Comments(CommentKind::Block, content));
                    } else {
                        // Parsuj: "// narzedzie [pakiet-apt]" lub "// narzedzie"
                        let raw_dep = self.read_line();
                        let raw_dep = raw_dep.trim();
                        // Rozdziel na bin_name i opcjonalny [apt-package]
                        let (bin_name, apt_pkg) = if let (Some(lb), Some(rb)) = (raw_dep.find('['), raw_dep.rfind(']')) {
                            let name = raw_dep[..lb].trim().to_string();
                            let pkg  = raw_dep[lb+1..rb].trim().to_string();
                            (name, if pkg.is_empty() { None } else { Some(pkg) })
                        } else {
                            (raw_dep.to_string(), None)
                        };
                        tokens.push(Token::Dependency(bin_name, apt_pkg));
                    }
                }

                // ── << file import ────────────────────────────────────────────
                // ── <* dir import (gen 2) ────────────────────────────────────────────────
                // <* katalog — ładuje katalog/imports.hl (odpowiednik mod.rs)
                '<' if self.peek_at(1) == Some('*') => {
                    self.skip_n(2); self.skip_ws();
                    let path = self.read_line().trim().to_string();
                    tokens.push(Token::DirImport { path });
                }

                '<' if self.peek_at(1) == Some('<') => {
                    self.skip_n(2); self.skip_ws();
                    let rest = self.read_line();
                    if let Some(p) = rest.find('|') {
                        let path   = rest[..p].trim().to_string();
                        let detail = rest[p+1..].trim().to_string();
                        tokens.push(Token::FileImport { path, detail: if detail.is_empty() { None } else { Some(detail) } });
                    } else {
                        tokens.push(Token::FileImport { path: rest.trim().to_string(), detail: None });
                    }
                }

                // ── -- func call ──────────────────────────────────────────────
                '-' if self.matches_seq(&['-', '-']) => {
                    self.skip_n(2); self.skip_ws();
                    tokens.push(Token::FuncCall(self.read_ident_full()));
                    self.read_line();
                }

                // ── => export ─────────────────────────────────────────────────
                '=' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2); self.skip_ws();
                    let name = self.read_ident_full(); self.skip_ws();
                    match self.peek() {
                        Some('=') => { self.advance(); self.skip_ws(); tokens.push(Token::ExportSingle { name, value: self.read_line() }); }
                        Some('[') => { self.advance(); self.read_line(); self.in_export_list = true; tokens.push(Token::ExportListStart(name)); }
                        _ => { tokens.push(Token::ExportSingle { name, value: String::new() }); }
                    }
                }

                '^' if self.matches_seq(&['^', '-', '>']) => { self.skip_n(3); tokens.push(Token::CmdIsolatedSudo(self.read_cmd())); }
                '^' if self.matches_seq(&['^', '>', '>']) => {
                    self.skip_n(3);
                    let line = self.read_cmd();
                    if let Some((cmd, var)) = Self::split_pipe_to_var(&line) {
                        tokens.push(Token::CmdPipeToVar { cmd, mode: PipeCmdMode::WithVars, var_name: var });
                    } else { tokens.push(Token::CmdWithVarsSudo(line)); }
                }
                '^' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2);
                    let line = self.read_cmd();
                    if let Some((cmd, var)) = Self::split_pipe_to_var(&line) {
                        tokens.push(Token::CmdPipeToVar { cmd, mode: PipeCmdMode::Sudo, var_name: var });
                    } else { tokens.push(Token::CmdSudo(line)); }
                }
                '^' => { self.advance(); }

                '-' if self.matches_seq(&['-', '>', '>']) => {
                    self.skip_n(3);
                    let line = self.read_cmd();
                    if let Some((cmd, var)) = Self::split_pipe_to_var(&line) {
                        tokens.push(Token::CmdPipeToVar { cmd, mode: PipeCmdMode::WithVars, var_name: var });
                    } else { tokens.push(Token::CmdWithVarsIsolated(line)); }
                }
                '-' if self.peek_at(1) == Some('>') => { self.skip_n(2); tokens.push(Token::CmdIsolated(self.read_cmd())); }

                '>' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2);
                    let line = self.read_cmd();
                    if let Some((cmd, var)) = Self::split_pipe_to_var(&line) {
                        tokens.push(Token::CmdPipeToVar { cmd, mode: PipeCmdMode::WithVars, var_name: var });
                    } else { tokens.push(Token::CmdWithVars(line)); }
                }
                '>' => {
                    self.advance();
                    let line = self.read_cmd();
                    if let Some((cmd, var)) = Self::split_pipe_to_var(&line) {
                        tokens.push(Token::CmdPipeToVar { cmd, mode: PipeCmdMode::Plain, var_name: var });
                    } else { tokens.push(Token::Cmd(line)); }
                }

                '&' => { self.advance(); self.skip_ws(); tokens.push(Token::Background(self.read_line())); }

                // ── _> extern ─────────────────────────────────────────────────
                // _> plik.sh [shell] def ... done
                // _> binarka [elf] def ... done
                '_' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2); self.skip_ws();
                    // Czytaj ścieżkę pliku (do pierwszego białego znaku lub '[')
                    let mut file = String::new();
                    while let Some(c) = self.peek() {
                        if c == ' ' || c == '\t' || c == '[' || c == '\n' { break; }
                        file.push(c); self.advance();
                    }
                    self.skip_ws();
                    // Czytaj [runtime]
                    let runtime = if self.peek() == Some('[') {
                        self.advance();
                        let mut rt = String::new();
                        while let Some(c) = self.peek() {
                            if c == ']' { self.advance(); break; }
                            if c == '\n' { break; }
                            rt.push(c); self.advance();
                        }
                        rt.trim().to_string()
                    } else {
                        // Próbuj zgadnąć runtime z rozszerzenia pliku
                        let ext = file.rsplit('.').next().unwrap_or("").to_lowercase();
                        match ext.as_str() {
                            "sh" | "bash"  => "shell".to_string(),
                            "py"           => "python".to_string(),
                            "jar"          => "java".to_string(),
                            "so"           => "so".to_string(),
                            _              => "elf".to_string(),
                        }
                    };
                    self.skip_ws();
                    // Pochłoń "def"
                    let kw = self.read_ident();
                    if kw != "def" {
                        // Cofnij — traktuj jako VarRef ze specjalną obsługą
                        tokens.push(Token::ExternStart { file, runtime });
                    } else {
                        self.read_line();
                        tokens.push(Token::ExternStart { file, runtime });
                    }
                }

                '*' if self.matches_seq(&['*', '-', '-']) => {
                    self.skip_n(3); self.skip_ws();
                    tokens.push(Token::ChannelOp(self.read_ident_full()));
                    self.read_line();
                }
                '*' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2); self.skip_ws();
                    tokens.push(Token::HshCmd(self.read_line()));
                }

                '_' => {
                    self.advance();
                    let mut num_str = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() { num_str.push(c); self.advance(); } else { break; }
                    }
                    if !num_str.is_empty() {
                        let next_is_alnum = self.peek()
                        .map(|c| c.is_alphanumeric() || c == '_')
                        .unwrap_or(false);
                        if !next_is_alnum {
                            tokens.push(Token::RepeatN(num_str.parse().unwrap_or(1)));
                            continue;
                        }
                        let mut id = format!("_{}", num_str);
                        id.push_str(&self.read_ident_full());
                        tokens.push(Token::VarRef(id));
                    } else {
                        let rest = self.read_ident_full();
                        if rest.is_empty() {
                            tokens.push(Token::Comments(CommentKind::Line, String::new()));
                        } else {
                            tokens.push(Token::VarRef(format!("_{}", rest)));
                        }
                    }
                }

                '@' => {
                    self.advance();
                    let name = self.read_ident_full();
                    self.skip_ws();
                    let looks_like_for = {
                        let mut tmp = self.pos;
                        let mut kw = String::new();
                        while tmp < self.source.len() && self.source[tmp].is_alphabetic() {
                            kw.push(self.source[tmp]);
                            tmp += 1;
                        }
                        kw == "in"
                    };
                    if looks_like_for {
                        let _in_kw = self.read_ident();
                        self.skip_ws();
                        tokens.push(Token::ForIn { var: name, iterable: self.read_line() });
                    } else {
                        tokens.push(Token::VarRef(name));
                    }
                }

                '%' => {
                    self.advance(); self.skip_ws();
                    let name = self.read_ident_full(); self.skip_ws();
                    let typ = if self.peek() == Some(':') {
                        self.advance(); self.skip_ws();
                        self.read_ident_full()
                    } else { String::new() };
                    self.skip_ws();
                    if self.peek() == Some('=') {
                        self.advance(); self.skip_ws();
                        tokens.push(Token::VarDecl { name, typ, value: self.read_line() });
                    } else {
                        tokens.push(Token::Ident(format!("%{}", name)));
                    }
                }

                '?' => {
                    self.advance(); self.skip_ws();
                    if self.peek() == Some('~') {
                        self.advance(); self.skip_ws();
                        tokens.push(Token::WhileStart(self.read_line()));
                    } else {
                        let kw = self.read_ident();
                        match kw.as_str() {
                            "ok"     => { tokens.push(Token::IfOk);  self.read_line(); }
                            "err"    => { tokens.push(Token::IfErr); self.read_line(); }
                            "switch" => { self.skip_ws(); tokens.push(Token::SwitchStart(self.read_line())); }
                            _        => tokens.push(Token::Ident(format!("?{}", kw))),
                        }
                    }
                }

                '#' => {
                    self.advance();
                    if self.peek() == Some('!') {
                        tokens.push(Token::Comments(CommentKind::Line, self.read_line()));
                        continue;
                    }
                    self.skip_ws();
                    let rest = self.read_line();
                    if let Some(decl) = parse_import_line(&rest) {
                        tokens.push(Token::Import { lib: decl.spec, detail: decl.detail });
                    } else {
                        tokens.push(Token::Import { lib: rest, detail: None });
                    }
                }

                '"' => { tokens.push(Token::StringLit(self.read_string_lit()?)); }

                c if c.is_ascii_digit() => { tokens.push(Token::Number(self.read_number())); }

                c if c.is_alphabetic() => {
                    let id = self.read_ident_full();
                    match id.as_str() {
                        "done"  => { tokens.push(Token::Done); self.read_line(); }
                        "using" => { self.skip_ws(); let rest = self.read_line(); tokens.push(Token::Using(format!("using {}", rest))); }
                        "true"  => tokens.push(Token::Bool(true)),
                        "false" => tokens.push(Token::Bool(false)),
                        _       => tokens.push(Token::Ident(id)),
                    }
                }

                '/' => {
                    let mut path = String::from("/");
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c.is_alphanumeric() || matches!(c, '/' | '.' | '-' | '_') {
                            path.push(c); self.advance();
                        } else { break; }
                    }
                    tokens.push(Token::Ident(path));
                }

                '=' => { self.advance(); }

                '.' => {
                    let mut path = String::from(".");
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c.is_alphanumeric() || matches!(c, '/' | '.' | '-' | '_') {
                            path.push(c); self.advance();
                        } else { break; }
                    }
                    tokens.push(Token::Ident(path));
                }

                _ => {
                    let (l, c) = (self.line, self.col);
                    self.advance();
                    return Err(LexError::UnexpectedChar(ch, l, c));
                }
            }
        }

        tokens.push(Token::Eof);
        Ok(tokens)
    }
}
