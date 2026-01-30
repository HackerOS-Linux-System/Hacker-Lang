use indextree::{Arena, NodeId};
use logos::Logos;
use chumsky::prelude::*;

// ==========================================
// 1. AST NODES
// ==========================================
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div
}

#[derive(Debug, Clone)]
pub enum Node {
    Program(String), // tryb pamięci
    Require(String),
    Import { source: String, name: String, details: String },

    // Deklaracja
    Var { name: String, value_id: NodeId },
    // Przypisanie do istniejącej zmiennej
    Assign { name: String, value_id: NodeId },

    Log(NodeId),
    Object { name: String, body: NodeId },

    // Pola obiektów
    FieldAccess { object: String, field: String },
    FieldAssign { object: String, field: String, value_id: NodeId },

    // Funkcje
    Func { name: String, args: Vec<String>, body: NodeId },
    Call { name: String, args_ids: Vec<NodeId> },
    MethodCall { object: String, method: String, args_ids: Vec<NodeId> },

    // Matematyka
    BinaryOp { op: BinaryOp, lhs: NodeId, rhs: NodeId },

    Block,
    StringLit(String),
    IntLit(i64),
    Ident(String),
}

// Struktura pośrednia dla parsera
#[derive(Debug, Clone)]
pub enum PreNode {
    Require(String),
    Import(String, String, String),
    Var(String, Box<PreNode>),
    Assign(String, Box<PreNode>),
    Log(Box<PreNode>),
    Object(String, Vec<PreNode>),
    Func(String, Vec<String>, Vec<PreNode>),
    Call(String, Vec<PreNode>),
    MethodCall(String, String, Vec<PreNode>),
    FieldAccess(String, String),
    FieldAssign(String, String, Box<PreNode>),
    Block(Vec<PreNode>),
    BinaryOp(BinaryOp, Box<PreNode>, Box<PreNode>),
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
    #[token("[")] OpenBracket,
    #[token("]")] CloseBracket,
    #[token("(")] OpenParen,
    #[token(")")] CloseParen,
    #[token(",")] Comma,
    #[token("=")] Equals,
    #[token(".")] Dot,

    // Operatory
    #[token("+")] Plus,
    #[token("-")] Minus,
    #[token("*")] Star,
    #[token("/")] Slash,

    #[token("log")] Log,
    #[token("object")] Object,
    #[token("func")] Func,
    #[token("var")] Var,
    #[token("require")] Require,
    #[token("import")] Import,

    #[regex(r"!.*", logos::skip)] Comment,
    #[regex(r#""([^"\\]|\\.)*""#, |lex| lex.slice().trim_matches('"').to_string())] StringLit(String),
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())] IntLit(i64),

    // Obsługa MemoryMode
    #[regex(r"---[ \t]*(auto|automatic|manual|safe)[ \t]*---", |lex| {
    let s = lex.slice();
    if s.contains("manual") { "manual".to_string() }
    else if s.contains("safe") { "safe".to_string() }
    else { "automatic".to_string() }
    })]
    MemoryMode(String),

    #[regex(r"<[^>]+>", |lex| lex.slice().trim_matches(|c| c == '<' || c == '>').to_string())] BracketedContent(String),
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())] Ident(String),

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

        // Wyrażenia z priorytetami
        let expr = recursive(|expr| {
            // Logika parsowania argumentów
            let args = expr.clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .delimited_by(just(Token::OpenParen), just(Token::CloseParen));

            // Metoda: obj.metoda(...)
            let method_call = ident_str.clone()
            .then_ignore(just(Token::Dot))
            .then(ident_str.clone())
            .then(args.clone())
            .map(|((obj, method), args)| PreNode::MethodCall(obj, method, args));

            // Dostęp do pola: obj.pole (brak nawiasów)
            let field_access = ident_str.clone()
            .then_ignore(just(Token::Dot))
            .then(ident_str.clone())
            .map(|(obj, field)| PreNode::FieldAccess(obj, field));

            // Zwykłe wywołanie: func(...)
            let call = ident_str.clone()
            .then(args)
            .map(|(name, args)| PreNode::Call(name, args));

            let atom = choice((
                method_call, // Najwyższy priorytet (kropka + nawiasy)
            call,        // Potem zwykłe wywołanie
            field_access,// Potem dostęp do pola
            int_lit.clone(),
                               str_lit.clone(),
                               ident_str.clone().map(PreNode::ValIdent),
                               expr.clone().delimited_by(just(Token::OpenParen), just(Token::CloseParen)),
            ));

            let product = atom.clone()
            .then(choice((just(Token::Star), just(Token::Slash))).then(atom).repeated())
            .foldl(|lhs, (op, rhs)| {
                let bin_op = match op { Token::Star => BinaryOp::Mul, _ => BinaryOp::Div };
                PreNode::BinaryOp(bin_op, Box::new(lhs), Box::new(rhs))
            });

            let sum = product.clone()
            .then(choice((just(Token::Plus), just(Token::Minus))).then(product).repeated())
            .foldl(|lhs, (op, rhs)| {
                let bin_op = match op { Token::Plus => BinaryOp::Add, _ => BinaryOp::Sub };
                PreNode::BinaryOp(bin_op, Box::new(lhs), Box::new(rhs))
            });

            sum
        });

        let block = stmt.clone()
        .repeated()
        .delimited_by(just(Token::OpenBracket), just(Token::CloseBracket))
        .map(PreNode::Block);

        // Deklaracja zmiennej: var x = ...
        let var_decl = just(Token::Var)
        .ignore_then(ident_str.clone())
        .then_ignore(just(Token::Equals))
        .then(choice((expr.clone(), block.clone())))
        .map(|(name, val)| PreNode::Var(name, Box::new(val)));

        // Przypisanie do pola: obj.pole = ...
        let field_assign = ident_str.clone()
        .then_ignore(just(Token::Dot))
        .then(ident_str.clone())
        .then_ignore(just(Token::Equals))
        .then(expr.clone())
        .map(|((obj, field), val)| PreNode::FieldAssign(obj, field, Box::new(val)));

        // Przypisanie do zmiennej: x = ...
        let assign = ident_str.clone()
        .then_ignore(just(Token::Equals))
        .then(expr.clone())
        .map(|(name, val)| PreNode::Assign(name, Box::new(val)));

        let log = just(Token::Log)
        .ignore_then(expr.clone())
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

        // Standalone calls as statements
        let args_stmt = expr.clone()
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .delimited_by(just(Token::OpenParen), just(Token::CloseParen));

        let method_call_stmt = ident_str.clone()
        .then_ignore(just(Token::Dot))
        .then(ident_str.clone())
        .then(args_stmt.clone())
        .map(|((obj, method), args)| PreNode::MethodCall(obj, method, args));

        let call_stmt = ident_str.clone()
        .then(args_stmt)
        .map(|(name, args)| PreNode::Call(name, args));

        // KOLEJNOŚĆ JEST KLUCZOWA (Najpierw specyficzne, potem ogólne)
        choice((
            mem, require, import,
            var_decl, // var x = ...
            field_assign, // x.y = ... (musi byc przed assign i method_call)
        assign, // x = ...
        log, object, func, block,
        method_call_stmt, // x.y(...)
        call_stmt // x(...)
        ))
    })
    .repeated()
    .then_ignore(end())
}

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
        PreNode::Import(s, n, d) => { let node = arena.new_node(Node::Import{source:s, name:n, details:d}); parent.append(node, arena); node },
        PreNode::Log(val) => {
            let l = arena.new_node(Node::Log(parent));
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
        PreNode::Assign(name, val) => {
            let a = arena.new_node(Node::Assign{name: name.clone(), value_id: parent});
            let val_id = append_node(*val, a, arena);
            *arena.get_mut(a).unwrap().get_mut() = Node::Assign{name, value_id: val_id};
            parent.append(a, arena);
            a
        },
        PreNode::FieldAssign(obj, field, val) => {
            let fa = arena.new_node(Node::FieldAssign{object: obj.clone(), field: field.clone(), value_id: parent});
            let val_id = append_node(*val, fa, arena);
            *arena.get_mut(fa).unwrap().get_mut() = Node::FieldAssign{object: obj, field, value_id: val_id};
            parent.append(fa, arena);
            fa
        },
        PreNode::FieldAccess(obj, field) => {
            let n = arena.new_node(Node::FieldAccess{object: obj, field});
            parent.append(n, arena);
            n
        },
        PreNode::BinaryOp(op, lhs, rhs) => {
            let b = arena.new_node(Node::BinaryOp { op: op.clone(), lhs: parent, rhs: parent });
            let l = append_node(*lhs, b, arena);
            let r = append_node(*rhs, b, arena);
            *arena.get_mut(b).unwrap().get_mut() = Node::BinaryOp { op, lhs: l, rhs: r };
            parent.append(b, arena);
            b
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
        PreNode::MethodCall(obj, method, args) => {
            let c = arena.new_node(Node::MethodCall{object: obj, method, args_ids: vec![]});
            let mut ids = vec![];
            for arg in args {
                ids.push(append_node(arg, c, arena));
            }
            if let Node::MethodCall{object, method, ..} = arena.get(c).unwrap().get().clone() {
                *arena.get_mut(c).unwrap().get_mut() = Node::MethodCall{object, method, args_ids: ids};
            }
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
