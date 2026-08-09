use anyhow::{Result};

use std::io::{self, Read};

use crate::BuiltinResult;


pub fn builtin_tr(args: &[String]) -> Result<BuiltinResult> {
    let mut delete = false;
    let mut _squeeze = false;
    let mut sets: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-d" => delete  = true,
            "-s" => _squeeze = true,
            s    => sets.push(s),
        }
    }

    let mut content = String::new();
    io::stdin().lock().read_to_string(&mut content)?;

    let expand_set = |s: &str| -> Vec<char> {
        let mut chars = Vec::new();
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if i + 2 < bytes.len() && bytes[i+1] == b'-' {
                let a = bytes[i] as char;
                let b = bytes[i+2] as char;
                for c in (a as u8)..=(b as u8) { chars.push(c as char); }
                i += 3;
            } else {
                chars.push(bytes[i] as char);
                i += 1;
            }
        }
        chars
    };

    let result = if delete && !sets.is_empty() {
        let del_set: Vec<char> = expand_set(sets[0]);
        content.chars().filter(|c| !del_set.contains(c)).collect()
    } else if sets.len() >= 2 {
        let set1 = expand_set(sets[0]);
        let set2 = expand_set(sets[1]);
        content.chars().map(|c| {
            if let Some(pos) = set1.iter().position(|&x| x == c) {
                *set2.get(pos).unwrap_or(set2.last().unwrap_or(&c))
            } else { c }
        }).collect()
    } else {
        content.clone()
    };

    print!("{}", result);
    Ok(BuiltinResult { exit_code: 0, stdout: Some(result.trim_end_matches('\n').to_string()) })
}
