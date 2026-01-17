use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use env_logger;
use log::info;
use rayon::prelude::*;
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
const HACKER_DIR_SUFFIX: &str = "/.hackeros/hacker-lang";
#[derive(Serialize, Clone)]
struct Param {
    name: String,
    type_: String,
    default: Option<String>,
}
#[derive(Serialize, Clone)]
struct Function {
    params: Vec<Param>,
    body: Vec<String>,
}
#[derive(Serialize, Clone)]
struct Plugin {
    path: String,
    #[serde(rename = "super")]
    is_super: bool,
}
#[derive(Serialize, Clone)]
struct ParseResult {
    deps: Vec<String>,
    libs: Vec<String>,
    rust_libs: Vec<String>,
    python_libs: Vec<String>,
    java_libs: Vec<String>,
    #[serde(rename = "vars")]
    vars_dict: HashMap<String, String>,
    local_vars: HashMap<String, String>,
    cmds: Vec<String>,
    cmds_with_vars: Vec<String>,
    cmds_separate: Vec<String>,
    includes: Vec<String>,
    binaries: Vec<String>,
    plugins: Vec<Plugin>,
    functions: HashMap<String, Function>,
    errors: Vec<String>,
    #[serde(rename = "config")]
    config_data: HashMap<String, String>,
}
fn trim(s: &str) -> String {
    s.trim().to_string()
}
fn parse_hacker_file(file_path: &str, verbose: bool) -> ParseResult {
    let mut res = ParseResult {
        deps: Vec::new(),
        libs: Vec::new(),
        rust_libs: Vec::new(),
        python_libs: Vec::new(),
        java_libs: Vec::new(),
        vars_dict: HashMap::new(),
        local_vars: HashMap::new(),
        cmds: Vec::new(),
        cmds_with_vars: Vec::new(),
        cmds_separate: Vec::new(),
        includes: Vec::new(),
        binaries: Vec::new(),
        plugins: Vec::new(),
        functions: HashMap::new(),
        errors: Vec::new(),
        config_data: HashMap::new(),
    };
    let mut in_config = false;
    let mut in_comment = false;
    let mut in_function: Option<String> = None;
    let mut line_num: u32 = 0;
    let home = env::var("HOME").unwrap_or_default();
    let hacker_dir: PathBuf = Path::new(&home).join(HACKER_DIR_SUFFIX.strip_prefix('/').unwrap_or(HACKER_DIR_SUFFIX));
    let file = match File::open(file_path) {
        Ok(f) => f,
        Err(_) => {
            if verbose {
                println!("File {} not found", file_path);
            }
            res.errors.push(format!("File {} not found", file_path));
            return res;
        }
    };
    let reader = io::BufReader::new(file);
    'line_loop: for line_res in reader.lines() {
        line_num += 1;
        let line_slice = match line_res {
            Ok(l) => l,
            Err(_) => continue,
        };
        let line_trimmed = trim(&line_slice);
        if line_trimmed.is_empty() {
            continue;
        }
        let mut line = line_trimmed.clone();
        let mut is_super = false;
        if line.starts_with('^') {
            is_super = true;
            line = trim(&line[1..]);
        }
        if line == "!!" {
            in_comment = !in_comment;
            continue;
        }
        if in_comment {
            continue;
        }
        if line == "[" {
            if in_config {
                res.errors
                .push(format!("Line {}: Nested config section", line_num));
            }
            if in_function.is_some() {
                res.errors
                .push(format!("Line {}: Config in function", line_num));
            }
            in_config = true;
            continue;
        } else if line == "]" {
            if !in_config {
                res.errors
                .push(format!("Line {}: Closing ] without [", line_num));
            }
            in_config = false;
            continue;
        }
        if in_config {
            if let Some(eq_pos) = line.find('=') {
                let key = trim(&line[..eq_pos]);
                let value = trim(&line[eq_pos + 1..]);
                res.config_data.insert(key, value);
            }
            continue;
        }
        if line == ":" {
            if in_function.is_some() {
                in_function = None;
            } else {
                res.errors
                .push(format!("Line {}: Ending function without start", line_num));
            }
            continue;
        } else if line.starts_with(':') {
            let rest = trim(&line[1..]);
            let (func_name, params_str_opt) = if let Some(pos) = rest.find('(') {
                (trim(&rest[..pos]), Some(trim(&rest[pos + 1..])))
            } else {
                (rest, None)
            };
            if func_name.is_empty() {
                res.errors
                .push(format!("Line {}: Empty function name", line_num));
                continue;
            }
            if in_function.is_some() {
                res.errors.push(format!("Line {}: Nested function", line_num));
                continue;
            }
            let mut params = Vec::new();
            if let Some(mut params_str) = params_str_opt {
                if !params_str.ends_with(')') {
                    res.errors.push(format!("Line {}: Missing ) in function definition", line_num));
                    continue;
                }
                params_str = trim(&params_str[..params_str.len() - 1]);
                for p in params_str.split(',') {
                    let p = trim(p);
                    let (name, rest) = if let Some(col_pos) = p.find(':') {
                        (trim(&p[..col_pos]), trim(&p[col_pos + 1..]))
                    } else {
                        (p, String::new())
                    };
                    let (type_, default) = if let Some(eq_pos) = rest.find('=') {
                        (trim(&rest[..eq_pos]), Some(trim(&rest[eq_pos + 1..])))
                    } else {
                        (rest, None)
                    };
                    let type_ = if type_.is_empty() { "str".to_string() } else { type_ };
                    params.push(Param { name, type_, default });
                }
            }
            res.functions.insert(func_name.clone(), Function { params, body: Vec::new() });
            in_function = Some(func_name);
            continue;
        } else if line.starts_with('.') {
            let rest = trim(&line[1..]);
            let (func_name, args_str_opt) = if let Some(pos) = rest.find('(') {
                (trim(&rest[..pos]), Some(trim(&rest[pos + 1..])))
            } else {
                (rest, None)
            };
            if func_name.is_empty() {
                res.errors
                .push(format!("Line {}: Empty function call", line_num));
                continue;
            }
            if let Some(func) = res.functions.get(&func_name).cloned() {
                let mut args = Vec::new();
                if let Some(mut args_str) = args_str_opt {
                    if !args_str.ends_with(')') {
                        res.errors.push(format!("Line {}: Missing ) in function call", line_num));
                        continue;
                    }
                    args_str = trim(&args_str[..args_str.len() - 1]);
                    args = args_str.split(',').map(trim).collect();
                }
                let params = func.params;
                if args.len() > params.len() {
                    res.errors.push(format!("Line {}: Too many arguments for {}", line_num, func_name));
                    continue;
                }
                let mut sub_map: HashMap<String, String> = HashMap::new();
                for (i, param) in params.iter().enumerate() {
                    let val = if i < args.len() {
                        args[i].clone()
                    } else if let Some(ref d) = param.default {
                        d.clone()
                    } else {
                        res.errors.push(format!("Line {}: Missing argument {} for {}", line_num, param.name, func_name));
                        continue 'line_loop;
                    };
                    let valid = match param.type_.as_str() {
                        "int" => val.parse::<i64>().is_ok(),
                        "bool" => val == "true" || val == "false",
                        "str" => true,
                        "list" => val.contains(' '), // rough check
                        "dict" => val.contains('='),
                        _ => true,
                    };
                    if !valid {
                        res.errors.push(format!("Line {}: Type mismatch for {}: expected {}, got {}", line_num, param.name, param.type_, val));
                        continue 'line_loop;
                    }
                    sub_map.insert(param.name.clone(), val);
                }
                let body = func.body;
                let sub_body: Vec<String> = body.par_iter().map(|cmd| {
                    let mut new_cmd = cmd.clone();
                    for (name, val) in &sub_map {
                        new_cmd = new_cmd.replace(&format!("${}", name), val);
                    }
                    new_cmd
                }).collect();
                if let Some(ref f) = in_function {
                    if let Some(target_func) = res.functions.get_mut(f) {
                        target_func.body.extend(sub_body);
                    }
                } else {
                    res.cmds.extend(sub_body);
                }
            } else {
                res.errors.push(format!(
                    "Line {}: Unknown function {}",
                    line_num, func_name
                ));
            }
            continue;
        }
        if let Some(_) = &in_function {
            if !(line.starts_with('>')
                || line.starts_with('=')
                || line.starts_with('?')
                || line.starts_with('&')
                || line.starts_with('!')
                || line.starts_with('@')
                || line.starts_with('$')
                || line.starts_with('\\')
                || line.starts_with(">>")
                || line.starts_with(">>>")
                || line.starts_with("%")
                || line.starts_with("T>"))
            {
                res.errors
                .push(format!("Line {}: Invalid in function", line_num));
                continue;
            }
        }
        let mut parsed = false;
        if line.starts_with("//") {
            parsed = true;
            if in_function.is_some() {
                res.errors
                .push(format!("Line {}: Deps not allowed in function", line_num));
                continue;
            }
            let dep = trim(&line[2..]);
            if !dep.is_empty() {
                res.deps.push(dep);
            } else {
                res.errors
                .push(format!("Line {}: Empty system dependency", line_num));
            }
        } else if line.starts_with('#') {
            parsed = true;
            if in_function.is_some() {
                res.errors
                .push(format!("Line {}: Libs not allowed in function", line_num));
                continue;
            }
            let full_lib = trim(&line[1..]);
            if full_lib.is_empty() {
                res.errors
                .push(format!("Line {}: Empty library/include", line_num));
                continue;
            }
            let (prefix, lib_name) = if let Some(colon_pos) = full_lib.find(':') {
                (trim(&full_lib[..colon_pos]), trim(&full_lib[colon_pos + 1..]))
            } else {
                ("bytes".to_string(), full_lib)
            };
            if lib_name.is_empty() {
                res.errors.push(format!(
                    "Line {}: Empty library name after prefix",
                    line_num
                ));
                continue;
            }
            if prefix == "rust" {
                res.rust_libs.push(lib_name);
            } else if prefix == "python" {
                res.python_libs.push(lib_name);
            } else if prefix == "java" {
                res.java_libs.push(lib_name);
            } else if prefix == "bytes" {
                let lib_dir = hacker_dir.join("libs").join(&lib_name);
                let lib_hacker_path = lib_dir.join("main.hacker");
                let lib_bin_path = hacker_dir.join("libs").join(&lib_name);
                if lib_hacker_path.exists() {
                    res.includes.push(lib_name.clone());
                    let sub = parse_hacker_file(&lib_hacker_path.to_string_lossy(), verbose);
                    res.deps.extend(sub.deps);
                    res.libs.extend(sub.libs);
                    res.rust_libs.extend(sub.rust_libs);
                    res.python_libs.extend(sub.python_libs);
                    res.java_libs.extend(sub.java_libs);
                    res.vars_dict.extend(sub.vars_dict);
                    res.local_vars.extend(sub.local_vars);
                    res.cmds.extend(sub.cmds);
                    res.cmds_with_vars.extend(sub.cmds_with_vars);
                    res.cmds_separate.extend(sub.cmds_separate);
                    res.includes.extend(sub.includes);
                    res.binaries.extend(sub.binaries);
                    res.plugins.extend(sub.plugins);
                    for (k, v) in sub.functions {
                        res.functions.insert(k, v);
                    }
                    for sub_err in sub.errors {
                        res.errors.push(format!("In {}: {}", lib_name, sub_err));
                    }
                }
                if let Ok(metadata) = fs::metadata(&lib_bin_path) {
                    let mode = metadata.mode();
                    if (mode & 0o111) != 0 {
                        res.binaries
                        .push(lib_bin_path.to_string_lossy().to_string());
                    } else {
                        res.libs.push(lib_name);
                    }
                } else {
                    res.libs.push(lib_name);
                }
            } else {
                res.errors.push(format!(
                    "Line {}: Unknown library prefix: {}",
                    line_num, prefix
                ));
            }
        } else if line.starts_with(">>>") {
            parsed = true;
            let cmd = trim(&line[3..]);
            let cmd = if let Some(excl) = cmd.find('!') {
                trim(&cmd[..excl])
            } else {
                cmd
            };
            let mut mut_cmd = cmd.clone();
            if is_super {
                mut_cmd = format!("sudo {}", mut_cmd);
            }
            if mut_cmd.is_empty() {
                res.errors
                .push(format!("Line {}: Empty separate file command", line_num));
            } else {
                let target = if let Some(ref f) = in_function {
                    &mut res.functions.get_mut(f).unwrap().body
                } else {
                    &mut res.cmds_separate
                };
                target.push(mut_cmd);
            }
        } else if line.starts_with(">>") {
            parsed = true;
            let cmd = trim(&line[2..]);
            let cmd = if let Some(excl) = cmd.find('!') {
                trim(&cmd[..excl])
            } else {
                cmd
            };
            let mut mut_cmd = cmd.clone();
            if is_super {
                mut_cmd = format!("sudo {}", mut_cmd);
            }
            if mut_cmd.is_empty() {
                res.errors
                .push(format!("Line {}: Empty command with vars", line_num));
            } else {
                let target = if let Some(ref f) = in_function {
                    &mut res.functions.get_mut(f).unwrap().body
                } else {
                    &mut res.cmds_with_vars
                };
                target.push(mut_cmd);
            }
        } else if line.starts_with('>') {
            parsed = true;
            let cmd = trim(&line[1..]);
            let cmd = if let Some(excl) = cmd.find('!') {
                trim(&cmd[..excl])
            } else {
                cmd
            };
            let mut mut_cmd = cmd.clone();
            if is_super {
                mut_cmd = format!("sudo {}", mut_cmd);
            }
            if mut_cmd.is_empty() {
                res.errors.push(format!("Line {}: Empty command", line_num));
            } else {
                let target = if let Some(ref f) = in_function {
                    &mut res.functions.get_mut(f).unwrap().body
                } else {
                    &mut res.cmds
                };
                target.push(mut_cmd);
            }
        } else if line.starts_with('@') {
            parsed = true;
            let mut pos = 1;
            while pos < line.len() && (line.as_bytes()[pos].is_ascii_alphanumeric() || line.as_bytes()[pos] == b'_') {
                pos += 1;
            }
            let key_str = &line[1..pos];
            let (key, type_) = if let Some(col_pos) = key_str.find(':') {
                (&key_str[..col_pos], &key_str[col_pos + 1..])
            } else {
                (key_str, "str")
            };
            let key = key.to_string();
            let type_ = type_.to_string();
            let after = trim(&line[pos..]);
            if !after.starts_with('=') {
                res.errors
                .push(format!("Line {}: Invalid variable", line_num));
                continue;
            }
            let mut value = trim(&after[1..]);
            if value.is_empty() {
                res.errors
                .push(format!("Line {}: Invalid variable", line_num));
                continue;
            }
            // Handle list and dict
            if type_ == "list" {
                if value.starts_with('[') && value.ends_with(']') {
                    let inner = &value[1..value.len() - 1];
                    let items: Vec<String> = inner.split(',').map(trim).collect();
                    value = items.join(" ");
                } else {
                    res.errors.push(format!("Line {}: Invalid list format for {}", line_num, key));
                    continue;
                }
            } else if type_ == "dict" {
                if value.starts_with('{') && value.ends_with('}') {
                    let inner = &value[1..value.len() - 1];
                    let pairs: Vec<String> = inner.split(',').map(|p| {
                        let pp: Vec<String> = p.splitn(2, ':').map(trim).collect();
                        if pp.len() == 2 {
                            format!("{}={}", pp[0], pp[1])
                        } else {
                            String::new()
                        }
                    }).filter(|s| !s.is_empty()).collect();
                    value = pairs.join(" ");
                } else {
                    res.errors.push(format!("Line {}: Invalid dict format for {}", line_num, key));
                    continue;
                }
            }
            // Validate type
            let valid = match type_.as_str() {
                "int" => value.parse::<i64>().is_ok(),
                "bool" => value == "true" || value == "false",
                "str" => true,
                "list" => true,
                "dict" => true,
                _ => {
                    res.errors.push(format!("Line {}: Unknown type {} for variable {}", line_num, type_, key));
                    false
                }
            };
            if !valid {
                res.errors.push(format!("Line {}: Type validation failed for {}: {}", line_num, key, value));
                continue;
            }
            res.vars_dict.insert(key, value);
        } else if line.starts_with('$') {
            parsed = true;
            let mut pos = 1;
            while pos < line.len() && (line.as_bytes()[pos].is_ascii_alphanumeric() || line.as_bytes()[pos] == b'_') {
                pos += 1;
            }
            let key_str = &line[1..pos];
            let (key, type_) = if let Some(col_pos) = key_str.find(':') {
                (&key_str[..col_pos], &key_str[col_pos + 1..])
            } else {
                (key_str, "str")
            };
            let key = key.to_string();
            let type_ = type_.to_string();
            let after = trim(&line[pos..]);
            if !after.starts_with('=') {
                res.errors
                .push(format!("Line {}: Invalid local variable", line_num));
                continue;
            }
            let mut value = trim(&after[1..]);
            if value.is_empty() {
                res.errors
                .push(format!("Line {}: Invalid local variable", line_num));
                continue;
            }
            // Handle list and dict
            if type_ == "list" {
                if value.starts_with('[') && value.ends_with(']') {
                    let inner = &value[1..value.len() - 1];
                    let items: Vec<String> = inner.split(',').map(trim).collect();
                    value = items.join(" ");
                } else {
                    res.errors.push(format!("Line {}: Invalid list format for {}", line_num, key));
                    continue;
                }
            } else if type_ == "dict" {
                if value.starts_with('{') && value.ends_with('}') {
                    let inner = &value[1..value.len() - 1];
                    let pairs: Vec<String> = inner.split(',').map(|p| {
                        let pp: Vec<String> = p.splitn(2, ':').map(trim).collect();
                        if pp.len() == 2 {
                            format!("{}={}", pp[0], pp[1])
                        } else {
                            String::new()
                        }
                    }).filter(|s| !s.is_empty()).collect();
                    value = pairs.join(" ");
                } else {
                    res.errors.push(format!("Line {}: Invalid dict format for {}", line_num, key));
                    continue;
                }
            }
            // Validate type
            let valid = match type_.as_str() {
                "int" => value.parse::<i64>().is_ok(),
                "bool" => value == "true" || value == "false",
                "str" => true,
                "list" => true,
                "dict" => true,
                _ => {
                    res.errors.push(format!("Line {}: Unknown type {} for local variable {}", line_num, type_, key));
                    false
                }
            };
            if !valid {
                res.errors.push(format!("Line {}: Type validation failed for local {}: {}", line_num, key, value));
                continue;
            }
            res.local_vars.insert(key, value);
        } else if line.starts_with('\\') {
            parsed = true;
            let plugin_name = trim(&line[1..]);
            if plugin_name.is_empty() {
                res.errors
                .push(format!("Line {}: Empty plugin name", line_num));
                continue;
            }
            let plugin_dir = hacker_dir.join("plugins").join(&plugin_name);
            if let Ok(metadata) = fs::metadata(&plugin_dir) {
                let mode = metadata.mode();
                if (mode & 0o111) != 0 {
                    res.plugins.push(Plugin {
                        path: plugin_dir.to_string_lossy().to_string(),
                                     is_super,
                    });
                } else {
                    res.errors.push(format!(
                        "Line {}: Plugin {} not found or not executable",
                        line_num, plugin_name
                    ));
                }
            } else {
                res.errors.push(format!(
                    "Line {}: Plugin {} not found or not executable",
                    line_num, plugin_name
                ));
            }
        } else if line.starts_with('=') {
            parsed = true;
            if let Some(gt_pos) = line.find('>') {
                let num_str = trim(&line[1..gt_pos]);
                let cmd_part = trim(&line[gt_pos + 1..]);
                let cmd_part = if let Some(excl) = cmd_part.find('!') {
                    trim(&cmd_part[..excl])
                } else {
                    cmd_part
                };
                let num: i32 = match num_str.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        res.errors
                        .push(format!("Line {}: Invalid loop count", line_num));
                        continue;
                    }
                };
                if num < 0 {
                    res.errors
                    .push(format!("Line {}: Negative loop count", line_num));
                    continue;
                }
                if cmd_part.is_empty() {
                    res.errors
                    .push(format!("Line {}: Empty loop command", line_num));
                    continue;
                }
                let cmd_base = if is_super {
                    format!("sudo {}", cmd_part)
                } else {
                    cmd_part
                };
                let target = if let Some(ref f) = in_function {
                    &mut res.functions.get_mut(f).unwrap().body
                } else {
                    &mut res.cmds
                };
                for _ in 0..num {
                    target.push(cmd_base.clone());
                }
            } else {
                res.errors
                .push(format!("Line {}: Invalid loop syntax", line_num));
            }
        } else if line.starts_with('?') {
            parsed = true;
            if let Some(gt_pos) = line.find('>') {
                let condition = trim(&line[1..gt_pos]);
                let cmd_part = trim(&line[gt_pos + 1..]);
                let cmd_part = if let Some(excl) = cmd_part.find('!') {
                    trim(&cmd_part[..excl])
                } else {
                    cmd_part
                };
                if condition.is_empty() || cmd_part.is_empty() {
                    res.errors
                    .push(format!("Line {}: Invalid conditional", line_num));
                    continue;
                }
                let cmd = if is_super {
                    format!("sudo {}", cmd_part)
                } else {
                    cmd_part
                };
                let if_cmd = format!("if {}; then {}; fi", condition, cmd);
                let target = if let Some(ref f) = in_function {
                    &mut res.functions.get_mut(f).unwrap().body
                } else {
                    &mut res.cmds
                };
                target.push(if_cmd);
            } else {
                res.errors
                .push(format!("Line {}: Invalid conditional", line_num));
            }
        } else if line.starts_with('&') {
            parsed = true;
            let cmd_part = trim(&line[1..]);
            let cmd_part = if let Some(excl) = cmd_part.find('!') {
                trim(&cmd_part[..excl])
            } else {
                cmd_part
            };
            if cmd_part.is_empty() {
                res.errors
                .push(format!("Line {}: Empty background command", line_num));
                continue;
            }
            let mut cmd = format!("{} &", cmd_part);
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            let target = if let Some(ref f) = in_function {
                &mut res.functions.get_mut(f).unwrap().body
            } else {
                &mut res.cmds
            };
            target.push(cmd);
        } else if line.starts_with('!') {
            parsed = true;
            // ignore
        } else if line.starts_with("%") {
            parsed = true;
            let rest = trim(&line[1..]);
            if let Some(gt_pos) = rest.find('>') {
                let list_var = trim(&rest[..gt_pos]);
                let cmd_part = trim(&rest[gt_pos + 1..]);
                if list_var.is_empty() || cmd_part.is_empty() {
                    res.errors.push(format!("Line {}: Invalid foreach syntax", line_num));
                    continue;
                }
                let mut foreach_cmd = format!("for item in {}; do {}; done", list_var, cmd_part);
                if is_super {
                    foreach_cmd = format!("sudo {}", foreach_cmd);
                }
                let target = if let Some(ref f) = in_function {
                    &mut res.functions.get_mut(f).unwrap().body
                } else {
                    &mut res.cmds
                };
                target.push(foreach_cmd);
            } else {
                res.errors.push(format!("Line {}: Invalid foreach syntax", line_num));
            }
        } else if line.starts_with("T>") {
            parsed = true;
            if let Some(c_pos) = line.find("C>") {
                let try_cmd = trim(&line[2..c_pos]);
                let rest_after_c = &line[c_pos + 2..];
                let f_pos_opt = rest_after_c.find("F>");
                let catch_cmd = if let Some(f_pos) = f_pos_opt {
                    trim(&rest_after_c[..f_pos])
                } else {
                    trim(rest_after_c)
                };
                let finally_cmd = if let Some(f_pos) = f_pos_opt {
                    trim(&rest_after_c[f_pos + 2..])
                } else {
                    String::new()
                };
                let mut try_catch_cmd = format!("( {} ) || {};", try_cmd, catch_cmd);
                if !finally_cmd.is_empty() {
                    try_catch_cmd = format!("{} {}", try_catch_cmd, finally_cmd);
                }
                if is_super {
                    try_catch_cmd = format!("sudo {}", try_catch_cmd);
                }
                let target = if let Some(ref f) = in_function {
                    &mut res.functions.get_mut(f).unwrap().body
                } else {
                    &mut res.cmds
                };
                target.push(try_catch_cmd);
            } else {
                res.errors.push(format!("Line {}: Invalid try-catch syntax", line_num));
            }
        }
        if !parsed {
            res.errors
            .push(format!("Line {}: Invalid syntax", line_num));
        }
    }
    if in_config {
        res.errors.push("Unclosed config section".to_string());
    }
    if in_comment {
        res.errors.push("Unclosed comment block".to_string());
    }
    if in_function.is_some() {
        res.errors.push("Unclosed function block".to_string());
    }
    // Sort with rayon
    res.deps.par_sort_unstable();
    res.libs.par_sort_unstable();
    res.rust_libs.par_sort_unstable();
    res.python_libs.par_sort_unstable();
    res.java_libs.par_sort_unstable();
    res
}
fn main() -> Result<()> {
    env_logger::init();
    info!("Starting hacker-plsa");
    #[derive(Parser)]
    #[command(version = "0.1.0")]
    struct Args {
        #[arg(long)]
        verbose: bool,
        file: String,
    }
    let args = Args::parse();
    let res = parse_hacker_file(&args.file, args.verbose);
    if args.verbose {
        if !res.errors.is_empty() {
            println!("\n{}", "Errors:".red().bold());
            for e in &res.errors {
                println!(" {} {}", "✖".red(), e);
            }
            println!();
        } else {
            println!("{}", "No errors found.".green());
        }
        println!("System Deps: [{}]", res.deps.join(", "));
        println!("Custom Libs (Bytes): [{}]", res.libs.join(", "));
        println!("Rust Libs: [{}]", res.rust_libs.join(", "));
        println!("Python Libs: [{}]", res.python_libs.join(", "));
        println!("Java Libs: [{}]", res.java_libs.join(", "));
        let vars_str: Vec<String> = res.vars_dict.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
        println!("Vars: {{{}}}", vars_str.join(", "));
        let local_vars_str: Vec<String> = res.local_vars.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
        println!("Local Vars: {{{}}}", local_vars_str.join(", "));
        println!("Cmds: [{}]", res.cmds.join(", "));
        println!("Cmds with Vars: [{}]", res.cmds_with_vars.join(", "));
        println!("Separate Cmds: [{}]", res.cmds_separate.join(", "));
        println!("Includes: [{}]", res.includes.join(", "));
        println!("Binaries: [{}]", res.binaries.join(", "));
        let plugins_str: Vec<String> = res.plugins.iter().map(|p| format!("{{path: {}, super: {}}}", p.path, p.is_super)).collect();
        println!("Plugins: [{}]", plugins_str.join(", "));
        let functions_str: Vec<String> = res.functions.iter().map(|(k, f)| {
            let params_str: Vec<String> = f.params.iter().map(|p| format!("{}:{}={:?}", p.name, p.type_, p.default)).collect();
            format!("{}: params[{}] body[{}]", k, params_str.join(","), f.body.join(", "))
        }).collect();
        println!("Functions: {{{}}}", functions_str.join(", "));
        let config_str: Vec<String> = res.config_data.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
        println!("Config: {{{}}}", config_str.join(", "));
    }
    let json = serde_json::to_string(&res)?;
    println!("{}", json);
    Ok(())
}
