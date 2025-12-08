use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Plugin {
    path: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_errors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_vars: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_functions: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
enum AstNode {
    Program(Box<ProgramNode>),
    Function(Box<FunctionNode>),
    Command(Box<CommandNode>),
    VarDecl(Box<VarDeclNode>),
    LocalVarDecl(Box<LocalVarDeclNode>),
    Dep(Box<DepNode>),
    Lib(Box<LibNode>),
    Plugin(Box<PluginNode>),
    Loop(Box<LoopNode>),
    Conditional(Box<ConditionalNode>),
    Background(Box<BackgroundNode>),
    Include(Box<IncludeNode>),
    Binary(Box<BinaryNode>),
    Config(Box<ConfigNode>),
    FunctionCall(Box<FunctionCallNode>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProgramNode {
    deps: Vec<AstNode>,
    includes: Vec<AstNode>,
    binaries: Vec<AstNode>,
    plugins: Vec<AstNode>,
    vars: Vec<AstNode>,
    functions: HashMap<String, AstNode>,
    config: AstNode,
    body: Vec<AstNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FunctionNode {
    name: String,
    body: Vec<AstNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandNode {
    cmd: String,
    is_super: bool,
    kind: CommandKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum CommandKind {
    Direct,
    WithVars,
    Separate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VarDeclNode {
    name: String,
    value: String,
    typ: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalVarDeclNode {
    name: String,
    value: String,
    typ: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DepNode {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LibNode {
    name: String,
    is_foreign: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginNode {
    path: String,
    is_super: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoopNode {
    count: usize,
    body: AstNode, // Command
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConditionalNode {
    condition: String,
    body: AstNode, // Command
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackgroundNode {
    cmd: String,
    is_super: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IncludeNode {
    lib: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BinaryNode {
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConfigNode {
    entries: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FunctionCallNode {
    name: String,
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

fn build_ast_from_parse(parse_result: ParseResult) -> Result<AstNode> {
    let mut program = ProgramNode {
        deps: Vec::new(),
        includes: Vec::new(),
        binaries: Vec::new(),
        plugins: Vec::new(),
        vars: Vec::new(),
        functions: HashMap::new(),
        config: AstNode::Config(Box::new(ConfigNode {
            entries: parse_result.config.clone(),
        })),
        body: Vec::new(),
    };

    // Build deps
    for dep in parse_result.deps {
        program.deps.push(AstNode::Dep(Box::new(DepNode { name: dep })));
    }

    // Build libs/includes
    for lib in parse_result.libs {
        let is_foreign = lib.starts_with("python:") || lib.starts_with("rust:") || lib.starts_with("java:");
        program.includes.push(AstNode::Lib(Box::new(LibNode {
            name: lib,
            is_foreign,
        })));
    }
    for inc in parse_result.includes {
        program.includes.push(AstNode::Include(Box::new(IncludeNode { lib: inc })));
    }

    // Build binaries
    for bin in parse_result.binaries {
        program.binaries.push(AstNode::Binary(Box::new(BinaryNode { path: bin })));
    }

    // Build plugins
    for p in parse_result.plugins {
        program.plugins.push(AstNode::Plugin(Box::new(PluginNode {
            path: p.path,
            is_super: p.is_super,
        })));
    }

    // Build vars
    for (name, value) in parse_result.vars {
        let typ = parse_result.types.as_ref().and_then(|t| t.get(&name).cloned());
        program.vars.push(AstNode::VarDecl(Box::new(VarDeclNode { name, value, typ })));
    }

    // Build local vars - treat as part of body
    for (name, value) in parse_result.local_vars {
        let typ = parse_result.types.as_ref().and_then(|t| t.get(&name).cloned());
        program.body.push(AstNode::LocalVarDecl(Box::new(LocalVarDeclNode { name, value, typ })));
    }

    // Build functions
    for (fname, fcmds) in parse_result.functions {
        let mut fbody = Vec::new();
        for cmd in fcmds {
            fbody.push(build_command_node(&cmd, false)?);
        }
        let func_node = AstNode::Function(Box::new(FunctionNode { name: fname.clone(), body: fbody }));
        program.functions.insert(fname.clone(), func_node.clone());
        // Also add to body if called, but for now, just store
    }

    // Build body from cmds
    for cmd in parse_result.cmds {
        program.body.push(build_command_node(&cmd, false)?);
    }
    for cmd in parse_result.cmds_with_vars {
        program.body.push(build_command_node(&cmd, false)?);
    }
    for cmd in parse_result.cmds_separate {
        program.body.push(build_command_node(&cmd, true)?); // Separate as special
    }

    // Handle resolved functions if present
    if let Some(res_funcs) = parse_result.resolved_functions {
        for (fname, rcmds) in res_funcs {
            if !program.functions.contains_key(&fname) {
                let mut fbody = Vec::new();
                for cmd in rcmds {
                    fbody.push(build_command_node(&cmd, false)?);
                }
                let func_node = AstNode::Function(Box::new(FunctionNode { name: fname.clone(), body: fbody }));
                program.functions.insert(fname, func_node);
            }
        }
    }

    // Function calls in body - but since cmds are strings, assume . calls are handled in build_command_node
    Ok(AstNode::Program(Box::new(program)))
}

fn build_command_node(cmd_str: &str, is_separate: bool) -> Result<AstNode> {
    if cmd_str.starts_with('.') {
        // Function call
        let name = cmd_str[1..].trim().to_string();
        Ok(AstNode::FunctionCall(Box::new(FunctionCallNode { name })))
    } else if cmd_str.starts_with('=') {
        // Loop
        let parts: Vec<&str> = cmd_str[1..].split('>').collect();
        if parts.len() == 2 {
            let count = parts[0].trim().parse::<usize>().unwrap_or(0);
            let body_cmd = parts[1].trim().to_string();
            let body = build_command_node(&body_cmd, false)?;
            Ok(AstNode::Loop(Box::new(LoopNode { count, body })))
        } else {
            Err(anyhow::anyhow!("Invalid loop syntax"))
        }
    } else if cmd_str.starts_with('?') {
        // Conditional
        let parts: Vec<&str> = cmd_str[1..].split('>').collect();
        if parts.len() == 2 {
            let condition = parts[0].trim().to_string();
            let body_cmd = parts[1].trim().to_string();
            let body = build_command_node(&body_cmd, false)?;
            Ok(AstNode::Conditional(Box::new(ConditionalNode { condition, body })))
        } else {
            Err(anyhow::anyhow!("Invalid conditional syntax"))
        }
    } else if cmd_str.ends_with(" &") {
        // Background
        let cmd = cmd_str.trim_end_matches(" &").to_string();
        let is_super = cmd.starts_with("sudo ");
        Ok(AstNode::Background(Box::new(BackgroundNode {
            cmd,
            is_super,
        })))
    } else {
        // Regular command
        let kind = if is_separate { CommandKind::Separate } else { CommandKind::Direct };
        let is_super = cmd_str.starts_with("sudo ");
        let cmd = if is_super { cmd_str[5..].to_string() } else { cmd_str.to_string() };
        Ok(AstNode::Command(Box::new(CommandNode { cmd, is_super, kind })))
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut verbose = false;
    let mut input_path = None;
    for arg in args.iter().skip(1) {
        if arg == "--verbose" {
            verbose = true;
        } else {
            input_path = Some(arg.clone());
        }
    }

    let parse_result = if let Some(path_str) = input_path {
        let path = Path::new(&path_str);
        let content = fs::read_to_string(path)?;
        serde_json::from_str(&content).context("Failed to parse input JSON file")?
    } else {
        read_parse_result()?
    };

    if !parse_result.errors.is_empty() || parse_result.semantic_errors.as_ref().map_or(false, |e| !e.is_empty()) {
        eprintln!("\x1b[31m\x1b[1mErrors present, skipping AST build.\x1b[0m");
        // Output the parse_result as is, or empty AST
        let empty_ast = AstNode::Program(Box::new(ProgramNode {
            deps: vec![],
            includes: vec![],
            binaries: vec![],
            plugins: vec![],
            vars: vec![],
            functions: HashMap::new(),
                                                  config: AstNode::Config(Box::new(ConfigNode { entries: HashMap::new() })),
                                                  body: vec![],
        }));
        let output = serde_json::to_string_pretty(&empty_ast)?;
        println!("{}", output);
        return Ok(());
    }

    let ast = build_ast_from_parse(parse_result)?;
    if verbose {
        println!("{:#?}", ast);
    }
    let output = serde_json::to_string_pretty(&ast)?;
    println!("{}", output);
    Ok(())
}

