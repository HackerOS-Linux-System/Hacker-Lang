use anyhow::{Result};
use std::fs;
use std::io::{self, BufRead, Read};

use crate::BuiltinResult;

pub fn parse_n_files(args: &[String], default: usize) -> (usize, Vec<String>) {
    let mut n = default;
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-n" => { i += 1; if i < args.len() { n = args[i].parse().unwrap_or(default); } }
            s if s.starts_with("-") && s[1..].parse::<usize>().is_ok() => {
                n = s[1..].parse().unwrap_or(default);
            }
            s => files.push(s.to_string()),
        }
        i += 1;
    }
    (n, files)
}

pub fn process_lines_limited(files: Vec<String>, n: usize, head: bool) -> Result<BuiltinResult> {
    let mut out = String::new();
    let read_source = |s: String| -> Vec<String> { s.lines().map(|l| l.to_string()).collect() };

    let process = |lines: Vec<String>, out: &mut String| {
        let selected: Vec<&String> = if head {
            lines.iter().take(n).collect()
        } else {
            let skip = if lines.len() > n { lines.len() - n } else { 0 };
            lines.iter().skip(skip).collect()
        };
        for line in selected {
            println!("{}", line);
            out.push_str(line);
            out.push('\n');
        }
    };

    if files.is_empty() {
        let mut buf = String::new();
        io::stdin().lock().read_to_string(&mut buf)?;
        process(read_source(buf), &mut out);
    } else {
        for file in files {
            let content = fs::read_to_string(&file)?;
            process(read_source(content), &mut out);
        }
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
