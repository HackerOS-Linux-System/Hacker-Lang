use anyhow::{Result, bail};
use crate::ast::StringPart;
use crate::env::{Env, Value};
use crate::executor::ExecResult;

/// Dispatch a quick-call: :: name args
pub fn exec_quick(name: &str, args: &[StringPart], env: &mut Env) -> Result<ExecResult> {
    let arg_str = env.resolve_string_parts(args);
    let arg_str = arg_str.trim();

    match name {
        // ── String operations ─────────────────────────────────────────────────
        "upper" => {
            println!("{}", arg_str.to_uppercase());
            Ok(ExecResult::ok())
        }
        "lower" => {
            println!("{}", arg_str.to_lowercase());
            Ok(ExecResult::ok())
        }
        "len" => {
            println!("{}", arg_str.len());
            Ok(ExecResult::ok())
        }
        "trim" => {
            println!("{}", arg_str.trim());
            Ok(ExecResult::ok())
        }
        "rev" => {
            println!("{}", arg_str.chars().rev().collect::<String>());
            Ok(ExecResult::ok())
        }
        "repeat" => {
            let (text, n) = split_last_word(arg_str);
            let n: usize = n.parse().unwrap_or(1);
            println!("{}", text.repeat(n));
            Ok(ExecResult::ok())
        }
        "replace" => {
            // :: replace text_with_spaces old new
            // Format: :: replace @text old new  (last two words are old/new)
            let parts: Vec<&str> = arg_str.splitn(3, ' ').collect();
            if parts.len() < 3 {
                bail!(":: replace wymaga: :: replace <text> <from> <to>");
            }
            println!("{}", parts[0].replace(parts[1], parts[2]));
            Ok(ExecResult::ok())
        }
        "contains" => {
            let (text, pat) = split_last_word(arg_str);
            let result = text.contains(pat);
            env.set_var("_last_bool", Value::Bool(result));
            println!("{}", result);
            Ok(if result { ExecResult::ok() } else { ExecResult::err(1) })
        }
        "startswith" => {
            let (text, pat) = split_last_word(arg_str);
            let result = text.starts_with(pat);
            env.set_var("_last_bool", Value::Bool(result));
            println!("{}", result);
            Ok(if result { ExecResult::ok() } else { ExecResult::err(1) })
        }
        "endswith" => {
            let (text, pat) = split_last_word(arg_str);
            let result = text.ends_with(pat);
            env.set_var("_last_bool", Value::Bool(result));
            println!("{}", result);
            Ok(if result { ExecResult::ok() } else { ExecResult::err(1) })
        }
        "split" => {
            // :: split @text :
            let (text, sep) = split_last_word(arg_str);
            for part in text.split(sep) {
                println!("{}", part);
            }
            Ok(ExecResult::ok())
        }
        "lines" => {
            for line in arg_str.lines() {
                println!("{}", line);
            }
            Ok(ExecResult::ok())
        }
        "words" => {
            for word in arg_str.split_whitespace() {
                println!("{}", word);
            }
            Ok(ExecResult::ok())
        }

        // ── Math ──────────────────────────────────────────────────────────────
        "abs" => {
            let n: f64 = arg_str.parse().unwrap_or(0.0);
            println!("{}", n.abs());
            Ok(ExecResult::ok())
        }
        "ceil" => {
            let n: f64 = arg_str.parse().unwrap_or(0.0);
            println!("{}", n.ceil() as i64);
            Ok(ExecResult::ok())
        }
        "floor" => {
            let n: f64 = arg_str.parse().unwrap_or(0.0);
            println!("{}", n.floor() as i64);
            Ok(ExecResult::ok())
        }
        "round" => {
            let n: f64 = arg_str.parse().unwrap_or(0.0);
            println!("{}", n.round() as i64);
            Ok(ExecResult::ok())
        }
        "max" => {
            let (a, b) = split_last_word(arg_str);
            let a: f64 = a.trim().parse().unwrap_or(0.0);
            let b: f64 = b.trim().parse().unwrap_or(0.0);
            println!("{}", if a > b { a } else { b });
            Ok(ExecResult::ok())
        }
        "min" => {
            let (a, b) = split_last_word(arg_str);
            let a: f64 = a.trim().parse().unwrap_or(0.0);
            let b: f64 = b.trim().parse().unwrap_or(0.0);
            println!("{}", if a < b { a } else { b });
            Ok(ExecResult::ok())
        }
        "rand" => {
            // Simple LCG random — no dep needed
            let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64;
            let r = (seed.wrapping_mul(6364136223846793005u64).wrapping_add(1442695040888963407u64)) % 100;
            println!("{}", r);
            Ok(ExecResult::ok())
        }

        // ── System / info ─────────────────────────────────────────────────────
        "env" => {
            match std::env::var(arg_str) {
                Ok(v)  => { println!("{}", v); Ok(ExecResult::ok()) }
                Err(_) => { println!(""); Ok(ExecResult::err(1)) }
            }
        }
        "date" => {
            let out = std::process::Command::new("date").arg("+%Y-%m-%d").output();
            match out {
                Ok(o) => print!("{}", String::from_utf8_lossy(&o.stdout)),
                Err(_) => println!("?"),
            }
            Ok(ExecResult::ok())
        }
        "time" => {
            let out = std::process::Command::new("date").arg("+%H:%M:%S").output();
            match out {
                Ok(o) => print!("{}", String::from_utf8_lossy(&o.stdout)),
                Err(_) => println!("?"),
            }
            Ok(ExecResult::ok())
        }
        "pid" => {
            println!("{}", std::process::id());
            Ok(ExecResult::ok())
        }
        "which" => {
            match which::which(arg_str) {
                Ok(p)  => { println!("{}", p.display()); Ok(ExecResult::ok()) }
                Err(_) => { println!(""); Ok(ExecResult::err(1)) }
            }
        }

        // ── Filesystem ───────────────────────────────────────────────────────
        "exists" => {
            let exists = std::path::Path::new(arg_str).exists();
            env.set_var("_last_bool", Value::Bool(exists));
            Ok(if exists { ExecResult::ok() } else { ExecResult::err(1) })
        }
        "isdir" => {
            let is = std::path::Path::new(arg_str).is_dir();
            env.set_var("_last_bool", Value::Bool(is));
            Ok(if is { ExecResult::ok() } else { ExecResult::err(1) })
        }
        "isfile" => {
            let is = std::path::Path::new(arg_str).is_file();
            env.set_var("_last_bool", Value::Bool(is));
            Ok(if is { ExecResult::ok() } else { ExecResult::err(1) })
        }
        "basename" => {
            let p = std::path::Path::new(arg_str);
            println!("{}", p.file_name().and_then(|n| n.to_str()).unwrap_or(""));
            Ok(ExecResult::ok())
        }
        "dirname" => {
            let p = std::path::Path::new(arg_str);
            println!("{}", p.parent().and_then(|n| n.to_str()).unwrap_or("."));
            Ok(ExecResult::ok())
        }
        "read" => {
            // :: read /path/to/file  → print file content
            match std::fs::read_to_string(arg_str) {
                Ok(content) => { print!("{}", content); Ok(ExecResult::ok()) }
                Err(e)      => bail!(":: read: nie można odczytać '{}': {}", arg_str, e),
            }
        }

        // ── Variable helpers ─────────────────────────────────────────────────
        "set" => {
            // :: set name value
            let (name, value) = split_first_word(arg_str);
            env.set_var(name, Value::String(value.to_string()));
            Ok(ExecResult::ok())
        }
        "get" => {
            let val = env.get_var(arg_str).to_string_val();
            println!("{}", val);
            Ok(ExecResult::ok())
        }
        "type" => {
            let t = match env.get_var(arg_str) {
                Value::String(_) => "string",
                Value::Number(_) => "number",
                Value::Bool(_)   => "bool",
                Value::Nil       => "nil",
            };
            println!("{}", t);
            Ok(ExecResult::ok())
        }
        "unset" => {
            env.vars.remove(arg_str);
            Ok(ExecResult::ok())
        }

        // ── Output helpers ───────────────────────────────────────────────────
        "nl" => {
            // :: nl  → print empty line
            println!();
            Ok(ExecResult::ok())
        }
        "hr" => {
            // :: hr  → print horizontal rule
            let width: usize = arg_str.parse().unwrap_or(60);
            println!("{}", "─".repeat(width));
            Ok(ExecResult::ok())
        }
        "bold" => {
            println!("\x1b[1m{}\x1b[0m", arg_str);
            Ok(ExecResult::ok())
        }
        "red" => {
            println!("\x1b[31m{}\x1b[0m", arg_str);
            Ok(ExecResult::ok())
        }
        "green" => {
            println!("\x1b[32m{}\x1b[0m", arg_str);
            Ok(ExecResult::ok())
        }
        "yellow" => {
            println!("\x1b[33m{}\x1b[0m", arg_str);
            Ok(ExecResult::ok())
        }
        "cyan" => {
            println!("\x1b[36m{}\x1b[0m", arg_str);
            Ok(ExecResult::ok())
        }

        // ── Unknown ──────────────────────────────────────────────────────────
        other => {
            bail!(
                "Nieznana quick-funkcja '::{}'. Użyj 'hl lib list' aby zobaczyć dostępne funkcje.\n\
Dostępne: upper, lower, len, trim, rev, replace, contains, split, words, lines,\n\
abs, ceil, floor, round, max, min, rand, env, date, time, pid, which,\n\
exists, isdir, isfile, basename, dirname, read,\n\
set, get, type, unset, nl, hr, bold, red, green, yellow, cyan",
other
            )
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Split "some text pattern" → ("some text", "pattern")
#[inline]
fn split_last_word(s: &str) -> (&str, &str) {
    match s.rsplit_once(' ') {
        Some((a, b)) => (a.trim(), b.trim()),
        None         => (s, ""),
    }
}

/// Split "name rest of stuff" → ("name", "rest of stuff")
#[inline]
fn split_first_word(s: &str) -> (&str, &str) {
    match s.splitn(2, ' ').collect::<Vec<_>>().as_slice() {
        [a, b] => (a.trim(), b.trim()),
        [a]    => (a.trim(), ""),
        _      => ("", ""),
    }
}
