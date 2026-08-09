use anyhow::{Result};
use std::fs;

use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_ls(args: &[String]) -> Result<BuiltinResult> {
    let mut long    = false;
    let mut all     = false;
    let mut paths: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-l"  => long = true,
            "-a"  => all  = true,
            "-la" | "-al" => { long = true; all = true; }
            s     => paths.push(s),
        }
    }
    if paths.is_empty() { paths.push("."); }

    let mut out = String::new();
    let mut code = 0i32;

    for path in &paths {
        let p = Path::new(path);
        if !p.exists() {
            eprintln!("[/> ls] Nie znaleziono: {}", path);
            code = 1; continue;
        }
        if p.is_dir() {
            let mut entries: Vec<_> = fs::read_dir(p)?
                .filter_map(|e| e.ok())
                .collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !all && name.starts_with('.') { continue; }
                let line = if long {
                    let meta = entry.metadata()?;
                    let size = meta.len();
                    let is_dir = if meta.is_dir() { "d" } else { "-" };
                    format!("{}{:>10}  {}\n", is_dir, size, name)
                } else {
                    format!("{}\n", name)
                };
                print!("{}", line);
                out.push_str(&line);
            }
        } else {
            let line = format!("{}\n", path);
            print!("{}", line);
            out.push_str(&line);
        }
    }
    Ok(BuiltinResult { exit_code: code, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
