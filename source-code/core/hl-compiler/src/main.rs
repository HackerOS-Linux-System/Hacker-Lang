use clap::Parser;
use colored::*;
use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::OptimizationLevel;
use inkwell::AddressSpace;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, exit};

const PLSA_BIN_NAME: &str = "hl-plsa";

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    file: String,
    #[arg(short, long)]
    output: Option<String>,
    #[arg(long)]
    verbose: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub enum CommandType {
    Raw(String),
    AssignEnv { key: String, val: String },
    AssignLocal { key: String, val: String },
    Loop { count: u64, cmd: String },
    If { cond: String, cmd: String },
    Background(String),
    Plugin { name: String, is_super: bool },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProgramNode {
    pub content: CommandType,
    pub is_sudo: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisResult {
    pub deps: Vec<String>,
    pub main_body: Vec<ProgramNode>,
    pub functions: HashMap<String, Vec<ProgramNode>>,
}

fn get_plsa_path() -> PathBuf {
    let home = dirs::home_dir().expect("Failed to determine home directory");
    let path = home.join(".hackeros/hacker-lang/bin").join(PLSA_BIN_NAME);
    if !path.exists() {
        eprintln!("{} Critical Error: {} not found at {:?}", "[x]".red(), PLSA_BIN_NAME, path);
        exit(127);
    }
    path
}

fn main() {
    let args = Args::parse();

    // 1. Get AST from PLSA
    let plsa_path = get_plsa_path();
    let output = Command::new(&plsa_path)
    .arg(&args.file)
    .arg("--json")
    .output()
    .expect(&format!("Failed to run hl-plsa at {:?}", plsa_path));

    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        exit(1);
    }

    let ast: AnalysisResult = serde_json::from_slice(&output.stdout).expect("JSON error");

    if args.verbose { println!("{} AST Loaded. Generating LLVM IR...", "[*]".green()); }

    // 2. Initialize LLVM
    let context = Context::create();
    let module = context.create_module("hacker_module");
    let builder = context.create_builder();

    let i32_type = context.i32_type();
    let i8_ptr_type = context.ptr_type(AddressSpace::default());

    // External 'system' function: int system(char*)
    let system_type = i32_type.fn_type(&[i8_ptr_type.into()], false);
    let system_fn = module.add_function("system", system_type, Some(Linkage::External));

    // Compile Function Definition Helper
    let compile_cmds = |ops: &Vec<ProgramNode>| {
        for node in ops {
            match &node.content {
                CommandType::Raw(cmd) | CommandType::Background(cmd) => {
                    // Create global string const
                    let final_cmd = if node.is_sudo { format!("sudo {}", cmd) } else { cmd.clone() };
                    let cmd_str = context.const_string(final_cmd.as_bytes(), true);
                    let global_cmd = module.add_global(cmd_str.get_type(), None, "cmd_str");
                    global_cmd.set_initializer(&cmd_str);
                    global_cmd.set_linkage(Linkage::Internal);

                    let zero = context.i64_type().const_int(0, false);
                    let ptr = unsafe {
                        builder.build_gep(cmd_str.get_type(), global_cmd.as_pointer_value(), &[zero, zero], "cmd_ptr")
                    }.unwrap();

                    builder.build_call(system_fn, &[ptr.into()], "call_system");
                },
                CommandType::Loop { count, cmd } => {
                    for _ in 0..*count {
                        let final_cmd = if node.is_sudo { format!("sudo {}", cmd) } else { cmd.clone() };
                        let cmd_str = context.const_string(final_cmd.as_bytes(), true);
                        let global_cmd = module.add_global(cmd_str.get_type(), None, "loop_cmd_str");
                        global_cmd.set_initializer(&cmd_str);
                        let zero = context.i64_type().const_int(0, false);
                        let ptr = unsafe { builder.build_gep(cmd_str.get_type(), global_cmd.as_pointer_value(), &[zero, zero], "") }.unwrap();
                        builder.build_call(system_fn, &[ptr.into()], "");
                    }
                },
                _ => { }
            }
        }
    };

    // 3. Compile Main
    let main_type = i32_type.fn_type(&[], false);
    let main_fn = module.add_function("main", main_type, None);
    let entry_block = context.append_basic_block(main_fn, "entry");
    builder.position_at_end(entry_block);

    compile_cmds(&ast.main_body);

    builder.build_return(Some(&i32_type.const_int(0, false)));

    // 4. Emit Object File
    Target::initialize_native(&InitializationConfig::default()).unwrap();
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).unwrap();
    let tm = target.create_target_machine(
        &triple, "generic", "",
        OptimizationLevel::Aggressive, RelocMode::PIC, CodeModel::Default
    ).unwrap();

    let output_path = args.output.unwrap_or_else(|| "a.out".to_string());
    let obj_path = format!("{}.o", output_path);

    tm.write_to_file(&module, FileType::Object, std::path::Path::new(&obj_path)).unwrap();

    // 5. Link
    if args.verbose { println!("{} Linking...", "[*]".green()); }
    let status = Command::new("gcc")
    .arg(&obj_path)
    .arg("-o")
    .arg(&output_path)
    .status()
    .expect("Failed to run gcc linker");

    if status.success() {
        if args.verbose { println!("{} Compilation successful: {}", "[+]".green(), output_path); }
        let _ = std::fs::remove_file(obj_path);
    } else {
        eprintln!("{} Linking failed", "[x]".red());
    }
}
