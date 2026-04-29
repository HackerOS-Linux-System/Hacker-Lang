use thiserror::Error;
use crate::import_spec::parse_import_line;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    LineComment(String),
    DocComment(String),
    BlockComment(String),
    Print(String),
    QuickCall { name: String, args: String },
    CmdIsolatedSudo(String),
    CmdWithVarsSudo(String),
    CmdWithVarsIsolated(String),
    CmdSudo(String),
    CmdIsolated(String),
    CmdWithVars(String),
    Cmd(String),
    /// *> komenda — uruchom przez hsh -c
    HshCmd(String),
    /// & komenda — uruchom w tle
    Background(String),
    VarDecl { name: String, value: String },
    VarRef(String),
    ExportSingle { name: String, value: String },
    ExportListStart(String),
    ExportListItem(String),
    ExportListEnd,
    Dependency(String),
    Import { lib: String, detail: Option<String> },
    /// << plik.hl | szczegoly — import zewnetrznego pliku .hl
    FileImport { path: String, detail: Option<String> },
    FuncDef(String),
    FuncCall(String),
    IfOk,
    IfErr,
    Done,
    /// using <gen N>
    Using(String),
    /// :* — goroutine start
    GoroutineStart,
    /// :** nazwa — channel declaration
    ChannelDecl(String),
    /// *-- nazwa — channel send/receive
    ChannelOp(String),
    /// _N — repeat N times (next token/block)
    RepeatN(u64),
    Ident(String),
    StringLit(String),
    Number(f64),
    Bool(bool),
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
    in_export_list: bool,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self { source: source.chars().collect(), pos: 0, line: 1, col: 1, in_export_list: false }
    }

    #[inline] pub fn peek(&self) -> Option<char> { self.source.get(self.pos).copied() }
    #[inline] fn peek_at(&self, n: usize) -> Option<char> { self.source.get(self.pos+n).copied() }
    #[inline] fn matches(&self, seq: &[char]) -> bool {
        self.source.get(self.pos..self.pos+seq.len()) == Some(seq)
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

    #[inline] fn skip_n(&mut self, n: usize) { for _ in 0..n { self.advance(); } }
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

    #[inline] fn read_cmd(&mut self) -> String { self.skip_ws(); self.read_line() }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::with_capacity(self.source.len() / 8 + 16);

        while self.pos < self.source.len() {
            let ch = match self.peek() { None => break, Some(c) => c };

            if self.in_export_list {
                match ch {
                    '\n' => { tokens.push(Token::Newline); self.advance(); continue; }
                    ' ' | '\t' | '\r' => { self.advance(); continue; }
                    '|' => {
                        self.advance(); self.skip_ws();
                        tokens.push(Token::ExportListItem(self.read_line()));
                        continue;
                    }
                    ']' => {
                        self.advance();
                        self.in_export_list = false;
                        tokens.push(Token::ExportListEnd);
                        self.read_line();
                        continue;
                    }
                    ';' if self.peek_at(1) == Some(';') => {
                        self.skip_n(2);
                        tokens.push(Token::LineComment(self.read_line()));
                        continue;
                    }
                    _ => {
                        let (l, c) = (self.line, self.col);
                        self.advance();
                        return Err(LexError::UnexpectedChar(ch, l, c));
                    }
                }
            }

            match ch {
                '\n' => { tokens.push(Token::Newline); self.advance(); }
                ' ' | '\t' | '\r' => { self.advance(); }

                '~' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2); self.skip_ws();
                    tokens.push(Token::Print(self.read_line()));
                }

                ':' if self.matches(&[':', '*', '*']) => {
                    // :** nazwa — channel declaration
                    self.skip_n(3); self.skip_ws();
                    tokens.push(Token::ChannelDecl(self.read_ident_full()));
                    self.read_line();
                }

                ':' if self.matches(&[':', '*']) => {
                    // :* — goroutine start
                    self.skip_n(2); self.read_line();
                    tokens.push(Token::GoroutineStart);
                }

                ':' if self.peek_at(1) == Some(':') => {
                    self.skip_n(2); self.skip_ws();
                    let name = self.read_ident(); self.skip_ws();
                    tokens.push(Token::QuickCall { name, args: self.read_line() });
                }

                ':' => {
                    self.advance(); self.skip_ws();
                    let name = self.read_ident_full(); self.skip_ws();
                    let kw = self.read_ident();
                    if kw == "def" {
                        tokens.push(Token::FuncDef(name)); self.read_line();
                    } else {
                        tokens.push(Token::Ident(format!(":{} {}", name, kw)));
                    }
                }

                ';' if self.peek_at(1) == Some(';') => {
                    self.skip_n(2);
                    tokens.push(Token::LineComment(self.read_line()));
                }

                '/' if self.matches(&['/', '/', '/']) => {
                    self.skip_n(3);
                    tokens.push(Token::DocComment(self.read_line()));
                }

                '/' if self.matches(&['/', '/']) => {
                    self.skip_n(2); self.skip_ws();
                    let rest: String = self.source[self.pos..].iter().collect();
                    if let Some(end) = rest.find("\\\\") {
                        let content = rest[..end].trim().to_string();
                        self.skip_n(end + 2);
                        tokens.push(Token::BlockComment(content));
                    } else {
                        tokens.push(Token::Dependency(self.read_line()));
                    }
                }

                // << plik.hl | szczegoly — import zewnetrznego pliku
                '<' if self.peek_at(1) == Some('<') => {
                    self.skip_n(2); self.skip_ws();
                    let rest = self.read_line();
                    if let Some(pipe_pos) = rest.find('|') {
                        let path   = rest[..pipe_pos].trim().to_string();
                        let detail = rest[pipe_pos+1..].trim().to_string();
                        tokens.push(Token::FileImport {
                            path,
                            detail: if detail.is_empty() { None } else { Some(detail) },
                        });
                    } else {
                        tokens.push(Token::FileImport { path: rest.trim().to_string(), detail: None });
                    }
                }

                '-' if self.matches(&['-', '-']) => {
                    self.skip_n(2); self.skip_ws();
                    tokens.push(Token::FuncCall(self.read_ident_full()));
                    self.read_line();
                }

                '=' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2); self.skip_ws();
                    let name = self.read_ident_full(); self.skip_ws();
                    match self.peek() {
                        Some('=') => {
                            self.advance(); self.skip_ws();
                            tokens.push(Token::ExportSingle { name, value: self.read_line() });
                        }
                        Some('[') => {
                            self.advance(); self.read_line();
                            self.in_export_list = true;
                            tokens.push(Token::ExportListStart(name));
                        }
                        _ => { tokens.push(Token::ExportSingle { name, value: String::new() }); }
                    }
                }

                '^' if self.matches(&['^', '-', '>']) => { self.skip_n(3); tokens.push(Token::CmdIsolatedSudo(self.read_cmd())); }
                '^' if self.matches(&['^', '>', '>']) => { self.skip_n(3); tokens.push(Token::CmdWithVarsSudo(self.read_cmd())); }
                '^' if self.peek_at(1) == Some('>') => { self.skip_n(2); tokens.push(Token::CmdSudo(self.read_cmd())); }
                '^' => { self.advance(); }

                '-' if self.matches(&['-', '>', '>']) => { self.skip_n(3); tokens.push(Token::CmdWithVarsIsolated(self.read_cmd())); }
                '-' if self.peek_at(1) == Some('>') => { self.skip_n(2); tokens.push(Token::CmdIsolated(self.read_cmd())); }

                '>' if self.peek_at(1) == Some('>') => { self.skip_n(2); tokens.push(Token::CmdWithVars(self.read_cmd())); }
                '>' => { self.advance(); tokens.push(Token::Cmd(self.read_cmd())); }

                // & komenda — uruchom w tle
                '&' => { self.advance(); self.skip_ws(); tokens.push(Token::Background(self.read_line())); }

                // *-- nazwa — channel op
                '*' if self.matches(&['*', '-', '-']) => {
                    self.skip_n(3); self.skip_ws();
                    tokens.push(Token::ChannelOp(self.read_ident_full()));
                    self.read_line();
                }

                // *> komenda — hsh shell
                '*' if self.peek_at(1) == Some('>') => {
                    self.skip_n(2); self.skip_ws();
                    tokens.push(Token::HshCmd(self.read_line()));
                }

                // _N > komenda lub _N ;; linia — powtorz N razy
                '_' => {
                    self.advance();
                    // Czytaj cyfry
                    let mut num_str = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() { num_str.push(c); self.advance(); } else { break; }
                    }
                    if !num_str.is_empty() {
                        let n: u64 = num_str.parse().unwrap_or(1);
                        tokens.push(Token::RepeatN(n));
                    } else {
                        // identyfikator zaczynajacy sie od _
                        let mut id = String::from("_");
                        id.push_str(&self.read_ident_full());
                        tokens.push(Token::Ident(id));
                    }
                }

                '%' => {
                    self.advance(); self.skip_ws();
                    let name = self.read_ident_full(); self.skip_ws();
                    if self.peek() == Some('=') {
                        self.advance(); self.skip_ws();
                        tokens.push(Token::VarDecl { name, value: self.read_line() });
                    } else {
                        tokens.push(Token::Ident(format!("%{}", name)));
                    }
                }

                '@' => { self.advance(); tokens.push(Token::VarRef(self.read_ident_full())); }

                '?' => {
                    self.advance(); self.skip_ws();
                    let kw = self.read_ident();
                    match kw.as_str() {
                        "ok"  => { tokens.push(Token::IfOk);  self.read_line(); }
                        "err" => { tokens.push(Token::IfErr); self.read_line(); }
                        _     => tokens.push(Token::Ident(format!("?{}", kw))),
                    }
                }

                '#' => {
                    self.advance(); self.skip_ws();
                    let rest = self.read_line();
                    if let Some(decl) = parse_import_line(&rest) {
                        tokens.push(Token::Import { lib: decl.spec, detail: decl.detail });
                    } else {
                        tokens.push(Token::Import { lib: rest, detail: None });
                    }
                }

                '"' => { tokens.push(Token::StringLit(self.read_string_lit()?)); }

                c if c.is_ascii_digit() => { tokens.push(Token::Number(self.read_number())); }

                c if c.is_alphabetic() || c == '_' => {
                    let id = self.read_ident_full();
                    match id.as_str() {
                        "done"  => { tokens.push(Token::Done); self.read_line(); }
                        "using" => { self.skip_ws(); let rest = self.read_line(); tokens.push(Token::Using(format!("using {}", rest))); }
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
