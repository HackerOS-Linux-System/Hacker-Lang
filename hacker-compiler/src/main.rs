// hacker-compiler/src/main.rs - Updated Rust compiler for Hacker Lang using Cranelift.
// Updated syntax: Dependencies with //, Config with [ ... ] (ignored).
// Added optional features: Variables @var=value (set as env vars via putenv).
// Loops *num > cmd (unroll in compiled code).
// Added --verbose flag for debug output.
// Compile with Cargo, place binary in bin dir.

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;

use cranelift::prelude::*;
use cranelift_codegen::isa;
use cranelift_codegen::settings;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataContext, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use subprocess::Exec;

fn parse_hacker_file(path: &Path, verbose: bool) -> io::Result<(Vec<String>, Vec<(String, String)>, Vec<String>)> {
    let file = File::open(path)?;
    let mut deps = Vec::new();
    let mut vars = Vec::new();
    let mut cmds = Vec::new();
    let mut in_config = false;
    let mut config_lines = Vec::new();
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        
        if line == "[" {
            in_config = true;
            config_lines = Vec::new();
            continue;
        } else if line == "]" {
            in_config = false;
            // Ignore config
            continue;
        }
        
        if in_config {
            config_lines.push(line);
            continue;
        }
        
        if line.starts_with("//") {
            let dep = line[2..].trim().to_string();
            if !dep.is_empty() {
                deps.push(dep);
            }
        } else if line.starts_with(">") {
            let parts: Vec<String> = line[1..].split('!').map(|s| s.trim().to_string()).collect();
            let cmd = parts[0].clone();
            if !cmd.is_empty() {
                cmds.push(cmd);
            }
        } else if line.starts_with("@") {
            if let Some(eq_idx) = line.find('=') {
                let var = line[1..eq_idx].trim().to_string();
                let value = line[eq_idx + 1..].trim().to_string();
                vars.push((var, value));
            }
        } else if line.starts_with("*") {
            let parts: Vec<String> = line[1..].split('>').map(|s| s.trim().to_string()).collect();
            if parts.len() == 2 {
                if let Ok(num) = parts[0].parse::<usize>() {
                    let cmd_parts: Vec<String> = parts[1].split('!').map(|s| s.trim().to_string()).collect();
                    let cmd = cmd_parts[0].clone();
                    for _ in 0..num {
                        cmds.push(cmd.clone());
                    }
                } else if verbose {
                    eprintln!("Invalid loop count in: {}", line);
                }
            }
        } else if line.starts_with("!") {
            // Ignore comment
        }
    }
    if verbose {
        println!("Parsed deps: {:?}", deps);
        println!("Parsed vars: {:?}", vars);
        println!("Parsed cmds: {:?}", cmds);
    }
    Ok((deps, vars, cmds))
}

fn generate_check_cmd(dep: &str) -> String {
    if dep == "sudo" {
        return String::new();
    }
    format!("command -v {} &> /dev/null || (sudo apt update && sudo apt install -y {})", dep, dep)
}

fn main() -> io::Result<()> {
    let mut args: Vec<String> = env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        eprintln!("Usage: hacker-compiler <input.hacker> <output> [--verbose]");
        process::exit(1);
    }
    let verbose = args.len() == 4 && args[3] == "--verbose";
    let input_path = Path::new(&args[1]);
    let output_path = Path::new(&args[2]);

    let (mut deps, vars, mut cmds) = parse_hacker_file(input_path, verbose)?;

    // Add dep checks to cmds
    for dep in deps {
        let check = generate_check_cmd(&dep);
        if !check.is_empty() {
            cmds.insert(0, check);
        }
    }

    // Cranelift setup
    let flag_builder = settings::builder();
    let flags = settings::Flags::new(flag_builder);

    let triple = target_lexicon::Triple::host();
    let isa_builder = isa::lookup(triple).expect("Host not supported");
    let isa = isa_builder.finish(flags).expect("ISA build failed");

    let builder = ObjectBuilder::new(isa, output_path.file_stem().unwrap().to_str().unwrap().as_bytes().to_vec(), cranelift_module::default_libcall_names()).unwrap();
    let mut module = ObjectModule::new(builder);

    // Declare externs: system, putenv
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

    // Data for vars: "var=value\0"
    let mut var_data_ids = Vec::new();
    for (var, value) in &vars {
        let env_str = format!("{}={}", var, value);
        let data_name = format!("env_{}", var_data_ids.len());
        let data_id = module.declare_data(&data_name, Linkage::Local, true, false).unwrap();
        let mut data_ctx = DataContext::new();
        let mut bytes: Vec<u8> = env_str.into_bytes();
        bytes.push(0);
        data_ctx.define(bytes.into_boxed_slice());
        module.define_data(data_id, &data_ctx).unwrap();
        var_data_ids.push(data_id);
    }

    // Call putenv for each var
    for data_id in var_data_ids {
        let global = module.declare_data_in_func(data_id, &mut builder.func);
        let ptr = builder.ins().global_value(pointer_type, global);
        let _ = builder.ins().call(local_putenv, &[ptr]);
    }

    // Data for cmds
    let mut cmd_data_ids = Vec::new();
    for (i, cmd) in cmds.iter().enumerate() {
        let data_name = format!("cmd_{i}");
        let data_id = module.declare_data(&data_name, Linkage::Local, true, false).unwrap();
        let mut data_ctx = DataContext::new();
        let mut bytes: Vec<u8> = cmd.as_bytes().to_vec();
        bytes.push(0);
        data_ctx.define(bytes.into_boxed_slice());
        module.define_data(data_id, &data_ctx).unwrap();
        cmd_data_ids.push(data_id);
    }

    // Call system for each cmd
    for data_id in cmd_data_ids {
        let global = module.declare_data_in_func(data_id, &mut builder.func);
        let ptr = builder.ins().global_value(pointer_type, global);
        let _ = builder.ins().call(local_system, &[ptr]);
    }

    // Return 0
    let zero = builder.ins().iconst(types::I32, 0);
    builder.ins().return_(&[zero]);
    builder.finalize();

    module.define_function(main_id, &mut ctx).unwrap();
    module.finalize_definitions();
    let obj = module.object.finish();

    // Write temp obj
    let temp_obj_path = output_path.with_extension("o");
    let mut file = File::create(&temp_obj_path)?;
    file.write_all(&obj)?;

    // Link with gcc (links libc for system and putenv)
    let status = Exec::shell(format!("gcc -o {} {}", output_path.display(), temp_obj_path.display())).join()?;
    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "Linking failed"));
    }

    fs::remove_file(temp_obj_path)?;

    Ok(())
}
