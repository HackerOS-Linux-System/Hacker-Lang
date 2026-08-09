use anyhow::{Result};
use std::fs;
use std::io::{self, Read, Write};

use crate::BuiltinResult;

pub fn builtin_tee(args: &[String]) -> Result<BuiltinResult> {
    let mut append = false;
    let mut files: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-a" => append = true,
            s    => files.push(s.to_string()),
        }
    }
    let mut content = String::new();
    io::stdin().lock().read_to_string(&mut content)?;
    print!("{}", content);
    for path in &files {
        if append { fs::OpenOptions::new().append(true).create(true).open(path)?.write_all(content.as_bytes())?; }
        else { fs::write(path, &content)?; }
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(content.trim_end_matches('\n').to_string()) })
}
