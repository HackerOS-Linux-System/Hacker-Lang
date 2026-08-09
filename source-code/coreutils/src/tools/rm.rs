use anyhow::{Result};
use std::fs;

use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_rm(args: &[String]) -> Result<BuiltinResult> {
    let mut recursive = false;
    let mut force     = false;
    let mut paths: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-r" | "-R" | "--recursive" => recursive = true,
            "-f" | "--force"            => force     = true,
            "-rf" | "-fr"               => { recursive = true; force = true; }
            s => paths.push(s.to_string()),
        }
    }
    let mut code = 0i32;
    for path in &paths {
        let p = Path::new(path);
        if !p.exists() {
            if !force { eprintln!("[/> rm] {} nie istnieje", path); code = 1; }
            continue;
        }
        let r = if p.is_dir() && recursive { fs::remove_dir_all(p) }
                else if p.is_file() { fs::remove_file(p) }
                else { eprintln!("[/> rm] {} jest katalogiem — użyj -r", path); code = 1; continue; };
        if let Err(e) = r {
            if !force { eprintln!("[/> rm] {}: {}", path, e); code = 1; }
        }
    }
    Ok(BuiltinResult::err(code))
}
