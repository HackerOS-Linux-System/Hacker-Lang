use anyhow::{Result};
use std::fs;


use crate::BuiltinResult;

pub fn builtin_mkdir(args: &[String]) -> Result<BuiltinResult> {
    let mut parents = false;
    let mut paths: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-p" | "--parents" => parents = true,
            s => paths.push(s.to_string()),
        }
    }
    for path in &paths {
        if parents { fs::create_dir_all(path)?; }
        else { fs::create_dir(path)?; }
    }
    Ok(BuiltinResult::ok())
}
