use clap::Parser as ClapParser;
use colored::*;
use memmap2::MmapOptions;
use miette::{Diagnostic, IntoDiagnostic, NamedSource, SourceSpan};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use thiserror::Error;
use which::which;

const HACKER_DIR: &str = "~/.hackeros/hacker-lang";

#[derive(ClapParser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input file
    input: String,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Error, Debug, Diagnostic)]
enum RuntimeError {
    #[error("IO Error: {0}")]
    Io(#[from] io::Error),

    #[error("Parse Error: {0}")]
    Parse(String, #[label("here")] SourceSpan),

    #[error("Execution Error: {0}")]
    Execution(String),

    #[error("Dependency Missing: {0}")]
    DependencyMissing(String),
}

#[derive(Debug, Clone)]
enum Action {
    Cmd {
        raw: String,
        is_sudo: bool,
        is_background: bool,
    },
    AssignEnv {
        key: String,
        value: String,
    },
    AssignLocal {
        key: String,
        value: String,
    },
    If {
        condition: String,
        body: String,
        is_sudo: bool,
    },
    Loop {
        count: u32,
        body: String,
        is_sudo: bool,
    },
    Plugin {
        name: String,
        is_super: bool,
    },
}

#[derive(Debug, Default)]
struct Program {
    deps: Vec<String>,
    includes: Vec<String>, // recursed during parsing
    actions: Vec<Action>,
}

struct Interpreter {
    verbose: bool,
    env_vars: HashMap<String, String>,
    local_vars: HashMap<String, String>,
}

impl Interpreter {
    fn new(verbose: bool) -> Self {
        Self {
            verbose,
            env_vars: env::vars().collect(),
            local_vars: HashMap::new(),
        }
    }

    fn expand_home(path: &str) -> PathBuf {
        if path.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                return home.join(&path[2..]);
            }
        }
        PathBuf::from(path)
    }

    // High-performance variable substitution using unsafe unchecked access
    // This avoids multiple allocations and repeated string scanning
    fn substitute_vars(&self, input: &str) -> String {
        if !input.contains('$') {
            return input.to_string();
        }

        let mut output = Vec::with_capacity(input.len() + 64);
        let bytes = input.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            // SAFETY: i is checked < len
            let b = unsafe { *bytes.get_unchecked(i) };
            if b == b'$' {
                let mut j = i + 1;
                if j < len {
                    let start = j;
                    // Scan for variable name
                    while j < len {
                        // SAFETY: j < len
                        let c = unsafe { *bytes.get_unchecked(j) };
                        if !c.is_ascii_alphanumeric() && c != b'_' {
                            break;
                        }
                        j += 1;
                    }

                    if j > start {
                        // SAFETY: start..j is valid range within input
                        let key = unsafe { input.get_unchecked(start..j) };
                        // Priority: Local > Env
                        let mut found = false;
                        if let Some(val) = self.local_vars.get(key) {
                            output.extend_from_slice(val.as_bytes());
                            found = true;
                        } else if let Some(val) = self.env_vars.get(key) {
                            output.extend_from_slice(val.as_bytes());
                            found = true;
                        }

                        if found {
                            i = j;
                            continue;
                        }
                    }
                }
                output.push(b'$');
                i += 1;
            } else {
                output.push(b);
                i += 1;
            }
        }

        // SAFETY: We constructed the vec from valid UTF8 strings (keys/values) and original input parts
        unsafe { String::from_utf8_unchecked(output) }
    }

    // Unsafe check for shell characters
    fn needs_shell(cmd: &str) -> bool {
        let bytes = cmd.as_bytes();
        for &b in bytes {
            if b == b'|' || b == b'>' || b == b'<' || b == b'&' || b == b';' {
                // We check & specially in parser, but double && is shell
                return true;
            }
        }
        // Check for "&&" or "||"
        if cmd.contains("&&") || cmd.contains("||") {
            return true;
        }
        false
    }

    fn check_dependencies(&self, deps: &[String]) -> Result<(), RuntimeError> {
        let mut missing = Vec::new();
        for dep in deps {
            if dep == "sudo" {
                continue;
            }
            if which(dep).is_err() {
                if self.verbose {
                    println!("{} Dependency '{}' not found. Installing...", "[-]".yellow(), dep);
                }
                missing.push(dep);
            } else if self.verbose {
                println!("{} Dependency '{}' found.", "[+]".green(), dep);
            }
        }

        if !missing.is_empty() {
            let missing_str = missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
            println!("{} Installing missing dependencies: {}", "[*]".blue(), missing_str);
            let mut apt_cmd = Command::new("sudo");
            apt_cmd.arg("apt").arg("update");
            apt_cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
            apt_cmd.status()?;

            let mut install_cmd = Command::new("sudo");
            install_cmd.arg("apt").arg("install").arg("-y").args(&missing);
            install_cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

            let status = install_cmd.status()?;
            if !status.success() {
                return Err(RuntimeError::Execution("Failed to install dependencies".into()));
            }
        }
        Ok(())
    }

    fn execute_command(&self, cmd_str: &str, is_sudo: bool) -> Result<(), RuntimeError> {
        let processed_cmd = self.substitute_vars(cmd_str);

        // Fast shell detection
        let use_shell = Self::needs_shell(&processed_cmd);

        let mut command;
        if use_shell {
            if is_sudo {
                command = Command::new("sudo");
                command.arg("sh").arg("-c").arg(&processed_cmd);
            } else {
                command = Command::new("sh");
                command.arg("-c").arg(&processed_cmd);
            }
        } else {
            let parts = shell_words::split(&processed_cmd).map_err(|e| RuntimeError::Execution(e.to_string()))?;
            if parts.is_empty() {
                return Ok(());
            }
            if is_sudo {
                command = Command::new("sudo");
                command.args(&parts);
            } else {
                command = Command::new(&parts[0]);
                command.args(&parts[1..]);
            }
        }

        command.envs(&self.env_vars);
        command.stdout(Stdio::inherit());
        command.stderr(Stdio::inherit());

        if self.verbose {
            println!("{} Executing: {}", "[>]".cyan(), processed_cmd);
        }

        match command.status() {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else {
                    Err(RuntimeError::Execution(format!("Command failed with exit code: {:?}", status.code())))
                }
            }
            Err(e) => Err(RuntimeError::Execution(format!("Failed to spawn command: {}", e))),
        }
    }

    fn execute_background(&self, cmd_str: &str, is_sudo: bool) {
        let cmd = cmd_str.to_string();
        let envs = self.env_vars.clone();
        let local_vars = self.local_vars.clone();
        let verbose = self.verbose;

        thread::spawn(move || {
            let runner = Interpreter { verbose, env_vars: envs, local_vars };
            if let Err(e) = runner.execute_command(&cmd, is_sudo) {
                eprintln!("{} Background task failed: {}", "[!]".red(), e);
            }
        });
    }

    fn run_program(&mut self, program: Program) -> Result<(), RuntimeError> {
        self.check_dependencies(&program.deps)?;

        for action in program.actions {
            match action {
                Action::AssignEnv { key, value } => {
                    let val = self.substitute_vars(&value);
                    if self.verbose { println!("{} Export {}={}", "[=]".blue(), key, val); }
                    self.env_vars.insert(key, val);
                }
                Action::AssignLocal { key, value } => {
                    let val = self.substitute_vars(&value);
                    if self.verbose { println!("{} Local {}={}", "[=]".blue(), key, val); }
                    self.local_vars.insert(key, val);
                }
                Action::Cmd { raw, is_sudo, is_background } => {
                    if is_background {
                        self.execute_background(&raw, is_sudo);
                    } else {
                        self.execute_command(&raw, is_sudo)?;
                    }
                }
                Action::If { condition, body, is_sudo } => {
                    if self.verbose { println!("{} Checking condition: {}", "[?]".yellow(), condition); }
                    match self.execute_command(&condition, is_sudo) {
                        Ok(_) => {
                            self.execute_command(&body, is_sudo)?;
                        }
                        Err(_) => {
                            if self.verbose { println!("{} Condition false, skipping.", "[.]".dimmed()); }
                        }
                    }
                }
                Action::Loop { count, body, is_sudo } => {
                    if self.verbose { println!("{} Looping {} times", "[@]".yellow(), count); }
                    for _ in 0..count {
                        self.execute_command(&body, is_sudo)?;
                    }
                }
                Action::Plugin { name, is_super } => {
                    let plugin_path = Self::expand_home(&format!("{}/plugins/{}", HACKER_DIR, name));
                    if !plugin_path.exists() {
                        return Err(RuntimeError::Execution(format!("Plugin not found: {:?}", plugin_path)));
                    }
                    self.execute_command(plugin_path.to_str().unwrap(), is_super)?;
                }
            }
        }
        Ok(())
    }
}

// --- Parsing Logic ---

fn parse_file(path: &Path, source: Arc<str>) -> Result<Program, ParseErrors> {
    // Fast line splitting without allocating a string for every line
    let mut program = Program::default();
    let mut errors = Vec::new();
    let mut in_function: Option<String> = None;
    let mut functions: HashMap<String, Vec<Action>> = HashMap::new();
    let mut current_func_body: Vec<Action> = Vec::new();

    let mut line_start = 0;
    let src_bytes = source.as_bytes();
    let src_len = src_bytes.len();
    let mut line_idx = 0;

    while line_start < src_len {
        let mut line_end = line_start;
        while line_end < src_len && src_bytes[line_end] != b'\n' {
            line_end += 1;
        }

        let line_str = unsafe { source.get_unchecked(line_start..line_end) };
        let line = line_str.trim();

        // Prepare for next iteration
        line_start = line_end + 1;

        if line.is_empty() || line.starts_with("!!") || line.starts_with('!') {
            line_idx += 1;
            continue;
        }

        let span = SourceSpan::new(line_idx.into(), line.len().into());

        if line == ":" {
            if let Some(name) = in_function.take() {
                functions.insert(name, current_func_body.clone());
                current_func_body.clear();
            } else {
                errors.push((span, "Ending function without start".to_string()));
            }
            line_idx += 1;
            continue;
        } else if line.starts_with(':') {
            let name = line[1..].trim().to_string();
            if in_function.is_some() {
                errors.push((span, "Nested functions not supported".to_string()));
            }
            in_function = Some(name);
            line_idx += 1;
            continue;
        }

        if line.starts_with('#') {
            let lib_name = line[1..].trim();
            let lib_path = Interpreter::expand_home(&format!("{}/libs/{}/main.hacker", HACKER_DIR, lib_name));
            if lib_path.exists() {
                // Use unsafe read for speed
                match fs::read(&lib_path) {
                    Ok(bytes) => {
                        // SAFETY: Assuming hacker files are UTF8. unsafe skips validation.
                        let src = unsafe { String::from_utf8_unchecked(bytes) };
                        match parse_file(&lib_path, Arc::from(src)) {
                            Ok(sub_prog) => {
                                program.deps.extend(sub_prog.deps);
                                program.includes.push(lib_name.to_string());
                                if in_function.is_none() {
                                    program.actions.extend(sub_prog.actions);
                                }
                            }
                            Err(e) => errors.extend(e.spans),
                        }
                    }
                    Err(e) => errors.push((span, format!("Failed to read lib: {}", e))),
                }
            }
            line_idx += 1;
            continue;
        }

        let mut action: Option<Action> = None;
        let mut is_sudo = false;
        let clean_line = if line.starts_with('^') {
            is_sudo = true;
            line[1..].trim()
        } else {
            line
        };

        if clean_line.starts_with("//") {
            program.deps.push(clean_line[2..].trim().to_string());
        } else if clean_line.starts_with('.') {
            let func_name = clean_line[1..].trim();
            if let Some(body) = functions.get(func_name) {
                if in_function.is_some() {
                    current_func_body.extend(body.clone());
                } else {
                    program.actions.extend(body.clone());
                }
            } else {
                errors.push((span, format!("Unknown function: {}", func_name)));
            }
        } else if clean_line.starts_with('>') {
            let cmd_str = if clean_line.starts_with(">>>") {
                clean_line[3..].trim()
            } else if clean_line.starts_with(">>") {
                clean_line[2..].trim()
            } else {
                clean_line[1..].trim()
            };
            let cmd_final = cmd_str.split('!').next().unwrap_or("").trim().to_string();

            action = Some(Action::Cmd {
                raw: cmd_final,
                is_sudo,
                is_background: false
            });

        } else if clean_line.starts_with('@') {
            if let Some((k, v)) = clean_line[1..].split_once('=') {
                action = Some(Action::AssignEnv {
                    key: k.trim().to_string(),
                              value: v.trim().to_string()
                });
            }
        } else if clean_line.starts_with('$') {
            if let Some((k, v)) = clean_line[1..].split_once('=') {
                action = Some(Action::AssignLocal {
                    key: k.trim().to_string(),
                              value: v.trim().to_string()
                });
            }
        } else if clean_line.starts_with('&') {
            let cmd_part = clean_line[1..].split('!').next().unwrap_or("").trim().to_string();
            action = Some(Action::Cmd {
                raw: cmd_part,
                is_sudo,
                is_background: true
            });
        } else if clean_line.starts_with('?') {
            if let Some((cond, rest)) = clean_line[1..].split_once('>') {
                let cmd_part = rest.split('!').next().unwrap_or("").trim().to_string();
                action = Some(Action::If {
                    condition: cond.trim().to_string(),
                              body: cmd_part,
                              is_sudo
                });
            }
        } else if clean_line.starts_with('=') {
            if let Some((num_str, rest)) = clean_line[1..].split_once('>') {
                if let Ok(count) = num_str.trim().parse::<u32>() {
                    let cmd_part = rest.split('!').next().unwrap_or("").trim().to_string();
                    action = Some(Action::Loop {
                        count,
                        body: cmd_part,
                        is_sudo
                    });
                }
            }
        } else if clean_line.starts_with('\\') {
            action = Some(Action::Plugin {
                name: clean_line[1..].trim().to_string(),
                          is_super: is_sudo
            });
        }

        if let Some(act) = action {
            if in_function.is_some() {
                current_func_body.push(act);
            } else {
                program.actions.push(act);
            }
        }
        line_idx += 1;
    }

    if !errors.is_empty() {
        return Err(ParseErrors {
            src: NamedSource::new(path.to_string_lossy(), source),
                   spans: errors,
        });
    }

    Ok(program)
}

#[derive(Debug)]
struct ParseErrors {
    src: NamedSource<Arc<str>>,
    spans: Vec<(SourceSpan, String)>,
}

impl std::error::Error for ParseErrors {}
impl std::fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parse errors encountered")
    }
}
impl Diagnostic for ParseErrors {
    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.src as &dyn miette::SourceCode)
    }
    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        Some(Box::new(self.spans.iter().map(|(s, m)| miette::LabeledSpan::new_with_span(Some(m.clone()), *s))))
    }
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    // Ensure dirs
    let bin_dir = Interpreter::expand_home("~/.hackeros/hacker-lang/bin");
    fs::create_dir_all(&bin_dir).into_diagnostic()?;

    let input_path = Path::new(&args.input);
    if !input_path.exists() {
        return Err(miette::miette!("Input file does not exist: {}", args.input));
    }

    // High performance file read: read bytes, unsafe cast to string
    let bytes = fs::read(input_path).into_diagnostic()?;
    let source = unsafe { String::from_utf8_unchecked(bytes) };

    if args.verbose {
        println!("{} Parsing {}", "[*]".blue(), args.input);
    }

    let program = parse_file(input_path, Arc::from(source))?;

    if args.verbose {
        println!("{} Parsed successfully. Executing...", "[*]".green());
    }

    let mut interpreter = Interpreter::new(args.verbose);
    match interpreter.run_program(program) {
        Ok(_) => {
            if args.verbose { println!("{} Execution finished successfully.", "[+]".green()); }
        },
        Err(e) => {
            eprintln!("{} Runtime Error: {}", "[x]".red(), e);
            std::process::exit(1);
        }
    }

    Ok(())
}
