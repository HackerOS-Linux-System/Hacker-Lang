use anyhow::{Result};
use std::fs;

use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_du(args: &[String]) -> Result<BuiltinResult> {
    let mut human   = false;
    let mut _summary = false;
    let mut paths: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-h" => human    = true,
            "-s" => _summary = true,
            "-sh" | "-hs" => { human = true; _summary = true; }
            s    => paths.push(s.to_string()),
        }
    }
    if paths.is_empty() { paths.push(".".to_string()); }
    let mut out = String::new();
    for path in &paths {
        let size = du_size(Path::new(path));
        let display = if human { human_size(size) } else { size.to_string() };
        let line = format!("{}\t{}\n", display, path);
        print!("{}", line);
        out.push_str(&line);
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out.trim_end_matches('\n').to_string()) })
}

pub fn du_size(path: &Path) -> u64 {
    if path.is_file() { return path.metadata().map(|m| m.len()).unwrap_or(0); }
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            total += du_size(&entry.path());
        }
    }
    total
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    let mut size = bytes as f64;
    let mut idx = 0;
    while size >= 1024.0 && idx < UNITS.len() - 1 { size /= 1024.0; idx += 1; }
    if idx == 0 { format!("{}{}", bytes, UNITS[0]) }
    else { format!("{:.1}{}", size, UNITS[idx]) }
}
