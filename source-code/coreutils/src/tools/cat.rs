use anyhow::{Result};
use std::fs;
use std::io::{self, Read};

use crate::BuiltinResult;

pub fn builtin_cat(args: &[String]) -> Result<BuiltinResult> {
    if args.is_empty() {
        // stdin mode
        let mut buf = String::new();
        io::stdin().lock().read_to_string(&mut buf)?;
        print!("{}", buf);
        return Ok(BuiltinResult { exit_code: 0, stdout: Some(buf.trim_end_matches('\n').to_string()) });
    }
    let mut all = String::new();
    let mut code = 0i32;
    for path in args {
        if path == "-" {
            let mut buf = String::new();
            io::stdin().lock().read_to_string(&mut buf)?;
            print!("{}", buf);
            all.push_str(&buf);
        } else {
            match fs::read_to_string(path) {
                Ok(s)  => { print!("{}", s); all.push_str(&s); }
                Err(e) => { eprintln!("[/> cat] {}: {}", path, e); code = 1; }
            }
        }
    }
    Ok(BuiltinResult { exit_code: code, stdout: Some(all.trim_end_matches('\n').to_string()) })
}
