use anyhow::{bail, Result};
use std::fs;

use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_mv(args: &[String]) -> Result<BuiltinResult> {
    if args.len() < 2 { bail!("[/> mv] Użycie: mv src dst"); }
    let src = Path::new(&args[0]);
    let dst = Path::new(&args[1]);
    fs::rename(src, dst)?;
    Ok(BuiltinResult::ok())
}
