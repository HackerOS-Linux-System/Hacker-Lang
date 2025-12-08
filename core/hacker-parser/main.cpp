use std::env;
use std::io::{self, Write};
use std::process;

mod parse;
mod utils;

use parse::parse_hacker_file;
use parse::ParseResult;

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut verbose = false;
    let mut file_path = None;
    for arg in args {
        if arg == "--verbose" {
            verbose = true;
        } else if file_path.is_none() {
            file_path = Some(arg);
        } else {
            let mut stderr = io::stderr().lock();
            writeln!(stderr, "Usage: hacker-parser [--verbose] <file>")?;
            process::exit(1);
        }
    }
    let file_path = match file_path {
        Some(p) => p,
        None => {
            let mut stderr = io::stderr().lock();
            writeln!(stderr, "Usage: hacker-parser [--verbose] <file>")?;
            process::exit(1);
        }
    };
    let res = parse_hacker_file(&file_path, verbose).unwrap_or_else(|e| {
        let mut stderr = io::stderr().lock();
        writeln!(stderr, "Error: {}", e).unwrap();
        process::exit(1);
    });
    output_json(&res)?;
    Ok(())
}

fn output_json(res: &ParseResult) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "{{")?;
    write!(stdout, "\"deps\":[")?;
    let mut first = true;
    for key in res.deps.keys() {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, key)?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"libs\":[")?;
    first = true;
    for key in res.libs.keys() {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, key)?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"vars\":{{")?;
    first = true;
    for (key, value) in &res.vars_dict {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, key)?;
        write!(stdout, ":")?;
        write_json_string(&mut stdout, value)?;
    }
    write!(stdout, "}},")?;
    write!(stdout, "\"local_vars\":{{")?;
    first = true;
    for (key, value) in &res.local_vars {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, key)?;
        write!(stdout, ":")?;
        write_json_string(&mut stdout, value)?;
    }
    write!(stdout, "}},")?;
    write!(stdout, "\"cmds\":[")?;
    first = true;
    for c in &res.cmds {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, c)?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"cmds_with_vars\":[")?;
    first = true;
    for c in &res.cmds_with_vars {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, c)?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"cmds_separate\":[")?;
    first = true;
    for c in &res.cmds_separate {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, c)?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"includes\":[")?;
    first = true;
    for i in &res.includes {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, i)?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"binaries\":[")?;
    first = true;
    for b in &res.binaries {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, b)?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"plugins\":[")?;
    first = true;
    for p in &res.plugins {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write!(stdout, "{{")?;
        write!(stdout, "\"path\":")?;
        write_json_string(&mut stdout, &p.path)?;
        write!(stdout, ",\"super\":{}", if p.is_super { "true" } else { "false" })?;
        write!(stdout, "}}")?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"functions\":{{")?;
    first = true;
    for (key, vec) in &res.functions {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, key)?;
        write!(stdout, ":[")?;
        let mut first2 = true;
        for c in vec {
            if !first2 {
                write!(stdout, ",")?;
            }
            first2 = false;
            write_json_string(&mut stdout, c)?;
        }
        write!(stdout, "]")?;
    }
    write!(stdout, "}},")?;
    write!(stdout, "\"errors\":[")?;
    first = true;
    for e in &res.errors {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, e)?;
    }
    write!(stdout, "],")?;
    write!(stdout, "\"config\":{{")?;
    first = true;
    for (key, value) in &res.config_data {
        if !first {
            write!(stdout, ",")?;
        }
        first = false;
        write_json_string(&mut stdout, key)?;
        write!(stdout, ":")?;
        write_json_string(&mut stdout, value)?;
    }
    write!(stdout, "}}")?;
    writeln!(stdout, "}}")?;
    Ok(())
}

fn write_json_string<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    write!(w, "\"")?;
    for c in s.chars() {
        match c {
            '\"' => write!(w, "\\\"")?,
            '\\' => write!(w, "\\\\")?,
            '\x08' => write!(w, "\\b")?,
            '\x0c' => write!(w, "\\f")?,
            '\n' => write!(w, "\\n")?,
            '\r' => write!(w, "\\r")?,
            '\t' => write!(w, "\\t")?,
            c if c.is_control() => write!(w, "\\u{:04x}", c as u32)?,
            _ => write!(w, "{}", c)?,
        }
    }
    write!(w, "\"")?;
    Ok(())
}
