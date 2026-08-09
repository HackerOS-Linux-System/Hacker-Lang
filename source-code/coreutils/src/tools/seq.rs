use anyhow::{bail, Result};



use crate::BuiltinResult;

pub fn builtin_seq(args: &[String]) -> Result<BuiltinResult> {
    let (start, end, step) = match args.len() {
        1 => (1f64, args[0].parse()?, 1f64),
        2 => (args[0].parse()?, args[1].parse()?, 1f64),
        3 => (args[0].parse()?, args[2].parse()?, args[1].parse()?),
        _ => bail!("[/> seq] Użycie: seq [start] [krok] koniec"),
    };
    let mut out = String::new();
    let mut n = start;
    while n <= end {
        let s = if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{}", n) };
        println!("{}", s);
        out.push_str(&s); out.push('\n');
        n += step;
    }
    Ok(BuiltinResult { exit_code: 0, stdout: Some(out.trim_end_matches('\n').to_string()) })
}
