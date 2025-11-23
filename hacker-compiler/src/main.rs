use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;
use cranelift::prelude::*;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa;
use cranelift_codegen::settings;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use subprocess::{Exec, PopenError};
use std::os::unix::fs::PermissionsExt;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use std::collections::HashMap;

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
            let lib = line[1..].trim().to_string();
            if lib.is_empty() {
                errors.push(format!("Line {line_num}: Empty library/include"));
            } else {
                let lib_dir = expand_home(&format!("{HACKER_DIR}/libs/{lib}"));
                let lib_hacker_path = format!("{lib_dir}/main.hacker");
                let lib_bin_path = lib_dir.clone();

                if Path::new(&lib_hacker_path).exists() {
                    includes.push(lib.clone());
                    let sub = parse_hacker_file(Path::new(&lib_hacker_path), verbose, bytes_mode)?;
                    deps.extend(sub.deps);
                    libs.extend(sub.libs);
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
                        errors.push(format!("In {lib}: {err}"));
                    }
                }
                if Path::new(&lib_bin_path).exists() && Path::new(&lib_bin_path).metadata()?.permissions().mode() & 0o111 != 0 {
                    if bytes_mode {
                        println!("Embedding library binary: {lib_bin_path}");
                    }
                    binaries.push(lib_bin_path);
                } else {
                    libs.push(lib);
                }
            }
        } else if line.starts_with(">>>") {
            let cmd_part_str: String = line[3..].split('!').next().unwrap_or("").trim().to_string();
            let mut cmd = cmd_part_str.clone();
            if is_super {
                cmd = format!("sudo {cmd}");
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
                cmd = format!("sudo {cmd}");
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
                cmd = format!("sudo {cmd}");
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
                let plugin_path = expand_home(&format!("{HACKER_DIR}/plugins/{plugin_name}"));
                if Path::new(&plugin_path).exists() && Path::new(&plugin_path).metadata()?.permissions().mode() & 0o111 != 0 {
                    plugins.push(Plugin { name: plugin_name.clone(), is_super });
                    if verbose {
                        println!("Loaded plugin: {plugin_name}");
                    }
                } else {
                    errors.push(format!("Line {line_num}: Plugin {plugin_name} not found or not executable"));
                }
            }
        } else if line.starts_with("=") {
            let parts: Vec<String> = line[1..].split('>').map(|s| s.trim().to_string()).collect();
            if parts.len() == 2 {
                if let Ok(num) = parts[0].parse::<usize>() {
                    let cmd_part_str = parts[1].split('!').next().unwrap_or("").trim().to_string();
                    let mut cmd = cmd_part_str.clone();
                    if is_super {
                        cmd = format!("sudo {cmd}");
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
                    cmd = format!("sudo {cmd}");
                }
                if condition.is_empty() || cmd.is_empty() {
                    errors.push(format!("Line {line_num}: Invalid conditional"));
                } else {
                    let if_cmd = format!("if {condition}; then {cmd}; fi");
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
            let mut cmd = format!("{cmd_part_str} &");
            if is_super {
                cmd = format!("sudo {cmd}");
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
    format!("command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})")
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

    let parse_result = parse_hacker_file(input_path, verbose, bytes_mode)?;
    let ParseResult {
        deps,
        libs: _,
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
            eprintln!("  \x1b[31m✖ \x1b[0m{err}");
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
            sub_cmd = sub_cmd.replace(&format!("${k}"), v);
        }
        substituted_direct.push(sub_cmd);
    }

    let mut substituted_separate: Vec<String> = Vec::new();
    for cmd in cmds_separate {
        let mut sub_cmd = cmd.clone();
        for (k, v) in &local_vars {
            sub_cmd = sub_cmd.replace(&format!("${k}"), v);
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
    final_cmds.extend(substituted_direct);

    // Separate file commands (>>>)
    for sub_cmd in substituted_separate {
        let script = format!("#!/bin/bash\nset -e\n{sub_cmd}\n");
        let encoded = BASE64_STANDARD.encode(script.as_bytes());
        let extract_cmd = format!("temp=$(mktemp /tmp/hacker_cmd.XXXXXX); echo '{encoded}' | base64 -d > $temp && chmod +x $temp && $temp");
        final_cmds.push(extract_cmd);
    }

    // Binary libraries
    for bin_path in binaries {
        let bin_data = fs::read(&bin_path)?;
        let encoded = BASE64_STANDARD.encode(&bin_data);
        let extract_cmd = format!("temp=$(mktemp /tmp/hacker_bin.XXXXXX); echo '{encoded}' | base64 -d > $temp && chmod +x $temp && $temp");
        final_cmds.push(extract_cmd);
    }

    // Plugins
    for plugin in plugins {
        let plugin_path = expand_home(&format!("{HACKER_DIR}/plugins/{}", plugin.name));
        let plugin_data = fs::read(&plugin_path)?;
        let encoded = BASE64_STANDARD.encode(&plugin_data);
        let mut run_cmd = " $temp &".to_string();
        if plugin.is_super {
            run_cmd = format!("sudo{run_cmd}");
        }
        let extract_cmd = format!("temp=$(mktemp /tmp/hacker_plugin.XXXXXX); echo '{encoded}' | base64 -d > $temp && chmod +x $temp &&{run_cmd}");
        final_cmds.push(extract_cmd);
    }

    // Cranelift setup
    let flag_builder = settings::builder();
    let flags = settings::Flags::new(flag_builder);
    let triple = target_lexicon::Triple::host();
    let isa_builder = isa::lookup(triple).expect("Host not supported");
    let isa = isa_builder.finish(flags).expect("ISA build failed");

    let builder = ObjectBuilder::new(
        isa,
        output_path.file_stem().unwrap().to_str().unwrap().as_bytes().to_vec(),
        cranelift_module::default_libcall_names(),
    ).expect("ObjectBuilder failed");
    let mut module = ObjectModule::new(builder);
    let pointer_type = module.target_config().pointer_type();

    // Declare external functions
    let mut sig_system = module.make_signature();
    sig_system.params.push(AbiParam::new(pointer_type));
    sig_system.returns.push(AbiParam::new(types::I32));
    sig_system.call_conv = module.target_config().default_call_conv;
    let system_id = module.declare_function("system", Linkage::Import, &sig_system).unwrap();

    let mut sig_putenv = module.make_signature();
    sig_putenv.params.push(AbiParam::new(pointer_type));
    sig_putenv.returns.push(AbiParam::new(types::I32));
    sig_putenv.call_conv = module.target_config().default_call_conv;
    let putenv_id = module.declare_function("putenv", Linkage::Import, &sig_putenv).unwrap();

    // Main function
    let mut sig_main = module.make_signature();
    sig_main.returns.push(AbiParam::new(types::I32));
    sig_main.call_conv = module.target_config().default_call_conv;
    let main_id = module.declare_function("main", Linkage::Export, &sig_main).unwrap();

    let mut ctx = cranelift_codegen::Context::for_function(Function::with_name_signature(Default::default(), sig_main.clone()));
    let mut func_builder_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut ctx.func, &mut func_builder_ctx);

    let entry_block = builder.create_block();
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);

    let local_system = module.declare_func_in_func(system_id, &mut builder.func);
    let local_putenv = module.declare_func_in_func(putenv_id, &mut builder.func);

    // Embed environment variables (@vars)
    let mut var_data_ids = Vec::new();
    for (i, (var, value)) in vars.iter().enumerate() {
        let env_str = format!("{var}={value}");
        let data_name = format!("env_{i}");
        let data_id = module.declare_data(&data_name, Linkage::Local, true, false).unwrap();
        let mut data_ctx = DataDescription::new();
        let mut bytes: Vec<u8> = env_str.into_bytes();
        bytes.push(0);
        data_ctx.define(bytes.into_boxed_slice());
        module.define_data(data_id, &data_ctx).unwrap();
        var_data_ids.push(data_id);
    }
    for data_id in var_data_ids {
        let global = module.declare_data_in_func(data_id, &mut builder.func);
        let ptr = builder.ins().global_value(pointer_type, global);
        builder.ins().call(local_putenv, &[ptr]);
    }

    // Embed commands
    let mut cmd_data_ids = Vec::new();
    for (i, cmd) in final_cmds.iter().enumerate() {
        let data_name = format!("cmd_{i}");
        let data_id = module.declare_data(&data_name, Linkage::Local, true, false).unwrap();
        let mut data_ctx = DataDescription::new();
        let mut bytes: Vec<u8> = cmd.as_bytes().to_vec();
        bytes.push(0);
        data_ctx.define(bytes.into_boxed_slice());
        module.define_data(data_id, &data_ctx).unwrap();
        cmd_data_ids.push(data_id);
    }
    for data_id in cmd_data_ids {
        let global = module.declare_data_in_func(data_id, &mut builder.func);
        let ptr = builder.ins().global_value(pointer_type, global);
        builder.ins().call(local_system, &[ptr]);
    }

    let zero = builder.ins().iconst(types::I32, 0);
    builder.ins().return_(&[zero]);
    builder.finalize();

    module.define_function(main_id, &mut ctx).unwrap();
    let obj = module.finish().object.write().expect("Write object failed");

    let temp_obj_path = output_path.with_extension("o");
    let mut file = File::create(&temp_obj_path)?;
    file.write_all(&obj)?;

    let status = Exec::shell(format!("gcc -o {} {}", output_path.display(), temp_obj_path.display()))
        .join()
        .map_err(|e: PopenError| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "Linking failed"));
    }

    fs::remove_file(temp_obj_path)?;
    Ok(())
}
