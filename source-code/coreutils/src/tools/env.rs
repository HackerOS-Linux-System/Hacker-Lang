use anyhow::{Result};



use crate::BuiltinResult;

pub fn builtin_env(args: &[String]) -> Result<BuiltinResult> {
    let mut out = String::new();
    if args.is_empty() {
        for (k, v) in std::env::vars() {
            let line = format!("{}={}\n", k, v);
            print!("{}", line);
            out.push_str(&line);
        }
    } else {
        // env VAR_NAME — wypisz wartość
        let val = std::env::var(&args[0]).unwrap_or_default();
        println!("{}", val);
        out = val;
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
