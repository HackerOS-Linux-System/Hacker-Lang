use crate::ast::*;
use crate::gen::{Gen, GenError, extract_gen};
use crate::shebang::preprocess;
use crate::lexer::{LexError, Lexer, Token};
use crate::{ParseMeta, ShebangInfo};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Lex error: {0}")]
    Lex(#[from] LexError),
    #[error("Unexpected token at position {0}: {1}")]
    UnexpectedToken(usize, String),
    #[error("Expected 'done' to close block, got EOF")]
    MissingDone,
    #[error("Expected 'def' after function name")]
    MissingDef,
    #[error("Expected ']' to close export list")]
    MissingExportListEnd,
    #[error("Gen error: {0}")]
    Gen(#[from] GenError),
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Self { tokens, pos: 0 } }

    #[inline] fn peek(&self) -> &Token { self.tokens.get(self.pos).unwrap_or(&Token::Eof) }

    fn advance(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        t
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline | Token::Indent(_)) { self.advance(); }
    }

    fn parse_var_value(value: String) -> VarValue {
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            let inner = &value[1..value.len()-1];
            let parts = parse_string_parts(inner);
            if parts.iter().any(|p| matches!(p, StringPart::Var(_))) { return VarValue::Interpolated(parts); }
            return VarValue::String(inner.to_string());
        }
        if let Ok(n) = value.parse::<f64>() { return VarValue::Number(n); }
        if value == "true"  { return VarValue::Bool(true);  }
        if value == "false" { return VarValue::Bool(false); }
        let parts = parse_string_parts(&value);
        if parts.iter().any(|p| matches!(p, StringPart::Var(_))) { VarValue::Interpolated(parts) }
        else { VarValue::String(value) }
    }

    fn parse_export_list(&mut self) -> Result<Vec<Vec<StringPart>>, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::ExportListEnd => { self.advance(); break; }
                Token::ExportListItem(val) => { self.advance(); items.push(parse_string_parts(&val)); }
                Token::Eof => return Err(ParseError::MissingExportListEnd),
                _ => { self.advance(); }
            }
        }
        Ok(items)
    }

    fn parse_node(&mut self) -> Result<Option<Node>, ParseError> {
        self.skip_newlines();
        match self.peek().clone() {
            Token::Eof | Token::Done => Ok(None),
            Token::Newline | Token::Indent(_) => { self.advance(); Ok(None) }
            Token::LineComment(t)  => { self.advance(); Ok(Some(Node::LineComment(t))) }
            Token::DocComment(t)   => { self.advance(); Ok(Some(Node::DocComment(t))) }
            Token::BlockComment(t) => { self.advance(); Ok(Some(Node::BlockComment(t))) }

            Token::Using(ref decl) => {
                self.advance();
                Ok(Some(Node::LineComment(format!("gen-decl: {}", decl))))
            }
            Token::Ident(ref s) if s == "using" => {
                self.advance();
                Ok(Some(Node::LineComment("gen-decl (legacy)".into())))
            }

            Token::Print(msg) => {
                self.advance();
                Ok(Some(Node::Print { parts: parse_string_parts(&msg) }))
            }
            Token::QuickCall { name, args } => {
                self.advance();
                Ok(Some(Node::QuickCall { name, args: parse_string_parts(&args) }))
            }

            // & komenda — uruchom w tle
            Token::Background(raw) => { self.advance(); Ok(Some(Node::Background { raw })) }

            // *> komenda — uruchom przez hsh
            Token::HshCmd(raw) => { self.advance(); Ok(Some(Node::HshCommand { raw })) }

            // _N — powtorz N razy (nastepny token lub blok)
            Token::RepeatN(n) => {
                self.advance();
                self.skip_newlines();
                // Zbierz nastepna komende/linie jako body
                let body = if let Some(node) = self.parse_node()? {
                    vec![node]
                } else {
                    vec![]
                };
                Ok(Some(Node::RepeatN { count: n, body }))
            }

            // << plik.hl — import zewnetrznego pliku
            Token::FileImport { path, detail } => {
                self.advance();
                Ok(Some(Node::FileImport { path, detail }))
            }

            // :* — goroutine
            Token::GoroutineStart => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::Goroutine { body }))
            }

            // :** nazwa — channel declaration
            Token::ChannelDecl(name) => {
                self.advance();
                Ok(Some(Node::Channel { name }))
            }

            // *-- nazwa — channel op
            Token::ChannelOp(name) => {
                self.advance();
                Ok(Some(Node::ChannelOp { name, value: None }))
            }

            Token::Cmd(raw)                 => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Plain,            interpolate: false })) }
            Token::CmdSudo(raw)             => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Sudo,             interpolate: false })) }
            Token::CmdIsolated(raw)         => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Isolated,         interpolate: false })) }
            Token::CmdIsolatedSudo(raw)     => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::IsolatedSudo,     interpolate: false })) }
            Token::CmdWithVars(raw)         => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVars,         interpolate: true  })) }
            Token::CmdWithVarsSudo(raw)     => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVarsSudo,     interpolate: true  })) }
            Token::CmdWithVarsIsolated(raw) => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVarsIsolated, interpolate: true  })) }
            Token::VarDecl { name, value }  => { self.advance(); Ok(Some(Node::VarDecl { name, value: Self::parse_var_value(value) })) }
            Token::VarRef(name)             => { self.advance(); Ok(Some(Node::VarRef(name))) }

            Token::ExportSingle { name, value } => {
                self.advance();
                Ok(Some(Node::Export { name, value: ExportValue::Single(parse_string_parts(&value)) }))
            }
            Token::ExportListStart(name) => {
                self.advance();
                let items = self.parse_export_list()?;
                Ok(Some(Node::Export { name, value: ExportValue::List(items) }))
            }
            Token::ExportListItem(_) | Token::ExportListEnd => {
                let pos = self.pos; self.advance();
                Err(ParseError::UnexpectedToken(pos, "ExportList poza listem".into()))
            }

            Token::Dependency(dep)         => { self.advance(); Ok(Some(Node::Dependency { name: dep })) }
            Token::Import { lib, detail }  => { self.advance(); Ok(Some(Node::Import { lib, detail })) }

            Token::FuncDef(name) => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::FuncDef { name, body }))
            }
            Token::FuncCall(name) => { self.advance(); Ok(Some(Node::FuncCall { name })) }

            Token::IfOk => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::Conditional { condition: ConditionKind::Ok, body }))
            }
            Token::IfErr => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::Conditional { condition: ConditionKind::Err, body }))
            }

            tok => {
                let pos = self.pos; self.advance();
                Err(ParseError::UnexpectedToken(pos, format!("{:?}", tok)))
            }
        }
    }

    fn parse_block(&mut self) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::with_capacity(8);
        loop {
            self.skip_newlines();
            match self.peek() {
                Token::Done => { self.advance(); break; }
                Token::Eof  => return Err(ParseError::MissingDone),
                _ => { if let Some(n) = self.parse_node()? { nodes.push(n); } }
            }
        }
        Ok(nodes)
    }

    pub fn parse(&mut self) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::with_capacity(32);
        loop {
            self.skip_newlines();
            if matches!(self.peek(), Token::Eof) { break; }
            if let Some(n) = self.parse_node()? { nodes.push(n); }
        }
        Ok(nodes)
    }
}

pub fn parse_source(source: &str) -> Result<Vec<Node>, ParseError> {
    Ok(parse_source_with_meta(source)?.nodes)
}

pub fn parse_source_with_meta(source: &str) -> Result<ParseMeta, ParseError> {
    let preprocessed = preprocess(source);
    let (gen, gen_err) = extract_gen(&preprocessed.source);
    if let Some(err) = gen_err {
        return Err(ParseError::Gen(err));
    }
    let mut lexer  = Lexer::new(&preprocessed.source);
    let tokens     = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let nodes      = parser.parse()?;
    Ok(ParseMeta { nodes, gen, shebang: preprocessed.shebang })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_background() {
        let src = "& python3 -m http.server 8080";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Background { .. })));
    }

    #[test]
    fn test_parse_hsh_cmd() {
        let src = "*> ls -la";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::HshCommand { .. })));
    }

    #[test]
    fn test_parse_repeat_n() {
        let src = "_10 > hacker update";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::RepeatN { count: 10, .. })));
    }

    #[test]
    fn test_parse_file_import() {
        let src = "<< utils.hl";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::FileImport { .. })));
    }

    #[test]
    fn test_parse_goroutine() {
        let src = ":*\n> ls\ndone";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Goroutine { .. })));
    }

    #[test]
    fn test_parse_channel() {
        let src = ":** moj_kanal";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Channel { .. })));
    }

    #[test]
    fn test_import_main() {
        let src = "# <main/net>";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Import { lib, .. } if lib == "main/net")));
    }

    #[test]
    fn test_import_bit() {
        let src = "# <bit/hashlib>";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Import { lib, .. } if lib == "bit/hashlib")));
    }

    #[test]
    fn test_import_github() {
        let src = "# <github/user/repo>";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Import { lib, .. } if lib == "github/user/repo")));
    }

    #[test]
    fn test_import_std_legacy() {
        // stara skladnia std/* -> main/*
        let src = "# <std/net>";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Import { lib, .. } if lib == "main/net")));
    }

    #[test]
    fn test_import_virus_legacy() {
        // stara skladnia virus/* -> bit/*
        let src = "# <virus/hashlib>";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Import { lib, .. } if lib == "bit/hashlib")));
    }
}
