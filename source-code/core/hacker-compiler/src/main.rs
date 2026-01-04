use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use std::collections::HashMap;
use subprocess::{Exec, PopenError};
use std::os::unix::fs::PermissionsExt;
use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::passes::PassManager;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::OptimizationLevel;
use inkwell::AddressSpace;
const HACKER_DIR: &str = "~/.hackeros/hacker-lang";
fn expand_home(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = env::var_os("HOME") {
            return path.replacen("~", home.to_str().unwrap_or(""), 1);
        }
    }
    path.to_string()
}
#[derive(Debug, Clone)]
struct Plugin {
    name: String,
    is_super: bool,
}
#[derive(Debug)]
struct ParseResult {
    deps: Vec<String>,
    libs: Vec<String>,
    rust_libs: Vec<String>,
    python_libs: Vec<String>,
    java_libs: Vec<String>,
    vars: Vec<(String, String)>,
    local_vars: Vec<(String, String)>,
    cmds: Vec<String>,
    cmds_with_vars: Vec<String>,
    cmds_separate: Vec<String>,
    includes: Vec<String>,
    binaries: HashMap<String, String>, // name to path
    plugins: Vec<Plugin>,
    functions: HashMap<String, Vec<String>>,
    errors: Vec<String>,
    config: HashMap<String, String>,
    built_in_libs: Vec<String>,
    translator_blocks: Vec<(String, String)>, // lang, code
}
fn parse_hacker_file(path: &Path, verbose: bool, bytes_mode: bool) -> io::Result<ParseResult> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().collect::<io::Result<_>>()?;
    let mut deps = Vec::new();
    let mut libs = Vec::new();
    let mut rust_libs = Vec::new();
    let mut python_libs = Vec::new();
    let mut java_libs = Vec::new();
    let mut vars = Vec::new();
    let mut local_vars = Vec::new();
    let mut cmds = Vec::new();
    let mut cmds_with_vars = Vec::new();
    let mut cmds_separate = Vec::new();
    let mut includes = Vec::new();
    let mut binaries: HashMap<String, String> = HashMap::new();
    let mut plugins = Vec::new();
    let mut functions: HashMap<String, Vec<String>> = HashMap::new();
    let mut errors = Vec::new();
    let mut config = HashMap::new();
    let mut built_in_libs = Vec::new();
    let mut translator_blocks = Vec::new();
    let mut translator_enabled = false;
    let mut in_config = false;
    let mut in_comment = false;
    let mut in_function: Option<String> = None;
    let mut i: usize = 0;
    while i < lines.len() {
        let original_line = &lines[i];
        let mut line = original_line.trim().to_string();
        if line.is_empty() {
            i += 1;
            continue;
        }
        if line == "!!" {
            in_comment = !in_comment;
            i += 1;
            continue;
        }
        if in_comment {
            i += 1;
            continue;
        }
        let mut is_super = false;
        if line.starts_with('^') {
            is_super = true;
            line = line[1..].trim().to_string();
        }
        if line == "[" {
            if in_config {
                errors.push("Nested config section".to_string());
            }
            if in_function.is_some() {
                errors.push("Config in function".to_string());
            }
            in_config = true;
            i += 1;
            continue;
        } else if line == "]" {
            if !in_config {
                errors.push("Closing ] without [".to_string());
            }
            in_config = false;
            i += 1;
            continue;
        }
        if in_config {
            if let Some(eq_idx) = line.find('=') {
                let key = line[..eq_idx].trim().to_string();
                let value = line[eq_idx + 1..].trim().to_string();
                config.insert(key, value);
            }
            i += 1;
            continue;
        }
        if line == ":" {
            if in_function.is_some() {
                in_function = None;
            } else {
                errors.push("Ending function without start".to_string());
            }
            i += 1;
            continue;
        } else if line.starts_with(":") {
            let func_name = line[1..].trim().to_string();
            if func_name.is_empty() {
                errors.push("Empty function name".to_string());
                i += 1;
                continue;
            }
            if in_function.is_some() {
                errors.push("Nested function".to_string());
            }
            functions.insert(func_name.clone(), Vec::new());
            in_function = Some(func_name);
            i += 1;
            continue;
        } else if line.starts_with(".") {
            let func_name = line[1..].trim().to_string();
            if func_name.is_empty() {
                errors.push("Empty function call".to_string());
                i += 1;
                continue;
            }
            if let Some(f_cmds) = functions.get(&func_name).cloned() {
                let target = if let Some(ref f) = in_function {
                    functions.get_mut(f).unwrap()
                } else {
                    &mut cmds
                };
                for c in f_cmds {
                    target.push(c);
                }
            } else {
                errors.push(format!("Unknown function {}", func_name));
            }
            i += 1;
            continue;
        }
        if in_function.is_some() {
            let valid_prefix = line.starts_with(">") || line.starts_with(">>") || line.starts_with(">>>") ||
            line.starts_with("=") || line.starts_with("?") || line.starts_with("&") ||
            line.starts_with("!") || line.starts_with("@") || line.starts_with("$") || line.starts_with("\\");
            if !valid_prefix {
                errors.push("Invalid in function".to_string());
                i += 1;
                continue;
            }
        }
        if line.starts_with("//") {
            if in_function.is_some() {
                errors.push("Deps not allowed in function".to_string());
                i += 1;
                continue;
            }
            let dep = line[2..].trim().to_string();
            if !dep.is_empty() {
                deps.push(dep);
            } else {
                errors.push("Empty system dependency".to_string());
            }
        } else if translator_enabled && line.starts_with("|> translator:") {
            if in_function.is_some() {
                errors.push("Translator blocks not allowed in function".to_string());
                i += 1;
                continue;
            }
            if in_config {
                errors.push("Translator blocks not allowed in config".to_string());
                i += 1;
                continue;
            }
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let header = parts[1].trim();
                let lang_opt = header.split('(').next().map(|s| s.trim().to_string());
                if let Some(lang) = lang_opt {
                    if !lang.is_empty() {
                        let mut code = String::new();
                        i += 1;
                        let mut depth = 1;
                        while i < lines.len() && depth > 0 {
                            let code_line = &lines[i];
                            code.push_str(code_line);
                            code.push('\n');
                            for c in code_line.chars() {
                                if c == '(' {
                                    depth += 1;
                                } else if c == ')' {
                                    depth -= 1;
                                }
                            }
                            i += 1;
                        }
                        if depth == 0 {
                            let code_trimmed = code.trim().to_string();
                            if verbose {
                                println!("Extracted {} block", lang);
                            }
                            translator_blocks.push((lang, code_trimmed));
                            cmds.push(format!("__TRANSLATOR_BLOCK__{}", translator_blocks.len() - 1));
                            continue;
                        } else {
                            if verbose {
                                eprintln!("Unclosed block for {}", lang);
                            }
                            errors.push(format!("Unclosed block for {}", lang));
                        }
                        continue;
                    }
                }
            }
            errors.push("Invalid translator block syntax".to_string());
        } else if line.starts_with("#") {
            if in_function.is_some() {
                errors.push("Libs not allowed in function".to_string());
                i += 1;
                continue;
            }
            let full_lib = line[1..].trim().to_string();
            if full_lib.starts_with(">") {
                let lib_name = full_lib[1..].trim().to_string();
                if !lib_name.is_empty() {
                    built_in_libs.push(lib_name.clone());
                    if lib_name == "translator" {
                        translator_enabled = true;
                    }
                    i += 1;
                    continue;
                } else {
                    errors.push("Empty built-in library name".to_string());
                }
            } else {
                let (prefix, lib_name) = if let Some(colon_idx) = full_lib.find(':') {
                    (full_lib[..colon_idx].trim().to_string(), full_lib[colon_idx + 1..].trim().to_string())
                } else {
                    ("bytes".to_string(), full_lib)
                };
                if lib_name.is_empty() {
                    errors.push("Empty library name after prefix".to_string());
                    i += 1;
                    continue;
                }
                match prefix.as_str() {
                    "rust" => {
                        rust_libs.push(lib_name);
                    }
                    "python" => {
                        python_libs.push(lib_name);
                    }
                    "java" => {
                        java_libs.push(lib_name);
                    }
                    "bytes" => {
                        let lib_dir = expand_home(&format!("{HACKER_DIR}/libs/{}", lib_name));
                        let lib_hacker_path = format!("{}/main.hacker", lib_dir);
                        let lib_bin_path = lib_dir;
                        if Path::new(&lib_hacker_path).exists() {
                            includes.push(lib_name.clone());
                            let sub = parse_hacker_file(Path::new(&lib_hacker_path), verbose, bytes_mode)?;
                            deps.extend(sub.deps);
                            libs.extend(sub.libs);
                            rust_libs.extend(sub.rust_libs);
                            python_libs.extend(sub.python_libs);
                            java_libs.extend(sub.java_libs);
                            vars.extend(sub.vars);
                            local_vars.extend(sub.local_vars);
                            cmds.extend(sub.cmds);
                            cmds_with_vars.extend(sub.cmds_with_vars);
                            cmds_separate.extend(sub.cmds_separate);
                            includes.extend(sub.includes);
                            for (k, v) in sub.binaries {
                                if binaries.contains_key(&k) {
                                    errors.push(format!("Duplicate binary name {}", k));
                                } else {
                                    binaries.insert(k, v);
                                }
                            }
                            plugins.extend(sub.plugins);
                            functions.extend(sub.functions);
                            config.extend(sub.config);
                            for err in sub.errors {
                                errors.push(format!("In {}: {}", lib_name, err));
                            }
                        }
                        if Path::new(&lib_bin_path).exists() {
                            let metadata = Path::new(&lib_bin_path).metadata()?;
                            if metadata.permissions().mode() & 0o111 != 0 {
                                if bytes_mode {
                                    println!("Embedding library binary: {}", lib_bin_path);
                                }
                                if binaries.contains_key(&lib_name) {
                                    errors.push(format!("Duplicate binary name {}", lib_name));
                                } else {
                                    binaries.insert(lib_name, lib_bin_path);
                                }
                            } else {
                                libs.push(lib_name);
                            }
                        } else {
                            libs.push(lib_name);
                        }
                    }
                    _ => {
                        errors.push(format!("Unknown library prefix {}", prefix));
                    }
                }
            }
        } else if line.starts_with(">>>") {
            let cmd_part_str: String = line[3..].split('!').next().unwrap_or("").trim().to_string();
            let mut cmd = cmd_part_str.clone();
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            if cmd.is_empty() {
                errors.push("Empty separate command".to_string());
            } else {
                let target = if let Some(ref f) = in_function {
                    functions.get_mut(f).unwrap()
                } else {
                    &mut cmds_separate
                };
                target.push(cmd);
            }
        } else if line.starts_with(">>") {
            let cmd_part_str: String = line[2..].split('!').next().unwrap_or("").trim().to_string();
            let mut cmd = cmd_part_str.clone();
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            if cmd.is_empty() {
                errors.push("Empty command with vars".to_string());
            } else {
                let target = if let Some(ref f) = in_function {
                    functions.get_mut(f).unwrap()
                } else {
                    &mut cmds_with_vars
                };
                target.push(cmd);
            }
        } else if line.starts_with(">") {
            let cmd_part_str: String = line[1..].split('!').next().unwrap_or("").trim().to_string();
            let mut cmd = cmd_part_str.clone();
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            if cmd.is_empty() {
                errors.push("Empty command".to_string());
            } else {
                let target = if let Some(ref f) = in_function {
                    functions.get_mut(f).unwrap()
                } else {
                    &mut cmds
                };
                target.push(cmd);
            }
        } else if line.starts_with("@") {
            if let Some(eq_idx) = line[1..].find('=') {
                let var = line[1..1 + eq_idx].trim().to_string();
                let value = line[1 + eq_idx + 1..].trim().to_string();
                if var.is_empty() || value.is_empty() {
                    errors.push("Invalid variable".to_string());
                } else {
                    vars.push((var, value));
                }
            } else {
                errors.push("Invalid @ syntax".to_string());
            }
        } else if line.starts_with("$") {
            if let Some(eq_idx) = line[1..].find('=') {
                let var = line[1..1 + eq_idx].trim().to_string();
                let value = line[1 + eq_idx + 1..].trim().to_string();
                if var.is_empty() || value.is_empty() {
                    errors.push("Invalid local variable".to_string());
                } else {
                    local_vars.push((var, value));
                }
            } else {
                errors.push("Invalid $ syntax".to_string());
            }
        } else if line.starts_with("\\") {
            let plugin_name = line[1..].trim().to_string();
            if plugin_name.is_empty() {
                errors.push("Empty plugin name".to_string());
            } else {
                let plugin_path = expand_home(&format!("{}/plugins/{}", HACKER_DIR, plugin_name));
                if Path::new(&plugin_path).exists() {
                    let metadata = Path::new(&plugin_path).metadata()?;
                    if metadata.permissions().mode() & 0o111 != 0 {
                        plugins.push(Plugin { name: plugin_name.clone(), is_super });
                        if verbose {
                            println!("Loaded plugin: {}", plugin_name);
                        }
                    } else {
                        errors.push(format!("Plugin {} not found or not executable", plugin_name));
                    }
                } else {
                    errors.push(format!("Plugin {} not found or not executable", plugin_name));
                }
            }
        } else if line.starts_with("=") {
            let parts: Vec<String> = line[1..].split('>').map(|s| s.trim().to_string()).collect();
            if parts.len() == 2 {
                if let Ok(num) = parts[0].parse::<usize>() {
                    let cmd_part_str = parts[1].split('!').next().unwrap_or("").trim().to_string();
                    let mut cmd = cmd_part_str.clone();
                    if is_super {
                        cmd = format!("sudo {}", cmd);
                    }
                    if cmd.is_empty() {
                        errors.push("Empty loop command".to_string());
                    } else {
                        let target = if let Some(ref f) = in_function {
                            functions.get_mut(f).unwrap()
                        } else {
                            &mut cmds
                        };
                        for _ in 0..num {
                            target.push(cmd.clone());
                        }
                    }
                } else {
                    errors.push("Invalid loop count".to_string());
                }
            } else {
                errors.push("Invalid loop syntax".to_string());
            }
        } else if line.starts_with("?") {
            let parts: Vec<String> = line[1..].split('>').map(|s| s.trim().to_string()).collect();
            if parts.len() == 2 {
                let condition = parts[0].clone();
                let cmd_part_str = parts[1].split('!').next().unwrap_or("").trim().to_string();
                let mut cmd = cmd_part_str.clone();
                if is_super {
                    cmd = format!("sudo {}", cmd);
                }
                if condition.is_empty() || cmd.is_empty() {
                    errors.push("Invalid conditional".to_string());
                } else {
                    let if_cmd = format!("if {}; then {}; fi", condition, cmd);
                    let target = if let Some(ref f) = in_function {
                        functions.get_mut(f).unwrap()
                    } else {
                        &mut cmds
                    };
                    target.push(if_cmd);
                }
            } else {
                errors.push("Invalid conditional syntax".to_string());
            }
        } else if line.starts_with("&") {
            let cmd_part_str = line[1..].split('!').next().unwrap_or("").trim().to_string();
            let mut cmd = format!("{} &", cmd_part_str);
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            if cmd_part_str.is_empty() {
                errors.push("Empty background command".to_string());
            } else {
                let target = if let Some(ref f) = in_function {
                    functions.get_mut(f).unwrap()
                } else {
                    &mut cmds
                };
                target.push(cmd);
            }
        } else if line.starts_with("!") {
            // Comment, ignore
        } else {
            errors.push("Invalid syntax".to_string());
        }
        i += 1;
    }
    if in_config {
        errors.push("Unclosed config section".to_string());
    }
    if in_comment {
        errors.push("Unclosed comment block".to_string());
    }
    if in_function.is_some() {
        errors.push("Unclosed function block".to_string());
    }
    if verbose {
        println!("System Deps: {:?}", deps);
        println!("Custom Libs: {:?}", libs);
        println!("Rust Libs: {:?}", rust_libs);
        println!("Python Libs: {:?}", python_libs);
        println!("Java Libs: {:?}", java_libs);
        println!("Vars: {:?}", vars);
        println!("Local Vars: {:?}", local_vars);
        println!("Cmds (direct): {:?}", cmds);
        println!("Cmds (with vars): {:?}", cmds_with_vars);
        println!("Cmds (separate): {:?}", cmds_separate);
        println!("Includes: {:?}", includes);
        println!("Binaries: {:?}", binaries);
        println!("Plugins: {:?}", plugins);
        println!("Functions: {:?}", functions);
        println!("Config: {:?}", config);
        println!("Built-in Libs: {:?}", built_in_libs);
        println!("Translator Blocks: {:?}", translator_blocks);
        if !errors.is_empty() {
            println!("Errors: {:?}", errors);
        }
    }
    Ok(ParseResult {
        deps,
       libs,
       rust_libs,
       python_libs,
       java_libs,
       vars,
       local_vars,
       cmds,
       cmds_with_vars,
       cmds_separate,
       includes,
       binaries,
       plugins,
       functions,
       errors,
       config,
       built_in_libs,
       translator_blocks,
    })
}
fn generate_check_cmd(dep: &str) -> String {
    if dep == "sudo" {
        return String::new();
    }
    format!("command -v {} &> /dev/null || (sudo apt update && sudo apt install -y {})", dep, dep)
}
fn generate_exec_script(lang: &str, code: &str, verbose: bool) -> String {
    let code_b64 = BASE64_STANDARD.encode(code);
    let mut script = String::new();
    if verbose {
        script.push_str("echo \"Executing ");
        script.push_str(lang);
        script.push_str(" code\"\n");
    }
    match lang {
        "rust" => {
            script.push_str("dir=$(mktemp -d)\n");
            if verbose {
                script.push_str("echo \"Temp dir: $dir\"\n");
            }
            script.push_str(&format!("echo '{}' | base64 -d > $dir/main.rs\n", code_b64));
            script.push_str("rustc $dir/main.rs -o $dir/a.out\n");
            script.push_str("if [ $? -eq 0 ]; then\n $dir/a.out\nfi\n");
            script.push_str("rm -rf $dir\n");
        }
        "java" => {
            script.push_str("dir=$(mktemp -d)\n");
            if verbose {
                script.push_str("echo \"Temp dir: $dir\"\n");
            }
            script.push_str(&format!("echo '{}' | base64 -d > $dir/Main.java\n", code_b64));
            script.push_str("javac $dir/Main.java\n");
            script.push_str("if [ $? -eq 0 ]; then\n java -cp $dir Main\nfi\n");
            script.push_str("rm -rf $dir\n");
        }
        "python" => {
            script.push_str(&format!("python3 -c \"$(echo '{}' | base64 -d)\"\n", code_b64));
        }
        "go" => {
            script.push_str("dir=$(mktemp -d)\n");
            if verbose {
                script.push_str("echo \"Temp dir: $dir\"\n");
            }
            script.push_str(&format!("echo '{}' | base64 -d > $dir/main.go\n", code_b64));
            script.push_str("go run $dir/main.go\n");
            script.push_str("rm -rf $dir\n");
        }
        "c" => {
            script.push_str("dir=$(mktemp -d)\n");
            if verbose {
                script.push_str("echo \"Temp dir: $dir\"\n");
            }
            script.push_str(&format!("echo '{}' | base64 -d > $dir/main.c\n", code_b64));
            script.push_str("gcc $(pkg-config --cflags dpdk) $dir/main.c $(pkg-config --libs dpdk) -o $dir/a.out\n");
            script.push_str("if [ $? -eq 0 ]; then\n $dir/a.out\nfi\n");
            script.push_str("rm -rf $dir\n");
        }
        _ => {
            script.push_str(&format!("echo \"Unsupported language: {}\"\n", lang));
        }
    }
    script
}
fn process_binary_cmd(cmd: &mut String, binary_data: &HashMap<String, Vec<u8>>) {
    for (name, data) in binary_data {
        if cmd.starts_with(name.as_str()) && (cmd.len() == name.len() || cmd.as_bytes()[name.len()] == b' ') {
            let args = if cmd.len() > name.len() { &cmd[name.len() + 1..] } else { "" };
            let b64 = BASE64_STANDARD.encode(data);
            let new_cmd = format!("temp=$(mktemp /tmp/hacker_bin.XXXXXX); echo '{}' | base64 -d > $temp && chmod +x $temp && $temp {} && rm $temp", b64, args);
            *cmd = new_cmd;
            break;
        }
    }
}
fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut bytes_mode = false;
    let mut verbose = false;
    let mut input_path_str = String::new();
    let mut output_path_str = String::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bytes" => bytes_mode = true,
            "--verbose" => verbose = true,
            _ => {
                if input_path_str.is_empty() {
                    input_path_str = args[i].clone();
                } else if output_path_str.is_empty() {
                    output_path_str = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if input_path_str.is_empty() || output_path_str.is_empty() {
        eprintln!("Usage: hacker-compiler <input.hacker> <output> [--verbose] [--bytes]");
        process::exit(1);
    }
    let input_path = Path::new(&input_path_str);
    let output_path = Path::new(&output_path_str);
    let mut parse_result = parse_hacker_file(input_path, verbose, bytes_mode)?;
    if parse_result.config.is_empty() {
        let config_path = Path::new(".hacker-config");
        if config_path.exists() {
            let content = fs::read_to_string(config_path)?;
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with("!") {
                    continue;
                }
                if let Some(eq_idx) = line.find('=') {
                    let key = line[..eq_idx].trim().to_string();
                    let value = line[eq_idx + 1..].trim().to_string();
                    parse_result.config.insert(key, value);
                }
            }
        }
    }
    let mut deps = parse_result.deps.clone();
    if !parse_result.rust_libs.is_empty() {
        if !deps.contains(&"cargo".to_string()) {
            deps.push("cargo".to_string());
        }
    }
    if !parse_result.python_libs.is_empty() {
        if !deps.contains(&"python3-pip".to_string()) {
            deps.push("python3-pip".to_string());
        }
    }
    if !parse_result.java_libs.is_empty() {
        if !deps.contains(&"maven".to_string()) {
            deps.push("maven".to_string());
        }
    }
    for (lang, _) in &parse_result.translator_blocks {
        let dep_to_add = match lang.as_str() {
            "rust" => Some("rustc"),
            "java" => Some("default-jdk"),
            "python" => Some("python3"),
            "go" => Some("golang-go"),
            "c" => {
                if !deps.contains(&"gcc".to_string()) {
                    deps.push("gcc".to_string());
                }
                Some("libdpdk-dev")
            }
            _ => None,
        };
        if let Some(d) = dep_to_add {
            if !deps.contains(&d.to_string()) {
                deps.push(d.to_string());
            }
        } else {
            parse_result.errors.push(format!("Unsupported translator language: {}", lang));
        }
    }
    parse_result.deps = deps;
    let ParseResult {
        deps,
        libs: _,
        rust_libs,
        python_libs,
        java_libs,
        vars,
        local_vars,
        cmds,
        cmds_with_vars,
        cmds_separate,
        includes: _,
        binaries,
        plugins,
        functions: _,
        errors,
        config: _,
        built_in_libs: _,
        translator_blocks,
    } = parse_result;
    if !errors.is_empty() {
        eprintln!("\x1b[31m\x1b[1mErrors:\x1b[0m");
        for err in errors {
            eprintln!(" \x1b[31m✖ \x1b[0m{}", err);
        }
        eprintln!();
        process::exit(1);
    }
    let mut binary_data: HashMap<String, Vec<u8>> = HashMap::new();
    for (name, path) in binaries {
        let data = fs::read(path)?;
        binary_data.insert(name, data);
    }
    // Substitute local vars in all command types
    let mut direct_cmds = cmds;
    direct_cmds.extend(cmds_with_vars);
    let mut substituted_direct: Vec<String> = direct_cmds.into_iter().map(|cmd| {
        let mut sub_cmd = cmd;
        for (k, v) in &local_vars {
            sub_cmd = sub_cmd.replace(&format!("${}", k), v);
        }
        sub_cmd
    }).collect();
    let mut substituted_separate: Vec<String> = cmds_separate.into_iter().map(|cmd| {
        let mut sub_cmd = cmd;
        for (k, v) in &local_vars {
            sub_cmd = sub_cmd.replace(&format!("${}", k), v);
        }
        sub_cmd
    }).collect();
    // Process binaries and translator blocks in commands
    for cmd in substituted_direct.iter_mut() {
        process_binary_cmd(cmd, &binary_data);
        if cmd.starts_with("__TRANSLATOR_BLOCK__") {
            let idx: usize = cmd[20..].parse().expect("Invalid block index");
            let (lang, code) = &translator_blocks[idx];
            let exec_script = generate_exec_script(lang, code, verbose);
            *cmd = exec_script;
        }
    }
    for cmd in substituted_separate.iter_mut() {
        process_binary_cmd(cmd, &binary_data);
        // Assume no translator blocks in separate
    }
    // Build final command list
    let mut final_cmds: Vec<String> = Vec::new();
    for dep in deps {
        let check = generate_check_cmd(&dep);
        if !check.is_empty() {
            final_cmds.push(check);
        }
    }
    // Add install commands for language libs
    for lib in rust_libs {
        final_cmds.push(format!("cargo install {}", lib));
    }
    for lib in python_libs {
        final_cmds.push(format!("pip3 install --user {}", lib));
    }
    for lib in java_libs {
        final_cmds.push(format!("mvn dependency:get -Dartifact={}", lib));
    }
    final_cmds.extend(substituted_direct);
    final_cmds.extend(substituted_separate);
    // Plugins
    for plugin in plugins {
        let plugin_path = expand_home(&format!("{}/plugins/{}", HACKER_DIR, plugin.name));
        let plugin_data = fs::read(&plugin_path)?;
        let encoded = BASE64_STANDARD.encode(&plugin_data);
        let mut run_cmd = "$temp".to_string();
        if plugin.is_super {
            run_cmd = format!("sudo {}", run_cmd);
        }
        let extract_cmd = format!("temp=$(mktemp /tmp/hacker_plugin.XXXXXX); echo '{}' | base64 -d > $temp && chmod +x $temp && {} &", encoded, run_cmd);
        final_cmds.push(extract_cmd);
    }
    // LLVM/Inkwell setup
    let context = Context::create();
    let module = context.create_module("hacker_module");
    let builder = context.create_builder();
    let i32_type = context.i32_type();
    let i8_type = context.i8_type();
    let i8_ptr_type = context.ptr_type(AddressSpace::default());
    // Declare external functions
    let system_type = i32_type.fn_type(&[i8_ptr_type.into()], false);
    let system_fn = module.add_function("system", system_type, Some(Linkage::External));
    let putenv_type = i32_type.fn_type(&[i8_ptr_type.into()], false);
    let putenv_fn = module.add_function("putenv", putenv_type, Some(Linkage::External));
    // Main function
    let main_type = i32_type.fn_type(&[], false);
    let main_fn = module.add_function("main", main_type, None);
    let entry_block = context.append_basic_block(main_fn, "entry");
    builder.position_at_end(entry_block);
    // Embed environment variables (@vars)
    for (i, (var, value)) in vars.iter().enumerate() {
        let env_str = format!("{}={}\0", var, value);
        let bytes = env_str.as_bytes();
        let array_type = i8_type.array_type(bytes.len() as u32);
        let global = module.add_global(array_type, None, &format!("env_{}", i));
        global.set_linkage(Linkage::Internal);
        global.set_initializer(&context.const_string(bytes, false));
        let zero = context.i64_type().const_int(0, false);
        let ptr = unsafe { builder.build_in_bounds_gep(array_type, global.as_pointer_value(), &[zero, zero], "env_ptr") }.unwrap();
        let cast_ptr = builder.build_bit_cast(ptr, i8_ptr_type, "cast_env_ptr").unwrap().into_pointer_value();
        builder.build_call(putenv_fn, &[cast_ptr.into()], "putenv_call");
    }
    // Embed commands
    for (i, cmd) in final_cmds.iter().enumerate() {
        let cmd_str = format!("{}\0", cmd);
        let bytes = cmd_str.as_bytes();
        let array_type = i8_type.array_type(bytes.len() as u32);
        let global = module.add_global(array_type, None, &format!("cmd_{}", i));
        global.set_linkage(Linkage::Internal);
        global.set_initializer(&context.const_string(bytes, false));
        let zero = context.i64_type().const_int(0, false);
        let ptr = unsafe { builder.build_in_bounds_gep(array_type, global.as_pointer_value(), &[zero, zero], "cmd_ptr") }.unwrap();
        let cast_ptr = builder.build_bit_cast(ptr, i8_ptr_type, "cast_cmd_ptr").unwrap().into_pointer_value();
        builder.build_call(system_fn, &[cast_ptr.into()], "system_call");
    }
    let zero_val = i32_type.const_zero();
    builder.build_return(Some(&zero_val));
    // Verify module
    if let Err(err) = module.verify() {
        eprintln!("LLVM verification error: {}", err.to_string());
        process::exit(1);
    }
    // Run passes
    let fpm = PassManager::create(&module);
    fpm.run_on(&main_fn);
    // Initialize targets
    Target::initialize_native(&InitializationConfig::default())
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple)
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();
    let target_machine = target
    .create_target_machine(
        &triple,
        &cpu,
        &features,
        OptimizationLevel::Default,
        RelocMode::PIC,
        CodeModel::Default,
    )
    .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to create target machine"))?;
    // Write object file
    let temp_obj_path = output_path.with_extension("o");
    target_machine
    .write_to_file(&module, FileType::Object, temp_obj_path.as_path())
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    // Link with gcc
    let status = Exec::shell(format!(
        "gcc -o {} {}",
        output_path.display(),
                                     temp_obj_path.display()
    ))
    .join()
    .map_err(|e: PopenError| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "Linking failed"));
    }
    fs::remove_file(temp_obj_path)?;
    Ok(())
}
