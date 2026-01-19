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
    libs: Vec<String>, // Now paths to .a files for static linking
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
}

fn parse_hacker_file(path: &Path, verbose: bool, bytes_mode: bool) -> io::Result<ParseResult> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().collect::<io::Result<Vec<String>>>()?;
    let mut deps = Vec::new();
    let mut libs = Vec::new();
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
        } else if line.starts_with("#") {
            if in_function.is_some() {
                errors.push("Libs not allowed in function".to_string());
                i += 1;
                continue;
            }
            let lib_name = line[1..].trim().to_string();
            if lib_name.is_empty() {
                errors.push("Empty library name".to_string());
                i += 1;
                continue;
            }
            let lib_dir = expand_home(&format!("{HACKER_DIR}/libs/{}", lib_name));
            let lib_hacker_path = format!("{}/main.hacker", lib_dir);
            let lib_bin_path = format!("{}/{}", lib_dir, lib_name);
            let lib_a_path = format!("{}/{}.a", lib_dir, lib_name);
            if Path::new(&lib_hacker_path).exists() {
                includes.push(lib_name.clone());
                let sub = parse_hacker_file(Path::new(&lib_hacker_path), verbose, bytes_mode)?;
                deps.extend(sub.deps);
                libs.extend(sub.libs);
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
                for err in sub.errors {
                    errors.push(format!("In {}: {}", lib_name, err));
                }
            }
            if Path::new(&lib_bin_path).exists() {
                let metadata = fs::metadata(&lib_bin_path)?;
                if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                    if bytes_mode {
                        println!("Embedding library binary: {}", lib_bin_path);
                    }
                    if binaries.contains_key(&lib_name) {
                        errors.push(format!("Duplicate binary name {}", lib_name));
                    } else {
                        binaries.insert(lib_name, lib_bin_path.clone());
                    }
                }
            }
            if Path::new(&lib_a_path).exists() {
                libs.push(lib_a_path);
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
                    let metadata = fs::metadata(&plugin_path)?;
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
                if let Ok(num) = parts[0].parse::<u32>() {
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
    if in_comment {
        errors.push("Unclosed comment block".to_string());
    }
    if in_function.is_some() {
        errors.push("Unclosed function block".to_string());
    }
    if verbose {
        println!("System Deps: {:?}", deps);
        println!("Libs (.a paths): {:?}", libs);
        println!("Vars: {:?}", vars);
        println!("Local Vars: {:?}", local_vars);
        println!("Cmds (direct): {:?}", cmds);
        println!("Cmds (with vars): {:?}", cmds_with_vars);
        println!("Cmds (separate): {:?}", cmds_separate);
        println!("Includes: {:?}", includes);
        println!("Binaries: {:?}", binaries);
        println!("Plugins: {:?}", plugins);
        println!("Functions: {:?}", functions);
        if !errors.is_empty() {
            println!("Errors: {:?}", errors);
        }
    }
    Ok(ParseResult {
        deps,
       libs,
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
    })
}

fn generate_check_cmd(dep: &str) -> String {
    if dep == "sudo" {
        return String::new();
    }
    format!("command -v {} &> /dev/null || (sudo apt update && sudo apt install -y {})", dep, dep)
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
        eprintln!("Usage: hacker-compiler [--verbose] [--bytes] <input.hacker> <output>");
        process::exit(1);
    }
    let input_path = Path::new(&input_path_str);
    let output_path = Path::new(&output_path_str);
    let parse_result = parse_hacker_file(input_path, verbose, bytes_mode)?;
    let ParseResult {
        deps,
        libs,
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
    // Process binaries in commands
    for cmd in substituted_direct.iter_mut() {
        process_binary_cmd(cmd, &binary_data);
    }
    for cmd in substituted_separate.iter_mut() {
        process_binary_cmd(cmd, &binary_data);
    }
    // Build final command list
    let mut final_cmds: Vec<String> = Vec::new();
    for dep in deps {
        let check = generate_check_cmd(&dep);
        if !check.is_empty() {
            final_cmds.push(check);
        }
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
        let ptr = unsafe { builder.build_gep(array_type, global.as_pointer_value(), &[zero, zero], "env_ptr") }.expect("GEP failed");
        let cast_ptr = builder.build_bit_cast(ptr, i8_ptr_type, "cast_env_ptr").expect("Bitcast failed").into_pointer_value();
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
        let ptr = unsafe { builder.build_gep(array_type, global.as_pointer_value(), &[zero, zero], "cmd_ptr") }.expect("GEP failed");
        let cast_ptr = builder.build_bit_cast(ptr, i8_ptr_type, "cast_cmd_ptr").expect("Bitcast failed").into_pointer_value();
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
        OptimizationLevel::Aggressive,
        RelocMode::PIC,
        CodeModel::Default,
    )
    .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to create target machine"))?;
    // Write object file
    let temp_obj_path = output_path.with_extension("o");
    target_machine
    .write_to_file(&module, FileType::Object, temp_obj_path.as_path())
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    // Link with gcc, including static libs
    let mut link_cmd = format!(
        "gcc -o {} {}",
        output_path.display(),
                               temp_obj_path.display()
    );
    if !libs.is_empty() {
        link_cmd.push_str(&format!(" {}", libs.join(" ")));
    }
    let status = Exec::shell(link_cmd)
    .join()
    .map_err(|e: PopenError| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "Linking failed"));
    }
    fs::remove_file(temp_obj_path)?;
    Ok(())
}
