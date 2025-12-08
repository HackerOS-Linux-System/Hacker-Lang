use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use cranelift::prelude::*;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa;
use cranelift_codegen::settings;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;
use subprocess::{Exec, PopenError};

const HACKER_DIR: &str = "~/.hackeros/hacker-lang";

fn expand_home(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = env::var_os("HOME") {
            return path.replacen("~", home.to_str().unwrap_or(""), 1);
        }
    }
    path.to_string()
}

#[derive(Debug, Clone, Deserialize)]
struct Plugin {
    path: String,
    #[serde(rename = "super")]
    is_super: bool,
}

#[derive(Debug, Deserialize)]
struct ParseResult {
    deps: Vec<String>,
    libs: Vec<String>,
    vars: HashMap<String, String>,
    local_vars: HashMap<String, String>,
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

fn read_parse_result() -> Result<ParseResult> {
    let mut input = String::new();
    for line in io::stdin().lock().lines() {
        input.push_str(&line?);
        input.push('\n');
    }
    let result: ParseResult = serde_json::from_str(&input)
    .context("Failed to parse JSON input from stdin")?;
    Ok(result)
}

fn substitute_vars(cmd: &str, local_vars: &HashMap<String, String>) -> String {
    let mut sub_cmd = cmd.to_string();
    for (k, v) in local_vars {
        sub_cmd = sub_cmd.replace(&format!("${}", k), v);
    }
    sub_cmd
}

fn generate_check_cmd(dep: &str) -> String {
    if dep == "sudo" {
        return String::new();
    }
    format!("command -v {} &> /dev/null || (sudo apt update && sudo apt install -y {})", dep, dep)
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut verbose = false;
    let mut bytes_mode = false;
    let mut output_path_str = String::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bytes" => bytes_mode = true,
            "--verbose" => verbose = true,
            _ => {
                if output_path_str.is_empty() {
                    output_path_str = args[i].clone();
                }
            }
        }
        i += 1;
    }
    if output_path_str.is_empty() {
        eprintln!("Usage: hacker-compiler <output> [--verbose] [--bytes]");
        eprintln!("Reads ParseResult JSON from stdin.");
        process::exit(1);
    }
    let output_path = Path::new(&output_path_str);
    let mut parse_result = read_parse_result()?;
    // Load config if empty
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
            eprintln!(" \x1b[31m✖ \x1b[0m{}", err);
        }
        eprintln!();
        process::exit(1);
    }
    // Substitute local vars in all command types
    let mut direct_cmds = cmds.clone();
    direct_cmds.extend(cmds_with_vars.clone());
    let mut substituted_direct: Vec<String> = Vec::new();
    for cmd in direct_cmds {
        let sub_cmd = substitute_vars(&cmd, &local_vars);
        substituted_direct.push(sub_cmd);
    }
    let mut substituted_separate: Vec<String> = Vec::new();
    for cmd in cmds_separate.clone() {
        let sub_cmd = substitute_vars(&cmd, &local_vars);
        substituted_separate.push(sub_cmd);
    }
    // Build final command list
    let mut final_cmds: Vec<String> = Vec::new();
    for dep in &deps {
        let check = generate_check_cmd(dep);
        if !check.is_empty() {
            final_cmds.push(check);
        }
    }
    final_cmds.extend(substituted_direct);
    // Separate file commands (>>>)
    for sub_cmd in substituted_separate {
        let script = format!("#!/bin/bash\nset -e\n{}\n", sub_cmd);
        let encoded = BASE64_STANDARD.encode(script.as_bytes());
        let extract_cmd = format!(
            "temp=$(mktemp /tmp/hacker_cmd.XXXXXX); echo '{}' | base64 -d > $temp && chmod +x $temp && $temp",
                                  encoded
        );
        final_cmds.push(extract_cmd);
    }
    // Binary libraries
    for bin_path in binaries {
        let bin_data = fs::read(&bin_path)?;
        let encoded = BASE64_STANDARD.encode(&bin_data);
        let extract_cmd = format!(
            "temp=$(mktemp /tmp/hacker_bin.XXXXXX); echo '{}' | base64 -d > $temp && chmod +x $temp && $temp",
                                  encoded
        );
        final_cmds.push(extract_cmd);
    }
    // Plugins
    for plugin in plugins {
        let plugin_path = expand_home(&format!("{}/plugins/{}", HACKER_DIR, plugin.path));
        let plugin_data = fs::read(&plugin_path)?;
        let encoded = BASE64_STANDARD.encode(&plugin_data);
        let mut run_cmd = " $temp &".to_string();
        if plugin.is_super {
            run_cmd = format!("sudo{}", run_cmd);
        }
        let extract_cmd = format!(
            "temp=$(mktemp /tmp/hacker_plugin.XXXXXX); echo '{}' | base64 -d > $temp && chmod +x $temp &&{}",
                                  encoded, run_cmd
        );
        final_cmds.push(extract_cmd);
    }
    // Cranelift setup for IR generation
    let mut flag_builder = settings::builder();
    flag_builder
    .set("is_pic", "true")
    .expect("Failed to set is_pic");
    let flags = settings::Flags::new(flag_builder);
    let triple = target_lexicon::Triple::host();
    let isa_builder = isa::lookup(triple).expect("Host not supported");
    let isa = isa_builder
    .finish(flags)
    .expect("ISA build failed");
    let builder = ObjectBuilder::new(
        isa,
        output_path
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .as_bytes()
        .to_vec(),
                                     cranelift_module::default_libcall_names(),
    )
    .expect("ObjectBuilder failed");
    let mut module = ObjectModule::new(builder);
    let pointer_type = module.target_config().pointer_type();
    // Declare external functions
    let mut sig_system = module.make_signature();
    sig_system.params.push(AbiParam::new(pointer_type));
    sig_system.returns.push(AbiParam::new(types::I32));
    sig_system.call_conv = module.target_config().default_call_conv;
    let system_id = module
    .declare_function("system", Linkage::Import, &sig_system)
    .unwrap();
    let mut sig_putenv = module.make_signature();
    sig_putenv.params.push(AbiParam::new(pointer_type));
    sig_putenv.returns.push(AbiParam::new(types::I32));
    sig_putenv.call_conv = module.target_config().default_call_conv;
    let putenv_id = module
    .declare_function("putenv", Linkage::Import, &sig_putenv)
    .unwrap();
    // Main function
    let mut sig_main = module.make_signature();
    sig_main.returns.push(AbiParam::new(types::I32));
    sig_main.call_conv = module.target_config().default_call_conv;
    let main_id = module
    .declare_function("main", Linkage::Export, &sig_main)
    .unwrap();
    let mut ctx = cranelift_codegen::Context::for_function(Function::with_name_signature(
        Default::default(),
                                                                                         sig_main.clone(),
    ));
    let mut func_builder_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut ctx.func, &mut func_builder_ctx);
    let entry_block = builder.create_block();
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);
    let local_system = module.declare_func_in_func(system_id, &mut builder.func);
    let local_putenv = module.declare_func_in_func(putenv_id, &mut builder.func);
    // Embed environment variables (@vars) - now from HashMap
    let mut var_data_ids = Vec::new();
    for (var, value) in &vars {
        let env_str = format!("{}={}", var, value);
        let data_name = format!("env_{}", var_data_ids.len());
        let data_id = module
        .declare_data(&data_name, Linkage::Local, true, false)
        .unwrap();
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
        let data_name = format!("cmd_{}", i);
        let data_id = module
        .declare_data(&data_name, Linkage::Local, true, false)
        .unwrap();
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
    let obj = module
    .finish()
    .object
    .write()
    .expect("Write object failed");
    let temp_obj_path = output_path.with_extension("o");
    let mut file = File::create(&temp_obj_path)?;
    file.write_all(&obj)?;
    let status = Exec::shell(format!(
        "gcc -o {} {}",
        output_path.display(),
                                     temp_obj_path.display()
    ))
    .join()
    .map_err(|e: PopenError| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    if !status.success() {
        return Err(anyhow::anyhow!("Linking failed"));
    }
    fs::remove_file(temp_obj_path)?;
    if verbose {
        println!("Compilation successful: {}", output_path.display());
    }
    Ok(())
}
