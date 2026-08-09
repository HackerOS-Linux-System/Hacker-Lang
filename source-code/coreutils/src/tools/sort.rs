use anyhow::{Result};

use std::io::{BufRead};

use crate::BuiltinResult;
use crate::read_files_or_stdin;

pub fn builtin_sort(args: &[String]) -> Result<BuiltinResult> {
    let mut reverse  = false;
    let mut numeric  = false;
    let mut unique   = false;
    let mut files: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-r" => reverse = true,
            "-n" => numeric = true,
            "-u" => unique  = true,
            s    => files.push(s.to_string()),
        }
    }
    let content = read_files_or_stdin(files)?;
    let mut lines: Vec<&str> = content.lines().collect();

    if numeric {
        lines.sort_by(|a, b| {
            let na: f64 = a.trim().parse().unwrap_or(0.0);
            let nb: f64 = b.trim().parse().unwrap_or(0.0);
            na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        lines.sort();
    }
    if reverse { lines.reverse(); }
    if unique  { lines.dedup(); }

    let out = lines.join("\n");
    println!("{}", out);
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out) })
}
