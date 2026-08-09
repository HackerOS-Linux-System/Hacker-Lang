use anyhow::{Result};
use std::fs;
use std::io::{self, BufRead, Read};

use crate::BuiltinResult;

pub fn builtin_wc(args: &[String]) -> Result<BuiltinResult> {
    let mut count_lines = false;
    let mut count_words = false;
    let mut count_chars = false;
    let mut files: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-l" => count_lines = true,
            "-w" => count_words = true,
            "-c" | "-m" => count_chars = true,
            s => files.push(s.to_string()),
        }
    }
    // Default: all
    if !count_lines && !count_words && !count_chars {
        count_lines = true; count_words = true; count_chars = true;
    }

    let process_content = |content: &str| -> (usize, usize, usize) {
        let lines = content.lines().count();
        let words = content.split_whitespace().count();
        let chars = content.len();
        (lines, words, chars)
    };

    let mut out = String::new();
    let content = if files.is_empty() {
        let mut buf = String::new();
        io::stdin().lock().read_to_string(&mut buf)?;
        buf
    } else {
        fs::read_to_string(&files[0])?
    };

    let (l, w, c) = process_content(&content);
    let mut parts = Vec::new();
    if count_lines { parts.push(l.to_string()); }
    if count_words { parts.push(w.to_string()); }
    if count_chars { parts.push(c.to_string()); }
    let line = parts.join("\t");
    println!("{}", line);
    out = line.clone();

    Ok(BuiltinResult { exit_code: 0, stdout: Some(out) })
}
