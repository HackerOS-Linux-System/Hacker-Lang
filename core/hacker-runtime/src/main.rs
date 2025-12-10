use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs::{self};
use std::io::{self, Write};
use std::path::{PathBuf};
use std::process::Command;
use std::sync::Arc;

use miette::{Diagnostic, IntoDiagnostic, LabeledSpan, NamedSource, Report, SourceSpan};
use serde::Deserialize;
use serde_json::Value;
use tempfile;
use std::os::unix::fs::PermissionsExt;

const VERSION: &str = "1.1";
const HACKER_DIR: &str = "~/.hackeros/hacker-lang";
const BIN_DIR: &str = "~/.hackeros/hacker-lang/bin";

#[derive(Deserialize)]
struct Parsed {
    deps: Vec<String>,
    libs: Vec<String>,
    vars: HashMap<String, String>,
    local_vars: HashMap<String, String>,
    cmds: Vec<String>,
    cmds_with_vars: Vec<String>,
    cmds_separate: Vec<String>,
    includes: Vec<String>,
    binaries: Vec<String>,
    errors: Vec<String>,
    config: HashMap<String, String>,
    plugins: Vec<HashMap<String, Value>>,
    memory: String,
    memory_commands: Vec<String>,
}

#[derive(Debug)]
struct ParseErrors {
    src: NamedSource<Arc<str>>,
    spans: Vec<(SourceSpan, String)>,
}

impl fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "There were {} parse errors in the file", self.spans.len())
    }
}

impl std::error::Error for ParseErrors {}

impl Diagnostic for ParseErrors {
    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.src as &dyn miette::SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(self.spans.iter().map(|(span, msg)| {
            LabeledSpan::new_with_span(Some(msg.clone()), *span)
        })))
    }
}

fn expand_home(path: &str) -> PathBuf {
    if path.starts_with('~') {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(path.replacen('~', &home, 1));
        }
    }
    PathBuf::from(path)
}

fn ensure_hacker_dir() -> io::Result<()> {
    fs::create_dir_all(expand_home(BIN_DIR))?;
    fs::create_dir_all(expand_home(&format!("{}/libs", HACKER_DIR)))?;
    Ok(())
}

fn get_span(source: &str, line_num: usize) -> Option<SourceSpan> {
    let mut current_line = 1;
    let mut start = None;

    for (i, c) in source.char_indices() {
        if current_line == line_num {
            if start.is_none() {
                start = Some(i);
            }
            if c == '\n' {
                let len = i - start.unwrap();
                return Some(SourceSpan::new(start.unwrap().into(), len.into()));
            }
        } else if c == '\n' {
            current_line += 1;
        }
    }

    if let Some(s) = start {
        let len = source.len() - s;
        Some(SourceSpan::new(s.into(), len.into()))
    } else {
        None
    }
}

fn run_command(file: &str, verbose: bool) -> miette::Result<()> {
    let source = fs::read_to_string(file).into_diagnostic()?;

    let parser_path = expand_home(&format!("{}/hacker-plsa", BIN_DIR));
    let mut cmd = Command::new(&parser_path);
    cmd.arg(file);
    if verbose {
        cmd.arg("--verbose");
    }
    let output = cmd.output().into_diagnostic()?;
    if !output.status.success() {
        return Err(miette::miette!(
            "Error parsing file: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let json_str = String::from_utf8(output.stdout).into_diagnostic()?;
    let parsed: Parsed = serde_json::from_str(&json_str).into_diagnostic()?;

    let mut config = parsed.config;
    if config.is_empty() {
        let config_file = ".hacker-config";
        if let Ok(content) = fs::read_to_string(config_file) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('!') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    config.insert(key.trim().to_string(), value.trim().to_string());
                }
            }
        }
    }

    if !parsed.errors.is_empty() {
        let mut errs = vec![];
        for e in parsed.errors {
            if let Some((line_part, msg)) = e.split_once(": ") {
                if line_part.starts_with("Line ") {
                    if let Ok(line_num) = line_part[5..].parse::<usize>() {
                        errs.push((line_num, msg.to_string()));
                        continue;
                    }
                }
            }
            // Fallback for unparseable error
            return Err(miette::miette!(e));
        }

        let mut spans = vec![];
        for (line_num, msg) in errs {
            if let Some(span) = get_span(&source, line_num) {
                spans.push((span, msg));
            } else {
                spans.push((
                    SourceSpan::from((0, 0)),
                    format!("Line {} out of range: {}", line_num, msg),
                ));
            }
        }

        let diag = ParseErrors {
            src: NamedSource::new(file, Arc::from(source)),
            spans,
        };
        return Err(Report::new(diag));
    }

    // Create main temp script
    let mut temp_sh = tempfile::Builder::new()
        .suffix(".sh")
        .tempfile()
        .into_diagnostic()?;
    temp_sh.write_all(b"#!/bin/bash\n").into_diagnostic()?;
    temp_sh.write_all(b"set -e\n").into_diagnostic()?;
    if parsed.memory == "manual" {
        for mc in &parsed.memory_commands {
            writeln!(temp_sh, "{}", mc).into_diagnostic()?;
        }
    }
    for (k, v) in &parsed.vars {
        writeln!(temp_sh, "export {}=\"{}\"", k, v).into_diagnostic()?;
    }
    for (k, v) in &parsed.local_vars {
        writeln!(temp_sh, "{}=\"{}\"", k, v).into_diagnostic()?;
    }
    for dep in &parsed.deps {
        if dep != "sudo" {
            writeln!(
                temp_sh,
                "command -v {} &> /dev/null || (sudo apt update && sudo apt install -y {})",
                dep, dep
            )
            .into_diagnostic()?;
        }
    }
    for inc in &parsed.includes {
        let lib_path = expand_home(&format!("{}/libs/{}/main.hacker", HACKER_DIR, inc));
        writeln!(temp_sh, "# Included from {}", inc).into_diagnostic()?;
        let lib_content = fs::read(&lib_path).into_diagnostic()?;
        temp_sh.write_all(&lib_content).into_diagnostic()?;
        temp_sh.write_all(b"\n").into_diagnostic()?;
    }
    for cmd in &parsed.cmds {
        writeln!(temp_sh, "{}", cmd).into_diagnostic()?;
    }
    for cmd in &parsed.cmds_with_vars {
        writeln!(temp_sh, "{}", cmd).into_diagnostic()?;
    }
    for bin in &parsed.binaries {
        writeln!(temp_sh, "{}", bin).into_diagnostic()?;
    }
    for plugin in &parsed.plugins {
        let path = plugin
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| miette::miette!("Invalid plugin path"))?;
        let is_super = plugin
            .get("super")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| miette::miette!("Invalid plugin super"))?;
        let cmd_str = if is_super {
            format!("sudo {} &", path)
        } else {
            format!("{} &", path)
        };
        writeln!(temp_sh, "{}", cmd_str).into_diagnostic()?;
    }
    temp_sh.flush().into_diagnostic()?;
    let temp_path = temp_sh.path().to_path_buf();
    fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755)).into_diagnostic()?;

    // Separate scripts
    let mut separate_temps: Vec<PathBuf> = vec![];
    for (i, sep_cmd) in parsed.cmds_separate.iter().enumerate() {
        let mut sep_temp = tempfile::Builder::new()
            .prefix(&format!("sep_{}_", i))
            .suffix(".sh")
            .tempfile()
            .into_diagnostic()?;
        sep_temp
            .write_all(b"#!/bin/bash\nset -e\n")
            .into_diagnostic()?;
        if parsed.memory == "manual" {
            for mc in &parsed.memory_commands {
                writeln!(sep_temp, "{}", mc).into_diagnostic()?;
            }
        }
        for (k, v) in &parsed.vars {
            writeln!(sep_temp, "export {}=\"{}\"", k, v).into_diagnostic()?;
        }
        for (k, v) in &parsed.local_vars {
            writeln!(sep_temp, "{}=\"{}\"", k, v).into_diagnostic()?;
        }
        writeln!(sep_temp, "{}", sep_cmd).into_diagnostic()?;
        sep_temp.flush().into_diagnostic()?;
        let sep_path = sep_temp.path().to_path_buf();
        fs::set_permissions(&sep_path, fs::Permissions::from_mode(0o755)).into_diagnostic()?;
        separate_temps.push(sep_path);
    }

    println!("Executing script: {}", file);
    println!("Config: {:?}", config);
    println!("Running...");

    // Run separate scripts
    for sep_path in &separate_temps {
        let mut run_sep = Command::new("bash");
        run_sep.arg(sep_path);
        let mut envs: HashMap<String, String> = env::vars().collect();
        for (k, v) in &parsed.vars {
            envs.insert(k.clone(), v.clone());
        }
        run_sep.envs(&envs);
        run_sep.stdout(std::process::Stdio::inherit());
        run_sep.stderr(std::process::Stdio::inherit());
        let status = run_sep.status().into_diagnostic()?;
        if !status.success() {
            return Err(miette::miette!("Separate command execution failed"));
        }
    }

    // Run main script
    let mut run_cmd = Command::new("bash");
    run_cmd.arg(&temp_path);
    let mut envs: HashMap<String, String> = env::vars().collect();
    for (k, v) in &parsed.vars {
        envs.insert(k.clone(), v.clone());
    }
    run_cmd.envs(&envs);
    run_cmd.stdout(std::process::Stdio::inherit());
    run_cmd.stderr(std::process::Stdio::inherit());
    let status = run_cmd.status().into_diagnostic()?;
    if !status.success() {
        return Err(miette::miette!("Execution failed"));
    }

    println!("Execution completed successfully!");
    Ok(())
}

fn inner_main() -> miette::Result<()> {
    ensure_hacker_dir().into_diagnostic()?;

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: hacker-runtime <file> [--verbose]");
        std::process::exit(1);
    }
    let file = &args[1];
    let verbose = args.len() > 2 && args[2] == "--verbose";

    run_command(file, verbose)?;
    Ok(())
}

fn main() {
    if let Err(err) = inner_main() {
        eprintln!("{:?}", err);
        std::process::exit(1);
    }
    std::process::exit(0);
}
