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
const HACKER_DIR: &str = "~/.hacker-lang";
fn expand_home(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return path.replacen("~", home.to_str().unwrap(), 1);
        }
    }
    path.to_string()
}
fn parse_hacker_file(path: &Path, verbose: bool) -> io::Result<(Vec<String>, Vec<String>, Vec<(String, String)>, Vec<String>, Vec<String>, Vec<String>)> {
    let file = File::open(path)?;
    let mut deps = Vec::new();
    let mut libs = Vec::new();
    let mut vars = Vec::new();
    let mut cmds = Vec::new();
    let mut includes = Vec::new();
    let mut errors = Vec::new();
    let mut in_config = false;
    let mut config_lines = Vec::new();
    let mut line_num = 0;
    for line in io::BufReader::new(file).lines() {
        line_num += 1;
        let line = line?;
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line == "[" {
            if in_config {
                errors.push(format!("Line {}: Nested config section", line_num));
            }
            in_config = true;
            config_lines = Vec::new();
            continue;
        } else if line == "]" {
            if !in_config {
                errors.push(format!("Line {}: Closing ] without [", line_num));
            }
            in_config = false;
            continue;
        }
        if in_config {
            config_lines.push(line);
            continue;
        }
        if line.starts_with("//") {
            let dep = line[2..].trim().to_string();
            if dep.is_empty() {
                errors.push(format!("Line {}: Empty system dependency", line_num));
            } else {
                deps.push(dep);
            }
        } else if line.starts_with("#") {
            let lib = line[1..].trim().to_string();
            if lib.is_empty() {
                errors.push(format!("Line {}: Empty library/include", line_num));
            } else {
                let lib_path_str = expand_home(&format!("{}/libs/{}/main.hacker", HACKER_DIR, lib));
                let lib_path = Path::new(&lib_path_str);
                if lib_path.exists() {
                    includes.push(lib.clone());
                    let (sub_deps, sub_libs, sub_vars, sub_cmds, sub_includes, sub_errors) = parse_hacker_file(lib_path, verbose)?;
                    deps.extend(sub_deps);
                    libs.extend(sub_libs);
                    vars.extend(sub_vars);
                    cmds.extend(sub_cmds);
                    includes.extend(sub_includes);
                    for err in sub_errors {
                        errors.push(format!("In {}: {}", lib, err));
                    }
                } else {
                    libs.push(lib);
                }
            }
        } else if line.starts_with(">") {
            let parts: Vec<String> = line[1..].split('!').map(|s| s.trim().to_string()).collect();
            let cmd = parts[0].clone();
            if cmd.is_empty() {
                errors.push(format!("Line {}: Empty command", line_num));
            } else {
                cmds.push(cmd);
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
                errors.push(format!("Line {}: Missing = in variable", line_num));
            }
        } else if line.starts_with("=") {
            let parts: Vec<String> = line[1..].split('>').map(|s| s.trim().to_string()).collect();
            if parts.len() == 2 {
                if let Ok(num) = parts[0].parse::<usize>() {
                    let cmd_parts: Vec<String> = parts[1].split('!').map(|s| s.trim().to_string()).collect();
                    let cmd = cmd_parts[0].clone();
                    if cmd.is_empty() {
                        errors.push(format!("Line {}: Empty loop command", line_num));
                    } else {
                        for _ in 0..num {
                            cmds.push(cmd.clone());
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
                let cmd = cmd_parts[0].clone();
                if condition.is_empty() || cmd.is_empty() {
                    errors.push(format!("Line {}: Invalid conditional", line_num));
                } else {
                    cmds.push(format!("if {}; then {}; fi", condition, cmd));
                }
            } else {
                errors.push(format!("Line {}: Invalid conditional syntax", line_num));
            }
        } else if line.starts_with("&") {
            let parts: Vec<String> = line[1..].split('!').map(|s| s.trim().to_string()).collect();
            let cmd = parts[0].clone();
            if cmd.is_empty() {
                errors.push(format!("Line {}: Empty background command", line_num));
            } else {
                cmds.push(format!("{} &", cmd));
            }
        } else if line.starts_with("!") {
            // Ignore comment
        } else {
            errors.push(format!("Line {}: Invalid syntax", line_num));
        }
    }
    if in_config {
        errors.push("Unclosed config section".to_string());
    }
    if verbose {
        println!("System Deps: {:?}", deps);
        println!("Custom Libs: {:?}", libs);
        println!("Vars: {:?}", vars);
        println!("Cmds: {:?}", cmds);
        println!("Includes: {:?}", includes);
        if !errors.is_empty() {
            println!("Errors: {:?}", errors);
        }
    }
    Ok((deps, libs, vars, cmds, includes, errors))
}
fn generate_check_cmd(dep: &str) -> String {
    if dep == "sudo" {
        return String::new();
    }
    format!("command -v {} &> /dev/null || (sudo apt update && sudo apt install -y {})", dep, dep)
}
fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        eprintln!("Usage: hacker-compiler <input.hacker> <output> [--verbose]");
        process::exit(1);
    }
    let verbose = args.len() == 4 && args[3] == "--verbose";
    let input_path = Path::new(&args[1]);
    let output_path = Path::new(&args[2]);
    let (deps, _libs, vars, cmds, _includes, errors) = parse_hacker_file(input_path, verbose)?;
    if !errors.is_empty() {
        for err in errors {
            eprintln!("{}", err);
        }
        process::exit(1);
    }
    let mut final_cmds = Vec::new();
    for dep in deps {
        let check = generate_check_cmd(&dep);
        if !check.is_empty() {
            final_cmds.push(check);
        }
    }
    final_cmds.extend(cmds);
    let flag_builder = settings::builder();
    let flags = settings::Flags::new(flag_builder);
    let triple = target_lexicon::Triple::host();
    let isa_builder = isa::lookup(triple).expect("Host not supported");
    let isa = isa_builder.finish(flags).expect("ISA build failed");
    let builder = ObjectBuilder::new(
        isa,
        output_path.file_stem().unwrap().to_str().unwrap().as_bytes().to_vec(),
                                     cranelift_module::default_libcall_names(),
    ).expect("Failed to create ObjectBuilder");
    let mut module = ObjectModule::new(builder);
    let pointer_type = module.target_config().pointer_type();
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
    let mut var_data_ids = Vec::new();
    for (var, value) in &vars {
        let env_str = format!("{}={}", var, value);
        let data_name = format!("env_{}", var_data_ids.len());
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
        let _ = builder.ins().call(local_putenv, &[ptr]);
    }
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
        let _ = builder.ins().call(local_system, &[ptr]);
    }
    let zero = builder.ins().iconst(types::I32, 0);
    builder.ins().return_(&[zero]);
    builder.finalize();
    module.define_function(main_id, &mut ctx).unwrap();
    let obj = module.finish().object.write().expect("Failed to write object");
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
