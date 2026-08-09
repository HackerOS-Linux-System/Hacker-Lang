use anyhow::{Result};
use std::fs;


use crate::BuiltinResult;

pub fn builtin_stat(args: &[String]) -> Result<BuiltinResult> {
    let mut out = String::new();
    for path in args {
        match fs::metadata(path) {
            Ok(meta) => {
                use std::os::unix::fs::MetadataExt;
                let size  = meta.len();
                let mode  = meta.mode();
                let typ   = if meta.is_dir() { "katalog" } else if meta.is_file() { "plik" } else { "inne" };
                let mtime = meta.mtime();
                let line  = format!("  Plik: {}\n  Typ: {}\n  Rozmiar: {} B\n  Tryb: {:o}\n  Modyfikacja: {}\n", path, typ, size, mode, mtime);
                print!("{}", line);
                out.push_str(&line);
            }
            Err(e) => { eprintln!("[/> stat] {}: {}", path, e); return Ok(BuiltinResult::err(1)); }
        }
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out) })
}
