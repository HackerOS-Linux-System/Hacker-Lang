use anyhow::{bail, Result};


use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_basename(args: &[String]) -> Result<BuiltinResult> {
    if args.is_empty() { bail!("[/> basename] brak argumentu"); }
    let path = Path::new(&args[0]);
    let mut name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    // Optional suffix removal
    if args.len() > 1 { name = name.trim_end_matches(&*args[1]).to_string(); }
    println!("{}", name);
    Ok(BuiltinResult { exit_code: 0, stdout: Some(name) })
}
