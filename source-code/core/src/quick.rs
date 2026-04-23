use anyhow::{bail, Result};
use hl_parser::ast::StringPart;
use crate::env::{Env, Value};
use crate::executor::ExecResult;

pub fn exec_quick(name: &str, args: &[StringPart], env: &mut Env) -> Result<ExecResult> {
    let arg_str = env.resolve_string_parts(args);
    let arg_str = arg_str.trim();

    match name {
        "upper"      => { println!("{}", arg_str.to_uppercase()); Ok(ExecResult::ok()) }
        "lower"      => { println!("{}", arg_str.to_lowercase()); Ok(ExecResult::ok()) }
        "len"        => { println!("{}", arg_str.len()); Ok(ExecResult::ok()) }
        "trim"       => { println!("{}", arg_str.trim()); Ok(ExecResult::ok()) }
        "rev"        => { println!("{}", arg_str.chars().rev().collect::<String>()); Ok(ExecResult::ok()) }
        "repeat"     => {
            let (text, n) = split_last(arg_str);
            println!("{}", text.repeat(n.parse().unwrap_or(1)));
            Ok(ExecResult::ok())
        }
        "replace" => {
            let parts: Vec<&str> = arg_str.splitn(3, ' ').collect();
            if parts.len() < 3 { bail!(":: replace wymaga: :: replace <text> <from> <to>"); }
            println!("{}", parts[0].replace(parts[1], parts[2]));
            Ok(ExecResult::ok())
        }
        "contains"   => { let (t, p) = split_last(arg_str); let r = t.contains(p);    env.set_var("_last_bool", Value::Bool(r)); println!("{}", r); Ok(if r { ExecResult::ok() } else { ExecResult::err(1) }) }
        "startswith" => { let (t, p) = split_last(arg_str); let r = t.starts_with(p); env.set_var("_last_bool", Value::Bool(r)); println!("{}", r); Ok(if r { ExecResult::ok() } else { ExecResult::err(1) }) }
        "endswith"   => { let (t, p) = split_last(arg_str); let r = t.ends_with(p);   env.set_var("_last_bool", Value::Bool(r)); println!("{}", r); Ok(if r { ExecResult::ok() } else { ExecResult::err(1) }) }
        "split"      => { let (t, s) = split_last(arg_str); for p in t.split(s) { println!("{}", p); } Ok(ExecResult::ok()) }
        "lines"      => { for l in arg_str.lines() { println!("{}", l); } Ok(ExecResult::ok()) }
        "words"      => { for w in arg_str.split_whitespace() { println!("{}", w); } Ok(ExecResult::ok()) }
        "abs"   => { let n: f64 = arg_str.parse().unwrap_or(0.0); println!("{}", n.abs()); Ok(ExecResult::ok()) }
        "ceil"  => { let n: f64 = arg_str.parse().unwrap_or(0.0); println!("{}", n.ceil() as i64); Ok(ExecResult::ok()) }
        "floor" => { let n: f64 = arg_str.parse().unwrap_or(0.0); println!("{}", n.floor() as i64); Ok(ExecResult::ok()) }
        "round" => { let n: f64 = arg_str.parse().unwrap_or(0.0); println!("{}", n.round() as i64); Ok(ExecResult::ok()) }
        "max"   => { let (a,b) = split_last(arg_str); let a:f64=a.trim().parse().unwrap_or(0.0); let b:f64=b.trim().parse().unwrap_or(0.0); println!("{}", if a>b{a}else{b}); Ok(ExecResult::ok()) }
        "min"   => { let (a,b) = split_last(arg_str); let a:f64=a.trim().parse().unwrap_or(0.0); let b:f64=b.trim().parse().unwrap_or(0.0); println!("{}", if a<b{a}else{b}); Ok(ExecResult::ok()) }
        "rand"  => {
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64;
            println!("{}", (seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)) % 100);
            Ok(ExecResult::ok())
        }
        "env"  => { match std::env::var(arg_str) { Ok(v) => { println!("{}", v); Ok(ExecResult::ok()) } Err(_) => { println!(); Ok(ExecResult::err(1)) } } }
        "date" => { if let Ok(o) = std::process::Command::new("date").arg("+%Y-%m-%d").output() { print!("{}", String::from_utf8_lossy(&o.stdout)); } Ok(ExecResult::ok()) }
        "time" => { if let Ok(o) = std::process::Command::new("date").arg("+%H:%M:%S").output() { print!("{}", String::from_utf8_lossy(&o.stdout)); } Ok(ExecResult::ok()) }
        "pid"  => { println!("{}", std::process::id()); Ok(ExecResult::ok()) }
        "which"=> { match which::which(arg_str) { Ok(p) => { println!("{}", p.display()); Ok(ExecResult::ok()) } Err(_) => { println!(); Ok(ExecResult::err(1)) } } }
        "exists"   => { let e = std::path::Path::new(arg_str).exists();   env.set_var("_last_bool", Value::Bool(e)); Ok(if e { ExecResult::ok() } else { ExecResult::err(1) }) }
        "isdir"    => { let e = std::path::Path::new(arg_str).is_dir();    env.set_var("_last_bool", Value::Bool(e)); Ok(if e { ExecResult::ok() } else { ExecResult::err(1) }) }
        "isfile"   => { let e = std::path::Path::new(arg_str).is_file();   env.set_var("_last_bool", Value::Bool(e)); Ok(if e { ExecResult::ok() } else { ExecResult::err(1) }) }
        "basename" => { println!("{}", std::path::Path::new(arg_str).file_name().and_then(|n|n.to_str()).unwrap_or("")); Ok(ExecResult::ok()) }
        "dirname"  => { println!("{}", std::path::Path::new(arg_str).parent().and_then(|n|n.to_str()).unwrap_or(".")); Ok(ExecResult::ok()) }
        "read"     => { match std::fs::read_to_string(arg_str) { Ok(c) => { print!("{}", c); Ok(ExecResult::ok()) } Err(e) => bail!(":: read '{}': {}", arg_str, e) } }
        "set"   => { let (name, value) = split_first(arg_str); env.set_var(name, Value::String(value.to_string())); Ok(ExecResult::ok()) }
        "get"   => { println!("{}", env.get_var(arg_str).to_string_val()); Ok(ExecResult::ok()) }
        "type"  => {
            let t = match env.get_var(arg_str) { Value::String(_)=>"string", Value::Number(_)=>"number", Value::Bool(_)=>"bool", Value::Nil=>"nil" };
            println!("{}", t); Ok(ExecResult::ok())
        }
        "unset" => { env.vars.remove(arg_str); Ok(ExecResult::ok()) }
        "nl"     => { println!(); Ok(ExecResult::ok()) }
        "hr"     => { let w: usize = arg_str.parse().unwrap_or(60); println!("{}", "-".repeat(w)); Ok(ExecResult::ok()) }
        "bold"   => { println!("\x1b[1m{}\x1b[0m", arg_str); Ok(ExecResult::ok()) }
        "red"    => { println!("\x1b[31m{}\x1b[0m", arg_str); Ok(ExecResult::ok()) }
        "green"  => { println!("\x1b[32m{}\x1b[0m", arg_str); Ok(ExecResult::ok()) }
        "yellow" => { println!("\x1b[33m{}\x1b[0m", arg_str); Ok(ExecResult::ok()) }
        "cyan"   => { println!("\x1b[36m{}\x1b[0m", arg_str); Ok(ExecResult::ok()) }
        other    => bail!("Nieznana quick-funkcja '::{}'.", other),
    }
}

#[inline] fn split_last(s: &str) -> (&str, &str) {
    match s.rsplit_once(' ') { Some((a,b)) => (a.trim(), b.trim()), None => (s, "") }
}
#[inline] fn split_first(s: &str) -> (&str, &str) {
    match s.splitn(2,' ').collect::<Vec<_>>().as_slice() {
        [a,b] => (a.trim(), b.trim()), [a] => (a.trim(), ""), _ => ("","")
    }
}
