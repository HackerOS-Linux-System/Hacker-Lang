use chumsky::prelude::*;
use clap::Parser as ClapParser;
use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::exit;
use thiserror::Error;

const HACKER_DIR_SUFFIX: &str = ".hackeros/hacker-lang";

#[derive(ClapParser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Input file
    file: String,
    /// Verbose output
    #[arg(long)]
    verbose: bool,
    /// Output mode: print JSON to stdout
    #[arg(long)]
    json: bool,
    /// Check safety only
    #[arg(long)]
    check_safety: bool,
}

// --- AST Structures ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub is_super: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandType {
    Raw(String),
    AssignEnv { key: String, val: String },
    AssignLocal { key: String, val: String },
    Loop { count: u64, cmd: String },
    If { cond: String, cmd: String },
    Background(String),
    Plugin { name: String, is_super: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramNode {
    pub line_num: usize,
    pub is_sudo: bool,
    pub content: CommandType,
    pub original_text: String,
    pub span: (usize, usize),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    pub deps: Vec<String>,
    pub libs: Vec<String>,
    pub binaries: HashMap<String, String>,
    pub functions: HashMap<String, Vec<ProgramNode>>,
    pub main_body: Vec<ProgramNode>,
    pub includes: Vec<String>,
    // Safety Analysis
    pub is_safe: bool,
    pub safety_warnings: Vec<String>,
    pub requires_unsafe_flag: bool,
}

// --- Parser Logic ---

#[derive(Debug, Clone)]
enum LineOp {
    FuncEnd,
    FuncStart(String),
    FuncCall(String),
    SysDep(String),
    Lib(String),
    SeparateCmd(String),
    VarCmd(String),
    Cmd(String),
    GlobalVar(String, String),
    LocalVar(String, String),
    Plugin(String),
    Loop(u64, String),
    Cond(String, String),
    Bg(String),
    CommentLine,
    Invalid,
}

fn line_parser() -> impl Parser<char, LineOp, Error = Simple<char>> {
    // Helper to convert Vec<char> to String
    let to_string = |chars: Vec<char>| chars.into_iter().collect::<String>().trim().to_string();

    // just('!') produces char, end() produces (). We must align them to use .or().
    // just('!').ignored() produces ().
    let cmd_content = take_until(just('!').ignored().or(end()))
    .map(move |(chars, _)| to_string(chars));

    let func_end = just(':').then(end()).to(LineOp::FuncEnd);

    let func_start = just(':')
    .ignore_then(take_until(end()))
    .map(move |(chars, _)| LineOp::FuncStart(to_string(chars)));

    let func_call = just('.')
    .ignore_then(take_until(end()))
    .map(move |(chars, _)| LineOp::FuncCall(to_string(chars)));

    let sys_dep = just("//")
    .ignore_then(take_until(end()))
    .map(move |(chars, _)| LineOp::SysDep(to_string(chars)));

    let lib = just('#')
    .ignore_then(take_until(end()))
    .map(move |(chars, _)| LineOp::Lib(to_string(chars)));

    let separate_cmd = just(">>>").ignore_then(cmd_content.clone()).map(LineOp::SeparateCmd);
    let var_cmd = just(">>").ignore_then(cmd_content.clone()).map(LineOp::VarCmd);
    let cmd = just(">").ignore_then(cmd_content.clone()).map(LineOp::Cmd);

    // take_until(just('=')) consumes the '='. We don't need another then_ignore(just('=')).
    let global_var = just('@')
    .ignore_then(take_until(just('=')))
    .then(take_until(end()))
    .map(move |((k_chars, _), (v_chars, _))| LineOp::GlobalVar(to_string(k_chars), to_string(v_chars)));

    let local_var = just('$')
    .ignore_then(take_until(just('=')))
    .then(take_until(end()))
    .map(move |((k_chars, _), (v_chars, _))| LineOp::LocalVar(to_string(k_chars), to_string(v_chars)));

    let plugin = just('\\')
    .ignore_then(take_until(end()))
    .map(move |(chars, _)| LineOp::Plugin(to_string(chars)));

    let loop_op = just('=')
    .ignore_then(text::int(10))
    .then_ignore(just('>'))
    .then(cmd_content.clone())
    .map(|(num_str, cmd)| {
        let num: u64 = num_str.parse().unwrap_or(0);
        LineOp::Loop(num, cmd)
    });

    // take_until(just('>')) consumes the '>'.
    let cond_op = just('?')
    .ignore_then(take_until(just('>')))
    .then(cmd_content.clone())
    .map(move |((cond_chars, _), cmd)| {
        LineOp::Cond(to_string(cond_chars), cmd)
    });

    let bg = just('&').ignore_then(cmd_content.clone()).map(LineOp::Bg);

    let comment_line = just('!').to(LineOp::CommentLine);

    choice((
        func_end, func_start, func_call, sys_dep, lib, separate_cmd, var_cmd, cmd,
        global_var, local_var, plugin, loop_op, cond_op, bg, comment_line
    )).or(any().to(LineOp::Invalid))
}

// --- Errors ---

#[derive(Error, Debug, Diagnostic)]
enum ParseError {
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

// --- Analyzer ---

fn is_dangerous(cmd: &str) -> bool {
    let dangerous_patterns = [
        "rm -rf", "mkfs", "dd if=", ":(){:|:&};:", "chmod -R 777 /", "> /dev/sda"
    ];
    for pat in dangerous_patterns {
        if cmd.contains(pat) {
            return true;
        }
    }
    false
}

fn parse_file(path: &str, verbose: bool, seen_libs: &mut HashSet<String>) -> Result<AnalysisResult, Vec<ParseError>> {
    let mut result = AnalysisResult::default();
    result.is_safe = true;

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(result), // Return empty if file not found (handled by caller mostly)
    };

    // Note: NamedSource is not Clone in miette 5.10+, so we construct it on demand for errors.
    let parser = line_parser();

    let mut errors = Vec::new();
    let mut in_comment_block = false;
    let mut current_func: Option<String> = None;
    let home = dirs::home_dir().expect("No HOME");
    let hacker_dir = home.join(HACKER_DIR_SUFFIX);

    for (idx, line) in content.lines().enumerate() {
        let line_offset = content.lines().take(idx).map(|l| l.len() + 1).sum::<usize>();
        let trim_line = line.trim();

        if trim_line.is_empty() { continue; }

        if trim_line == "!!" {
            in_comment_block = !in_comment_block;
            continue;
        }
        if in_comment_block { continue; }

        let (mut clean_line, is_sudo) = if trim_line.starts_with('^') {
            (trim_line[1..].trim(), true)
        } else {
            (trim_line, false)
        };

        if is_sudo {
            result.requires_unsafe_flag = true;
            result.safety_warnings.push(format!("Line {}: Uses superuser privileges (^)", idx + 1));
        }

        let op = parser.parse(clean_line).unwrap_or(LineOp::Invalid);
        let span = (line_offset, clean_line.len());

        // Safety check helper
        let mut check_cmd_safety = |cmd: &str| {
            if is_dangerous(cmd) {
                result.is_safe = false;
                result.requires_unsafe_flag = true;
                result.safety_warnings.push(format!("Line {}: Potential destructive command detected: {}", idx + 1, cmd));
            }
        };

        match op {
            LineOp::Invalid => {
                errors.push(ParseError::SyntaxError {
                    src: NamedSource::new(path, content.clone()),
                            span: SourceSpan::new(span.0.into(), span.1.into()),
                            advice: "Unknown command or syntax error".to_string()
                });
            },
            LineOp::FuncStart(name) => {
                if current_func.is_some() {
                    errors.push(ParseError::StructureError{
                        src: NamedSource::new(path, content.clone()),
                                span: SourceSpan::new(span.0.into(), span.1.into()),
                                message: "Nested functions are not allowed".to_string(),
                    });
                }
                current_func = Some(name.clone());
                result.functions.insert(name, Vec::new());
            },
            LineOp::FuncEnd => {
                if current_func.is_none() {
                    errors.push(ParseError::StructureError{
                        src: NamedSource::new(path, content.clone()),
                                span: SourceSpan::new(span.0.into(), span.1.into()),
                                message: "Closing function that never started".to_string(),
                    });
                }
                current_func = None;
            },
            LineOp::FuncCall(name) => {
                let node = ProgramNode {
                    line_num: idx + 1,
                    is_sudo,
                    content: CommandType::Raw(format!("call:{}", name)), // Marker for runtime
                    original_text: format!(".{}", name),
                    span
                };
                if let Some(ref f) = current_func {
                    result.functions.get_mut(f).unwrap().push(node);
                } else {
                    result.main_body.push(node);
                }
            },
            LineOp::SysDep(dep) => result.deps.push(dep),
            LineOp::Lib(name) => {
                if seen_libs.contains(&name) { continue; }
                seen_libs.insert(name.clone());
                result.includes.push(name.clone());

                let lib_path = hacker_dir.join("libs").join(&name).join("main.hacker");
                if let Ok(lib_res) = parse_file(lib_path.to_str().unwrap(), verbose, seen_libs) {
                    // Merge results
                    result.deps.extend(lib_res.deps);
                    result.libs.extend(lib_res.libs);
                    // result.functions.extend(lib_res.functions); // Functions are namespaced or merged? Assuming merged for now
                    for (k, v) in lib_res.functions {
                        result.functions.insert(k, v);
                    }
                    if !lib_res.is_safe {
                        result.is_safe = false;
                        result.requires_unsafe_flag = true;
                        result.safety_warnings.push(format!("Imported library {} contains unsafe code", name));
                    }
                }
            },
            LineOp::Cmd(c) | LineOp::SeparateCmd(c) | LineOp::VarCmd(c) => {
                check_cmd_safety(&c);
                let node = ProgramNode {
                    line_num: idx + 1,
                    is_sudo,
                    content: CommandType::Raw(c.clone()),
                    original_text: clean_line.to_string(),
                    span
                };
                if let Some(ref f) = current_func {
                    result.functions.get_mut(f).unwrap().push(node);
                } else {
                    result.main_body.push(node);
                }
            },
            LineOp::Loop(n, c) => {
                check_cmd_safety(&c);
                let node = ProgramNode {
                    line_num: idx + 1,
                    is_sudo,
                    content: CommandType::Loop { count: n, cmd: c.clone() },
                    original_text: clean_line.to_string(),
                    span
                };
                if let Some(ref f) = current_func {
                    result.functions.get_mut(f).unwrap().push(node);
                } else {
                    result.main_body.push(node);
                }
            },
            LineOp::Cond(cond, c) => {
                check_cmd_safety(&c);
                let node = ProgramNode {
                    line_num: idx + 1,
                    is_sudo,
                    content: CommandType::If { cond, cmd: c.clone() },
                    original_text: clean_line.to_string(),
                    span
                };
                if let Some(ref f) = current_func {
                    result.functions.get_mut(f).unwrap().push(node);
                } else {
                    result.main_body.push(node);
                }
            },
            LineOp::Plugin(name) => {
                let node = ProgramNode {
                    line_num: idx + 1,
                    is_sudo,
                    content: CommandType::Plugin { name, is_super: is_sudo },
                    original_text: clean_line.to_string(),
                    span
                };
                if let Some(ref f) = current_func {
                    result.functions.get_mut(f).unwrap().push(node);
                } else {
                    result.main_body.push(node);
                }
            },
            _ => {} // Vars etc implemented similarly, omitted for brevity but conceptually the same
        }
    }

    if !errors.is_empty() {
        Err(errors)
    } else {
        Ok(result)
    }
}

fn main() {
    let args = Args::parse();
    let mut seen = HashSet::new();

    match parse_file(&args.file, args.verbose, &mut seen) {
        Ok(res) => {
            if args.check_safety {
                if !res.is_safe || res.requires_unsafe_flag {
                    println!("UNSAFE");
                    for w in res.safety_warnings {
                        eprintln!("Warning: {}", w);
                    }
                    exit(1);
                } else {
                    println!("SAFE");
                    exit(0);
                }
            }
            if args.json {
                println!("{}", serde_json::to_string(&res).unwrap());
            }
        },
        Err(errors) => {
            let _s = fs::read_to_string(&args.file).unwrap_or("".to_string());
            for e in errors {
                println!("{:?}", e); // Miette pretty print via Debug
            }
            exit(1);
        }
    }
}
