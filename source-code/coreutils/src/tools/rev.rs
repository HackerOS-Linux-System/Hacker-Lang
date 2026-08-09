use anyhow::{Result};



use crate::BuiltinResult;
use crate::read_files_or_stdin;

pub fn builtin_rev(args: &[String]) -> Result<BuiltinResult> {
    let files: Vec<String> = args.iter().filter(|a| !a.starts_with('-')).cloned().collect();
    let content = read_files_or_stdin(files)?;
    let mut out = String::new();
    for line in content.lines() {
        let rev: String = line.chars().rev().collect();
        println!("{}", rev);
        out.push_str(&rev); out.push('\n');
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
