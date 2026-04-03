pub mod ast;
pub mod deps;
pub mod diagnostics;
pub mod env;
pub mod executor;
pub mod lexer;
pub mod parser;

use anyhow::Result;
use env::Env;
use executor::exec_nodes;
use parser::parse_source;

/// High-level: parse and execute a Hacker Lang source string
pub fn run_source(source: &str, env: &mut Env) -> Result<executor::ExecResult> {
    let nodes = parse_source(source)?;
    exec_nodes(&nodes, env)
}

/// Parse only — returns ParseError directly so callers can use parse_error_to_diag()
pub fn check_source(source: &str) -> std::result::Result<Vec<ast::Node>, parser::ParseError> {
    parse_source(source)
}

pub use env::Value;
pub use executor::ExecResult;
pub use parser::ParseError;
pub use lexer::LexError;
pub use diagnostics::{Diag, DiagLevel, DiagRenderer, DiagSummary, Span, lint_source};
