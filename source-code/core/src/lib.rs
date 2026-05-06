pub mod deps;
pub mod diagnostics;
pub mod env;
pub mod executor;
pub mod libs;
pub mod quick;

pub use hl_parser::{
    ast, lexer, parser, gen, shebang,
    Node, StringPart, VarValue, VarType, ExportValue, CommandMode, ConditionKind,
    MatchArm, HackerOsTool,
    parse_source, parse_source_with_meta, ParseError, LexError,
    Gen, GenError, GenFeature, extract_gen, HL_MAX_GEN, HL_DEFAULT_GEN,
    ShebangInfo, PreprocessResult, preprocess,
    ParseMeta,
};

use anyhow::Result;
use env::Env;
use executor::exec_nodes;

pub fn run_source(source: &str, env: &mut Env) -> Result<executor::ExecResult> {
    let nodes = parse_source(source)?;
    exec_nodes(&nodes, env)
}

pub fn run_source_full(source: &str, env: &mut Env) -> Result<(executor::ExecResult, ParseMeta)> {
    let meta = parse_source_with_meta(source)?;
    env.set_var("HL_GEN", Value::String(meta.gen.number().to_string()));
    if let Some(ref sb) = meta.shebang {
        env.set_var("HL_SHEBANG", Value::String(sb.raw.clone()));
    }
    let result = exec_nodes(&meta.nodes, env)?;
    Ok((result, meta))
}

pub fn check_source(source: &str) -> std::result::Result<Vec<Node>, ParseError> {
    parse_source(source)
}

pub fn exec_nodes_pub(nodes: &[Node], env: &mut Env) -> Result<executor::ExecResult> {
    exec_nodes(nodes, env)
}

pub use env::Value;
pub use executor::ExecResult;
pub use diagnostics::{Diag, DiagLevel, DiagRenderer, DiagSummary, Span, lint_source};
pub use libs::{cmd_lib_list, cmd_lib_install, cmd_lib_remove, cmd_clean_cache};
pub use diagnostics::lint_gen;
