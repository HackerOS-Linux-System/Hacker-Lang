#![allow(dead_code, unused_assignments, unused_variables)]
use chumsky::prelude::*;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    Raw(String),
    AssignGlobal { key: String, ty: Option<String>, val: Expr },
    AssignLocal { key: String, ty: Option<String>, val: Expr },
    LoopTimes { count: u64, body: Vec<Stmt> },
    If { cond: Expr, body: Vec<Stmt> },
    Background(Vec<Stmt>),
    Plugin { name: String, is_super: bool },
    Function { name: String, params: Vec<(String, String)>, ret_ty: Option<String>, body: Vec<Stmt> },
    Return { expr: Expr },
    Object {
        name: String,
        fields: Vec<(bool, String, String, Option<Expr>)>,
        methods: HashMap<String, (Vec<(String, String)>, Option<String>, Vec<Stmt>)>,
    },
    Try { body: Vec<Stmt>, catches: Vec<(String, String, Vec<Stmt>)>, finally: Option<Vec<Stmt>> },
    Match { expr: Expr, arms: Vec<(Expr, Vec<Stmt>)> },
    For { var: String, iter: Expr, body: Vec<Stmt> },
    ForIndexed { idx: String, var: String, iter: Expr, body: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    Break,
    Continue,
    Import { prefix: String, name: String, version: Option<String> },
    Expr(Expr),
    Block(Vec<Stmt>),
}

impl Default for Stmt {
    fn default() -> Self {
        Stmt::Raw(String::new())
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
    /// name → (params, return_type, body_nodes)
    pub functions: HashMap<String, (Vec<(String, String)>, Option<String>, Vec<ProgramNode>)>,
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
        message: String,
    },
}

// --- Parser ---

fn parser() -> impl Parser<char, Vec<ProgramNode>, Error = Simple<char>> {
    let comment = just('!').then(take_until(just('\n').ignored().or(end()))).ignored();
    let whitespace = choice((comment, text::whitespace().at_least(1).ignored())).repeated().ignored();

    recursive(|stmt| {
        let ident = text::ident().padded_by(whitespace.clone());
        let ty = ident.clone().labelled("type");

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
                              just('"')
                              .ignore_then(take_until(just('"')))
                              .map(|(chars, _)| Value::Str(chars.into_iter().collect())),
                              just("none").to(Value::None),
        ))
        .map(Expr::Lit);

        let expr = recursive(|expr| {
            let atom = choice((
                literal,
                ident
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
                               ident.clone().map(Expr::Var),
                               expr.clone().delimited_by(
                                   just('(').padded_by(whitespace.clone()),
                                                         just(')').padded_by(whitespace.clone()),
                               ),
            ))
            .padded_by(whitespace.clone());

            let pipe = atom.clone().then(just("|>").padded_by(whitespace.clone()).ignore_then(expr.clone()).repeated()).foldl(|left, right| Expr::Pipe {
                left: Box::new(left),
                                                                                                                              right: Box::new(right),
            });

            let product = pipe.clone().then(choice((just('*'), just('/'))).padded_by(whitespace.clone()).then(pipe).repeated()).foldl(|left, (op, right): (char, Expr)| Expr::BinOp {
                op: op.to_string(),
                                                                                                                                      left: Box::new(left),
                                                                                                                                      right: Box::new(right),
            });

            let sum = product.clone().then(choice((just('+'), just('-'))).padded_by(whitespace.clone()).then(product).repeated()).foldl(|left, (op, right): (char, Expr)| Expr::BinOp {
                op: op.to_string(),
                                                                                                                                        left: Box::new(left),
                                                                                                                                        right: Box::new(right),
            });

            let comparison = sum.clone().then(
                choice((
                    just("=="), just("!="), just("<="), just(">="), just("<"), just(">")
                )).padded_by(whitespace.clone()).then(sum).repeated()
            ).foldl(|left, (op, right)| Expr::BinOp {
                op: op.to_string(),
                    left: Box::new(left),
                    right: Box::new(right),
            });

            let logical_and = comparison.clone().then(just("&&").padded_by(whitespace.clone()).then(comparison).repeated()).foldl(|left, (op, right)| Expr::BinOp {
                op: op.to_string(),
                                                                                                                                  left: Box::new(left),
                                                                                                                                  right: Box::new(right),
            });

            let logical_or = logical_and.clone().then(just("||").padded_by(whitespace.clone()).then(logical_and).repeated()).foldl(|left, (op, right)| Expr::BinOp {
                op: op.to_string(),
                                                                                                                                   left: Box::new(left),
                                                                                                                                   right: Box::new(right),
            });

            logical_or
        });

        let assign_global = just('@')
        .ignore_then(ident.clone())
        .then(just(':').padded_by(whitespace.clone()).ignore_then(ty.clone()).or_not())
        .then_ignore(just('=').padded_by(whitespace.clone()))
        .then(expr.clone())
        .map(|((key, ty), val)| Stmt::AssignGlobal { key, ty, val });

        let assign_local = just('$')
        .ignore_then(ident.clone())
        .then(just(':').padded_by(whitespace.clone()).ignore_then(ty.clone()).or_not())
        .then_ignore(just('=').padded_by(whitespace.clone()))
        .then(expr.clone())
        .map(|((key, ty), val)| Stmt::AssignLocal { key, ty, val });

        let param = ident.clone().then_ignore(just(':').padded_by(whitespace.clone())).then(ty.clone());
        let params = param
        .separated_by(just(',').padded_by(whitespace.clone()))
        .allow_trailing()
        .delimited_by(
            just('(').padded_by(whitespace.clone()),
                      just(')').padded_by(whitespace.clone()),
        );

        let func_def = just(':')
        .ignore_then(ident.clone())
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
        .map(|(((name, params), ret_ty), body): (((String, Vec<(String, String)>), Option<String>), Vec<ProgramNode>)| Stmt::Function {
            name,
            params,
            ret_ty,
            body: body.into_iter().map(|n| n.content).collect(),
        });

        let ret = just("<-")
        .padded_by(whitespace.clone())
        .ignore_then(expr.clone())
        .map(|expr| Stmt::Return { expr });

        let field = just("mut")
        .padded_by(whitespace.clone())
        .or_not()
        .map(|m: Option<&str>| m.is_some())
        .then(ident.clone())
        .then_ignore(just(':').padded_by(whitespace.clone()))
        .then(ty.clone())
        .then(just('=').padded_by(whitespace.clone()).ignore_then(expr.clone()).or_not())
        .map(|(((mut_, name), ty), init)| (mut_, name, ty, init));

        let method = just(':')
        .ignore_then(ident.clone())
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

        let obj_def = just("obj")
        .ignore_then(ident.clone())
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
            .ignore_then(ident.clone())
            .then_ignore(just(':').padded_by(whitespace.clone()))
            .then(ident.clone())
            .then_ignore(just(')').padded_by(whitespace.clone())),
        )
        .then(
            stmt.clone()
            .repeated()
            .delimited_by(
                just('[').padded_by(whitespace.clone()),
                          just(']').padded_by(whitespace.clone()),
            ),
        );

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
        .then(finally)
        .map(|((body, catches), finally): ((Vec<ProgramNode>, Vec<((String, String), Vec<ProgramNode>)>), Option<Vec<ProgramNode>>)| Stmt::Try {
            body: body.into_iter().map(|n| n.content).collect(),
             catches: catches
             .into_iter()
             .map(|((var, ty), body)| (var, ty, body.into_iter().map(|n| n.content).collect()))
             .collect(),
             finally: finally.map(|f| f.into_iter().map(|n| n.content).collect()),
        });

        let arm = expr
        .clone()
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

        let for_stmt = just("loop")
        .ignore_then(ident.clone())
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

        let for_indexed = just("loop")
        .ignore_then(ident.clone())
        .then_ignore(just(',').padded_by(whitespace.clone()))
        .then(ident.clone())
        .then_ignore(just("in").padded_by(whitespace.clone()))
        .then(
            just("enumerate(")
            .padded_by(whitespace.clone())
            .ignore_then(expr.clone())
            .then_ignore(just(')').padded_by(whitespace.clone())),
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

        let while_stmt = just("loop")
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
        .map(|(cond, body): (Expr, Vec<ProgramNode>)| Stmt::If {
            cond,
            body: body.into_iter().map(|n| n.content).collect(),
        });

        let loop_times = just("loop")
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
        .map(|(count, body): (String, Vec<ProgramNode>)| Stmt::LoopTimes {
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
        .ignore_then(ident.clone())
        .map(|name| Stmt::Plugin { name, is_super: false });

        let raw = just('>')
        .ignore_then(take_until(just('\n').ignored().or(end())))
        .map(|(chars, _): (Vec<char>, ())| Stmt::Raw(chars.into_iter().collect::<String>().trim().to_string()));

        let block = stmt.clone()
        .repeated()
        .delimited_by(
            just('[').padded_by(whitespace.clone()),
                      just(']').padded_by(whitespace.clone()),
        )
        .map(|body| Stmt::Block(body.into_iter().map(|n| n.content).collect()));

        let expr_stmt = expr.clone().map(Stmt::Expr);

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
                    loop_times,
                    while_stmt,
                        break_stmt,
                      continue_stmt,
                      import_stmt,
                      plugin,
                      raw,
                      block,
                      expr_stmt,
        )))
        .map_with_span(|((is_sudo, is_bg), stmt): ((bool, bool), Stmt), span: std::ops::Range<usize>| {
            let final_stmt = if is_bg {
                match stmt {
                    Stmt::Block(body) => Stmt::Background(body),
                       _ => Stmt::Background(vec![stmt]),
                }
            } else {
                stmt
            };
            ProgramNode {
                is_sudo,
                content: final_stmt,
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
    if let Stmt::Raw(cmd) = stmt {
        let dangerous_patterns = ["rm -rf", "mkfs", "dd if=", ":(){:|:&};:", "> /dev/sda"];
        for pat in dangerous_patterns {
            if cmd.contains(pat) {
                return true;
            }
        }
    }
    false
}

/// Parse a Hacker Lang source file and return an `AnalysisResult`.
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
            for (idx, node) in stmts.iter_mut().enumerate() {
                node.line_num = idx + 1;
                if is_dangerous(&node.content) {
                    result.is_potentially_unsafe = true;
                    result
                    .safety_warnings
                    .push(format!("Line {}: Potential destructive command", idx + 1));
                }

                match &node.content {
                    Stmt::Function { name, params, ret_ty, body } => {
                        let body_nodes = body
                        .iter()
                        .map(|s| ProgramNode {
                            content: s.clone(),
                             ..Default::default()
                        })
                        .collect();
                        result.functions.insert(
                            name.clone(),
                                                (params.clone(), ret_ty.clone(), body_nodes),
                        );
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
                                    PathBuf::from(path)
                                    .parent()
                                    .unwrap()
                                    .to_path_buf()
                                } else if prefix == "core" {
                                    PathBuf::from("/usr/lib/Hacker-Lang/libs/core")
                                } else {
                                    dirs::home_dir()
                                    .unwrap()
                                    .join(HACKER_DIR_SUFFIX)
                                    .join("libs")
                                };

                                let lib_path = libs_dir.join(format!("{}.hl", name));
                                if verbose {
                                    println!(
                                        "{} Resolving library: {:?}",
                                        "[*]".blue(),
                                             lib_path
                                    );
                                }

                                if lib_path.exists() {
                                    if let Ok(lib_res) = parse_file(
                                        lib_path.to_str().unwrap(),
                                                                    resolve_libs,
                                                                    verbose,
                                                                    seen_libs,
                                    ) {
                                        result.deps.extend(lib_res.deps);
                                        result.libs.extend(lib_res.libs);
                                        for (k, v) in lib_res.functions {
                                            result.functions.insert(k, v);
                                        }
                                        result.objects.extend(lib_res.objects);
                                        if lib_res.is_potentially_unsafe {
                                            result.is_potentially_unsafe = true;
                                            result.safety_warnings.push(format!(
                                                "Imported library {} contains unsafe code",
                                                name
                                            ));
                                        }
                                    }
                                } else if verbose {
                                    eprintln!(
                                        "{} Library source not found at {:?}",
                                        "[!]".yellow(),
                                              lib_path
                                    );
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
                              advice: format!("{:?}", e.reason()),
                });
            }
            Err(m_errors)
        }
    }
}
