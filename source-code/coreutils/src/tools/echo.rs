use anyhow::{Result};



use crate::BuiltinResult;

pub fn builtin_echo(args: &[String]) -> Result<BuiltinResult> {
    let mut newline  = true;
    let mut escape   = false;
    let mut parts    = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-n" => newline  = false,
            "-e" => escape   = true,
            "-E" => escape   = false,
            s    => parts.push(s.to_string()),
        }
    }

    let mut out = parts.join(" ");
    if escape {
        out = out.replace("\\n",  "\n")
                 .replace("\\t",  "\t")
                 .replace("\\r",  "\r")
                 .replace("\\\\", "\\");
    }

    let s = if newline { format!("{}\n", out) } else { out };
    print!("{}", s);
    Ok(BuiltinResult { exit_code: 0, stdout: Some(s.trim_end_matches('\n').to_string()) })
}
