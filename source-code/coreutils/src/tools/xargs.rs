use anyhow::{bail, Result};

use std::io::{self, Read};

use crate::BuiltinResult;

pub fn builtin_xargs(args: &[String]) -> Result<BuiltinResult> {
    if args.is_empty() { bail!("[/> xargs] podaj komendę"); }
    let mut stdin_content = String::new();
    io::stdin().lock().read_to_string(&mut stdin_content)?;
    let xargs_items: Vec<&str> = stdin_content.split_whitespace().collect();
    let mut full_args = args.to_vec();
    full_args.extend(xargs_items.iter().map(|s| s.to_string()));
    let prog = full_args.remove(0);
    let out_bytes = std::process::Command::new(&prog)
        .args(&full_args)
        .stdin(std::process::Stdio::null())
        .output()?;
    let s = String::from_utf8_lossy(&out_bytes.stdout).to_string();
    print!("{}", s);
    Ok(BuiltinResult { exit_code: out_bytes.status.code().unwrap_or(1),
                       stdout: Some(s.trim_end_matches('\n').to_string()) })
}
