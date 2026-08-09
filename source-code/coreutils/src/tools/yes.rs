use anyhow::{Result};



use crate::BuiltinResult;

pub fn builtin_yes(args: &[String]) -> Result<BuiltinResult> {
    let text = if args.is_empty() { "y".to_string() } else { args.join(" ") };
    // Ogranicz do 100 linii (żeby nie było nieskończonej pętli w skryptach)
    let mut out = String::new();
    for _ in 0..100 {
        println!("{}", text);
        out.push_str(&text); out.push('\n');
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
