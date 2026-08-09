use anyhow::{bail, Result};


use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_dirname(args: &[String]) -> Result<BuiltinResult> {
    if args.is_empty() { bail!("[/> dirname] brak argumentu"); }
    let path = Path::new(&args[0]);
    let dir = path.parent().and_then(|p| p.to_str()).unwrap_or(".").to_string();
    let dir = if dir.is_empty() { ".".to_string() } else { dir };
    println!("{}", dir);
    Ok(BuiltinResult { exit_code: 0, stdout: Some(dir) })
}
