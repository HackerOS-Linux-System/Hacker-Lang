use anyhow::{Result};



use crate::BuiltinResult;

pub fn builtin_pwd() -> Result<BuiltinResult> {
    let dir = std::env::current_dir()?.display().to_string();
    println!("{}", dir);
    Ok(BuiltinResult { exit_code: 0, stdout: Some(dir) })
}
