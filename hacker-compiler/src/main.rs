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

const HACKER_DIR: &str = "~/.hackeros/hacker-lang";

fn expand_home(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = env::var_os("HOME") {
            return path.replacen("~", home.to_str().unwrap_or(""), 1);
        }
    }
    path.to_string()
}

#[derive(Debug)]
struct Plugin {
    name: String,
    is_super: bool,
}

fn parse_hacker_file(path: &Path, verbose: bool, bytes_mode: bool) -> io::Result<(Vec<String>, Vec<String>, Vec<(String, String)>, Vec<String>, Vec<String>, Vec<String>, Vec<(String, String)>, Vec<String>, std::collections::HashMap<String, String>, Vec<Plugin>, std::collections::HashMap<String, Vec<String>>)> {
    let file = File::open(path)?;
    let mut deps: Vec<String> = Vec::new();
    let mut libs: Vec<String> = Vec::new();
    let mut vars: Vec<(String, String)> = Vec::new();
    let mut local_vars: Vec<(String, String)> = Vec::new();
    let mut cmds: Vec<String> = Vec::new();
    let mut includes: Vec<String> = Vec::new();
    let mut binaries: Vec<String> = Vec::new(); // Binary lib paths
    let mut errors: Vec<String> = Vec::new();
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut plugins: Vec<Plugin> = Vec::new();
    let mut functions: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut in_config = false;
    let mut in_comment = false;
    let mut in_function: Option<String> = None;
    let mut line_num = 0;
    for line in io::BufReader::new(file).lines() {
        line_num += 1;
        let mut line = line?;
        line = line.trim().to_string();
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
        let is_super = line.starts_with('^');
        if is_super {
            line = line[1..].trim().to_string();
        }
        if line == "[" {
            if in_config {
                errors.push(format!("Line {}: Nested config section", line_num));
            }
            if in_function.is_some() {
                errors.push(format!("Line {}: Config in function", line_num));
            }
            in_config = true;
            continue;
        } else if line == "]" {
            if !in_config {
                errors.push(format!("Line {}: Closing ] without [", line_num));
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
                errors.push(format!("Line {}: Ending function without start", line_num));
            }
            continue;
        } else if line.starts_with(":") {
            let func_name = line[1..].trim().to_string();
            if func_name.is_empty() {
                errors.push(format!("Line {}: Empty function name", line_num));
                continue;
            }
            if in_function.is_some() {
                errors.push(format!("Line {}: Nested function", line_num));
            }
            functions.insert(func_name.clone(), Vec::new());
            in_function = Some(func_name);
            continue;
        } else if line.starts_with(".") {
            let func_name = line[1..].trim().to_string();
            if func_name.is_empty() {
                errors.push(format!("Line {}: Empty function call", line_num));
                continue;
            }
            if let Some(f_cmds) = functions.get(&func_name).cloned() {
                let target = if let Some(ref f) = in_function {
                    functions.get_mut(f).unwrap()
                } else {
                    &mut cmds
                };
                for c in &f_cmds {
                    target.push(c.clone());
                }
            } else {
                errors.push(format!("Line {}: Unknown function {}", line_num, func_name));
            }
            continue;
        }
        if in_function.is_some() {
            if !line.starts_with(">") && !line.starts_with("=") && !line.starts_with("?") && !line.starts_with("&") && !line.starts_with("!") && !line.starts_with("@") && !line.starts_with("$") && !line.starts_with("\\") {
                errors.push(format!("Line {}: Invalid in function", line_num));
                continue;
            }
        }
        if line.starts_with("//") {
            if in_function.is_some() {
                errors.push(format!("Line {}: Deps not allowed in function", line_num));
                continue;
            }
            let dep = line[2..].trim().to_string();
            if dep.is_empty() {
                errors.push(format!("Line {}: Empty system dependency", line_num));
            } else {
                deps.push(dep);
            }
        } else if line.starts_with("#") {
            if in_function.is_some() {
                errors.push(format!("Line {}: Libs not allowed in function", line_num));
                continue;
            }
            let lib = line[1..].trim().to_string();
            if lib.is_empty() {
                errors.push(format!("Line {}: Empty library/include", line_num));
            } else {
                let lib_dir = expand_home(&format!("{}/libs/{}", HACKER_DIR, lib));
                let lib_hacker_path = format!("{}/main.hacker", lib_dir);
                let lib_bin_path = expand_home(&format!("{}/libs/{}", HACKER_DIR, lib)); // Binary file
                if Path::new(&lib_hacker_path).exists() {
                    includes.push(lib.clone());
                    let (sub_deps, sub_libs, sub_vars, sub_cmds, sub_includes, sub_binaries, sub_local_vars, sub_errors, sub_config, sub_plugins, sub_functions) = parse_hacker_file(Path::new(&lib_hacker_path), verbose, bytes_mode)?;
                    deps.extend(sub_deps);
                    libs.extend(sub_libs);
                    vars.extend(sub_vars);
                    local_vars.extend(sub_local_vars);
                    cmds.extend(sub_cmds);
                    includes.extend(sub_includes);
                    binaries.extend(sub_binaries);
                    for err in sub_errors {
                        errors.push(format!("In {}: {}", lib, err));
                    }
                    config.extend(sub_config);
                    plugins.extend(sub_plugins);
                    functions.extend(sub_functions);
                } else if Path::new(&lib_bin_path).exists() && Path::new(&lib_bin_path).metadata()?.permissions().mode() & 0o111 != 0 {
                    binaries.push(lib_bin_path.clone());
                    if bytes_mode {
                        // W trybie --bytes, embeduj binarkę biblioteki
                        println!("Embedding library binary: {}", lib_bin_path);
                    }
                } else {
                    libs.push(lib);
                }
            }
        } else if line.starts_with(">") {
            let parts: Vec<String> = line[1..].split('!').map(|s| s.trim().to_string()).collect();
            let mut cmd = parts[0].clone();
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            if cmd.is_empty() {
                errors.push(format!("Line {}: Empty command", line_num));
            } else {
                let target = if let Some(ref f) = in_function { functions.get_mut(f).unwrap() } else { &mut cmds };
                target.push(cmd);
            }
        } else if line.starts_with("@") {
            if let Some(eq_idx) = line.find('=') {
                let var = line[1..eq_idx].trim().to_string();
                let value = line[eq_idx + 1..].trim().to_string();
                if var.is_empty() || value.is_empty() {
                    errors.push(format!("Line {}: Invalid variable", line_num));
                } else {
                    vars.push((var, value));
                }
            } else {
                errors.push(format!("Line {}: Invalid @ syntax", line_num));
            }
        } else if line.starts_with("$") {
            if let Some(eq_idx) = line.find('=') {
                let var = line[1..eq_idx].trim().to_string();
                let value = line[eq_idx + 1..].trim().to_string();
                if var.is_empty() || value.is_empty() {
                    errors.push(format!("Line {}: Invalid local variable", line_num));
                } else {
                    local_vars.push((var, value));
                }
            } else {
                errors.push(format!("Line {}: Invalid $ syntax", line_num));
            }
        } else if line.starts_with("\\") {
            let plugin_name = line[1..].trim().to_string();
            if plugin_name.is_empty() {
                errors.push(format!("Line {}: Empty plugin name", line_num));
            } else {
                let plugin_path = expand_home(&format!("{}/plugins/{}", HACKER_DIR, plugin_name));
                if Path::new(&plugin_path).exists() && Path::new(&plugin_path).metadata()?.permissions().mode() & 0o111 != 0 {
                    plugins.push(Plugin { name: plugin_name.clone(), is_super });
                    if verbose {
                        println!("Loaded plugin: {}", plugin_name);
                    }
                } else {
                    errors.push(format!("Line {}: Plugin {} not found or not executable", line_num, plugin_name));
                }
            }
        } else if line.starts_with("=") {
            let parts: Vec<String> = line[1..].split('>').map(|s| s.trim().to_string()).collect();
            if parts.len() == 2 {
                if let Ok(num) = parts[0].parse::<usize>() {
                    let cmd_parts: Vec<String> = parts[1].split('!').map(|s| s.trim().to_string()).collect();
                    let mut cmd = cmd_parts[0].clone();
                    if is_super {
                        cmd = format!("sudo {}", cmd);
                    }
                    if cmd.is_empty() {
                        errors.push(format!("Line {}: Empty loop command", line_num));
                    } else {
                        let target = if let Some(ref f) = in_function { functions.get_mut(f).unwrap() } else { &mut cmds };
                        for _ in 0..num {
                            target.push(cmd.clone());
                        }
                    }
                } else {
                    errors.push(format!("Line {}: Invalid loop count", line_num));
                }
            } else {
                errors.push(format!("Line {}: Invalid loop syntax", line_num));
            }
        } else if line.starts_with("?") {
            let parts: Vec<String> = line[1..].split('>').map(|s| s.trim().to_string()).collect();
            if parts.len() == 2 {
                let condition = parts[0].clone();
                let cmd_parts: Vec<String> = parts[1].split('!').map(|s| s.trim().to_string()).collect();
                let mut cmd = cmd_parts[0].clone();
                if is_super {
                    cmd = format!("sudo {}", cmd);
                }
                if condition.is_empty() || cmd.is_empty() {
                    errors.push(format!("Line {}: Invalid conditional", line_num));
                } else {
                    let if_cmd = format!("if {}; then {}; fi", condition, cmd);
                    let target = if let Some(ref f) = in_function { functions.get_mut(f).unwrap() } else { &mut cmds };
                    target.push(if_cmd);
                }
            } else {
                errors.push(format!("Line {}: Invalid conditional syntax", line_num));
            }
        } else if line.starts_with("&") {
            let parts: Vec<String> = line[1..].split('!').map(|s| s.trim().to_string()).collect();
            let mut cmd = format!("{} &", parts[0]);
            if is_super {
                cmd = format!("sudo {}", cmd);
            }
            if parts[0].is_empty() {
                errors.push(format!("Line {}: Empty background command", line_num));
            } else {
                let target = if let Some(ref f) = in_function { functions.get_mut(f).unwrap() } else { &mut cmds };
                target.push(cmd);
            }
        } else if line.starts_with("!") {
            // Comment
        } else {
            errors.push(format!("Line {}: Invalid syntax", line_num));
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
        println!("Cmds: {:?}", cmds);
        println!("Includes: {:?}", includes);
        println!("Binaries: {:?}", binaries);
        println!("Plugins: {:?}", plugins);
        println!("Functions: {:?}", functions);
        println!("Config: {:?}", config);
        if !errors.is_empty() {
            println!("Errors: {:?}", errors);
        }
    }
    Ok((deps, libs, vars, cmds, includes, binaries, local_vars, errors, config, plugins, functions))
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
    let (deps, _libs, vars, cmds, _includes, binaries, local_vars, errors, _config, plugins, _functions) = parse_hacker_file(input_path, verbose, bytes_mode)?;
    if !errors.is_empty() {
        for err in errors {
            eprintln!("{}", err);
        }
        process::exit(1);
    }
    let mut substituted_cmds: Vec<String> = Vec::new();
    for cmd in cmds {
        let mut sub_cmd = cmd.clone();
        for (k, v) in &local_vars {
            let pattern = format!("${}", k);
            sub_cmd = sub_cmd.replace(&pattern, v);
        }
        substituted_cmds.push(sub_cmd);
    }
    let mut final_cmds: Vec<String> = Vec::new();
    for dep in deps {
        let check = generate_check_cmd(&dep);
        if !check.is_empty() {
            final_cmds.push(check);
        }
    }
    final_cmds.extend(substituted_cmds);
    // Dla pluginów, dodaj komendy wykonania
    for plugin in plugins {
        let plugin_path = expand_home(&format!("{}/plugins/{}", HACKER_DIR, plugin.name));
        let cmd = if plugin.is_super {
            format!("sudo {} &", plugin_path)
        } else {
            format!("{} &", plugin_path)
        };
        final_cmds.push(cmd);
    }
    // Dla binarek, extract cmds
    let mut bin_extract_cmds: Vec<String> = Vec::new();
    for (i, bin_path) in binaries.iter().enumerate() {
        let bin_data = fs::read(bin_path)?;
        let data_name = format!("bin_lib_{i}");
        let extract_cmd = format!("echo '{}' | base64 -d > /tmp/{} && chmod +x /tmp/{} && /tmp/{}", BASE64_STANDARD.encode(&bin_data), data_name, data_name, data_name);
        bin_extract_cmds.push(extract_cmd);
    }
    final_cmds.extend(bin_extract_cmds);
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
    // Embed vars as putenv
    let mut var_data_ids = Vec::new();
    for (i, (var, value)) in vars.iter().enumerate() {
        let env_str = format!("{}={}", var, value);
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
    // Embed cmd strings
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
    // Embed binary libs data
    for (i, bin_path) in binaries.iter().enumerate() {
        let bin_data = fs::read(bin_path)?;
        let data_name = format!("bin_lib_data_{i}");
        let data_id = module.declare_data(&data_name, Linkage::Local, true, false).unwrap();
        let mut data_ctx = DataDescription::new();
        data_ctx.define(bin_data.into_boxed_slice());
        module.define_data(data_id, &data_ctx).unwrap();
        // In runtime, we would need to extract, but since we're using system calls, we embed extraction cmds above
    }
    if bytes_mode {
        for (i, bin_path) in binaries.iter().enumerate() {
            let bin_data = fs::read(bin_path)?;
            let encoded = BASE64_STANDARD.encode(&bin_data);
            let extract_str = format!("echo '{}' | base64 -d > /tmp/bin_lib_{i} && chmod +x /tmp/bin_lib_{i} && /tmp/bin_lib_{i}", encoded);
            let data_name = format!("bin_lib_data_{i}");
            let data_id = module.declare_data(&data_name, Linkage::Local, true, false).unwrap();
            let mut data_ctx = DataDescription::new();
            data_ctx.define(bin_data.into_boxed_slice());
            module.define_data(data_id, &data_ctx).unwrap();
            // Dodaj kod do wyodrębniania w runtime
            // Dodaj call do system z komendą extract
            let extract_data_id = module.declare_data(&format!("extract_cmd_{i}"), Linkage::Local, true, false).unwrap();
            let mut extract_ctx = DataDescription::new();
            let mut bytes: Vec<u8> = extract_str.as_bytes().to_vec();
            bytes.push(0);
            extract_ctx.define(bytes.into_boxed_slice());
            module.define_data(extract_data_id, &extract_ctx).unwrap();
            let extract_global = module.declare_data_in_func(extract_data_id, &mut builder.func);
            let extract_ptr = builder.ins().global_value(pointer_type, extract_global);
            builder.ins().call(local_system, &[extract_ptr]);
        }
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
