use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::env;
use std::os::unix::fs::PermissionsExt;

use nom::IResult;
use nom::bytes::complete::{tag, take_while1, take_until};
use nom::character::complete::space0;
use nom::combinator::{map, opt, recognize};
use nom::sequence::tuple;
use nom::branch::alt;

use crate::utils;

#[derive(Clone, Debug)]
pub struct Plugin {
    pub path: String,
    pub is_super: bool,
}

#[derive(Default, Debug)]
pub struct ParseResult {
    pub deps: HashMap<String, ()>,
    pub libs: HashMap<String, ()>,
    pub vars_dict: HashMap<String, String>,
    pub local_vars: HashMap<String, String>,
    pub cmds: Vec<String>,
    pub cmds_with_vars: Vec<String>,
    pub cmds_separate: Vec<String>,
    pub includes: Vec<String>,
    pub binaries: Vec<String>,
    pub plugins: Vec<Plugin>,
    pub functions: HashMap<String, Vec<String>>,
    pub errors: Vec<String>,
    pub config_data: HashMap<String, String>,
}

enum LineType {
    Dep(String),
    Lib(String),
    Cmd(String),
    CmdVars(String),
    CmdSeparate(String),
    Var(String, String),
    LocalVar(String, String),
    Plugin(String),
    Loop(String, String),
    Conditional(String, String),
    Background(String),
    Ignore,
}

pub fn parse_hacker_file(file_path: &str, verbose: bool) -> Result<ParseResult, Box<dyn std::error::Error>> {
    let mut res = ParseResult::default();
    let mut in_config = false;
    let mut in_comment = false;
    let mut in_function: Option<String> = None;
    let mut line_num: u32 = 0;
    let home = env::var("HOME").unwrap_or_default();
    let hacker_dir = PathBuf::from(home).join(utils::HACKER_DIR_SUFFIX);
    let mut console = io::stdout().lock();
    let file = match fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if verbose {
                writeln!(console, "File {} not found", file_path)?;
            }
            res.errors.push(format!("File {} not found", file_path));
            return Ok(res);
        }
        Err(e) => return Err(Box::new(e)),
    };
    let reader = BufReader::new(file);
    for line_slice in reader.lines() {
        let line_slice = line_slice?;
        line_num += 1;
        let line_trimmed = line_slice.trim();
        if line_trimmed.is_empty() {
            continue;
        }
        let mut line = line_trimmed.to_string();
        if line == "!!" {
            in_comment = !in_comment;
            continue;
        }
        if in_comment {
            continue;
        }
        let mut is_super = false;
        if line.starts_with('^') {
            is_super = true;
            line = line[1..].trim().to_string();
        }
        if line == "[" {
            if in_config {
                res.errors.push(format!("Line {}: Nested config section", line_num));
            }
            if in_function.is_some() {
                res.errors.push(format!("Line {}: Config in function", line_num));
            }
            in_config = true;
            continue;
        } else if line == "]" {
            if !in_config {
                res.errors.push(format!("Line {}: Closing ] without [", line_num));
            }
            in_config = false;
            continue;
        }
        if in_config {
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim().to_string();
                let value = line[eq_pos + 1..].trim().to_string();
                res.config_data.insert(key, value);
            }
            continue;
        }
        if line == ":" {
            if in_function.is_some() {
                in_function = None;
            } else {
                res.errors.push(format!("Line {}: Ending function without start", line_num));
            }
            continue;
        } else if line.starts_with(':') {
            let func_name = line[1..].trim().to_string();
            if func_name.is_empty() {
                res.errors.push(format!("Line {}: Empty function name", line_num));
                continue;
            }
            if in_function.is_some() {
                res.errors.push(format!("Line {}: Nested function", line_num));
            }
            res.functions.insert(func_name.clone(), Vec::new());
            in_function = Some(func_name);
            continue;
        } else if line.starts_with('.') {
            let func_name = line[1..].trim().to_string();
            if func_name.is_empty() {
                res.errors.push(format!("Line {}: Empty function call", line_num));
                continue;
            }
            if let Some(func_cmds) = res.functions.get(&func_name).map(|v| v.clone()) {
                let target = if let Some(ref f) = in_function {
                    res.functions.get_mut(f).unwrap()
                } else {
                    &mut res.cmds
                };
                target.extend(func_cmds);
            } else {
                res.errors.push(format!("Line {}: Unknown function {}", line_num, func_name));
            }
            continue;
        }
        if in_function.is_some() {
            if !line.starts_with(">") && !line.starts_with(">>") && !line.starts_with(">>>") && !line.starts_with("=") && !line.starts_with("?") && !line.starts_with("&") && !line.starts_with("!") && !line.starts_with("@") && !line.starts_with("$") && !line.starts_with("\\") {
                res.errors.push(format!("Line {}: Invalid in function", line_num));
                continue;
            }
        }
        let parse_result = alt((
            parse_dep,
            parse_lib,
            parse_cmd,
            parse_cmd_vars,
            parse_cmd_separate,
            parse_var,
            parse_local_var,
            parse_plugin,
            parse_loop,
            parse_conditional,
            parse_background,
            parse_ignore,
        ))(&line);
        match parse_result {
            Ok((_, line_type)) => {
                match line_type {
                    LineType::Dep(dep) => {
                        if in_function.is_some() {
                            res.errors.push(format!("Line {}: Deps not allowed in function", line_num));
                            continue;
                        }
                        if !dep.is_empty() {
                            res.deps.insert(dep, ());
                        } else {
                            res.errors.push(format!("Line {}: Empty system dependency", line_num));
                        }
                    }
                    LineType::Lib(lib) => {
                        if in_function.is_some() {
                            res.errors.push(format!("Line {}: Libs not allowed in function", line_num));
                            continue;
                        }
                        if lib.is_empty() {
                            res.errors.push(format!("Line {}: Empty library/include", line_num));
                            continue;
                        }
                        let lib_dir = hacker_dir.join("libs").join(&lib);
                        let lib_hacker_path = lib_dir.join("main.hacker");
                        let lib_bin_path = hacker_dir.join("libs").join(&lib);
                        if lib_hacker_path.exists() {
                            res.includes.push(lib.clone());
                            let sub = parse_hacker_file(lib_hacker_path.to_str().unwrap(), verbose)?;
                            utils::merge_hash_maps(&mut res.deps, sub.deps);
                            utils::merge_hash_maps(&mut res.libs, sub.libs);
                            utils::merge_string_hash_maps(&mut res.vars_dict, sub.vars_dict);
                            utils::merge_string_hash_maps(&mut res.local_vars, sub.local_vars);
                            res.cmds.extend(sub.cmds);
                            res.cmds_with_vars.extend(sub.cmds_with_vars);
                            res.cmds_separate.extend(sub.cmds_separate);
                            res.includes.extend(sub.includes);
                            res.binaries.extend(sub.binaries);
                            res.plugins.extend(sub.plugins);
                            utils::merge_function_maps(&mut res.functions, sub.functions);
                            for sub_err in sub.errors {
                                res.errors.push(format!("In {}: {}", lib, sub_err));
                            }
                        }
                        if let Ok(meta) = lib_bin_path.metadata() {
                            if meta.permissions().mode() & 0o111 != 0 {
                                res.binaries.push(lib_bin_path.to_str().unwrap().to_string());
                            } else {
                                res.libs.insert(lib, ());
                            }
                        } else {
                            res.libs.insert(lib, ());
                        }
                    }
                    LineType::Cmd(mut cmd) => {
                        if is_super {
                            cmd = format!("sudo {}", cmd);
                        }
                        if !cmd.is_empty() {
                            let target = if let Some(ref f) = in_function {
                                res.functions.get_mut(f).unwrap()
                            } else {
                                &mut res.cmds
                            };
                            target.push(cmd);
                        } else {
                            res.errors.push(format!("Line {}: Empty command", line_num));
                        }
                    }
                    LineType::CmdVars(mut cmd) => {
                        if is_super {
                            cmd = format!("sudo {}", cmd);
                        }
                        if !cmd.is_empty() {
                            let target = if let Some(ref f) = in_function {
                                res.functions.get_mut(f).unwrap()
                            } else {
                                &mut res.cmds_with_vars
                            };
                            target.push(cmd);
                        } else {
                            res.errors.push(format!("Line {}: Empty command with vars", line_num));
                        }
                    }
                    LineType::CmdSeparate(mut cmd) => {
                        if is_super {
                            cmd = format!("sudo {}", cmd);
                        }
                        if !cmd.is_empty() {
                            let target = if let Some(ref f) = in_function {
                                res.functions.get_mut(f).unwrap()
                            } else {
                                &mut res.cmds_separate
                            };
                            target.push(cmd);
                        } else {
                            res.errors.push(format!("Line {}: Empty separate file command", line_num));
                        }
                    }
                    LineType::Var(key, value) => {
                        if !key.is_empty() && !value.is_empty() {
                            res.vars_dict.insert(key, value);
                        } else {
                            res.errors.push(format!("Line {}: Invalid variable", line_num));
                        }
                    }
                    LineType::LocalVar(key, value) => {
                        if !key.is_empty() && !value.is_empty() {
                            res.local_vars.insert(key, value);
                        } else {
                            res.errors.push(format!("Line {}: Invalid local variable", line_num));
                        }
                    }
                    LineType::Plugin(plugin_name) => {
                        if plugin_name.is_empty() {
                            res.errors.push(format!("Line {}: Empty plugin name", line_num));
                            continue;
                        }
                        let plugin_dir = hacker_dir.join("plugins").join(&plugin_name);
                        if let Ok(meta) = plugin_dir.metadata() {
                            if meta.permissions().mode() & 0o111 != 0 {
                                res.plugins.push(Plugin {
                                    path: plugin_dir.to_str().unwrap().to_string(),
                                                 is_super,
                                });
                            } else {
                                res.errors.push(format!("Line {}: Plugin {} not found or not executable", line_num, plugin_name));
                            }
                        } else {
                            res.errors.push(format!("Line {}: Plugin {} not found or not executable", line_num, plugin_name));
                        }
                    }
                    LineType::Loop(num_str, cmd_part) => {
                        let num: i32 = match num_str.parse() {
                            Ok(n) => n,
                            Err(_) => {
                                res.errors.push(format!("Line {}: Invalid loop count", line_num));
                                continue;
                            }
                        };
                        if num < 0 {
                            res.errors.push(format!("Line {}: Negative loop count", line_num));
                            continue;
                        }
                        if cmd_part.is_empty() {
                            res.errors.push(format!("Line {}: Empty loop command", line_num));
                            continue;
                        }
                        let mut cmd_base = cmd_part;
                        if is_super {
                            cmd_base = format!("sudo {}", cmd_base);
                        }
                        let target = if let Some(ref f) = in_function {
                            res.functions.get_mut(f).unwrap()
                        } else {
                            &mut res.cmds
                        };
                        for _ in 0..num {
                            target.push(cmd_base.clone());
                        }
                    }
                    LineType::Conditional(condition, cmd_part) => {
                        if condition.is_empty() || cmd_part.is_empty() {
                            res.errors.push(format!("Line {}: Invalid conditional", line_num));
                            continue;
                        }
                        let mut cmd = cmd_part;
                        if is_super {
                            cmd = format!("sudo {}", cmd);
                        }
                        let if_cmd = format!("if {}; then {}; fi", condition, cmd);
                        let target = if let Some(ref f) = in_function {
                            res.functions.get_mut(f).unwrap()
                        } else {
                            &mut res.cmds
                        };
                        target.push(if_cmd);
                    }
                    LineType::Background(cmd_part) => {
                        if cmd_part.is_empty() {
                            res.errors.push(format!("Line {}: Empty background command", line_num));
                            continue;
                        }
                        let mut cmd = format!("{} &", cmd_part);
                        if is_super {
                            cmd = format!("sudo {}", cmd);
                        }
                        let target = if let Some(ref f) = in_function {
                            res.functions.get_mut(f).unwrap()
                        } else {
                            &mut res.cmds
                        };
                        target.push(cmd);
                    }
                    LineType::Ignore => {}
                }
            }
            Err(_) => {
                res.errors.push(format!("Line {}: Invalid syntax", line_num));
            }
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
    if verbose {
        if !res.errors.is_empty() {
            writeln!(console, "\n\x1b[31m\x1b[1mErrors:\x1b[0m")?;
            for e in &res.errors {
                writeln!(console, " \x1b[31m✖ \x1b[0m{}", e)?;
            }
            writeln!(console, "")?;
        } else {
            writeln!(console, "\x1b[32mNo errors found.\x1b[0m")?;
        }
        let dep_keys: Vec<&String> = res.deps.keys().collect();
        writeln!(console, "System Deps: {:?}", dep_keys)?;
        let lib_keys: Vec<&String> = res.libs.keys().collect();
        writeln!(console, "Custom Libs: {:?}", lib_keys)?;
        writeln!(console, "Vars: {:?}", res.vars_dict)?;
        writeln!(console, "Local Vars: {:?}", res.local_vars)?;
        writeln!(console, "Cmds: {:?}", res.cmds)?;
        writeln!(console, "Cmds with Vars: {:?}", res.cmds_with_vars)?;
        writeln!(console, "Separate Cmds: {:?}", res.cmds_separate)?;
        writeln!(console, "Includes: {:?}", res.includes)?;
        writeln!(console, "Binaries: {:?}", res.binaries)?;
        writeln!(console, "Plugins: {:?}", res.plugins)?;
        writeln!(console, "Functions: {:?}", res.functions)?;
        writeln!(console, "Config: {:?}", res.config_data)?;
    }
    Ok(res)
}

fn is_var_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn parse_cmd_part(input: &str) -> IResult<&str, String> {
    let (input, cmd) = alt((
        map(tuple((take_until("!"), tag("!"))), |(c, _): (&str, &str)| c.to_string()),
                            map(recognize(take_while1(|_| true)), |c: &str| c.to_string()),
    ))(input)?;
    Ok((input, cmd.trim().to_string()))
}

fn parse_dep(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag("//"), space0))(input)?;
    let dep = input.trim().to_string();
    Ok((input, LineType::Dep(dep)))
}

fn parse_lib(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag("#"), space0))(input)?;
    let lib = input.trim().to_string();
    Ok((input, LineType::Lib(lib)))
}

fn parse_cmd(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag(">"), space0, opt(tag(">")), opt(tag(">"))))(input)?;
    if input.starts_with(">") {
        return Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag)));
    }
    let (input, cmd) = parse_cmd_part(input)?;
    Ok((input, LineType::Cmd(cmd)))
}

fn parse_cmd_vars(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag(">>"), space0))(input)?;
    let (input, cmd) = parse_cmd_part(input)?;
    Ok((input, LineType::CmdVars(cmd)))
}

fn parse_cmd_separate(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag(">>>"), space0))(input)?;
    let (input, cmd) = parse_cmd_part(input)?;
    Ok((input, LineType::CmdSeparate(cmd)))
}

fn parse_var(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tag("@")(input)?;
    let (input, key) = take_while1(is_var_char)(input)?;
    let (input, _) = tuple((space0, tag("="), space0))(input)?;
    let value = input.trim().to_string();
    Ok((input, LineType::Var(key.to_string(), value)))
}

fn parse_local_var(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tag("$")(input)?;
    let (input, key) = take_while1(is_var_char)(input)?;
    let (input, _) = tuple((space0, tag("="), space0))(input)?;
    let value = input.trim().to_string();
    Ok((input, LineType::LocalVar(key.to_string(), value)))
}

fn parse_plugin(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag("\\"), space0))(input)?;
    let plugin_name = input.trim().to_string();
    Ok((input, LineType::Plugin(plugin_name)))
}

fn parse_loop(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag("="), space0))(input)?;
    let (input, num) = take_while1(|c: char| c.is_digit(10))(input)?;
    let (input, _) = tuple((space0, tag(">"), space0))(input)?;
    let (input, cmd) = parse_cmd_part(input)?;
    Ok((input, LineType::Loop(num.to_string(), cmd)))
}

fn parse_conditional(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag("?"), space0))(input)?;
    let (input, condition) = take_until(">")(input)?;
    let (input, _) = tag(">")(input)?;
    let (input, _) = space0(input)?;
    let (input, cmd) = parse_cmd_part(input)?;
    Ok((input, LineType::Conditional(condition.trim().to_string(), cmd)))
}

fn parse_background(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tuple((tag("&"), space0))(input)?;
    let (input, cmd) = parse_cmd_part(input)?;
    Ok((input, LineType::Background(cmd)))
}

fn parse_ignore(input: &str) -> IResult<&str, LineType> {
    let (input, _) = tag("!")(input)?;
    Ok((input, LineType::Ignore))
}
