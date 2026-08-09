use anyhow::{Result};
use std::fs;

use std::path::Path;
use crate::BuiltinResult;

pub fn builtin_touch(args: &[String]) -> Result<BuiltinResult> {
    for path in args {
        let p = Path::new(path);
        if !p.exists() {
            fs::File::create(p)?;
        } else {
            // Update modification time via read+write
            let content = fs::read(p)?;
            fs::write(p, content)?;
        }
    }
    Ok(BuiltinResult::ok())
}
