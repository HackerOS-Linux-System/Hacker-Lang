use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Comments
    LineComment(String),
    DocComment(String),
    BlockComment(String),

    // Output
    Print(String), // ::

    // Commands
    Cmd(String),          // >
    CmdSudo(String),      // ^>
    CmdIsolated(String),  // ->
    CmdIsolatedSudo(String), // ^->
    CmdWithVars(String),  // >>
    CmdWithVarsSudo(String), // ^>>

    // Variables
    VarDecl { name: String, value: String }, // % name = value
    VarRef(String), // @name

    // Dependencies
    Dependency(String), // //

    // Functions
    FuncDef(String),  // : name def
    FuncCall(String), // -- name

    // Logic
    IfOk,    // ? ok
    IfErr,   // ? err
    Done,    // done

    // Identifiers and literals
    Ident(String),
    StringLit(String),
    Number(f64),
    Bool(bool),

    // Structural
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
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.source.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.source.get(self.pos).copied();
        if let Some(c) = ch {
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        ch
    }

    fn skip_whitespace_inline(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_until_newline(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            s.push(c);
            self.advance();
        }
        s.trim().to_string()
    }

    fn read_string(&mut self) -> Result<String, LexError> {
        let start_line = self.line;
        self.advance(); // consume opening "
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err(LexError::UnterminatedString(start_line)),
                Some('"') => break,
                Some('\\') => {
                    match self.advance() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('"') => s.push('"'),
                        Some('\\') => s.push('\\'),
                        Some(c) => { s.push('\\'); s.push(c); }
                        None => return Err(LexError::UnterminatedString(start_line)),
                    }
                }
                Some(c) => s.push(c),
            }
        }
        Ok(s)
    }

    fn read_ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn read_number(&mut self) -> f64 {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        s.parse().unwrap_or(0.0)
    }

    fn read_cmd_args(&mut self) -> String {
        self.skip_whitespace_inline();
        self.read_until_newline()
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();

        while self.pos < self.source.len() {
            let ch = match self.peek() {
                None => break,
                Some(c) => c,
            };

            match ch {
                '\n' => {
                    tokens.push(Token::Newline);
                    self.advance();
                }
                ' ' | '\t' | '\r' => {
                    self.advance();
                }
                // Doc comment ///
                '/' if self.source.get(self.pos..self.pos+3) == Some(&['/', '/', '/']) => {
                    self.pos += 3;
                    self.col += 3;
                    let text = self.read_until_newline();
                    tokens.push(Token::DocComment(text));
                }
                // Block comment // ... \\
                '/' if self.source.get(self.pos..self.pos+2) == Some(&['/', '/']) => {
                    self.pos += 2;
                    self.col += 2;
                    self.skip_whitespace_inline();
                    // Check if it's a dependency or block comment
                    // Block comment ends with \\
                    let rest: String = self.source[self.pos..].iter().collect();
                    if let Some(end_pos) = rest.find("\\\\") {
                        let content = rest[..end_pos].trim().to_string();
                        // advance past content and \\
                        for _ in 0..(end_pos + 2) {
                            self.advance();
                        }
                        tokens.push(Token::BlockComment(content));
                    } else {
                        // It's a dependency declaration
                        let dep = self.read_until_newline();
                        tokens.push(Token::Dependency(dep));
                    }
                }
                // Line comment ;;
                ';' if self.peek2() == Some(';') => {
                    self.advance();
                    self.advance();
                    let text = self.read_until_newline();
                    tokens.push(Token::LineComment(text));
                }
                // Print ::
                ':' if self.peek2() == Some(':') => {
                    self.advance();
                    self.advance();
                    self.skip_whitespace_inline();
                    let msg = self.read_until_newline();
                    tokens.push(Token::Print(msg));
                }
                // Function def : name def
                ':' => {
                    self.advance();
                    self.skip_whitespace_inline();
                    let name = self.read_ident();
                    self.skip_whitespace_inline();
                    // expect 'def'
                    let keyword = self.read_ident();
                    if keyword == "def" {
                        tokens.push(Token::FuncDef(name));
                    } else {
                        tokens.push(Token::Ident(format!(":{} {}", name, keyword)));
                    }
                }
                // Function call -- name
                '-' if self.source.get(self.pos..self.pos+2) == Some(&['-', '-']) => {
                    self.advance();
                    self.advance();
                    self.skip_whitespace_inline();
                    let name = self.read_ident();
                    tokens.push(Token::FuncCall(name));
                }
                // CmdIsolatedSudo ^->
                '^' if self.source.get(self.pos..self.pos+3) == Some(&['^', '-', '>']) => {
                    self.advance(); self.advance(); self.advance();
                    let args = self.read_cmd_args();
                    tokens.push(Token::CmdIsolatedSudo(args));
                }
                // CmdWithVarsSudo ^>>
                '^' if self.source.get(self.pos..self.pos+3) == Some(&['^', '>', '>']) => {
                    self.advance(); self.advance(); self.advance();
                    let args = self.read_cmd_args();
                    tokens.push(Token::CmdWithVarsSudo(args));
                }
                // CmdSudo ^>
                '^' if self.source.get(self.pos..self.pos+2) == Some(&['^', '>']) => {
                    self.advance(); self.advance();
                    let args = self.read_cmd_args();
                    tokens.push(Token::CmdSudo(args));
                }
                '^' => {
                    self.advance();
                    tokens.push(Token::Ident("^".to_string()));
                }
                // CmdIsolated ->
                '-' if self.peek2() == Some('>') => {
                    self.advance(); self.advance();
                    let args = self.read_cmd_args();
                    tokens.push(Token::CmdIsolated(args));
                }
                // CmdWithVars >>
                '>' if self.peek2() == Some('>') => {
                    self.advance(); self.advance();
                    let args = self.read_cmd_args();
                    tokens.push(Token::CmdWithVars(args));
                }
                // Cmd >
                '>' => {
                    self.advance();
                    let args = self.read_cmd_args();
                    tokens.push(Token::Cmd(args));
                }
                // Variable declaration % name = value
                '%' => {
                    self.advance();
                    self.skip_whitespace_inline();
                    let name = self.read_ident();
                    self.skip_whitespace_inline();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.skip_whitespace_inline();
                        let value = self.read_until_newline();
                        tokens.push(Token::VarDecl { name, value });
                    } else {
                        tokens.push(Token::Ident(format!("%{}", name)));
                    }
                }
                // Variable reference @name
                '@' => {
                    self.advance();
                    let name = self.read_ident();
                    tokens.push(Token::VarRef(name));
                }
                // Logic ? ok / ? err
                '?' => {
                    self.advance();
                    self.skip_whitespace_inline();
                    let keyword = self.read_ident();
                    match keyword.as_str() {
                        "ok" => tokens.push(Token::IfOk),
                        "err" => tokens.push(Token::IfErr),
                        _ => tokens.push(Token::Ident(format!("?{}", keyword))),
                    }
                }
                // String literal
                '"' => {
                    let s = self.read_string()?;
                    tokens.push(Token::StringLit(s));
                }
                // Number
                c if c.is_ascii_digit() => {
                    let n = self.read_number();
                    tokens.push(Token::Number(n));
                }
                // Identifier or keyword
                c if c.is_alphabetic() || c == '_' => {
                    let ident = self.read_ident();
                    match ident.as_str() {
                        "done" => tokens.push(Token::Done),
                        "true" => tokens.push(Token::Bool(true)),
                        "false" => tokens.push(Token::Bool(false)),
                        _ => tokens.push(Token::Ident(ident)),
                    }
                }
                _ => {
                    let line = self.line;
                    let col = self.col;
                    self.advance();
                    return Err(LexError::UnexpectedChar(ch, line, col));
                }
            }
        }

        tokens.push(Token::Eof);
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print() {
        let mut lexer = Lexer::new(":: Hello World");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Print("Hello World".to_string()));
    }

    #[test]
    fn test_cmd() {
        let mut lexer = Lexer::new("> ls -la");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Cmd("ls -la".to_string()));
    }

    #[test]
    fn test_var_decl() {
        let mut lexer = Lexer::new("% target = 192.168.1.1");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::VarDecl {
            name: "target".to_string(),
                   value: "192.168.1.1".to_string(),
        });
    }
}
