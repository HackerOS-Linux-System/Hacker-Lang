use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // ── Komentarze ───────────────────────────────────────────────────────────
    LineComment(String),
    DocComment(String),
    BlockComment(String),

    // ── Wyjście ──────────────────────────────────────────────────────────────
    /// ~> message  — główny operator print (zastępuje echo)
    Print(String),
    /// :: name [args]  — wywołanie quick-funkcji (:upper, :lower, :len, ...)
    QuickCall { name: String, args: String },

    // ── Komendy systemowe (longest prefix first) ─────────────────────────────
    CmdIsolatedSudo(String),       // ^->
    CmdWithVarsSudo(String),       // ^>>
    CmdWithVarsIsolated(String),   // ->>
    CmdSudo(String),               // ^>
    CmdIsolated(String),           // ->
    CmdWithVars(String),           // >>
    Cmd(String),                   // >

    // ── Zmienne ──────────────────────────────────────────────────────────────
    VarDecl { name: String, value: String },
    VarRef(String),

    // ── Zależności / importy ─────────────────────────────────────────────────
    Dependency(String),
    Import { lib: String, detail: Option<String> },

    // ── Funkcje ──────────────────────────────────────────────────────────────
    FuncDef(String),
    FuncCall(String),

    // ── Logika ───────────────────────────────────────────────────────────────
    IfOk,
    IfErr,
    Done,

    // ── Literały ─────────────────────────────────────────────────────────────
    Ident(String),
    StringLit(String),
    Number(f64),
    Bool(bool),

    // ── Struktura ────────────────────────────────────────────────────────────
    Newline,
    Indent(usize),
    Eof,
}

#[derive(Debug, Error)]
pub enum LexError {
    #[error("Unexpected character '{0}' at line {1}:{2}")]
    UnexpectedChar(char, usize, usize),
    #[error("Unterminated string at line {0}")]
    UnterminatedString(usize),
    #[error("Unterminated block comment")]
    UnterminatedBlockComment,
}

pub struct Lexer {
    source: Vec<char>,
    pub pos:  usize,
    pub line: usize,
    pub col:  usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        // Pre-allocate chars vec from str — faster than indexing UTF-8 repeatedly
        Self {
            source: source.chars().collect(),
            pos: 0, line: 1, col: 1,
        }
    }

    #[inline] pub fn peek(&self) -> Option<char> { self.source.get(self.pos).copied() }
    #[inline] fn peek_at(&self, n: usize) -> Option<char> { self.source.get(self.pos+n).copied() }

    #[inline]
    fn matches(&self, seq: &[char]) -> bool {
        self.source.get(self.pos..self.pos+seq.len()) == Some(seq)
    }

    #[inline]
    pub fn advance(&mut self) -> Option<char> {
        let ch = self.source.get(self.pos).copied();
        if let Some(c) = ch {
            self.pos += 1;
            if c == '\n' { self.line += 1; self.col = 1; }
            else { self.col += 1; }
        }
        ch
    }

    #[inline] fn skip_n(&mut self, n: usize) { for _ in 0..n { self.advance(); } }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t')) { self.advance(); }
    }

    /// Optimised: collect chars until newline using extend, then trim in-place
    fn read_line(&mut self) -> String {
        let start = self.pos;
        let mut end = self.pos;
        while end < self.source.len() && self.source[end] != '\n' {
            end += 1;
        }
        let s: String = self.source[start..end].iter().collect();
        // advance pos
        for _ in start..end { self.advance(); }
        // trim_end without re-allocation
        let trimmed = s.trim_end().to_string();
        // also trim_start without re-allocation
        let trim_len = trimmed.trim_start().len();
        if trim_len < trimmed.len() {
            trimmed[trimmed.len()-trim_len..].to_string()
        } else {
            trimmed
        }
    }

    fn read_string_lit(&mut self) -> Result<String, LexError> {
        let start_line = self.line;
        self.advance(); // consume "
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
            if c.is_alphanumeric() || c == '_' { s.push(c); self.advance(); }
            else { break; }
        }
        s
    }

    /// Ident with hyphens (function names: check-host, port-scan)
    fn read_ident_full(&mut self) -> String {
        let mut s = String::with_capacity(16);
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' { s.push(c); self.advance(); }
            else { break; }
        }
        // strip trailing hyphens (avoid swallowing -> etc.)
        while s.ends_with('-') { s.pop(); }
        s
    }

    fn read_number(&mut self) -> f64 {
        let mut s = String::with_capacity(16);
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' { s.push(c); self.advance(); }
            else { break; }
        }
        s.parse().unwrap_or(0.0)
    }

    #[inline]
    fn read_cmd(&mut self) -> String {
        self.skip_ws();
        self.read_line()
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        // Pre-allocate with generous capacity to avoid resizes
        let mut tokens = Vec::with_capacity(self.source.len() / 8 + 16);

        while self.pos < self.source.len() {
            let ch = match self.peek() { None => break, Some(c) => c };

            match ch {
                '\n' => { tokens.push(Token::Newline); self.advance(); }
                ' ' | '\t' | '\r' => { self.advance(); }

                // ── ~> print (główny operator output) ────────────────────────
                '~' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2);
                    self.skip_ws();
                    tokens.push(Token::Print(self.read_line()));
                }

                // ── :: quick-call ────────────────────────────────────────────
                ':' if self.peek_at(1) == Some(':') => {
                    self.skip_n(2);
                    self.skip_ws();
                    let name = self.read_ident();
                    self.skip_ws();
                    let args = self.read_line();
                    tokens.push(Token::QuickCall { name, args });
                }

                // ── : funcname def ───────────────────────────────────────────
                ':' => {
                    self.advance();
                    self.skip_ws();
                    let name = self.read_ident_full();
                    self.skip_ws();
                    let kw = self.read_ident();
                    if kw == "def" {
                        tokens.push(Token::FuncDef(name));
                        self.read_line();
                    } else {
                        tokens.push(Token::Ident(format!(":{} {}", name, kw)));
                    }
                }

                // ── ;; line comment ──────────────────────────────────────────
                ';' if self.peek_at(1) == Some(';') => {
                    self.skip_n(2);
                    tokens.push(Token::LineComment(self.read_line()));
                }

                // ── /// doc comment ──────────────────────────────────────────
                '/' if self.matches(&['/', '/', '/']) => {
                    self.skip_n(3);
                    tokens.push(Token::DocComment(self.read_line()));
                }

                // ── // block or dep ──────────────────────────────────────────
                '/' if self.matches(&['/', '/']) => {
                    self.skip_n(2);
                    self.skip_ws();
                    let rest: String = self.source[self.pos..].iter().collect();
                    if let Some(end) = rest.find("\\\\") {
                        let content = rest[..end].trim().to_string();
                        self.skip_n(end + 2);
                        tokens.push(Token::BlockComment(content));
                    } else {
                        tokens.push(Token::Dependency(self.read_line()));
                    }
                }

                // ── -- func call ─────────────────────────────────────────────
                '-' if self.matches(&['-', '-']) => {
                    self.skip_n(2);
                    self.skip_ws();
                    let name = self.read_ident_full();
                    tokens.push(Token::FuncCall(name));
                    self.read_line();
                }

                // ── ^-> isolated+sudo ────────────────────────────────────────
                '^' if self.matches(&['^', '-', '>']) => {
                    self.skip_n(3);
                    tokens.push(Token::CmdIsolatedSudo(self.read_cmd()));
                }
                // ── ^>> vars+sudo ────────────────────────────────────────────
                '^' if self.matches(&['^', '>', '>']) => {
                    self.skip_n(3);
                    tokens.push(Token::CmdWithVarsSudo(self.read_cmd()));
                }
                // ── ^> sudo ──────────────────────────────────────────────────
                '^' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2);
                    tokens.push(Token::CmdSudo(self.read_cmd()));
                }
                '^' => { self.advance(); }

                // ── ->> vars+isolated ────────────────────────────────────────
                '-' if self.matches(&['-', '>', '>']) => {
                    self.skip_n(3);
                    tokens.push(Token::CmdWithVarsIsolated(self.read_cmd()));
                }
                // ── -> isolated ──────────────────────────────────────────────
                '-' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2);
                    tokens.push(Token::CmdIsolated(self.read_cmd()));
                }

                // ── >> vars ──────────────────────────────────────────────────
                '>' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2);
                    tokens.push(Token::CmdWithVars(self.read_cmd()));
                }
                // ── > plain ──────────────────────────────────────────────────
                '>' => {
                    self.advance();
                    tokens.push(Token::Cmd(self.read_cmd()));
                }

                // ── % var = val ──────────────────────────────────────────────
                '%' => {
                    self.advance();
                    self.skip_ws();
                    let name = self.read_ident_full();
                    self.skip_ws();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.skip_ws();
                        let val = self.read_line();
                        tokens.push(Token::VarDecl { name, value: val });
                    } else {
                        tokens.push(Token::Ident(format!("%{}", name)));
                    }
                }

                // ── @varref ──────────────────────────────────────────────────
                '@' => {
                    self.advance();
                    tokens.push(Token::VarRef(self.read_ident_full()));
                }

                // ── ? ok / ? err ─────────────────────────────────────────────
                '?' => {
                    self.advance();
                    self.skip_ws();
                    let kw = self.read_ident();
                    match kw.as_str() {
                        "ok"  => { tokens.push(Token::IfOk);  self.read_line(); }
                        "err" => { tokens.push(Token::IfErr); self.read_line(); }
                        _     => tokens.push(Token::Ident(format!("?{}", kw))),
                    }
                }

                // ── # lib or # lib <- detail ─────────────────────────────────
                '#' => {
                    self.advance();
                    self.skip_ws();
                    let rest = self.read_line();
                    if let Some(pos) = rest.find("<-") {
                        let lib    = rest[..pos].trim().to_string();
                        let detail = rest[pos+2..].trim().to_string();
                        tokens.push(Token::Import {
                            lib,
                            detail: if detail.is_empty() { None } else { Some(detail) },
                        });
                    } else {
                        tokens.push(Token::Import { lib: rest, detail: None });
                    }
                }

                '"' => {
                    tokens.push(Token::StringLit(self.read_string_lit()?));
                }

                c if c.is_ascii_digit() => {
                    tokens.push(Token::Number(self.read_number()));
                }

                c if c.is_alphabetic() || c == '_' => {
                    let id = self.read_ident_full();
                    match id.as_str() {
                        "done"  => { tokens.push(Token::Done); self.read_line(); }
                        "true"  => tokens.push(Token::Bool(true)),
                        "false" => tokens.push(Token::Bool(false)),
                        _       => tokens.push(Token::Ident(id)),
                    }
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
