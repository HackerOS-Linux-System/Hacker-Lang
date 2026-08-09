use anyhow::{Result};
use std::fs;

use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_find(args: &[String]) -> Result<BuiltinResult> {
    let start = args.first().map(|s| s.as_str()).unwrap_or(".");
    let mut name_pat: Option<String> = None;
    let mut type_filter: Option<char> = None;
    let mut maxdepth: Option<usize>   = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-name"     => { i += 1; if i < args.len() { name_pat = Some(args[i].clone()); } }
            "-type"     => { i += 1; if i < args.len() { type_filter = args[i].chars().next(); } }
            "-maxdepth" => { i += 1; if i < args.len() { maxdepth = args[i].parse().ok(); } }
            _ => {}
        }
        i += 1;
    }

    let mut out = String::new();
    find_recursive(Path::new(start), &name_pat, type_filter, maxdepth, 0, &mut out)?;
    print!("{}", out);
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out.trim_end_matches('\n').to_string()) })
}

pub fn find_recursive(dir: &Path, name_pat: &Option<String>, type_filter: Option<char>,
                  maxdepth: Option<usize>, depth: usize, out: &mut String) -> Result<()> {
    if let Some(max) = maxdepth { if depth > max { return Ok(()); } }
    if !dir.exists() { return Ok(()); }

    if dir.is_file() {
        if matches_find(dir, name_pat, type_filter) {
            let s = format!("{}\n", dir.display());
            out.push_str(&s);
        }
        return Ok(());
    }

    if let Ok(entries) = fs::read_dir(dir) {
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            if matches_find(&path, name_pat, type_filter) {
                let s = format!("{}\n", path.display());
                out.push_str(&s);
            }
            if path.is_dir() {
                find_recursive(&path, name_pat, type_filter, maxdepth, depth + 1, out)?;
            }
        }
    }
    Ok(())
}

pub fn matches_find(path: &Path, name_pat: &Option<String>, type_filter: Option<char>) -> bool {
    if let Some(f) = type_filter {
        match f {
            'f' if !path.is_file() => return false,
            'd' if !path.is_dir()  => return false,
            _ => {}
        }
    }
    if let Some(pat) = name_pat {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if pat.contains('*') {
            let pat = pat.replace("*.", "").replace('*', "");
            if !pat.is_empty() && !name.contains(&*pat) { return false; }
        } else if name != pat.as_str() {
            return false;
        }
    }
    true
}
