pub mod ast;
pub mod deps;
pub mod diagnostics;
pub mod env;
pub mod executor;
pub mod lexer;
pub mod libs;
pub mod parser;
pub mod quick;

use anyhow::Result;
use env::Env;
use executor::exec_nodes;
use parser::parse_source;

pub fn run_source(source: &str, env: &mut Env) -> Result<executor::ExecResult> {
    let nodes = parse_source(source)?;
    exec_nodes(&nodes, env)
}

pub fn check_source(source: &str) -> std::result::Result<Vec<ast::Node>, parser::ParseError> {
    parse_source(source)
}

pub use env::Value;
pub use executor::ExecResult;
pub use parser::ParseError;
pub use lexer::LexError;
pub use diagnostics::{Diag, DiagLevel, DiagRenderer, DiagSummary, Span, lint_source};
pub use libs::{cmd_lib_list, cmd_lib_install, cmd_lib_remove, cmd_clean_cache};
