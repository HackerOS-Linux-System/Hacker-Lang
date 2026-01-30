use indextree::{Arena, NodeId};
use logos::Logos;
use chumsky::prelude::*;

// ==========================================
// 1. AST NODES
// ==========================================
#[derive(Debug, Clone)]
pub enum Node {
    Program(String), // tryb pamięci
    Require(String),
    Import { source: String, name: String, details: String },
    Var { name: String, value_id: NodeId },
    Log(NodeId),
    Object { name: String, body: NodeId },

    // Funkcje
    Func { name: String, args: Vec<String>, body: NodeId },
    Call { name: String, args_ids: Vec<NodeId> },

    Block,
    StringLit(String),
    IntLit(i64),
    Ident(String),
}

// Struktura pośrednia dla parsera (aby ominąć ograniczenia typów w recursive closure)
#[derive(Debug, Clone)]
pub enum PreNode {
    Require(String),
    Import(String, String, String),
    Var(String, Box<PreNode>),
    Log(Box<PreNode>),
    Object(String, Vec<PreNode>),
    Func(String, Vec<String>, Vec<PreNode>),
    Call(String, Vec<PreNode>),
    Block(Vec<PreNode>),
    ValString(String),
    ValInt(i64),
    ValIdent(String),
    MemoryDecl(String),
}

// ==========================================
// 2. LEXER (Logos)
// ==========================================
#[derive(Logos, Debug, PartialEq, Eq, Hash, Clone)]
#[logos(skip r"[ \t\n\f]+")]
pub enum Token {
    #[token("[")]
    OpenBracket,
    #[token("]")]
    CloseBracket,
    #[token("(")]
    OpenParen,
    #[token(")")]
    CloseParen,
    #[token(",")]
    Comma,
    #[token("=")]
    Equals,
    #[token("log")]
    Log,
    #[token("object")]
    Object,
    #[token("func")]
    Func,
    #[token("var")]
    Var,
    #[token("require")]
    Require,
    #[token("import")]
    Import,

    #[regex(r"!.*", logos::skip)]
    Comment,

    #[regex(r#""([^"\\]|\\.)*""#, |lex| lex.slice().trim_matches('"').to_string())]
    StringLit(String),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    IntLit(i64),

    #[regex(r"---.*tryb.*pamieci.*---\s*(auto|automatic|manual|safe)", |lex| {
    let s = lex.slice();
    if s.contains("manual") { "manual".to_string() }
    else if s.contains("safe") { "safe".to_string() }
    else { "automatic".to_string() }
    })]
    MemoryMode(String),

    #[regex(r"<[^>]+>", |lex| lex.slice().trim_matches(|c| c == '<' || c == '>').to_string())]
    BracketedContent(String),

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    Error,
}

// ==========================================
// 3. PARSER Logic
// ==========================================
pub fn parser() -> impl Parser<Token, Vec<PreNode>, Error = Simple<Token>> {
    recursive(|stmt| {
        let int_lit = filter_map(|span, tok| match tok {
            Token::IntLit(i) => Ok(PreNode::ValInt(i)),
                                 _ => Err(Simple::custom(span, "Expected int")),
        });

        let str_lit = filter_map(|span, tok| match tok {
            Token::StringLit(s) => Ok(PreNode::ValString(s)),
                                 _ => Err(Simple::custom(span, "Expected string")),
        });

        let ident_str = filter_map(|span, tok| match tok {
            Token::Ident(s) => Ok(s),
                                   _ => Err(Simple::custom(span, "Expected identifier")),
        });

        let expr = recursive(|expr| {
            let call = ident_str.clone()
            .then(
                expr.clone()
                .separated_by(just(Token::Comma))
                .delimited_by(just(Token::OpenParen), just(Token::CloseParen))
            )
            .map(|(name, args)| PreNode::Call(name, args));

            choice((
                call,
                int_lit.clone(),
                    str_lit.clone(),
                    ident_str.clone().map(PreNode::ValIdent),
            ))
        });

        let block = stmt.clone()
        .repeated()
        .delimited_by(just(Token::OpenBracket), just(Token::CloseBracket))
        .map(PreNode::Block);

        // Instrukcje
        let var = just(Token::Var)
        .ignore_then(ident_str.clone())
        .then_ignore(just(Token::Equals))
        .then(choice((expr.clone(), block.clone())))
        .map(|(name, val)| PreNode::Var(name, Box::new(val)));

        let log = just(Token::Log)
        .ignore_then(choice((expr.clone(), block.clone())))
        .map(|v| PreNode::Log(Box::new(v)));

        let object = just(Token::Object)
        .ignore_then(ident_str.clone())
        .then(block.clone().map(|b| match b { PreNode::Block(v) => v, _ => vec![] }))
        .map(|(name, body)| PreNode::Object(name, body));

        let func = just(Token::Func)
        .ignore_then(ident_str.clone())
        .then(
            ident_str.clone()
            .separated_by(just(Token::Comma))
            .delimited_by(just(Token::OpenParen), just(Token::CloseParen))
            .or_not()
            .map(|opt| opt.unwrap_or_default())
        )
        .then(block.clone().map(|b| match b { PreNode::Block(v) => v, _ => vec![] }))
        .map(|((name, args), body)| PreNode::Func(name, args, body));

        let require = just(Token::Require)
        .ignore_then(filter_map(|span, tok| match tok {
            Token::BracketedContent(s) => Ok(s),
                                _ => Err(Simple::custom(span, "Expected <file>")),
        }))
        .map(PreNode::Require);

        let import = just(Token::Import)
        .ignore_then(filter_map(|span, tok| match tok {
            Token::BracketedContent(s) => Ok(s),
                                _ => Err(Simple::custom(span, "Expected <import>")),
        }))
        .map(|s| {
            let p: Vec<&str> = s.split(':').collect();
            PreNode::Import(
                p.get(0).unwrap_or(&"?").to_string(),
                            p.get(1).unwrap_or(&"?").to_string(),
                            p.get(2).unwrap_or(&"").to_string()
            )
        });

        let mem = filter_map(|span, tok| match tok {
            Token::MemoryMode(m) => Ok(PreNode::MemoryDecl(m)),
                             _ => Err(Simple::custom(span, "Not mode")),
        });

        // Samodzielne wywołanie np. foo(x)
        let stand_alone_call = ident_str.clone()
        .then(
            expr.clone()
            .separated_by(just(Token::Comma))
            .delimited_by(just(Token::OpenParen), just(Token::CloseParen))
        )
        .map(|(name, args)| PreNode::Call(name, args));

        choice((
            mem, require, import, var, log, object, func, block, stand_alone_call
        ))
    }).repeated()
}

// Konwersja PreNode -> Indextree
pub fn build_arena(nodes: Vec<PreNode>, arena: &mut Arena<Node>) -> NodeId {
    let mut mode = "automatic".to_string();
    for n in &nodes {
        if let PreNode::MemoryDecl(m) = n {
            mode = m.clone();
        }
    }
    let root = arena.new_node(Node::Program(mode));
    for n in nodes {
        append_node(n, root, arena);
    }
    root
}

fn append_node(pn: PreNode, parent: NodeId, arena: &mut Arena<Node>) -> NodeId {
    match pn {
        PreNode::MemoryDecl(_) => parent,
        PreNode::Require(f) => { let n = arena.new_node(Node::Require(f)); parent.append(n, arena); n },
        PreNode::Import(s,n,d) => { let node = arena.new_node(Node::Import{source:s, name:n, details:d}); parent.append(node, arena); node },
        PreNode::Log(val) => {
            let l = arena.new_node(Node::Log(parent)); // ID tymczasowe
            let val_id = append_node(*val, l, arena);
            *arena.get_mut(l).unwrap().get_mut() = Node::Log(val_id);
            parent.append(l, arena);
            l
        },
        PreNode::Var(name, val) => {
            let v = arena.new_node(Node::Var{name: name.clone(), value_id: parent});
            let val_id = append_node(*val, v, arena);
            *arena.get_mut(v).unwrap().get_mut() = Node::Var{name, value_id: val_id};
            parent.append(v, arena);
            v
        },
        PreNode::Object(name, body) => {
            let o = arena.new_node(Node::Object{name: name.clone(), body: parent});
            let b_node = arena.new_node(Node::Block);
            for child in body {
                append_node(child, b_node, arena);
            }
            *arena.get_mut(o).unwrap().get_mut() = Node::Object{name, body: b_node};
            parent.append(o, arena);
            o
        },
        PreNode::Func(name, args, body) => {
            let f = arena.new_node(Node::Func{name: name.clone(), args, body: parent});
            let b_node = arena.new_node(Node::Block);
            for child in body {
                append_node(child, b_node, arena);
            }
            let args_data = match arena.get(f).unwrap().get() { Node::Func{args,..} => args.clone(), _ => vec![] };
            *arena.get_mut(f).unwrap().get_mut() = Node::Func{name, args: args_data, body: b_node};
            parent.append(f, arena);
            f
        },
        PreNode::Call(name, args) => {
            let c = arena.new_node(Node::Call{name: name.clone(), args_ids: vec![]});
            let mut ids = vec![];
            for arg in args {
                ids.push(append_node(arg, c, arena));
            }
            *arena.get_mut(c).unwrap().get_mut() = Node::Call{name, args_ids: ids};
            parent.append(c, arena);
            c
        },
        PreNode::Block(children) => {
            let b = arena.new_node(Node::Block);
            for child in children {
                append_node(child, b, arena);
            }
            parent.append(b, arena);
            b
        },
        PreNode::ValInt(i) => { let n = arena.new_node(Node::IntLit(i)); parent.append(n, arena); n },
        PreNode::ValString(s) => { let n = arena.new_node(Node::StringLit(s)); parent.append(n, arena); n },
        PreNode::ValIdent(s) => { let n = arena.new_node(Node::Ident(s)); parent.append(n, arena); n },
    }
}
