use clap::Parser;
use colored::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command};
use std::time::Instant;

const CACHE_DIR: &str = "/tmp/Hacker-Lang/cache";
const PLSA_BIN_NAME: &str = "hl-plsa";

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    file: String,
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    unsafe_mode: bool,
}

// Mirroring PLSA structures for Deserialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandType {
    Raw(String),
    AssignEnv { key: String, val: String },
    AssignLocal { key: String, val: String },
    Loop { count: u64, cmd: String },
    If { cond: String, cmd: String },
    Background(String),
    Plugin { name: String, is_super: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramNode {
    pub line_num: usize,
    pub is_sudo: bool,
    pub content: CommandType,
    pub original_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub deps: Vec<String>,
    pub libs: Vec<String>,
    pub functions: HashMap<String, Vec<ProgramNode>>,
    pub main_body: Vec<ProgramNode>,
    pub is_safe: bool,
    pub requires_unsafe_flag: bool,
    pub safety_warnings: Vec<String>,
}

// --- Bytecode VM Structures ---

#[derive(Debug, Clone, Serialize, Deserialize)]
enum OpCode {
    Exec { cmd: String, sudo: bool },
    SetEnv { key: String, val: String },
    SetLocal { key: String, val: String },
    Jump { target: usize },
    JumpIfFalse { cond: String, target: usize },
    Call { func_name: String },
    Plugin { name: String, sudo: bool },
    Return,
    Exit,
}

#[derive(Serialize, Deserialize)]
struct BytecodeProgram {
    ops: Vec<OpCode>,
    functions: HashMap<String, usize>, // Func name -> Op index
}

fn get_plsa_path() -> PathBuf {
    let home = dirs::home_dir().expect("Failed to determine home directory");
    let path = home.join(".hackeros/hacker-lang/bin").join(PLSA_BIN_NAME);
    if !path.exists() {
        eprintln!("{} Critical Error: {} not found at {:?}", "[x]".red(), PLSA_BIN_NAME, path);
        std::process::exit(127);
    }
    path
}

fn compile_to_bytecode(ast: &AnalysisResult) -> BytecodeProgram {
    let mut ops = Vec::new();
    let mut functions = HashMap::new();

    // Compile Main
    for node in &ast.main_body {
        compile_node(node, &mut ops);
    }
    ops.push(OpCode::Exit);

    // Compile Functions
    for (name, nodes) in &ast.functions {
        functions.insert(name.clone(), ops.len());
        for node in nodes {
            compile_node(node, &mut ops);
        }
        ops.push(OpCode::Return);
    }

    BytecodeProgram { ops, functions }
}

fn compile_node(node: &ProgramNode, ops: &mut Vec<OpCode>) {
    match &node.content {
        CommandType::Raw(s) => {
            if s.starts_with("call:") {
                let fname = s.strip_prefix("call:").unwrap();
                ops.push(OpCode::Call { func_name: fname.to_string() });
            } else {
                ops.push(OpCode::Exec { cmd: s.clone(), sudo: node.is_sudo });
            }
        },
        CommandType::Loop { count, cmd } => {
            for _ in 0..*count {
                ops.push(OpCode::Exec { cmd: cmd.clone(), sudo: node.is_sudo });
            }
        },
        CommandType::If { cond, cmd } => {
            let cmd_str = format!("if {}; then {}; fi", cond, cmd);
            ops.push(OpCode::Exec { cmd: cmd_str, sudo: node.is_sudo });
        },
        CommandType::AssignEnv { key, val } => ops.push(OpCode::SetEnv { key: key.clone(), val: val.clone() }),
        CommandType::AssignLocal { key, val } => ops.push(OpCode::SetLocal { key: key.clone(), val: val.clone() }),
        CommandType::Plugin { name, is_super } => ops.push(OpCode::Plugin { name: name.clone(), sudo: *is_super }),
        _ => {}
    }
}

// --- VM Execution ---

struct VM {
    env: HashMap<String, String>,
    locals: HashMap<String, String>,
}

impl VM {
    fn new() -> Self {
        Self { env: std::env::vars().collect(), locals: HashMap::new() }
    }

    fn substitute(&self, text: &str) -> String {
        let mut res = text.to_string();
        for (k, v) in &self.locals {
            res = res.replace(&format!("${}", k), v);
        }
        for (k, v) in &self.env {
            res = res.replace(&format!("${}", k), v);
        }
        res
    }

    fn run(&mut self, prog: BytecodeProgram, verbose: bool) {
        let mut ip = 0;
        let mut call_stack = Vec::new();

        while ip < prog.ops.len() {
            match &prog.ops[ip] {
                OpCode::Exec { cmd, sudo } => {
                    let final_cmd = self.substitute(cmd);
                    if verbose { println!("{} Exec: {}", "[>]".cyan(), final_cmd); }

                    let status = if *sudo {
                        Command::new("sudo").arg("sh").arg("-c").arg(&final_cmd).status()
                    } else {
                        Command::new("sh").arg("-c").arg(&final_cmd).status()
                    };

                    if let Err(e) = status {
                        eprintln!("Command failed: {}", e);
                    }
                },
                OpCode::Call { func_name } => {
                    if let Some(addr) = prog.functions.get(func_name) {
                        call_stack.push(ip + 1);
                        ip = *addr;
                        continue;
                    } else {
                        eprintln!("Runtime Error: Function {} not found", func_name);
                        return;
                    }
                },
                OpCode::Return => {
                    if let Some(ret_addr) = call_stack.pop() {
                        ip = ret_addr;
                        continue;
                    }
                },
                OpCode::Exit => break,
                OpCode::SetEnv { key, val } => {
                    let v = self.substitute(val);
                    self.env.insert(key.clone(), v.clone());
                    std::env::set_var(key, v);
                },
                OpCode::SetLocal { key, val } => {
                    let v = self.substitute(val);
                    self.locals.insert(key.clone(), v);
                },
                OpCode::Plugin { name, .. } => {
                    if verbose { println!("Running plugin: {}", name); }
                },
                _ => {}
            }
            ip += 1;
        }
    }
}

fn get_file_hash(path: &str) -> String {
    let bytes = fs::read(path).unwrap_or(vec![]);
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn main() {
    let args = Args::parse();

    // 1. Check Cache
    fs::create_dir_all(CACHE_DIR).unwrap();
    let hash = get_file_hash(&args.file);
    let bc_path = PathBuf::from(CACHE_DIR).join(format!("{}.bc", hash));

    let program: BytecodeProgram;

    if bc_path.exists() {
        if args.verbose { println!("{} Cache hit. Loading bytecode.", "[*]".green()); }
        let data = fs::read(bc_path).unwrap();
        program = bincode::deserialize(&data).expect("Corrupt bytecode");
    } else {
        if args.verbose { println!("{} Cache miss. Analyzing source.", "[*]".yellow()); }

        // 2. Call HL-PLSA (absolute path)
        let plsa_path = get_plsa_path();
        let output = Command::new(&plsa_path)
        .arg(&args.file)
        .arg("--json")
        .output()
        .expect(&format!("Failed to run hl-plsa at {:?}", plsa_path));

        if !output.status.success() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            std::process::exit(1);
        }

        let ast: AnalysisResult = serde_json::from_slice(&output.stdout).expect("Invalid JSON from PLSA");

        // Safety check
        if ast.requires_unsafe_flag && !args.unsafe_mode {
            eprintln!("{} ERROR: Unsafe commands detected.", "[x]".red().bold());
            for w in ast.safety_warnings {
                eprintln!("  - {}", w);
            }
            eprintln!("Use --unsafe to run this script.");
            std::process::exit(1);
        }

        // 3. Compile to Bytecode
        program = compile_to_bytecode(&ast);

        // 4. Save Cache
        let bc_data = bincode::serialize(&program).unwrap();
        fs::write(bc_path, bc_data).unwrap();
    }

    // 5. Run VM
    let mut vm = VM::new();
    let start = Instant::now();
    vm.run(program, args.verbose);
    if args.verbose {
        println!("{} Execution time: {:?}", "[INFO]".blue(), start.elapsed());
    }
}
