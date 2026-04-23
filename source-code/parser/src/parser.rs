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

            // `using <gen N>` — traktowane jako komentarz w AST (meta jest wyciagnieta wczesniej)
            // Obsluguje zarowno Token::Using (nowy) jak i Token::Ident("using") (fallback)
            Token::Using(ref decl) => {
                self.advance();
                // Gen jest wyciagniety wczesniej w parse_source_with_meta — tu tylko zapisz jako komentarz
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

/// Parsuj zrodlo HL (prosta wersja — kompatybilna wstecz)
///
/// Automatycznie:
///   1. Usuwa shebang (jesli istnieje)
///   2. Wyciaga deklaracje gena
///   3. Parsuje AST
///
/// Jesli gen jest za wysoki — zwraca ParseError::Gen
pub fn parse_source(source: &str) -> Result<Vec<Node>, ParseError> {
    Ok(parse_source_with_meta(source)?.nodes)
}

/// Parsuj zrodlo HL z pelna meta-informacja (gen, shebang)
pub fn parse_source_with_meta(source: &str) -> Result<ParseMeta, ParseError> {
    // ── 1. Pre-processing: shebang ───────────────────────────────────────────
    let preprocessed = preprocess(source);

    // ── 2. Wyciagnij gen ─────────────────────────────────────────────────────
    let (gen, gen_err) = extract_gen(&preprocessed.source);
    if let Some(err) = gen_err {
        return Err(ParseError::Gen(err));
    }

    // ── 3. Leksuj i parsuj ───────────────────────────────────────────────────
    let mut lexer  = Lexer::new(&preprocessed.source);
    let tokens     = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let nodes      = parser.parse()?;

    Ok(ParseMeta {
        nodes,
        gen,
        shebang: preprocessed.shebang,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_shebang() {
        let src = "#!/usr/bin/env hl\n~> Hello";
        let meta = parse_source_with_meta(src).unwrap();
        assert!(meta.shebang.is_some());
        assert_eq!(meta.gen.number(), 1);
    }

    #[test]
    fn test_parse_with_gen() {
        let src = "using <gen 1>\n~> Hello";
        let meta = parse_source_with_meta(src).unwrap();
        assert_eq!(meta.gen.number(), 1);
    }

    #[test]
    fn test_parse_shebang_and_gen() {
        let src = "#!/usr/bin/env hl\nusing <gen 1>\n/// doc\n~> Hello";
        let meta = parse_source_with_meta(src).unwrap();
        assert!(meta.shebang.is_some());
        assert_eq!(meta.gen.number(), 1);
        // Sprawdz ze AST zawiera Print
        assert!(meta.nodes.iter().any(|n| matches!(n, Node::Print { .. })));
    }

    #[test]
    fn test_shebang_does_not_conflict_with_import() {
        // # <std/net> to import, nie shebang
        let src = "# <std/net>\n~> Hello";
        let meta = parse_source_with_meta(src).unwrap();
        assert!(meta.shebang.is_none());
        assert!(meta.nodes.iter().any(|n| matches!(n, Node::Import { .. })));
    }
}
