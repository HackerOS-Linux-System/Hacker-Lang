use crate::ast::*;
use crate::gen::{GenError, extract_gen};
use crate::shebang::preprocess;
use crate::lexer::{LexError, Lexer, Token, CommentKind, PipeCmdMode};
use crate::ParseMeta;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Blad leksera: {0}")]
    Lex(#[from] LexError),
    #[error("Nieoczekiwany token na pozycji {0}: {1}")]
    UnexpectedToken(usize, String),
    #[error("Brakujace 'done' — blok nie jest zamkniety")]
    MissingDone,
    #[error("Brakujace 'def' po nazwie funkcji")]
    MissingDef,
    #[error("Brakujace ']' — lista eksportu nie jest zamknieta")]
    MissingExportListEnd,
    #[error("Blad deklaracji gena: {0}")]
    Gen(#[from] GenError),
}

pub struct Parser {
    tokens: Vec<Token>,
    pos:    usize,
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
        while matches!(self.peek(), Token::Newline) { self.advance(); }
    }

    fn parse_var_value(value: &str, typ: &str) -> VarValue {
        let value = value.trim();
        match typ {
            "int"            => { if let Ok(n) = value.parse::<i64>() { return VarValue::Int(n); } }
            "float"          => { if let Ok(n) = value.parse::<f64>() { return VarValue::Float(n); } }
            "bool"           => {
                if value == "true"  { return VarValue::Bool(true); }
                if value == "false" { return VarValue::Bool(false); }
            }
            "str" | "string" => { return VarValue::String(value.trim_matches('"').to_string()); }
            _ => {}
        }

        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            let inner = &value[1..value.len()-1];
            let parts = parse_string_parts(inner);
            if parts.iter().any(|p| matches!(p, StringPart::Var(_))) {
                return VarValue::Interpolated(parts);
            }
            return VarValue::String(inner.to_string());
        }
        if value.starts_with("$(") && value.ends_with(')') {
            return VarValue::Arithmetic(value[2..value.len()-1].trim().to_string());
        }
        if let Ok(n) = value.parse::<i64>() { return VarValue::Int(n); }
        if let Ok(n) = value.parse::<f64>() { return VarValue::Number(n); }
        if value == "true"  { return VarValue::Bool(true);  }
        if value == "false" { return VarValue::Bool(false); }

        let parts = parse_string_parts(value);
        if parts.iter().any(|p| matches!(p, StringPart::Var(_))) {
            VarValue::Interpolated(parts)
        } else {
            VarValue::String(value.to_string())
        }
    }

    fn parse_export_list(&mut self) -> Result<Vec<Vec<StringPart>>, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::ExportListEnd          => { self.advance(); break; }
                Token::ExportListItem(val)    => { self.advance(); items.push(parse_string_parts(&val)); }
                Token::Eof                    => return Err(ParseError::MissingExportListEnd),
                _                             => { self.advance(); }
            }
        }
        Ok(items)
    }

    fn parse_switch_arms(&mut self) -> Result<Vec<MatchArm>, ParseError> {
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::Done => { self.advance(); break; }
                Token::Eof  => return Err(ParseError::MissingDone),
                Token::SwitchArm { pattern } => {
                    self.advance();
                    let mut body = Vec::new();
                    loop {
                        self.skip_newlines();
                        match self.peek() {
                            Token::SwitchArm { .. } | Token::Done | Token::Eof => break,
                            _ => { if let Some(n) = self.parse_node()? { body.push(n); } }
                        }
                    }
                    arms.push(MatchArm { pattern, body });
                }
                _ => { self.advance(); }
            }
        }
        Ok(arms)
    }

    fn parse_node(&mut self) -> Result<Option<Node>, ParseError> {
        self.skip_newlines();
        match self.peek().clone() {
            Token::Eof | Token::Done => Ok(None),
            Token::Newline           => { self.advance(); Ok(None) }

            Token::Comments(CommentKind::Line,  t) => { self.advance(); Ok(Some(Node::LineComment(t))) }
            Token::Comments(CommentKind::Doc,   t) => { self.advance(); Ok(Some(Node::DocComment(t))) }
            Token::Comments(CommentKind::Block, t) => { self.advance(); Ok(Some(Node::BlockComment(t))) }

            Token::Using(ref decl) => {
                self.advance();
                Ok(Some(Node::LineComment(format!("gen-decl: {}", decl))))
            }

            Token::Print(msg)             => { self.advance(); Ok(Some(Node::Print { parts: parse_string_parts(&msg) })) }
            Token::QuickCall { name, args }=> { self.advance(); Ok(Some(Node::QuickCall { name, args: parse_string_parts(&args) })) }
            Token::Background(raw)        => { self.advance(); Ok(Some(Node::Background { raw })) }
            Token::HshCmd(raw)            => { self.advance(); Ok(Some(Node::HshCommand { raw })) }

            Token::RepeatN(n) => {
                self.advance();
                self.skip_newlines();
                let body = if let Some(node) = self.parse_node()? { vec![node] } else { vec![] };
                Ok(Some(Node::RepeatN { count: n, body }))
            }

            Token::FileImport { path, detail } => { self.advance(); Ok(Some(Node::FileImport { path, detail })) }

            Token::GoroutineStart { name } => {
                self.advance();
                Ok(Some(Node::Goroutine { name, body: self.parse_block()? }))
            }
            Token::ChannelDecl(name) => { self.advance(); Ok(Some(Node::Channel { name })) }
            Token::ChannelOp(name)   => { self.advance(); Ok(Some(Node::ChannelOp { name, value: None })) }

            Token::ForIn { var, iterable } => {
                self.advance();
                Ok(Some(Node::ForIn { var, iterable: parse_string_parts(&iterable), body: self.parse_block()? }))
            }
            Token::WhileStart(condition) => {
                self.advance();
                Ok(Some(Node::WhileLoop { condition: parse_string_parts(&condition), body: self.parse_block()? }))
            }
            Token::SwitchStart(subject) => {
                self.advance();
                Ok(Some(Node::MatchExpr { subject: parse_string_parts(&subject), arms: self.parse_switch_arms()? }))
            }

            Token::Arithmetic { expr, assign_to } => { self.advance(); Ok(Some(Node::Arithmetic { expr, assign_to })) }

            // CmdPipeToVar — obsluguje > |>, >> |>, ^> |>, ^>> |>
            Token::CmdPipeToVar { cmd, mode, var_name } => {
                self.advance();
                let cmd_mode = match mode {
                    PipeCmdMode::Plain    => CommandMode::Plain,
                    PipeCmdMode::Sudo     => CommandMode::Sudo,
                    PipeCmdMode::WithVars => CommandMode::WithVars,
                };
                Ok(Some(Node::PipeToVar { command: cmd, mode: cmd_mode, var_name }))
            }

            Token::HackerOsApi { tool, args } => {
                self.advance();
                Ok(Some(Node::HackerOsApi {
                    tool: HackerOsTool::from_str(&tool),
                        args: parse_string_parts(&args),
                }))
            }

            Token::Cmd(raw)                 => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Plain,            interpolate: false })) }
            Token::CmdSudo(raw)             => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Sudo,             interpolate: false })) }
            Token::CmdIsolated(raw)         => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Isolated,         interpolate: false })) }
            Token::CmdIsolatedSudo(raw)     => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::IsolatedSudo,     interpolate: false })) }
            Token::CmdWithVars(raw)         => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVars,         interpolate: true  })) }
            Token::CmdWithVarsSudo(raw)     => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVarsSudo,     interpolate: true  })) }
            Token::CmdWithVarsIsolated(raw) => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVarsIsolated, interpolate: true  })) }

            Token::VarDecl { name, typ, value } => {
                self.advance();
                let var_type = VarType::from_str(&typ);
                Ok(Some(Node::VarDecl { name, typ: var_type, value: Self::parse_var_value(&value, &typ) }))
            }
            Token::VarRef(name) => { self.advance(); Ok(Some(Node::VarRef(name))) }

            Token::ExportSingle { name, value } => {
                self.advance();
                Ok(Some(Node::Export { name, value: ExportValue::Single(parse_string_parts(&value)) }))
            }
            Token::ExportListStart(name) => {
                self.advance();
                Ok(Some(Node::Export { name, value: ExportValue::List(self.parse_export_list()?) }))
            }
            Token::ExportListItem(_) | Token::ExportListEnd => { self.advance(); Ok(None) }

            Token::Dependency(dep)        => { self.advance(); Ok(Some(Node::Dependency { name: dep })) }
            Token::Import { lib, detail } => { self.advance(); Ok(Some(Node::Import { lib, detail })) }

            Token::FuncDef(name) => {
                self.advance();
                Ok(Some(Node::FuncDef { name, body: self.parse_block()? }))
            }
            Token::FuncCall(name) => { self.advance(); Ok(Some(Node::FuncCall { name })) }

            Token::IfOk  => { self.advance(); Ok(Some(Node::Conditional { condition: ConditionKind::Ok,  body: self.parse_block()? })) }
            Token::IfErr => { self.advance(); Ok(Some(Node::Conditional { condition: ConditionKind::Err, body: self.parse_block()? })) }

            Token::SwitchArm { .. }                              => { self.advance(); Ok(None) }
            Token::Bool(_) | Token::Number(_) | Token::Ident(_) |
            Token::StringLit(_)                                  => { self.advance(); Ok(None) }

            #[allow(unreachable_patterns)]
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
                _           => { if let Some(n) = self.parse_node()? { nodes.push(n); } }
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
    if let Some(err) = gen_err { return Err(ParseError::Gen(err)); }
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
    fn test_pipe_to_var_simple() {
        let src = "> hostname |> @myhost";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::PipeToVar { .. })));
    }

    #[test]
    fn test_pipe_to_var_with_vars() {
        let src = ">> jq -r '.version' /tmp/x.json | awk '{print $1}' |> @LOCAL_VERSION";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::PipeToVar { .. })));
    }

    #[test]
    fn test_pipe_to_var_in_quotes_ignored() {
        // |> wewnatrz cudzyslowow nie jest pipeToVar
        let src = r#">> bash -c "echo hello" |> @out"#;
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::PipeToVar { .. })));
    }

    #[test]
    fn test_underscore_var() {
        let src = "% _d = test\n% _result = 0";
        let nodes = parse_source(src).unwrap();
        assert!(!nodes.is_empty());
    }

    #[test]
    fn test_for_in() {
        let src = "@ item in a b c\n~> @item\ndone";
        assert!(parse_source(src).is_ok());
    }

    #[test]
    fn test_switch() {
        let src = "? switch @x\n| a\n~> A\n| *\n~> other\ndone";
        assert!(parse_source(src).is_ok());
    }
}
