use anyhow::{Result};



use crate::BuiltinResult;
use crate::common::*;

pub fn builtin_tail(args: &[String]) -> Result<BuiltinResult> {
    let (n, files) = parse_n_files(args, 10);
    process_lines_limited(files, n, false)
}
