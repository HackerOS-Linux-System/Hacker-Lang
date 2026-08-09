use anyhow::{Result};



use crate::BuiltinResult;
use crate::read_files_or_stdin;

pub fn builtin_uniq(args: &[String]) -> Result<BuiltinResult> {
    let files: Vec<String> = args.iter().filter(|a| !a.starts_with('-')).cloned().collect();
    let content = read_files_or_stdin(files)?;
    // Deduplikacja kolejnych identycznych linii (jak POSIX uniq)
    let lines: Vec<String> = content.lines()
        .scan(None::<String>, |prev, line| {
            let show = prev.as_deref() != Some(line);
            *prev = Some(line.to_string());
            if show { Some(Some(line.to_string())) } else { Some(None) }
        })
        .flatten()
        .collect();
    for line in &lines { println!("{}", line); }
    let result = lines.join("\n");
    Ok(BuiltinResult { exit_code: 0, stdout: Some(result) })
}
