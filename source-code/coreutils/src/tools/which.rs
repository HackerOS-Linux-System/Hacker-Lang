use anyhow::{Result};



use crate::BuiltinResult;

pub fn builtin_which(args: &[String]) -> Result<BuiltinResult> {
    let mut out = String::new();
    let mut code = 0i32;
    for name in args {
        match which::which(name) {
            Ok(p)  => { let s = p.display().to_string(); println!("{}", s); out.push_str(&s); out.push('\n'); }
            Err(_) => { eprintln!("{}: nie znaleziono", name); code = 1; }
        }
    }
    Ok(BuiltinResult { exit_code: code, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
