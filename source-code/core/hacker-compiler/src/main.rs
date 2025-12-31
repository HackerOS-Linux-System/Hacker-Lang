use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead};
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
    binaries: Vec<String>,
    plugins: Vec<Plugin>,
    functions: HashMap<String, Vec<String>>,
    errors: Vec<String>,
    config: HashMap<String, String>,
}

fn parse_hacker_file(path: &Path, verbose: bool, bytes_mode: bool) -> io::Result<ParseResult> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);
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
    let mut binaries = Vec::new();
    let mut plugins = Vec::new();
    let mut functions: HashMap<String, Vec<String>> = HashMap::new();
    let mut errors = Vec::new();
    let mut config = HashMap::new();
    let mut in_config = false;
    let mut in_comment = false;
    let mut in_function: Option<String> = None;
    let mut line_num = 0;
    for line_result in reader.lines() {
        line_num += 1;
        let original_line = line_result?;
        let mut line = original_line.trim().to_string();
        if line.is_empty() {
            continue;
        }
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
                errors.push(format!("Line {line_num}: Nested config section"));
            }
            if in_function.is_some() {
                errors.push(format!("Line {line_num}: Config in function"));
            }
            in_config = true;
            continue;
        } else if line == "]" {
            if !in_config {
                errors.push(format!("Line {line_num}: Closing ] without ["));
            }
            in_config = false;
            continue;
        }
        if in_config {
            if let Some(eq_idx) = line.find('=') {
                let key = line[..eq_idx].trim().to_string();
                let value = line[eq_idx + 1..].trim().to_string();
                config.insert(key, value);
            }
            continue;
        }
        if line == ":" {
            if in_function.is_some() {
                in_function = None;
            } else {
                errors.push(format!("Line {line_num}: Ending function without start"));
            }
            continue;
        } else if line.starts_with(":") {
            let func_name = line[1..].trim().to_string();
            if func_name.is_empty() {
                errors.push(format!("Line {line_num}: Empty function name"));
                continue;
            }
            if in_function.is_some() {
                errors.push(format!("Line {line_num}: Nested function"));
            }
            functions.insert(func_name.clone(), Vec::new());
            in_function = Some(func_name);
            continue;
        } else if line.starts_with(".") {
            let func_name = line[1..].trim().to_string();
            if func_name.is_empty() {
                errors.push(format!("Line {line_num}: Empty function call"));
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
                errors.push(format!("Line {line_num}: Unknown function {func_name}"));
            }
            continue;
        }
        if in_function.is_some() {
            let valid_prefix = line.starts_with(">") || line.starts_with(">>") || line.starts_with(">>>") ||
            line.starts_with("=") || line.starts_with("?") || line.starts_with("&") ||
            line.starts_with("!") || line.starts_with("@") || line.starts_with("$") || line.starts_with("\\");
            if !valid_prefix {
                errors.push(format!("Line {line_num}: Invalid in function"));
                continue;
            }
        }
        if line.starts_with("//") {
            if in_function.is_some() {
                errors.push(format!("Line {line_num}: Deps not allowed in function"));
                continue;
            }
            let dep = line[2..].trim().to_string();
            if !dep.is_empty() {
                deps.push(dep);
            } else {
                errors.push(format!("Line {line_num}: Empty system dependency"));
            }
        } else if line.starts_with("#") {
            if in_function.is_some() {
                errors.push(format!("Line {line_num}: Libs not allowed in function"));
                continue;
            }
            let full_lib = line[1..].trim().to_string();
            if full_lib.is_empty() {
                errors.push(format!("Line {line_num}: Empty library/include"));
                continue;
            }
            let (prefix, lib_name) = if let Some(colon_idx) = full_lib.find(':') {
                (full_lib[..colon_idx].trim().to_string(), full_lib[colon_idx + 1..].trim().to_string())
            } else {
                ("bytes".to_string(), full_lib)
            };
            if lib_name.is_empty() {
                errors.push(format!("Line {line_num}: Empty library name after prefix"));
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
                    let lib_bin_path = lib_dir.clone();
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
                        binaries.extend(sub.binaries);
                        plugins.extend(sub.plugins);
                        functions.extend(sub.functions);
                        config.extend(sub.config);
                        for err in sub.errors {
                            errors.push(format!("In {}: {}", lib_name, err));
                        }
                    }
                    if Path::new(&lib_bin_path).exists() && Path::new(&lib_bin_path).metadata()?.permissions().mode() & 0o111 != 0 {
                        if bytes_mode {
                            println!("Embedding library binary: {}", lib_bin_path);
                        }
                        binaries.push(lib_bin_path);
                    } else {
                        libs.push(lib_name);
                    }
                }
                _ => {
                    errors.push(format!("Line {line_num}: Unknown library prefix {}", prefix));
                }
            }
        } else if line.starts_with(">>>") {
            let cmd_part_str: String = line[3..].split('!').next().unwrap_or("").trim().to_string();
            let mut cmd = cmd_part_str.clone();
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            if cmd.is_empty() {
                errors.push(format!("Line {line_num}: Empty separate command"));
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
                errors.push(format!("Line {line_num}: Empty command with vars"));
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
                errors.push(format!("Line {line_num}: Empty command"));
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
                    errors.push(format!("Line {line_num}: Invalid variable"));
                } else {
                    vars.push((var, value));
                }
            } else {
                errors.push(format!("Line {line_num}: Invalid @ syntax"));
            }
        } else if line.starts_with("$") {
            if let Some(eq_idx) = line[1..].find('=') {
                let var = line[1..1 + eq_idx].trim().to_string();
                let value = line[1 + eq_idx + 1..].trim().to_string();
                if var.is_empty() || value.is_empty() {
                    errors.push(format!("Line {line_num}: Invalid local variable"));
                } else {
                    local_vars.push((var, value));
                }
            } else {
                errors.push(format!("Line {line_num}: Invalid $ syntax"));
            }
        } else if line.starts_with("\\") {
            let plugin_name = line[1..].trim().to_string();
            if plugin_name.is_empty() {
                errors.push(format!("Line {line_num}: Empty plugin name"));
            } else {
                let plugin_path = expand_home(&format!("{}/plugins/{}", HACKER_DIR, plugin_name));
                if Path::new(&plugin_path).exists() && Path::new(&plugin_path).metadata()?.permissions().mode() & 0o111 != 0 {
                    plugins.push(Plugin { name: plugin_name.clone(), is_super });
                    if verbose {
                        println!("Loaded plugin: {}", plugin_name);
                    }
                } else {
                    errors.push(format!("Line {line_num}: Plugin {} not found or not executable", plugin_name));
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
                        errors.push(format!("Line {line_num}: Empty loop command"));
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
                    errors.push(format!("Line {line_num}: Invalid loop count"));
                }
            } else {
                errors.push(format!("Line {line_num}: Invalid loop syntax"));
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
                    errors.push(format!("Line {line_num}: Invalid conditional"));
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
                errors.push(format!("Line {line_num}: Invalid conditional syntax"));
            }
        } else if line.starts_with("&") {
            let cmd_part_str = line[1..].split('!').next().unwrap_or("").trim().to_string();
            let mut cmd = format!("{} &", cmd_part_str);
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            if cmd_part_str.is_empty() {
                errors.push(format!("Line {line_num}: Empty background command"));
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
            errors.push(format!("Line {line_num}: Invalid syntax"));
        }
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
    })
}

fn generate_check_cmd(dep: &str) -> String {
    if dep == "sudo" {
        return String::new();
    }
    format!("command -v {} &> /dev/null || (sudo apt update && sudo apt install -y {})", dep, dep)
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
    } = parse_result;
    if !errors.is_empty() {
        eprintln!("\x1b[31m\x1b[1mErrors:\x1b[0m");
        for err in errors {
            eprintln!(" \x1b[31m✖ \x1b[0m{}", err);
        }
        eprintln!();
        process::exit(1);
    }
    // Substitute local vars in all command types
    let mut direct_cmds = cmds.clone();
    direct_cmds.extend(cmds_with_vars);
    let mut substituted_direct: Vec<String> = Vec::new();
    for cmd in direct_cmds {
        let mut sub_cmd = cmd.clone();
        for (k, v) in &local_vars {
            sub_cmd = sub_cmd.replace(&format!("${}", k), v);
        }
        substituted_direct.push(sub_cmd);
    }
    let mut substituted_separate: Vec<String> = Vec::new();
    for cmd in cmds_separate {
        let mut sub_cmd = cmd.clone();
        for (k, v) in &local_vars {
            sub_cmd = sub_cmd.replace(&format!("${}", k), v);
        }
        substituted_separate.push(sub_cmd);
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
    // Separate file commands (>>>)
    for sub_cmd in substituted_separate {
        let script = format!("#!/bin/bash\nset -e\n{}\n", sub_cmd);
        let encoded = BASE64_STANDARD.encode(script.as_bytes());
        let extract_cmd = format!("temp=$(mktemp /tmp/hacker_cmd.XXXXXX); echo '{}' | base64 -d > $temp && chmod +x $temp && $temp", encoded);
        final_cmds.push(extract_cmd);
    }
    // Binary libraries
    for bin_path in binaries {
        let bin_data = fs::read(&bin_path)?;
        let encoded = BASE64_STANDARD.encode(&bin_data);
        let extract_cmd = format!("temp=$(mktemp /tmp/hacker_bin.XXXXXX); echo '{}' | base64 -d > $temp && chmod +x $temp && $temp", encoded);
        final_cmds.push(extract_cmd);
    }
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
        let ptr = unsafe { builder.build_in_bounds_gep(array_type, global.as_pointer_value(), &[zero], "env_ptr") }.unwrap();
        let cast_ptr = builder.build_bit_cast(ptr, i8_ptr_type, "cast_env_ptr").unwrap();
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
        let ptr = unsafe { builder.build_in_bounds_gep(array_type, global.as_pointer_value(), &[zero], "cmd_ptr") }.unwrap();
        let cast_ptr = builder.build_bit_cast(ptr, i8_ptr_type, "cast_cmd_ptr").unwrap();
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
