use anyhow::{Result};
use std::fs;
use std::io::{self, BufRead};

use crate::BuiltinResult;

pub fn builtin_grep(args: &[String]) -> Result<BuiltinResult> {
    let mut ignore_case = false;
    let mut invert      = false;
    let mut count_only  = false;
    let mut recursive   = false;
    let mut pattern_opt: Option<String> = None;
    let mut files: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--ignore-case"  => ignore_case = true,
            "-v" | "--invert-match" => invert      = true,
            "-c" | "--count"        => count_only  = true,
            "-r" | "-R"             => recursive   = true,
            "-e" => { i += 1; if i < args.len() { pattern_opt = Some(args[i].clone()); } }
            s if s.starts_with('-') => {} // ignore unknown flags
            s => {
                if pattern_opt.is_none() { pattern_opt = Some(s.to_string()); }
                else { files.push(s.to_string()); }
            }
        }
        i += 1;
    }

    let pattern = pattern_opt.unwrap_or_default();
    let pat = if ignore_case { pattern.to_lowercase() } else { pattern.clone() };

    let mut out   = String::new();
    let mut found = false;
    let mut code  = 1i32;

    let process_lines = |lines: Vec<String>, source: &str, out: &mut String, found: &mut bool, count: &mut i32| {
        let mut matches = 0usize;
        for line in &lines {
            let check = if ignore_case { line.to_lowercase() } else { line.clone() };
            let matched = check.contains(&pat);
            let show = if invert { !matched } else { matched };
            if show {
                matches += 1;
                *found = true;
                *count = 0;
                if !count_only {
                    let formatted = if files.len() > 1 || recursive {
                        format!("{}:{}\n", source, line)
                    } else {
                        format!("{}\n", line)
                    };
                    print!("{}", formatted);
                    out.push_str(&formatted);
                }
            }
        }
        if count_only {
            let s = format!("{}\n", matches);
            print!("{}", s);
            out.push_str(&s);
        }
    };

    let mut dummy_count = 0i32;

    if files.is_empty() {
        // stdin
        let stdin = io::stdin();
        let lines: Vec<String> = stdin.lock().lines().filter_map(|l| l.ok()).collect();
        process_lines(lines, "stdin", &mut out, &mut found, &mut dummy_count);
    } else {
        for file in &files {
            match fs::read_to_string(file) {
                Ok(content) => {
                    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
                    process_lines(lines, file, &mut out, &mut found, &mut dummy_count);
                }
                Err(e) => { eprintln!("[/> grep] {}: {}", file, e); code = 2; }
            }
        }
    }

    if found { code = 0; }
    Ok(BuiltinResult { exit_code: code, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
