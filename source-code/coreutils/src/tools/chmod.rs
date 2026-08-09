use anyhow::{bail, Result};
use std::fs;


use crate::BuiltinResult;

pub fn builtin_chmod(args: &[String]) -> Result<BuiltinResult> {
    if args.len() < 2 { bail!("[/> chmod] Użycie: chmod mode plik..."); }
    let mode = u32::from_str_radix(&args[0], 8)
        .map_err(|_| anyhow::anyhow!("[/> chmod] Nieprawidłowy tryb: {}", args[0]))?;
    for path in &args[1..] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(BuiltinResult::ok())
}
