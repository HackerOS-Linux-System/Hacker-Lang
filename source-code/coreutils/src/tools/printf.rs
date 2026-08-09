use anyhow::{Result};



use crate::BuiltinResult;

pub fn builtin_printf(args: &[String]) -> Result<BuiltinResult> {
    if args.is_empty() { return Ok(BuiltinResult::ok()); }
    let fmt = args[0].replace("\\n", "\n").replace("\\t", "\t").replace("\\\\", "\\");
    // Simple %s substitution
    let mut result = fmt.clone();
    let mut idx = 1;
    while let Some(pos) = result.find("%s") {
        let replacement = args.get(idx).map(|s| s.as_str()).unwrap_or("");
        result.replace_range(pos..pos+2, replacement);
        idx += 1;
    }
    // %d substitution
    while let Some(pos) = result.find("%d") {
        let replacement = args.get(idx).map(|s| s.as_str()).unwrap_or("0");
        result.replace_range(pos..pos+2, replacement);
        idx += 1;
    }
    print!("{}", result);
    Ok(BuiltinResult { exit_code: 0, stdout: Some(result.trim_end_matches('\n').to_string()) })
}
