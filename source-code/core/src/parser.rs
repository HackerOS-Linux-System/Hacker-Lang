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
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline | Token::Indent(_)) {
            self.advance();
        }
    }

    fn parse_node(&mut self) -> Result<Option<Node>, ParseError> {
        self.skip_newlines();

        match self.peek().clone() {
            Token::Eof => Ok(None),

            Token::LineComment(text) => {
                self.advance();
                Ok(Some(Node::LineComment(text)))
            }

            Token::DocComment(text) => {
                self.advance();
                Ok(Some(Node::DocComment(text)))
            }

            Token::BlockComment(text) => {
                self.advance();
                Ok(Some(Node::BlockComment(text)))
            }

            Token::Print(msg) => {
                self.advance();
                let parts = parse_string_parts(&msg);
                Ok(Some(Node::Print { parts }))
            }

            Token::Cmd(raw) => {
                self.advance();
                Ok(Some(Node::Command {
                    raw,
                    mode: CommandMode::Plain,
                    interpolate: false,
                }))
            }

            Token::CmdSudo(raw) => {
                self.advance();
                Ok(Some(Node::Command {
                    raw,
                    mode: CommandMode::Sudo,
                    interpolate: false,
                }))
            }

            Token::CmdIsolated(raw) => {
                self.advance();
                Ok(Some(Node::Command {
                    raw,
                    mode: CommandMode::Isolated,
                    interpolate: false,
                }))
            }

            Token::CmdIsolatedSudo(raw) => {
                self.advance();
                Ok(Some(Node::Command {
                    raw,
                    mode: CommandMode::IsolatedSudo,
                    interpolate: false,
                }))
            }

            Token::CmdWithVars(raw) => {
                self.advance();
                Ok(Some(Node::Command {
                    raw,
                    mode: CommandMode::WithVars,
                    interpolate: true,
                }))
            }

            Token::CmdWithVarsSudo(raw) => {
                self.advance();
                Ok(Some(Node::Command {
                    raw,
                    mode: CommandMode::WithVarsSudo,
                    interpolate: true,
                }))
            }

            Token::VarDecl { name, value } => {
                self.advance();
                let var_value = if value.starts_with('"') && value.ends_with('"') {
                    let inner = &value[1..value.len()-1];
                    let parts = parse_string_parts(inner);
                    if parts.iter().any(|p| matches!(p, StringPart::Var(_))) {
                        VarValue::Interpolated(parts)
                    } else {
                        VarValue::String(inner.to_string())
                    }
                } else if let Ok(n) = value.parse::<f64>() {
                    VarValue::Number(n)
                } else if value == "true" {
                    VarValue::Bool(true)
                } else if value == "false" {
                    VarValue::Bool(false)
                } else {
                    // Check for @vars in unquoted strings
                    let parts = parse_string_parts(&value);
                    if parts.iter().any(|p| matches!(p, StringPart::Var(_))) {
                        VarValue::Interpolated(parts)
                    } else {
                        VarValue::String(value)
                    }
                };
                Ok(Some(Node::VarDecl { name, value: var_value }))
            }

            Token::VarRef(name) => {
                self.advance();
                Ok(Some(Node::VarRef(name)))
            }

            Token::Dependency(dep) => {
                self.advance();
                Ok(Some(Node::Dependency { name: dep }))
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
                Ok(Some(Node::Conditional {
                    condition: ConditionKind::Ok,
                    body,
                }))
            }

            Token::IfErr => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::Conditional {
                    condition: ConditionKind::Err,
                    body,
                }))
            }

            Token::Done => {
                // Done is consumed by parse_block, hitting it here is unexpected in top-level
                // but we return None to signal block end
                Ok(None)
            }

            Token::Newline | Token::Indent(_) => {
                self.advance();
                Ok(None)
            }

            tok => {
                let pos = self.pos;
                self.advance();
                Err(ParseError::UnexpectedToken(pos, format!("{:?}", tok)))
            }
        }
    }

    /// Parse a block of statements terminated by `done` or EOF
    fn parse_block(&mut self) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Token::Done => {
                    self.advance(); // consume 'done'
                    break;
                }
                Token::Eof => {
                    return Err(ParseError::MissingDone);
                }
                _ => {
                    if let Some(node) = self.parse_node()? {
                        nodes.push(node);
                    }
                }
            }
        }
        Ok(nodes)
    }

    pub fn parse(&mut self) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), Token::Eof) {
                break;
            }
            if let Some(node) = self.parse_node()? {
                nodes.push(node);
            }
        }
        Ok(nodes)
    }
}

/// Convenience function: lex + parse source string
pub fn parse_source(source: &str) -> Result<Vec<Node>, ParseError> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse()
}
