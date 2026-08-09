use anyhow::{Result};



use crate::BuiltinResult;
use crate::read_files_or_stdin;

pub fn builtin_cut(args: &[String]) -> Result<BuiltinResult> {
    let mut delim = '\t';
    let mut fields: Vec<usize> = Vec::new();
    let mut chars_range: Option<(usize, usize)> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" => { i += 1; if i < args.len() { delim = args[i].chars().next().unwrap_or('\t'); } }
            "-f" => {
                i += 1;
                if i < args.len() {
                    for part in args[i].split(',') {
                        if let Ok(n) = part.trim().parse::<usize>() { fields.push(n); }
                    }
                }
            }
            "-c" => {
                i += 1;
                if i < args.len() {
                    let parts: Vec<&str> = args[i].split('-').collect();
                    if parts.len() == 2 {
                        let a = parts[0].parse().unwrap_or(1);
                        let b = parts[1].parse().unwrap_or(usize::MAX);
                        chars_range = Some((a, b));
                    }
                }
            }
            s if !s.starts_with('-') => files.push(s.to_string()),
            _ => {}
        }
        i += 1;
    }

    let content = read_files_or_stdin(files)?;
    let mut out = String::new();

    for line in content.lines() {
        let result = if let Some((a, b)) = chars_range {
            let chars: Vec<char> = line.chars().collect();
            let start = (a - 1).min(chars.len());
            let end   = b.min(chars.len());
            chars[start..end].iter().collect::<String>()
        } else if !fields.is_empty() {
            let cols: Vec<&str> = line.split(delim).collect();
            fields.iter()
                  .filter_map(|&f| cols.get(f - 1).copied())
                  .collect::<Vec<_>>()
                  .join(&delim.to_string())
        } else {
            line.to_string()
        };
        println!("{}", result);
        out.push_str(&result);
        out.push('\n');
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
