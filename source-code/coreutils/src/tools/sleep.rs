use anyhow::{Result};



use crate::BuiltinResult;

pub fn builtin_sleep(args: &[String]) -> Result<BuiltinResult> {
    let secs: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(BuiltinResult::ok())
}
