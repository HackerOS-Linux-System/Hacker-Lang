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

#[derive(Debug, Deserialize, Serialize)]
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
    // Added for SA output
    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_errors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_vars: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_functions: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<HashMap<String, String>>, // Simple type map: var -> type
}

#[derive(Debug, Clone, PartialEq)]
enum VarType {
    Global,
    Local,
    FunctionArg,
}

impl ParseResult {
    fn new() -> Self {
        Self {
            deps: Vec::new(),
            libs: Vec::new(),
            vars: HashMap::new(),
            local_vars: HashMap::new(),
            cmds: Vec::new(),
            cmds_with_vars: Vec::new(),
            cmds_separate: Vec::new(),
            includes: Vec::new(),
            binaries: Vec::new(),
            plugins: Vec::new(),
            functions: HashMap::new(),
            errors: Vec::new(),
            config: HashMap::new(),
            semantic_errors: None,
            resolved_vars: None,
            resolved_functions: None,
            types: None,
        }
    }

    fn merge(&mut self, other: ParseResult) {
        self.deps.extend(other.deps);
        self.libs.extend(other.libs);
        self.vars.extend(other.vars);
        self.local_vars.extend(other.local_vars);
        self.cmds.extend(other.cmds);
        self.cmds_with_vars.extend(other.cmds_with_vars);
        self.cmds_separate.extend(other.cmds_separate);
        self.includes.extend(other.includes);
        self.binaries.extend(other.binaries);
        self.plugins.extend(other.plugins);
        for (k, v) in other.functions {
            self.functions.entry(k).or_insert_with(Vec::new).extend(v);
        }
        self.errors.extend(other.errors);
        self.config.extend(other.config);
    }
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

fn analyze_semantics(mut parse_result: ParseResult) -> Result<ParseResult> {
    let mut semantic_errors = Vec::new();
    let mut resolved_vars = HashMap::new();
    let mut resolved_functions = HashMap::new();
    let mut types = HashMap::new();

    // Scope resolution for variables
    let mut global_scope = HashMap::new();
    global_scope.extend(parse_result.vars.clone());

    for (var_name, var_value) in &parse_result.vars {
        // Simple type inference: if value starts with number, int; else string
        let inferred_type = if let Ok(_) = var_value.parse::<i32>() {
            "int".to_string()
        } else {
            "string".to_string()
        };
        types.insert(var_name.clone(), inferred_type);
        resolved_vars.insert(var_name.clone(), var_value.clone());
    }

    // Local vars in cmds_with_vars - check usage
    for cmd in &parse_result.cmds_with_vars {
        for (var_name, _) in &parse_result.local_vars {
            if cmd.contains(&format!("${}", var_name)) {
                if !global_scope.contains_key(var_name) && !parse_result.local_vars.contains_key(var_name) {
                    semantic_errors.push(format!("Undefined variable in cmd: ${}", var_name));
                }
            }
        }
    }

    // Function resolution
    for (func_name, func_cmds) in &parse_result.functions {
        let mut func_resolved = Vec::new();
        for cmd in func_cmds {
            // Resolve vars in function cmds
            let mut resolved_cmd = cmd.clone();
            for (var_name, var_value) in &parse_result.vars {
                resolved_cmd = resolved_cmd.replace(&format!("${}", var_name), var_value);
            }
            func_resolved.push(resolved_cmd);
        }
        resolved_functions.insert(func_name.clone(), func_resolved);
    }

    // Check function calls
    for cmd in &parse_result.cmds {
        if cmd.starts_with('.') {
            let func_name = cmd[1..].trim().to_string();
            if !parse_result.functions.contains_key(&func_name) {
                semantic_errors.push(format!("Undefined function call: {}", func_name));
            }
        }
    }

    // Deps and libs consistency - simple check
    for dep in &parse_result.deps {
        if dep == "sudo" {
            if !parse_result.plugins.iter().any(|p| p.is_super) {
                semantic_errors.push("Warning: sudo dep but no super plugins".to_string());
            }
        }
    }

    // Config validation
    for (key, value) in &parse_result.config {
        if key == "version" && value.parse::<f32>().is_err() {
            semantic_errors.push(format!("Invalid config version: {}", value));
        }
    }

    // Foreign libs handling - assume #> in libs
    for lib in &parse_result.libs {
        if lib.starts_with("python:") || lib.starts_with("rust:") || lib.starts_with("java:") {
            // Validate prefix
            types.insert(format!("lib:{}", lib), "foreign".to_string());
        }
    }

    parse_result.semantic_errors = if semantic_errors.is_empty() { None } else { Some(semantic_errors) };
    parse_result.resolved_vars = Some(resolved_vars);
    parse_result.resolved_functions = Some(resolved_functions);
    parse_result.types = Some(types);

    if parse_result.semantic_errors.as_ref().map_or(false, |e| !e.is_empty()) {
        eprintln!("\x1b[31m\x1b[1mSemantic Errors:\x1b[0m");
        if let Some(errors) = &parse_result.semantic_errors {
            for err in errors {
                eprintln!(" \x1b[31m✖ \x1b[0m{}", err);
            }
        }
        // Continue but flag
    }

    Ok(parse_result)
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

    let analyzed = analyze_semantics(parse_result)?;
    if verbose {
        println!("{:#?}", analyzed);
    }
    let output = serde_json::to_string_pretty(&analyzed)?;
    println!("{}", output);
    Ok(())
}

