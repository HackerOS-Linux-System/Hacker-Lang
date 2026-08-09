use anyhow::{Result};



use crate::BuiltinResult;

pub fn builtin_date(args: &[String]) -> Result<BuiltinResult> {
    // Bez chrono — użyj systemu ale przez std
    let output = std::process::Command::new("date")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output();
    match output {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            println!("{}", s);
            Ok(BuiltinResult { exit_code: 0, stdout: Some(s) })
        }
        Err(_) => {
            // Pure fallback
            let s = "data nieznana".to_string();
            println!("{}", s);
            Ok(BuiltinResult { exit_code: 0, stdout: Some(s) })
        }
    }
}
