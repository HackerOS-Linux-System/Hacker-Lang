#![allow(dead_code, unused_assignments, unused_variables)]
use chumsky::prelude::*;
use chumsky::error::SimpleReason;
use chumsky::Parser as ChumskyParser;
use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;
use colored::Colorize;

pub const HACKER_DIR_SUFFIX: &str = ".hackeros/hacker-lang";

// --- AST Structures ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    I32(i32),
    F64(f64),
    Str(String),
    Bool(bool),
    List(Vec<Value>),
    Map(HashMap<String, Value>),
    Option(Option<Box<Value>>),
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    Lit(Value),
    Var(String),
    Call { name: String, args: Vec<Expr> },
    BinOp { op: String, left: Box<Expr>, right: Box<Expr> },
    Pipe { left: Box<Expr>, right: Box<Expr> },
    List(Vec<Expr>),
    Map(HashMap<String, Expr>),
    Wildcard,
    Spawn { body: Vec<Stmt> },
    Await(Box<Expr>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    Raw { mode: String, cmd: String },
    AssignGlobal { key: String, ty: Option<String>, val: Expr },
    AssignLocal { key: String, ty: Option<String>, val: Expr },
    Repeat { count: u64, body: Vec<Stmt> },
    If { cond: Expr, body: Vec<Stmt>, else_ifs: Vec<(Expr, Vec<Stmt>)>, else_body: Option<Vec<Stmt>> },
    Background(Vec<Stmt>),
    Plugin { name: String, is_super: bool },
    Function { name: String, params: Vec<(String, String)>, ret_ty: Option<String>, body: Vec<Stmt>, is_quick: bool },
    Return { expr: Expr },
    Object {
        name: String,
        fields: Vec<(bool, String, String, Option<Expr>)>,
        methods: HashMap<String, (Vec<(String, String)>, Option<String>, Vec<Stmt>)>,
    },
    Try { body: Vec<Stmt>, catches: Vec<(String, String, Vec<Stmt>)>, else_body: Option<Vec<Stmt>>, finally: Option<Vec<Stmt>> },
    Match { expr: Expr, arms: Vec<(Expr, Vec<Stmt>)> },
    For { var: String, iter: Expr, body: Vec<Stmt> },
    ForIndexed { idx: String, var: String, iter: Expr, body: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    Break,
    Continue,
    Import { prefix: String, name: String, version: Option<String> },
    Expr(Expr),
    Block(Vec<Stmt>),
    Log(String),
    Lock(String),
    Unlock(String),
    WithLock { var: String, body: Vec<Stmt> },
    Module { name: String, body: Vec<Stmt> },
}

impl Default for Stmt {
    fn default() -> Self {
        Stmt::Raw { mode: String::new(), cmd: String::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgramNode {
    pub line_num: usize,
    pub is_sudo: bool,
    pub content: Stmt,
    pub original_text: String,
    pub span: (usize, usize),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    pub deps: Vec<String>,
    pub libs: Vec<String>,
    /// name - (params, return_type, body_nodes, is_quick)
    pub functions: HashMap<String, (Vec<(String, String)>, Option<String>, Vec<ProgramNode>, bool)>,
    pub objects: HashMap<String, ProgramNode>,
    pub main_body: Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings: Vec<String>,
}

// --- Parse Errors ---

#[derive(Error, Debug, Diagnostic)]
pub enum ParseError {
    #[error("Syntax Error")]
    #[diagnostic(code(hl::syntax_error))]
    SyntaxError {
        #[source_code]
        src: NamedSource,
        #[label("Invalid syntax")]
        span: SourceSpan,
        #[help]
        advice: String,
    },
    #[error("Structure Error")]
    #[diagnostic(code(hl::structure_error))]
    StructureError {
        #[source_code]
        src: NamedSource,
        #[label("Here")]
        span: SourceSpan,
        #[help]
        message: String,
    },
}

// --- Parser ---

fn parser() -> impl Parser<char, Vec<ProgramNode>, Error = Simple<char>> {
    let comment = just('!').then(take_until(just('\n').ignored().or(end()))).ignored();
    let whitespace = choice((comment, text::whitespace().at_least(1).ignored())).repeated().ignored();

    recursive(|stmt| {
        let qual_ident = text::ident()
        .then(just("::").then(text::ident()).repeated())
        .foldl(|acc, (_, next)| format!("{}::{}", acc, next))
        .padded_by(whitespace.clone());

        let ty = recursive(|ty: Recursive<'_, char, String, Simple<char>>| {
            text::ident()
            .then(
                just('<')
                .padded_by(whitespace.clone())
                .ignore_then(ty.separated_by(just(',').padded_by(whitespace.clone())))
                .then_ignore(just('>').padded_by(whitespace.clone()))
                .or_not(),
            )
            .map(|(name, tys)| {
                if let Some(tys) = tys {
                    format!("{}<{}>", name, tys.join(","))
                } else {
                    name
                }
            })
            .padded_by(whitespace.clone())
        });

        let expr = recursive(|expr| {
            let literal = choice((
                text::int(10)
                .then_ignore(just('.'))
                .then(text::int(10))
                .map(|(i, f): (String, String)| {
                    Value::F64(format!("{}.{}", i, f).parse().unwrap())
                }),
                text::int(10).map(|s: String| Value::I32(s.parse().unwrap())),
                                  just("true").to(Value::Bool(true)),
                                  just("false").to(Value::Bool(false)),
                                  just('"').ignore_then(take_until(just('"'))).map(|(chars, _)| Value::Str(chars.into_iter().collect())),
                                  just("none").to(Value::None),
            ))
            .map(Expr::Lit);

            let atom = choice((
                literal,
                qual_ident
                .clone()
                .then(
                    expr.clone()
                    .separated_by(just(',').padded_by(whitespace.clone()))
                    .delimited_by(
                        just('(').padded_by(whitespace.clone()),
                                  just(')').padded_by(whitespace.clone()),
                    ),
                )
                .map(|(name, args)| Expr::Call { name, args }),
                               qual_ident.clone().map(Expr::Var),
                               expr.clone().delimited_by(
                                   just('(').padded_by(whitespace.clone()),
                                                         just(')').padded_by(whitespace.clone()),
                               ),
                               expr.clone()
                               .separated_by(just(',').padded_by(whitespace.clone()))
                               .delimited_by(just('['), just(']'))
                               .map(Expr::List),
                               just('{')
                               .padded_by(whitespace.clone())
                               .ignore_then(
                                   qual_ident
                                   .clone()
                                   .then_ignore(just(':').padded_by(whitespace.clone()))
                                   .then(expr.clone())
                                   .separated_by(just(',').padded_by(whitespace.clone())),
                               )
                               .then_ignore(just('}').padded_by(whitespace.clone()))
                               .collect::<HashMap<_, _>>()
                               .map(Expr::Map),
            ))
            .padded_by(whitespace.clone());

            let pipe = atom.clone().then(just("|>").padded_by(whitespace.clone()).ignore_then(expr.clone()).repeated()).foldl(|left, right| Expr::Pipe {
                left: Box::new(left),
                                                                                                                              right: Box::new(right),
            });

            let product = pipe.clone().then(choice((just('*').to("*".to_string()), just('/').to("/".to_string()))).padded_by(whitespace.clone()).then(pipe).repeated()).foldl(|left, (op, right): (String, Expr)| Expr::BinOp {
                op,
                left: Box::new(left),
                                                                                                                                                                              right: Box::new(right),
            });

            let sum = product.clone().then(choice((just('+').to("+".to_string()), just('-').to("-".to_string()))).padded_by(whitespace.clone()).then(product).repeated()).foldl(|left, (op, right): (String, Expr)| Expr::BinOp {
                op,
                left: Box::new(left),
                                                                                                                                                                                right: Box::new(right),
            });

            let comparison = sum.clone().then(
                choice((
                    just("==").to("==".to_string()),
                        just("!=").to("!=".to_string()),
                        just("<=").to("<=".to_string()),
                        just(">=").to(">=".to_string()),
                        just("<").to("<".to_string()),
                        just(">").to(">".to_string())
                )).padded_by(whitespace.clone()).then(sum).repeated()
            ).foldl(|left, (op, right): (String, Expr)| Expr::BinOp {
                op,
                left: Box::new(left),
                    right: Box::new(right),
            });

            let logical_and = comparison.clone().then(just("&&").to("&&".to_string()).padded_by(whitespace.clone()).then(comparison).repeated()).foldl(|left, (op, right): (String, Expr)| Expr::BinOp {
                op,
                left: Box::new(left),
                                                                                                                                                       right: Box::new(right),
            });

            let logical_or = logical_and.clone().then(just("||").to("||".to_string()).padded_by(whitespace.clone()).then(logical_and).repeated()).foldl(|left, (op, right): (String, Expr)| Expr::BinOp {
                op,
                left: Box::new(left),
                                                                                                                                                        right: Box::new(right),
            });

            logical_or
        });

        let assign_global = just('@')
        .ignore_then(qual_ident.clone())
        .then(just(':').padded_by(whitespace.clone()).ignore_then(ty.clone()).or_not())
        .then_ignore(just('=').padded_by(whitespace.clone()))
        .then(expr.clone())
        .map(|((key, ty), val)| Stmt::AssignGlobal { key, ty, val });

        let assign_local = just('$')
        .ignore_then(qual_ident.clone())
        .then(just(':').padded_by(whitespace.clone()).ignore_then(ty.clone()).or_not())
        .then_ignore(just('=').padded_by(whitespace.clone()))
        .then(expr.clone())
        .map(|((key, ty), val)| Stmt::AssignLocal { key, ty, val });

        let param = qual_ident.clone().then_ignore(just(':').padded_by(whitespace.clone())).then(ty.clone());
        let params = param
        .separated_by(just(',').padded_by(whitespace.clone()))
        .allow_trailing()
        .delimited_by(
            just('(').padded_by(whitespace.clone()),
                      just(')').padded_by(whitespace.clone()),
        );

        let func_prefix = choice((
            just("::").to(true),
                                  just(':').to(false),
        ));

        let func_def = func_prefix
        .then(qual_ident.clone())
        .then(params.clone())
        .then(just("->").padded_by(whitespace.clone()).ignore_then(ty.clone()).or_not())
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|((((is_quick, name), params), ret_ty), body): ((((bool, String), Vec<(String, String)>), Option<String>), Vec<ProgramNode>)| Stmt::Function {
            name,
            params,
            ret_ty,
            body: body.into_iter().map(|n| n.content).collect(),
             is_quick,
        });

        let ret = just("<-")
        .padded_by(whitespace.clone())
        .ignore_then(expr.clone())
        .map(|expr| Stmt::Return { expr });

        let field = just("mut")
        .padded_by(whitespace.clone())
        .or_not()
        .map(|m: Option<&str>| m.is_some())
        .then(qual_ident.clone())
        .then_ignore(just(':').padded_by(whitespace.clone()))
        .then(ty.clone())
        .then(just('=').padded_by(whitespace.clone()).ignore_then(expr.clone()).or_not())
        .map(|(((mut_, name), ty), init)| (mut_, name, ty, init));

        let method = just(':')
        .ignore_then(qual_ident.clone())
        .then(params.clone())
        .then(just("->").padded_by(whitespace.clone()).ignore_then(ty.clone()).or_not())
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|(((name, params), ret_ty), body): (((String, Vec<(String, String)>), Option<String>), Vec<ProgramNode>)| (name, (params, ret_ty, body.into_iter().map(|n| n.content).collect())));

        let obj_def = just(";;")
        .ignore_then(qual_ident.clone())
        .then(
            field
            .separated_by(just(',').padded_by(whitespace.clone()))
            .then(method.repeated())
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|(name, (fields, methods)): (String, (Vec<(bool, String, String, Option<Expr>)>, Vec<(String, (Vec<(String, String)>, Option<String>, Vec<Stmt>))>))| Stmt::Object {
            name,
            fields,
            methods: methods.into_iter().collect(),
        });

        let catch = just("catch")
        .padded_by(whitespace.clone())
        .ignore_then(
            just('(')
            .padded_by(whitespace.clone())
            .ignore_then(qual_ident.clone())
            .then_ignore(just(':').padded_by(whitespace.clone()))
            .then(qual_ident.clone())
            .then_ignore(just(')').padded_by(whitespace.clone()))
        )
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        );

        let else_body = just("else")
        .padded_by(whitespace.clone())
        .ignore_then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .or_not();

        let finally = just("finally")
        .padded_by(whitespace.clone())
        .ignore_then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .or_not();

        let try_stmt = just("try")
        .ignore_then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .then(catch.repeated())
        .then(else_body.clone())
        .then(finally)
        .map(|(((body, catches), else_body), finally): (((Vec<ProgramNode>, Vec<((String, String), Vec<ProgramNode>)>), Option<Vec<ProgramNode>>), Option<Vec<ProgramNode>>)| Stmt::Try {
            body: body.into_iter().map(|n| n.content).collect(),
             catches: catches
             .into_iter()
             .map(|((var, ty), body)| (var, ty, body.into_iter().map(|n| n.content).collect()))
             .collect(),
             else_body: else_body.map(|f| f.into_iter().map(|n| n.content).collect()),
                 finally: finally.map(|f| f.into_iter().map(|n| n.content).collect()),
        });

        let arm = choice((expr.clone(), just("_").to(Expr::Wildcard)))
        .then(stmt.clone().repeated())
        .padded_by(whitespace.clone());

        let match_stmt = just("match")
        .ignore_then(expr.clone())
        .then(
            arm.repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|(expr, arms): (Expr, Vec<(Expr, Vec<ProgramNode>)>)| Stmt::Match {
            expr,
            arms: arms.into_iter().map(|(e, b)| (e, b.into_iter().map(|n| n.content).collect())).collect(),
        });

        let for_stmt = just("for")
        .ignore_then(qual_ident.clone())
        .then_ignore(just("in").padded_by(whitespace.clone()))
        .then(expr.clone())
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|((var, iter), body): ((String, Expr), Vec<ProgramNode>)| Stmt::For {
            var,
            iter,
            body: body.into_iter().map(|n| n.content).collect(),
        });

        let for_indexed = just("for")
        .ignore_then(qual_ident.clone())
        .then_ignore(just(',').padded_by(whitespace.clone()))
        .then(qual_ident.clone())
        .then_ignore(just("in").padded_by(whitespace.clone()))
        .then(
            just("enumerate(")
            .padded_by(whitespace.clone())
            .ignore_then(expr.clone())
            .then_ignore(just(')').padded_by(whitespace.clone()))
        )
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|(((idx, var), iter), body): (((String, String), Expr), Vec<ProgramNode>)| Stmt::ForIndexed {
            idx,
             var,
             iter,
             body: body.into_iter().map(|n| n.content).collect(),
        });

        let while_stmt = just("while")
        .ignore_then(expr.clone())
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|(cond, body): (Expr, Vec<ProgramNode>)| Stmt::While {
            cond,
            body: body.into_iter().map(|n| n.content).collect(),
        });

        let elif = just("elif")
        .padded_by(whitespace.clone())
        .ignore_then(expr.clone())
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        );

        let else_stmt = just("else")
        .padded_by(whitespace.clone())
        .ignore_then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .or_not();

        let if_stmt = just("if")
        .padded_by(whitespace.clone())
        .ignore_then(expr.clone())
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .then(elif.repeated())
        .then(else_stmt)
        .map(|(((cond, body), else_ifs), else_body): (((Expr, Vec<ProgramNode>), Vec<(Expr, Vec<ProgramNode>)>), Option<Vec<ProgramNode>>)| Stmt::If {
            cond,
            body: body.into_iter().map(|n| n.content).collect(),
             else_ifs: else_ifs.into_iter().map(|(c, b)| (c, b.into_iter().map(|n| n.content).collect())).collect(),
                 else_body: else_body.map(|b| b.into_iter().map(|n| n.content).collect()),
        });

        let repeat_stmt = just("repeat")
        .padded_by(whitespace.clone())
        .ignore_then(text::int(10).padded_by(whitespace.clone()))
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|(count, body): (String, Vec<ProgramNode>)| Stmt::Repeat {
            count: count.parse().unwrap(),
             body: body.into_iter().map(|n| n.content).collect(),
        });

        let break_stmt = just("break").padded_by(whitespace.clone()).to(Stmt::Break);
        let continue_stmt = just("continue").padded_by(whitespace.clone()).to(Stmt::Continue);

        let import_stmt = just('#')
        .ignore_then(just('<'))
        .ignore_then(text::ident())
        .then_ignore(just('/'))
        .then(filter(|c| *c != ':' && *c != '>').repeated().map(|chars: Vec<char>| chars.into_iter().collect::<String>()))
        .then(just(':').ignore_then(filter(|c| *c != '>').repeated().map(|chars: Vec<char>| chars.into_iter().collect::<String>())).or_not())
        .then_ignore(just('>'))
        .map(|((prefix, name), version)| Stmt::Import { prefix, name, version });

        let plugin = just('\\')
        .ignore_then(qual_ident.clone())
        .map(|name| Stmt::Plugin { name, is_super: false });

        let raw = just('>')
        .repeated()
        .at_least(1)
        .at_most(3)
        .collect::<String>()
        .then(take_until(just('\n').ignored().or(end())))
        .map(|(mode, (chars, _))| Stmt::Raw {
            mode,
            cmd: chars.into_iter().collect::<String>().trim().to_string(),
        });

        let log_stmt = just("log")
        .padded_by(whitespace.clone())
        .ignore_then(
            just('"')
            .ignore_then(take_until(just('"')))
            .map(|(chars, _)| chars.into_iter().collect::<String>())
        )
        .map(Stmt::Log);

        let lock_stmt = just("lock")
        .padded_by(whitespace.clone())
        .ignore_then(qual_ident.clone())
        .map(Stmt::Lock);

        let unlock_stmt = just("unlock")
        .padded_by(whitespace.clone())
        .ignore_then(qual_ident.clone())
        .map(Stmt::Unlock);

        let withlock_stmt = just("withlock")
        .padded_by(whitespace.clone())
        .ignore_then(qual_ident.clone())
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|(var, body): (String, Vec<ProgramNode>)| Stmt::WithLock { var, body: body.into_iter().map(|n| n.content).collect() });

        let module_stmt = just("module")
        .padded_by(whitespace.clone())
        .ignore_then(qual_ident.clone())
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        )
        .map(|(name, body): (String, Vec<ProgramNode>)| Stmt::Module { name, body: body.into_iter().map(|n| n.content).collect() });

        let block = stmt.clone()
        .repeated()
        .delimited_by(
            just('[').padded_by(whitespace.clone()),
                      just(']').padded_by(whitespace.clone()),
        )
        .map(|body: Vec<ProgramNode>| Stmt::Block(body.into_iter().map(|n| n.content).collect()));

        let expr_stmt = expr.clone().map(Stmt::Expr);
        let await_stmt = just("await").ignore_then(expr.clone()).map(|e| Stmt::Expr(Expr::Await(Box::new(e))));

        let decorated = just('^')
        .padded_by(whitespace.clone())
        .to(true)
        .or_not()
        .map(|s: Option<bool>| s.unwrap_or(false))
        .then(
            just('&')
            .padded_by(whitespace.clone())
            .to(true)
            .or_not()
            .map(|b: Option<bool>| b.unwrap_or(false)),
        )
        .then(choice((
            assign_global,
            assign_local,
            func_def,
            if_stmt,
            ret,
            obj_def,
            try_stmt,
            match_stmt,
            for_indexed,
                for_stmt,
                    repeat_stmt,
                    while_stmt,
                        break_stmt,
                      continue_stmt,
                      import_stmt,
                      plugin,
                      raw,
                      block,
                      expr_stmt,
                      log_stmt,
                      lock_stmt,
                      unlock_stmt,
                      withlock_stmt,
                      module_stmt,
                      await_stmt,
        )))
        .map_with_span(|((is_sudo, is_bg), mut stmt): ((bool, bool), Stmt), span: std::ops::Range<usize>| {
            if is_bg {
                if let Stmt::Block(body) = stmt {
                    stmt = Stmt::Expr(Expr::Spawn { body });
                } else {
                    stmt = Stmt::Expr(Expr::Spawn { body: vec![stmt] });
                }
            }
            ProgramNode {
                is_sudo,
                content: stmt,
                span: (span.start, span.end),
                       ..Default::default()
            }
        });

        decorated.padded_by(whitespace.clone()).recover_with(skip_then_retry_until(['\n']))
    })
    .repeated()
    .then_ignore(whitespace)
    .then_ignore(end())
}

fn is_dangerous(stmt: &Stmt) -> bool {
    if let Stmt::Raw { cmd, .. } = stmt {
        let dangerous_patterns = ["rm -rf", "mkfs", "dd if=", ":(){:|:&};:", "> /dev/sda"];
        for pat in dangerous_patterns {
            if cmd.contains(pat) {
                return true;
            }
        }
    }
    false
}

fn validate_ast(nodes: &[ProgramNode], in_func: bool, is_quick: bool, path: &str, content: &str) -> Vec<ParseError> {
    let mut errors = Vec::new();
    for node in nodes {
        match &node.content {
            Stmt::Return { .. } if !in_func => {
                errors.push(ParseError::StructureError {
                    src: NamedSource::new(path, content.to_string()),
                            span: SourceSpan::new((node.span.0).into(), (node.span.1 - node.span.0).into()),
                            message: "Return statement outside of function".to_string(),
                });
            }
            Stmt::Lock(_) | Stmt::Unlock(_) if !is_quick => {
                errors.push(ParseError::StructureError {
                    src: NamedSource::new(path, content.to_string()),
                            span: SourceSpan::new((node.span.0).into(), (node.span.1 - node.span.0).into()),
                            message: "Lock/Unlock can only be used in quick functions (::)".to_string(),
                });
            }
            Stmt::Function { body, is_quick: func_quick, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, true, *func_quick, path, content));
            }
            Stmt::If { body, else_ifs, else_body, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
                for (_, b) in else_ifs {
                    let b_nodes: Vec<ProgramNode> = b.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                    errors.extend(validate_ast(&b_nodes, in_func, is_quick, path, content));
                }
                if let Some(b) = else_body {
                    let b_nodes: Vec<ProgramNode> = b.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                    errors.extend(validate_ast(&b_nodes, in_func, is_quick, path, content));
                }
            }
            Stmt::While { body, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
            }
            Stmt::For { body, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
            }
            Stmt::ForIndexed { body, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
            }
            Stmt::Repeat { body, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
            }
            Stmt::Try { body, catches, else_body, finally, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
                for (_, _, b) in catches {
                    let b_nodes: Vec<ProgramNode> = b.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                    errors.extend(validate_ast(&b_nodes, in_func, is_quick, path, content));
                }
                if let Some(b) = else_body {
                    let b_nodes: Vec<ProgramNode> = b.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                    errors.extend(validate_ast(&b_nodes, in_func, is_quick, path, content));
                }
                if let Some(f) = finally {
                    let f_nodes: Vec<ProgramNode> = f.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                    errors.extend(validate_ast(&f_nodes, in_func, is_quick, path, content));
                }
            }
            Stmt::Match { arms, .. } => {
                for (_, b) in arms {
                    let b_nodes: Vec<ProgramNode> = b.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                    errors.extend(validate_ast(&b_nodes, in_func, is_quick, path, content));
                }
            }
            Stmt::Block(body) => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
            }
            Stmt::WithLock { body, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
            }
            Stmt::Module { body, .. } => {
                let body_nodes: Vec<ProgramNode> = body.iter().map(|s| ProgramNode { content: s.clone(), span: node.span, ..Default::default() }).collect();
                errors.extend(validate_ast(&body_nodes, in_func, is_quick, path, content));
            }
            _ => {}
        }
    }
    errors
}

fn nice_advice(error: &Simple<char>) -> String {
    let reason = match error.reason() {
        SimpleReason::Unexpected => "unexpected input".to_string(),
        SimpleReason::Unclosed { delimiter, .. } => format!("unclosed delimiter {:?}", delimiter),
        SimpleReason::Custom(msg) => msg.clone(),
    };

    let expected: Vec<_> = error
    .expected()
    .map(|e| match e {
        Some(c) => format!("{:?}", c),
         None => "end of input".to_string(),
    })
    .collect();

    if !expected.is_empty() {
        format!("{}, expected one of {}", reason, expected.join(" or "))
    } else {
        reason
    }
}

/// Parse a Hacker Lang source file and return an AnalysisResult.
pub fn parse_file(
    path: &str,
    resolve_libs: bool,
    verbose: bool,
    seen_libs: &mut HashSet<String>,
) -> Result<AnalysisResult, Vec<ParseError>> {
    let mut result = AnalysisResult::default();
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(result),
    };

    match parser().parse(content.clone()) {
        Ok(mut stmts) => {
            let mut m_errors: Vec<ParseError> = Vec::new();
            for node in stmts.iter() {
                if let Stmt::Raw { mode, cmd } = &node.content {
                    if !mode.is_empty() && cmd.trim_start().starts_with("log") {
                        m_errors.push(ParseError::StructureError {
                            src: NamedSource::new(path, content.clone()),
                                      span: SourceSpan::new((node.span.0).into(), (node.span.1 - node.span.0).into()),
                                      message: "log cannot be used together with > or >> or >>>".to_string(),
                        });
                    }
                    if mode == ">" {
                        if cmd.contains('$') {
                            m_errors.push(ParseError::StructureError {
                                src: NamedSource::new(path, content.clone()),
                                          span: SourceSpan::new((node.span.0).into(), (node.span.1 - node.span.0).into()),
                                          message: "No variables allowed in > mode".to_string(),
                            });
                        }
                    } else if mode == ">>>" {
                        result.safety_warnings.push("Using >>> mode for separate process".to_string());
                    }
                }
            }
            m_errors.extend(validate_ast(&stmts, false, false, path, &content));
            if !m_errors.is_empty() {
                return Err(m_errors);
            }

            for (idx, node) in stmts.iter_mut().enumerate() {
                node.line_num = idx + 1;
                if is_dangerous(&node.content) {
                    result.is_potentially_unsafe = true;
                    result.safety_warnings.push(format!("Line {}: Potential destructive command", idx + 1));
                }
                match &node.content {
                    Stmt::Function { name, params, ret_ty, body, is_quick } => {
                        let body_nodes = body.iter().map(|s| ProgramNode { content: s.clone(), ..Default::default() }).collect();
                        result.functions.insert(name.clone(), (params.clone(), ret_ty.clone(), body_nodes, *is_quick));
                    }
                    Stmt::Object { name, .. } => {
                        result.objects.insert(name.clone(), node.clone());
                    }
                    Stmt::Import { prefix, name, version } => {
                        let lib_key = format!("{}:{}", prefix, name);
                        if seen_libs.contains(&lib_key) {
                            continue;
                        }
                        seen_libs.insert(lib_key.clone());
                        if prefix == "bytes" || prefix == "crates" {
                            let dep_str = format!("{} = \"{}\"", name.replace("-", "_"), version.clone().unwrap_or("*".to_string()));
                            result.deps.push(dep_str);
                        } else if prefix == "github" {
                            let repo_name = name.split('/').last().map(|s| s.replace(".git", "")).unwrap_or(name.clone());
                            let mut git_str = format!("{} = {{ git = \"https://github.com/{}\"", repo_name.replace("-", "_"), name);
                            if let Some(v) = version.clone() {
                                git_str.push_str(&format!(", branch = \"{}\"", v));
                            }
                            git_str.push_str(" }");
                            result.deps.push(git_str);
                        } else {
                            result.libs.push(lib_key.clone());
                            if resolve_libs {
                                let libs_dir = if prefix == "local" {
                                    PathBuf::from(path).parent().unwrap().to_path_buf()
                                } else if prefix == "core" {
                                    PathBuf::from("/usr/lib/Hacker-Lang/libs/core")
                                } else {
                                    dirs::home_dir().unwrap().join(HACKER_DIR_SUFFIX).join("libs")
                                };
                                let lib_path = libs_dir.join(format!("{}.hl", name));
                                if verbose {
                                    println!("{} Resolving library: {:?}", "[*]".blue(), lib_path);
                                }
                                if lib_path.exists() {
                                    if let Ok(lib_res) = parse_file(lib_path.to_str().unwrap(), resolve_libs, verbose, seen_libs) {
                                        result.deps.extend(lib_res.deps);
                                        result.libs.extend(lib_res.libs);
                                        for (k, v) in lib_res.functions {
                                            result.functions.insert(k, v);
                                        }
                                        result.objects.extend(lib_res.objects);
                                        if lib_res.is_potentially_unsafe {
                                            result.is_potentially_unsafe = true;
                                            result.safety_warnings.push(format!("Imported library {} contains unsafe code", name));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        result.main_body.push(node.clone());
                    }
                }
            }
            Ok(result)
        }
        Err(errors) => {
            let mut m_errors = Vec::new();
            for e in errors {
                m_errors.push(ParseError::SyntaxError {
                    src: NamedSource::new(path, content.clone()),
                              span: e.span().into(),
                              advice: nice_advice(&e),
                });
            }
            Err(m_errors)
        }
    }
}
