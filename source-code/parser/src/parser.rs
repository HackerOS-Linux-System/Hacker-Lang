use crate::ast::*;
use crate::lexer::{LexError, Lexer, Token};
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
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Self { tokens, pos: 0 } }

    #[inline]
    fn peek(&self) -> &Token { self.tokens.get(self.pos).unwrap_or(&Token::Eof) }

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
            if parts.iter().any(|p| matches!(p, StringPart::Var(_))) {
                return VarValue::Interpolated(parts);
            }
            return VarValue::String(inner.to_string());
        }
        if let Ok(n) = value.parse::<f64>() { return VarValue::Number(n); }
        if value == "true"  { return VarValue::Bool(true); }
        if value == "false" { return VarValue::Bool(false); }
        let parts = parse_string_parts(&value);
        if parts.iter().any(|p| matches!(p, StringPart::Var(_))) {
            VarValue::Interpolated(parts)
        } else {
            VarValue::String(value)
        }
    }

    fn parse_node(&mut self) -> Result<Option<Node>, ParseError> {
        self.skip_newlines();

        match self.peek().clone() {
            Token::Eof | Token::Done => Ok(None),
            Token::Newline | Token::Indent(_) => { self.advance(); Ok(None) }

            Token::LineComment(t)  => { self.advance(); Ok(Some(Node::LineComment(t))) }
            Token::DocComment(t)   => { self.advance(); Ok(Some(Node::DocComment(t))) }
            Token::BlockComment(t) => { self.advance(); Ok(Some(Node::BlockComment(t))) }

            // ~> print
            Token::Print(msg) => {
                self.advance();
                Ok(Some(Node::Print { parts: parse_string_parts(&msg) }))
            }

            // :: quick-call
            Token::QuickCall { name, args } => {
                self.advance();
                Ok(Some(Node::QuickCall {
                    name,
                    args: parse_string_parts(&args),
                }))
            }

            Token::Cmd(raw) => {
                self.advance();
                Ok(Some(Node::Command { raw, mode: CommandMode::Plain, interpolate: false }))
            }
            Token::CmdSudo(raw) => {
                self.advance();
                Ok(Some(Node::Command { raw, mode: CommandMode::Sudo, interpolate: false }))
            }
            Token::CmdIsolated(raw) => {
                self.advance();
                Ok(Some(Node::Command { raw, mode: CommandMode::Isolated, interpolate: false }))
            }
            Token::CmdIsolatedSudo(raw) => {
                self.advance();
                Ok(Some(Node::Command { raw, mode: CommandMode::IsolatedSudo, interpolate: false }))
            }
            Token::CmdWithVars(raw) => {
                self.advance();
                Ok(Some(Node::Command { raw, mode: CommandMode::WithVars, interpolate: true }))
            }
            Token::CmdWithVarsSudo(raw) => {
                self.advance();
                Ok(Some(Node::Command { raw, mode: CommandMode::WithVarsSudo, interpolate: true }))
            }
            Token::CmdWithVarsIsolated(raw) => {
                self.advance();
                Ok(Some(Node::Command { raw, mode: CommandMode::WithVarsIsolated, interpolate: true }))
            }

            Token::VarDecl { name, value } => {
                self.advance();
                Ok(Some(Node::VarDecl { name, value: Self::parse_var_value(value) }))
            }

            Token::VarRef(name) => { self.advance(); Ok(Some(Node::VarRef(name))) }

            Token::Dependency(dep) => {
                self.advance();
                Ok(Some(Node::Dependency { name: dep }))
            }

            Token::Import { lib, detail } => {
                self.advance();
                Ok(Some(Node::Import { lib, detail }))
            }

            Token::FuncDef(name) => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::FuncDef { name, body }))
            }

            Token::FuncCall(name) => {
                self.advance();
                Ok(Some(Node::FuncCall { name }))
            }

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
                let pos = self.pos;
                self.advance();
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
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse()
}
