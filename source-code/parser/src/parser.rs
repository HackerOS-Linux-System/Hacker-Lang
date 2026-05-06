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

        // Typowane (gen 2)
        match typ {
            "int" => {
                if let Ok(n) = value.parse::<i64>() { return VarValue::Int(n); }
            }
            "float" => {
                if let Ok(n) = value.parse::<f64>() { return VarValue::Float(n); }
            }
            "bool" => {
                if value == "true"  { return VarValue::Bool(true);  }
                if value == "false" { return VarValue::Bool(false); }
            }
            "str" | "string" => {
                let s = value.trim_matches('"').to_string();
                return VarValue::String(s);
            }
            _ => {}
        }

        // Automatyczne wykrywanie
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            let inner = &value[1..value.len()-1];
            let parts = parse_string_parts(inner);
            if parts.iter().any(|p| matches!(p, StringPart::Var(_))) {
                return VarValue::Interpolated(parts);
            }
            return VarValue::String(inner.to_string());
        }

        // Gen 2 — $( expr ) jako wartosc zmiennej
        if value.starts_with("$(") && value.ends_with(')') {
            let expr = value[2..value.len()-1].trim().to_string();
            return VarValue::Arithmetic(expr);
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
                Token::ExportListEnd => { self.advance(); break; }
                Token::ExportListItem(val) => { self.advance(); items.push(parse_string_parts(&val)); }
                Token::Eof => return Err(ParseError::MissingExportListEnd),
                _ => { self.advance(); }
            }
        }
        Ok(items)
    }

    /// Parsuj blok switch — zbierz arms do done
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
                    // Zbierz body do nastepnego | lub done
                    loop {
                        self.skip_newlines();
                        match self.peek() {
                            Token::SwitchArm { .. } | Token::Done | Token::Eof => break,
                            _ => {
                                if let Some(n) = self.parse_node()? { body.push(n); }
                            }
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

            Token::Print(msg) => {
                self.advance();
                Ok(Some(Node::Print { parts: parse_string_parts(&msg) }))
            }

            Token::QuickCall { name, args } => {
                self.advance();
                Ok(Some(Node::QuickCall { name, args: parse_string_parts(&args) }))
            }

            // & background
            Token::Background(raw) => { self.advance(); Ok(Some(Node::Background { raw })) }

            // *> hsh
            Token::HshCmd(raw) => { self.advance(); Ok(Some(Node::HshCommand { raw })) }

            // _N repeat
            Token::RepeatN(n) => {
                self.advance();
                self.skip_newlines();
                let body = if let Some(node) = self.parse_node()? { vec![node] } else { vec![] };
                Ok(Some(Node::RepeatN { count: n, body }))
            }

            // << file import
            Token::FileImport { path, detail } => {
                self.advance();
                Ok(Some(Node::FileImport { path, detail }))
            }

            // :* goroutine (z opcjonalna nazwa, gen 2: :* nazwa def)
            Token::GoroutineStart { name } => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::Goroutine { name, body }))
            }

            // :** channel
            Token::ChannelDecl(name) => {
                self.advance();
                Ok(Some(Node::Channel { name }))
            }

            // *-- channel op
            Token::ChannelOp(name) => {
                self.advance();
                Ok(Some(Node::ChannelOp { name, value: None }))
            }

            // Gen 2 — @ item in lista (for-in)
            Token::ForIn { var, iterable } => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::ForIn {
                    var,
                    iterable: parse_string_parts(&iterable),
                    body,
                }))
            }

            // Gen 2 — ?~ warunek (while)
            Token::WhileStart(condition) => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Some(Node::WhileLoop {
                    condition: parse_string_parts(&condition),
                    body,
                }))
            }

            // Gen 2 — ? switch @var (switch/match)
            Token::SwitchStart(subject) => {
                self.advance();
                let arms = self.parse_switch_arms()?;
                Ok(Some(Node::MatchExpr {
                    subject: parse_string_parts(&subject),
                    arms,
                }))
            }

            // Gen 2 — $( expr ) -> @var
            Token::Arithmetic { expr, assign_to } => {
                self.advance();
                Ok(Some(Node::Arithmetic { expr, assign_to }))
            }

            // Gen 2 — > cmd |> @var
            Token::CmdPipeToVar { cmd, mode, var_name } => {
                self.advance();
                let cmd_mode = match mode {
                    PipeCmdMode::Plain => CommandMode::Plain,
                    PipeCmdMode::Sudo  => CommandMode::Sudo,
                };
                Ok(Some(Node::PipeToVar { command: cmd, mode: cmd_mode, var_name }))
            }

            // Gen 2 — || tool args
            Token::HackerOsApi { tool, args } => {
                self.advance();
                Ok(Some(Node::HackerOsApi {
                    tool: HackerOsTool::from_str(&tool),
                    args: parse_string_parts(&args),
                }))
            }

            // Komendy
            Token::Cmd(raw)                 => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Plain,            interpolate: false })) }
            Token::CmdSudo(raw)             => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Sudo,             interpolate: false })) }
            Token::CmdIsolated(raw)         => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::Isolated,         interpolate: false })) }
            Token::CmdIsolatedSudo(raw)     => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::IsolatedSudo,     interpolate: false })) }
            Token::CmdWithVars(raw)         => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVars,         interpolate: true  })) }
            Token::CmdWithVarsSudo(raw)     => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVarsSudo,     interpolate: true  })) }
            Token::CmdWithVarsIsolated(raw) => { self.advance(); Ok(Some(Node::Command { raw, mode: CommandMode::WithVarsIsolated, interpolate: true  })) }

            // % var: typ = val (gen 2 typowane)
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
                let items = self.parse_export_list()?;
                Ok(Some(Node::Export { name, value: ExportValue::List(items) }))
            }
            Token::ExportListItem(_) | Token::ExportListEnd => {
                let pos = self.pos; self.advance();
                Err(ParseError::UnexpectedToken(pos, "ExportList poza listem".into()))
            }

            Token::Dependency(dep)        => { self.advance(); Ok(Some(Node::Dependency { name: dep })) }
            Token::Import { lib, detail } => { self.advance(); Ok(Some(Node::Import { lib, detail })) }

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

            Token::SwitchArm { .. } => {
                // Switch arm poza switch — ignoruj
                self.advance(); Ok(None)
            }

            // Bool/Number tokens jako standalone — ignoruj (uzywane w VarDecl)
            Token::Bool(_) | Token::Number(_) => {
                self.advance(); Ok(None)
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
    fn test_for_in() {
        let src = "@ item in /usr/bin /usr/local/bin\n~> @item\ndone";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::ForIn { .. })));
    }

    #[test]
    fn test_while() {
        let src = "?~ @running == true\n> sleep 1\ndone";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::WhileLoop { .. })));
    }

    #[test]
    fn test_switch() {
        let src = "? switch @os\n| linux\n~> Linux\n| windows\n~> Windows\n| *\n~> Inne\ndone";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::MatchExpr { .. })));
    }

    #[test]
    fn test_arithmetic() {
        let src = "$(2 + 2) -> @result";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Arithmetic { assign_to: Some(_), .. })));
    }

    #[test]
    fn test_typed_var_int() {
        let src = "% count: int = 42";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::VarDecl { typ: VarType::Int, .. })));
    }

    #[test]
    fn test_hackeros_api() {
        let src = "|| hacker update";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::HackerOsApi { .. })));
    }

    #[test]
    fn test_pipe_to_var() {
        let src = "> hostname |> @myhost";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::PipeToVar { .. })));
    }

    #[test]
    fn test_goroutine_with_name() {
        let src = ":* scanner def\n> nmap -sn 192.168.1.0/24\ndone";
        let nodes = parse_source(src).unwrap();
        assert!(nodes.iter().any(|n| matches!(n, Node::Goroutine { name: Some(_), .. })));
    }
}
