pub mod ast;
pub mod deps;
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

/// Parse only (for syntax checking, tooling, etc.)
pub fn check_source(source: &str) -> Result<Vec<ast::Node>> {
    Ok(parse_source(source)?)
}

pub use env::Value;
pub use executor::ExecResult;
pub use parser::ParseError;
pub use lexer::LexError;
