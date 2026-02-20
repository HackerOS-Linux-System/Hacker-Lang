use chumsky::prelude::*;
use clap::Parser as ClapParser;
use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::exit;
use thiserror::Error;
use colored::*;

const HACKER_DIR_SUFFIX: &str = ".hackeros/hacker-lang";

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
    Object { name: String, fields: Vec<(bool, String, String, Option<Expr>)>, methods: HashMap<String, (Vec<(String, String)>, Option<String>, Vec<Stmt>)> },
    Try { body: Vec<Stmt>, catches: Vec<(String, String, Vec<Stmt>)>, finally: Option<Vec<Stmt>> },
    Match { expr: Expr, arms: Vec<(Expr, Vec<Stmt>)> },
    For { var: String, iter: Expr, body: Vec<Stmt> },
    ForIndexed { idx: String, var: String, iter: Expr, body: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    Break,
    Continue,
    Import { prefix: String, name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgramNode {
    pub line_num: usize,
    pub is_sudo: bool,
    pub content: Stmt,
    pub original_text: String,
    pub span: (usize, usize),
}

impl Default for Stmt {
    fn default() -> Self {
        Stmt::Raw("".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    pub deps: Vec<String>,
    pub libs: Vec<String>,
    pub functions: HashMap<String, (Vec<(String, String)>, Option<String>, Vec<ProgramNode>)>,
    pub objects: HashMap<String, ProgramNode>,
    pub main_body: Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings: Vec<String>,
}

// --- Parser Logic ---
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
    }
}

fn parser() -> impl Parser<char, Vec<Stmt>, Error = Simple<char>> {
    recursive(|stmt| {
        let ident = text::ident().padded();
        let ty = ident.clone().labelled("type");

        let literal = choice((
            text::int(10).then_ignore(just('.')).then(text::int(10))
            .map(|(i, f): (String, String)| Value::F64(format!("{}.{}", i, f).parse().unwrap())),
                              text::int(10).map(|s: String| Value::I32(s.parse().unwrap())),
                              just("true").to(Value::Bool(true)),
                              just("false").to(Value::Bool(false)),
                              just('"').ignore_then(take_until(just('"'))).map(|(chars, _)| Value::Str(chars.into_iter().collect())),
                              just("none").to(Value::None),
        )).map(Expr::Lit);

        let expr: Recursive<char, Expr, Simple<char>> = recursive(|expr| {
            let atom = choice((
                literal,
                ident.map(Expr::Var),
                               expr.clone().delimited_by(just('('), just(')')),
            ));

            let pipe = atom.clone()
            .then(just("|>").padded().ignore_then(expr.clone()).repeated())
            .foldl(|left, right| Expr::Pipe { left: Box::new(left), right: Box::new(right) });

            let binop = pipe.clone()
            .then(choice((just("+"), just("-"), just("*"), just("/"))).padded().then(pipe).repeated())
            .foldl(|left, (op, right)| Expr::BinOp { op: op.to_string(), left: Box::new(left), right: Box::new(right) });

            let call = ident
            .then(expr.clone().separated_by(just(',')).delimited_by(just('('), just(')')))
            .map(|(name, args)| Expr::Call { name, args });

            choice((call, binop))
        });

        let assign_global = just('@').ignore_then(ident.clone())
        .then(just(':').ignore_then(ty.clone()).or_not())
        .then_ignore(just('='))
        .then(expr.clone())
        .map(|((key, ty), val)| Stmt::AssignGlobal { key, ty, val });

        let assign_local = just('$').ignore_then(ident.clone())
        .then(just(':').ignore_then(ty.clone()).or_not())
        .then_ignore(just('='))
        .then(expr.clone())
        .map(|((key, ty), val)| Stmt::AssignLocal { key, ty, val });

        let param = ident.clone().then_ignore(just(':')).then(ty.clone());
        let params = param.separated_by(just(',')).allow_trailing().delimited_by(just('('), just(')'));

        let func_def = just(':').ignore_then(ident.clone())
        .then(params.clone())
        .then(just("->").ignore_then(ty.clone()).or_not())
        .then(stmt.clone().repeated().delimited_by(just('['), just(']')))
        .map(|(((name, params), ret_ty), body)| Stmt::Function { name, params, ret_ty, body });

        let ret = just("<-").ignore_then(expr.clone()).map(|expr| Stmt::Return { expr });

        let field = just("mut").or_not().map(|m| m.is_some())
        .then(ident.clone())
        .then_ignore(just(':'))
        .then(ty.clone())
        .then(just('=').ignore_then(expr.clone()).or_not())
        .map(|(((mut_, name), ty), init)| (mut_, name, ty, init));

        let method = just(':').ignore_then(ident.clone())
        .then(params)
        .then(just("->").ignore_then(ty).or_not())
        .then(stmt.clone().repeated().delimited_by(just('['), just(']')))
        .map(|(((name, params), ret_ty), body)| (name, (params, ret_ty, body)));

        let obj_def = just("obj").ignore_then(ident.clone())
        .then(field.separated_by(just(',')).then(method.repeated()).delimited_by(just('['), just(']')))
        .map(|(name, (fields, methods))| Stmt::Object { name, fields, methods: methods.into_iter().collect() });

        let catch = just("catch")
        .ignore_then(just('(').ignore_then(ident.clone()).then_ignore(just(':')).then(ident.clone()).then_ignore(just(')')))
        .then(stmt.clone().repeated().delimited_by(just('['), just(']')));

        let finally = just("finally")
        .ignore_then(stmt.clone().repeated().delimited_by(just('['), just(']')))
        .or_not();

        let try_stmt = just("try")
        .ignore_then(stmt.clone().repeated().delimited_by(just('['), just(']')))
        .then(catch.repeated())
        .then(finally)
        .map(|((body, catches), finally)| Stmt::Try {
            body,
            catches: catches.into_iter().map(|((var, ty), body)| (var, ty, body)).collect(),
             finally
        });

        let arm = expr.clone().then_ignore(just('>')).then(stmt.clone().repeated()).padded();
        let match_stmt = just("match").ignore_then(expr.clone())
        .then(arm.repeated().delimited_by(just('['), just(']')))
        .map(|(expr, arms)| Stmt::Match { expr, arms });

        let for_stmt = just("loop").ignore_then(ident.clone())
        .then_ignore(just("in"))
        .then(expr.clone())
        .then(stmt.clone().repeated().delimited_by(just('['), just(']')))
        .map(|((var, iter), body)| Stmt::For { var, iter, body });

        let for_indexed = just("loop").ignore_then(ident.clone())
        .then_ignore(just(','))
        .then(ident.clone())
        .then_ignore(just("in"))
        .then(just("enumerate(").ignore_then(expr.clone()).then_ignore(just(')')))
        .then(stmt.clone().repeated().delimited_by(just('['), just(']')))
        .map(|(((idx, var), iter), body)| Stmt::ForIndexed { idx, var, iter, body });

        let while_stmt = just("loop").ignore_then(expr.clone())
        .then(stmt.clone().repeated().delimited_by(just('['), just(']')))
        .map(|(cond, body)| Stmt::While { cond, body });

        let break_stmt = just("break").to(Stmt::Break);
        let continue_stmt = just("continue").to(Stmt::Continue);

        let import_stmt = just('#').ignore_then(ident.clone())
        .then_ignore(just(':'))
        .then(ident.clone())
        .map(|(prefix, name)| Stmt::Import { prefix, name });

        let plugin = just('\\').ignore_then(ident).map(|name| Stmt::Plugin { name, is_super: false });

        let raw = take_until(end().or(just('!').ignored()))
        .map(|(chars, _)| Stmt::Raw(chars.into_iter().collect::<String>().trim().to_string()));

        let decorated = choice((just('^').to(true), just('&').to(false))).or_not()
        .then(choice((
            assign_global, assign_local, func_def, ret, obj_def, try_stmt, match_stmt,
            for_indexed, for_stmt, while_stmt, break_stmt, continue_stmt, import_stmt, plugin, raw
        )))
        .map(|(dec, stmt)| {
            if let Some(false) = dec {
                Stmt::Background(vec![stmt])
            } else {
                stmt
            }
        });

        decorated.padded().recover_with(skip_then_retry_until([]))
    }).repeated().then_ignore(end())
}

fn is_dangerous(stmt: &Stmt) -> bool {
    if let Stmt::Raw(cmd) = stmt {
        let dangerous_patterns = ["rm -rf", "mkfs", "dd if=", ":(){:|:&};:", "> /dev/sda"];
        for pat in dangerous_patterns {
            if cmd.contains(pat) { return true; }
        }
    }
    false
}

pub fn parse_file(path: &str, resolve_libs: bool, verbose: bool, seen_libs: &mut HashSet<String>) -> Result<AnalysisResult, Vec<ParseError>> {
    let mut result = AnalysisResult::default();
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(result),
    };

    let lines: Vec<_> = content.lines().filter(|l| !l.trim().starts_with('!') && !l.trim().is_empty()).collect();
    let clean_content = lines.join("\n");
    let p = parser();

    match p.parse(clean_content) {
        Ok(stmts) => {
            for (idx, stmt) in stmts.iter().enumerate() {
                if is_dangerous(stmt) {
                    result.is_potentially_unsafe = true;
                    result.safety_warnings.push(format!("Line {}: Potential destructive command", idx + 1));
                }
                let node = ProgramNode {
                    line_num: idx + 1,
                    is_sudo: false,
                    content: stmt.clone(),
                    original_text: "".to_string(),
                    span: (0, 0),
                };
                match stmt {
                    Stmt::Function { name, params, ret_ty, body } => {
                        let body_nodes = body.iter().map(|s| ProgramNode { content: s.clone(), ..Default::default() }).collect();
                        result.functions.insert(name.clone(), (params.clone(), ret_ty.clone(), body_nodes));
                    },
                    Stmt::Object { name, .. } => {
                        result.objects.insert(name.clone(), node.clone());
                    },
                    Stmt::Import { prefix, name } => {
                        let lib_key = format!("{}:{}", prefix, name);
                        if seen_libs.contains(&lib_key) { continue; }
                        seen_libs.insert(lib_key.clone());
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
                            if verbose { println!("{} Resolving library: {:?}", "[*]".blue(), lib_path); }
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
                            } else if verbose {
                                eprintln!("{} Library source not found at {:?}", "[!]".yellow(), lib_path);
                            }
                        }
                    },
                    _ => {
                        result.main_body.push(node);
                    }
                }
            }
            Ok(result)
        },
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

fn main() {
    let args = Args::parse();
    let mut seen = HashSet::new();
    match parse_file(&args.file, args.resolve_libs, args.verbose, &mut seen) {
        Ok(res) => {
            if args.verbose && res.is_potentially_unsafe {
                eprintln!("{} Scripts contains potentially unsafe commands.", "[!]".yellow());
            }
            if args.json {
                println!("{}", serde_json::to_string(&res).unwrap());
            }
        },
        Err(errors) => {
            for e in errors {
                eprintln!("{:?}", miette::Report::new(e));
            }
            exit(1);
        }
    }
}
