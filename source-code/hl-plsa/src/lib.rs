#![allow(dead_code, unused_assignments, unused_variables)]
use miette::{Diagnostic, NamedSource, SourceSpan};
use pest::iterators::Pair;
use pest::Parser;
use pest::error::{ErrorVariant, LineColLocation};
use pest_derive::Parser;
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
    Spawn { body: Vec<ProgramNode> },
    Await(Box<Expr>),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    Raw { mode: String, cmd: String },
    AssignGlobal { key: String, ty: Option<String>, val: Expr },
    AssignLocal { key: String, ty: Option<String>, val: Expr },
    Repeat { count: u64, body: Vec<ProgramNode> },
    If { cond: Expr, body: Vec<ProgramNode>, else_ifs: Vec<(Expr, Vec<ProgramNode>)>, else_body: Option<Vec<ProgramNode>> },
    Plugin { name: String, is_super: bool },
    Function { name: String, params: Vec<(String, String)>, ret_ty: Option<String>, body: Vec<ProgramNode>, is_quick: bool },
    Return { expr: Expr },
    Object {
        name: String,
        fields: Vec<(bool, String, String, Option<Expr>)>,
        methods: HashMap<String, (Vec<(String, String)>, Option<String>, Vec<ProgramNode>)>,
    },
    Try { body: Vec<ProgramNode>, catches: Vec<(String, String, Vec<ProgramNode>)>, else_body: Option<Vec<ProgramNode>>, finally: Option<Vec<ProgramNode>> },
    Match { expr: Expr, arms: Vec<(Expr, Vec<ProgramNode>)> },
    For { var: String, iter: Expr, body: Vec<ProgramNode> },
    ForIndexed { idx: String, var: String, iter: Expr, body: Vec<ProgramNode> },
    While { cond: Expr, body: Vec<ProgramNode> },
    Break,
    Continue,
    Import { prefix: String, name: String, version: Option<String> },
    Expr(Expr),
    Block(Vec<ProgramNode>),
    Log(String),
    Lock(String),
    Unlock(String),
    WithLock { var: String, body: Vec<ProgramNode> },
    Module { name: String, body: Vec<ProgramNode> },
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
#[derive(Parser)]
#[grammar = "grammar.pest"]
struct HLParser;
fn build_value(pair: Pair<'_, Rule>) -> Value {
    match pair.as_rule() {
        Rule::int => Value::I32(pair.as_str().parse().unwrap()),
        Rule::float => Value::F64(pair.as_str().parse().unwrap()),
        Rule::bool_true => Value::Bool(true),
        Rule::bool_false => Value::Bool(false),
        Rule::str => {
            let s = pair.as_str();
            Value::Str(s[1..s.len() - 1].to_string())
        }
        Rule::none => Value::None,
        _ => unreachable!(),
    }
}
fn build_expr(pair: Pair<'_, Rule>) -> Expr {
    match pair.as_rule() {
        Rule::expr | Rule::logical_or | Rule::logical_and | Rule::comparison | Rule::sum | Rule::product => {
            let mut inners = pair.into_inner();
            let mut left = build_expr(inners.next().unwrap());
            while let Some(op_pair) = inners.next() {
                let op = op_pair.as_str().to_string();
                let right = build_expr(inners.next().unwrap());
                left = Expr::BinOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
            left
        }
        Rule::pipe => {
            let mut inners = pair.into_inner();
            let mut left = build_expr(inners.next().unwrap());
            while let Some(_) = inners.next() {
                let right = build_expr(inners.next().unwrap());
                left = Expr::Pipe {
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
            left
        }
        Rule::atom => build_expr(pair.into_inner().next().unwrap()),
        Rule::lit => Expr::Lit(build_value(pair.into_inner().next().unwrap())),
        Rule::var => Expr::Var(pair.as_str().to_string()),
        Rule::call => {
            let mut inners = pair.into_inner();
            let name = inners.next().unwrap().as_str().to_string();
            let mut args = vec![];
            if let Some(args_pair) = inners.next() {
                let mut arg_inners = args_pair.into_inner();
                args.push(build_expr(arg_inners.next().unwrap()));
                while arg_inners.next().is_some() {
                    args.push(build_expr(arg_inners.next().unwrap()));
                }
            }
            Expr::Call { name, args }
        }
        Rule::paren_expr => build_expr(pair.into_inner().next().unwrap()),
        Rule::list => {
            let mut list = vec![];
            for item in pair.into_inner() {
                list.push(build_expr(item));
            }
            Expr::List(list)
        }
        Rule::map => {
            let mut map = HashMap::new();
            for entry in pair.into_inner() {
                let mut e_inners = entry.into_inner();
                let key = e_inners.next().unwrap().as_str().to_string();
                let val = build_expr(e_inners.next().unwrap());
                map.insert(key, val);
            }
            Expr::Map(map)
        }
        Rule::wildcard => Expr::Wildcard,
        _ => unreachable!(),
    }
}
fn build_stmt(pair: Pair<'_, Rule>) -> Stmt {
    match pair.as_rule() {
        Rule::assign_global => {
            let mut inners = pair.into_inner();
            let key = inners.next().unwrap().as_str().to_string();
            let ty = inners.next().and_then(|t| t.into_inner().next()).map(|t| t.as_str().to_string());
            let val = build_expr(inners.next().unwrap());
            Stmt::AssignGlobal { key, ty, val }
        }
        Rule::assign_local => {
            let mut inners = pair.into_inner();
            let key = inners.next().unwrap().as_str().to_string();
            let ty = inners.next().and_then(|t| t.into_inner().next()).map(|t| t.as_str().to_string());
            let val = build_expr(inners.next().unwrap());
            Stmt::AssignLocal { key, ty, val }
        }
        Rule::func_def => {
            let mut inners = pair.into_inner();
            let prefix = inners.next().unwrap().as_str();
            let is_quick = prefix == "::";
            let name = inners.next().unwrap().as_str().to_string();
            let mut params = vec![];
            let param_pair = inners.next().unwrap();
            for p in param_pair.into_inner() {
                let mut p_inners = p.into_inner();
                let p_name = p_inners.next().unwrap().as_str().to_string();
                let p_ty = p_inners.next().unwrap().as_str().to_string();
                params.push((p_name, p_ty));
            }
            let ret_ty = inners.next().and_then(|r| r.into_inner().next()).map(|r| r.as_str().to_string());
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            Stmt::Function { name, params, ret_ty, body, is_quick }
        }
        Rule::ret => {
            let mut inners = pair.into_inner();
            let expr = build_expr(inners.next().unwrap());
            Stmt::Return { expr }
        }
        Rule::obj_def => {
            let mut inners = pair.into_inner();
            let name = inners.next().unwrap().as_str().to_string();
            let mut fields = vec![];
            let mut methods = HashMap::new();
            let content_pair = inners.next().unwrap();
            let mut content_inners = content_pair.into_inner();
            let fields_pair = content_inners.next();
            if let Some(fp) = fields_pair {
                for f in fp.into_inner() {
                    let mut f_inners = f.into_inner();
                    let first = f_inners.next().unwrap();
                    let (mut_, f_name) = if first.as_str() == "mut" {
                        (true, f_inners.next().unwrap().as_str().to_string())
                    } else {
                        (false, first.as_str().to_string())
                    };
                    let f_ty = f_inners.next().unwrap().as_str().to_string();
                    let init = f_inners.next().and_then(|i| i.into_inner().next()).map(build_expr);
                    fields.push((mut_, f_name, f_ty, init));
                }
            }
            for m in content_inners {
                let mut m_inners = m.into_inner();
                let m_name = m_inners.next().unwrap().as_str().to_string();
                let m_params_pair = m_inners.next().unwrap();
                let mut m_params = vec![];
                for mp in m_params_pair.into_inner() {
                    let mut mp_inners = mp.into_inner();
                    let mp_name = mp_inners.next().unwrap().as_str().to_string();
                    let mp_ty = mp_inners.next().unwrap().as_str().to_string();
                    m_params.push((mp_name, mp_ty));
                }
                let m_ret_ty = m_inners.next().and_then(|mr| mr.into_inner().next()).map(|mr| mr.as_str().to_string());
                let m_body_pair = m_inners.next().unwrap();
                let m_body: Vec<ProgramNode> = m_body_pair.into_inner().map(build_program_node).collect();
                methods.insert(m_name, (m_params, m_ret_ty, m_body));
            }
            Stmt::Object { name, fields, methods }
        }
        Rule::try_stmt => {
            let mut inners = pair.into_inner();
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            let mut catches = vec![];
            let mut else_body = None;
            let mut finally = None;
            while let Some(next) = inners.next() {
                match next.as_rule() {
                    Rule::catch => {
                        let mut c_inners = next.into_inner();
                        let var = c_inners.next().unwrap().as_str().to_string();
                        let ty = c_inners.next().unwrap().as_str().to_string();
                        let c_body_pair = c_inners.next().unwrap();
                        let c_body: Vec<ProgramNode> = c_body_pair.into_inner().map(build_program_node).collect();
                        catches.push((var, ty, c_body));
                    }
                    Rule::else_body => {
                        let e_body_pair = next.into_inner().next().unwrap();
                        else_body = Some(e_body_pair.into_inner().map(build_program_node).collect());
                    }
                    Rule::finally => {
                        let f_body_pair = next.into_inner().next().unwrap();
                        finally = Some(f_body_pair.into_inner().map(build_program_node).collect());
                    }
                    _ => unreachable!(),
                }
            }
            Stmt::Try { body, catches, else_body, finally }
        }
        Rule::match_stmt => {
            let mut inners = pair.into_inner();
            let expr = build_expr(inners.next().unwrap());
            let arms_pair = inners.next().unwrap();
            let mut arms = vec![];
            for a in arms_pair.into_inner() {
                let mut a_inners = a.into_inner();
                let pattern = build_expr(a_inners.next().unwrap());
                let a_body_pair = a_inners.next().unwrap();
                let a_body: Vec<ProgramNode> = a_body_pair.into_inner().map(build_program_node).collect();
                arms.push((pattern, a_body));
            }
            Stmt::Match { expr, arms }
        }
        Rule::for_stmt => {
            let mut inners = pair.into_inner();
            let var = inners.next().unwrap().as_str().to_string();
            let iter = build_expr(inners.next().unwrap());
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            Stmt::For { var, iter, body }
        }
        Rule::for_indexed => {
            let mut inners = pair.into_inner();
            let idx = inners.next().unwrap().as_str().to_string();
            let var = inners.next().unwrap().as_str().to_string();
            let iter = build_expr(inners.next().unwrap());
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            Stmt::ForIndexed { idx, var, iter, body }
        }
        Rule::while_stmt => {
            let mut inners = pair.into_inner();
            let cond = build_expr(inners.next().unwrap());
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            Stmt::While { cond, body }
        }
        Rule::if_stmt => {
            let mut inners = pair.into_inner();
            let cond = build_expr(inners.next().unwrap());
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            let mut else_ifs = vec![];
            let mut else_body = None;
            while let Some(next) = inners.next() {
                match next.as_rule() {
                    Rule::elif => {
                        let mut el_inners = next.into_inner();
                        let el_cond = build_expr(el_inners.next().unwrap());
                        let el_body_pair = el_inners.next().unwrap();
                        let el_body: Vec<ProgramNode> = el_body_pair.into_inner().map(build_program_node).collect();
                        else_ifs.push((el_cond, el_body));
                    }
                    Rule::else_body => {
                        let e_body_pair = next.into_inner().next().unwrap();
                        else_body = Some(e_body_pair.into_inner().map(build_program_node).collect());
                    }
                    _ => unreachable!(),
                }
            }
            Stmt::If { cond, body, else_ifs, else_body }
        }
        Rule::repeat_stmt => {
            let mut inners = pair.into_inner();
            let count: u64 = inners.next().unwrap().as_str().parse().unwrap();
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            Stmt::Repeat { count, body }
        }
        Rule::break_stmt => Stmt::Break,
        Rule::continue_stmt => Stmt::Continue,
        Rule::import_stmt => {
            let mut inners = pair.into_inner();
            let prefix = inners.next().unwrap().as_str().to_string();
            let name = inners.next().unwrap().as_str().to_string();
            let version = inners.next().and_then(|v| v.into_inner().next()).map(|v| v.as_str().to_string());
            Stmt::Import { prefix, name, version }
        }
        Rule::plugin => {
            let name = pair.into_inner().next().unwrap().as_str().to_string();
            Stmt::Plugin { name, is_super: false }
        }
        Rule::raw => {
            let s = pair.as_str();
            let mode_len = if s.starts_with(">>>") { 3 } else if s.starts_with(">>") { 2 } else if s.starts_with(">") { 1 } else { 0 };
            let mode = s[0..mode_len].to_string();
            let cmd = s[mode_len..].trim().to_string();
            Stmt::Raw { mode, cmd }
        }
        Rule::block => {
            let body: Vec<ProgramNode> = pair.into_inner().map(build_program_node).collect();
            Stmt::Block(body)
        }
        Rule::expr_stmt => Stmt::Expr(build_expr(pair.into_inner().next().unwrap())),
        Rule::log_stmt => {
            let s = pair.into_inner().next().unwrap().as_str();
            Stmt::Log(s[1..s.len() - 1].to_string())
        }
        Rule::lock_stmt => Stmt::Lock(pair.into_inner().next().unwrap().as_str().to_string()),
        Rule::unlock_stmt => Stmt::Unlock(pair.into_inner().next().unwrap().as_str().to_string()),
        Rule::withlock_stmt => {
            let mut inners = pair.into_inner();
            let var = inners.next().unwrap().as_str().to_string();
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            Stmt::WithLock { var, body }
        }
        Rule::module_stmt => {
            let mut inners = pair.into_inner();
            let name = inners.next().unwrap().as_str().to_string();
            let body_pair = inners.next().unwrap();
            let body: Vec<ProgramNode> = body_pair.into_inner().map(build_program_node).collect();
            Stmt::Module { name, body }
        }
        Rule::await_stmt => Stmt::Expr(Expr::Await(Box::new(build_expr(pair.into_inner().next().unwrap())))),
        _ => unreachable!(),
    }
}
fn build_program_node(pair: Pair<'_, Rule>) -> ProgramNode {
    let span = pair.as_span();
    let start = span.start();
    let end = span.end();
    let mut is_sudo = false;
    let mut is_bg = false;
    let mut content = Stmt::default();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::sudo => is_sudo = true,
            Rule::bg => is_bg = true,
            Rule::inner_stmt => content = build_stmt(inner),
            _ => {}
        }
    }
    if is_bg {
        content = match content {
            Stmt::Block(body) => Stmt::Expr(Expr::Spawn { body }),
            s => Stmt::Expr(Expr::Spawn { body: vec![ProgramNode { content: s, span: (start, end), original_text: "".to_string(), ..Default::default() }] }),
        };
    }
    ProgramNode {
        line_num: 0,
        is_sudo,
        content,
        original_text: span.as_str().to_string(),
        span: (start, end),
    }
}
fn is_dangerous(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Raw { cmd, .. } => {
            let dangerous_patterns = ["rm -rf", "mkfs", "dd if=", ":(){:|:&};:", "> /dev/sda"];
            dangerous_patterns.iter().any(|pat| cmd.contains(pat))
        }
        Stmt::If { body, else_ifs, else_body, .. } => {
            body.iter().any(|n| is_dangerous(&n.content)) ||
            else_ifs.iter().any(|(_, b)| b.iter().any(|n| is_dangerous(&n.content))) ||
                else_body.as_ref().map_or(false, |b| b.iter().any(|n| is_dangerous(&n.content)))
        }
        Stmt::While { body, .. } => body.iter().any(|n| is_dangerous(&n.content)),
        Stmt::For { body, .. } => body.iter().any(|n| is_dangerous(&n.content)),
        Stmt::ForIndexed { body, .. } => body.iter().any(|n| is_dangerous(&n.content)),
        Stmt::Repeat { body, .. } => body.iter().any(|n| is_dangerous(&n.content)),
        Stmt::Try { body, catches, else_body, finally, .. } => {
            body.iter().any(|n| is_dangerous(&n.content)) ||
            catches.iter().any(|(_, _, b)| b.iter().any(|n| is_dangerous(&n.content))) ||
            else_body.as_ref().map_or(false, |b| b.iter().any(|n| is_dangerous(&n.content))) ||
                finally.as_ref().map_or(false, |f| f.iter().any(|n| is_dangerous(&n.content)))
        }
        Stmt::Match { arms, .. } => arms.iter().any(|(_, b)| b.iter().any(|n| is_dangerous(&n.content))),
        Stmt::Block(body) => body.iter().any(|n| is_dangerous(&n.content)),
        Stmt::WithLock { body, .. } => body.iter().any(|n| is_dangerous(&n.content)),
        Stmt::Module { body, .. } => body.iter().any(|n| is_dangerous(&n.content)),
        Stmt::Function { body, .. } => body.iter().any(|n| is_dangerous(&n.content)),
        Stmt::Object { methods, .. } => methods.values().any(|(_, _, b)| b.iter().any(|n| is_dangerous(&n.content))),
        _ => false,
    }
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
                errors.extend(validate_ast(body, true, *func_quick, path, content));
            }
            Stmt::If { body, else_ifs, else_body, .. } => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
                for (_, b) in else_ifs {
                    errors.extend(validate_ast(b, in_func, is_quick, path, content));
                }
                if let Some(b) = else_body {
                    errors.extend(validate_ast(b, in_func, is_quick, path, content));
                }
            }
            Stmt::While { body, .. } => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
            }
            Stmt::For { body, .. } => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
            }
            Stmt::ForIndexed { body, .. } => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
            }
            Stmt::Repeat { body, .. } => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
            }
            Stmt::Try { body, catches, else_body, finally, .. } => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
                for (_, _, b) in catches {
                    errors.extend(validate_ast(b, in_func, is_quick, path, content));
                }
                if let Some(b) = else_body {
                    errors.extend(validate_ast(b, in_func, is_quick, path, content));
                }
                if let Some(f) = finally {
                    errors.extend(validate_ast(f, in_func, is_quick, path, content));
                }
            }
            Stmt::Match { arms, .. } => {
                for (_, b) in arms {
                    errors.extend(validate_ast(b, in_func, is_quick, path, content));
                }
            }
            Stmt::Block(body) => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
            }
            Stmt::WithLock { body, .. } => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
            }
            Stmt::Module { body, .. } => {
                errors.extend(validate_ast(body, in_func, is_quick, path, content));
            }
            _ => {}
        }
    }
    errors
}
fn nice_advice(error: &pest::error::Error<Rule>) -> String {
    match &error.variant {
        ErrorVariant::ParsingError { positives, negatives } => {
            let expected = if positives.is_empty() {
                "end of input".to_string()
            } else {
                positives.iter().map(|r| format!("{:?}", r)).collect::<Vec<_>>().join(" or ")
            };
            let unexpected = if negatives.is_empty() {
                "".to_string()
            } else {
                negatives.iter().map(|r| format!("{:?}", r)).collect::<Vec<_>>().join(" or ")
            };
            format!("unexpected {}; expected {}", unexpected, expected)
        }
        ErrorVariant::CustomError { message } => message.clone(),
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
    let pairs = HLParser::parse(Rule::file, &content).map_err(|e| {
        let span = match e.line_col {
            LineColLocation::Pos((line, col)) => {
                let mut offset = 0usize;
                for (i, l) in content.lines().enumerate() {
                    if i + 1 == line {
                        offset += col - 1;
                        break;
                    }
                    offset += l.len() + 1;
                }
                SourceSpan::new(offset.into(), 1.into())
            }
            LineColLocation::Span((line1, col1), (line2, col2)) => {
                let mut start = 0usize;
                for (i, l) in content.lines().enumerate() {
                    if i + 1 == line1 {
                        start += col1 - 1;
                        break;
                    }
                    start += l.len() + 1;
                }
                let mut end = 0usize;
                for (i, l) in content.lines().enumerate() {
                    if i + 1 == line2 {
                        end += col2 - 1;
                        break;
                    }
                    end += l.len() + 1;
                }
                SourceSpan::new(start.into(), (end - start).into())
            }
        };
        vec![ParseError::SyntaxError {
            src: NamedSource::new(path, content.clone()),
                                                              span,
                                                              advice: nice_advice(&e),
        }]
    })?;
    let mut stmts = vec![];
    for pair in pairs {
        if pair.as_rule() == Rule::stmt {
            stmts.push(build_program_node(pair));
        }
    }
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
                result.functions.insert(name.clone(), (params.clone(), ret_ty.clone(), body.clone(), *is_quick));
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
                    if let Some(v) = version {
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
                        } else if prefix == "virus" {
                            dirs::home_dir().unwrap().join(HACKER_DIR_SUFFIX).join(".virus")
                        } else if prefix == "vira" {
                            dirs::home_dir().unwrap().join(HACKER_DIR_SUFFIX).join(".vira")
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
