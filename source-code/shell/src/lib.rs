pub mod builtins;
pub mod completion;
pub mod prompt;

use anyhow::Result;
use colored::Colorize;
use hacker_core::env::Env;
use hacker_core::{run_source, check_source};
use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Config, EditMode, Editor};
use std::path::Path;
use tracing::{debug, warn};

use builtins::{try_builtin, BuiltinResult};
use completion::HlCompleter;
use prompt::Prompt;

const HISTORY_FILE: &str = ".hl_history";

/// Start the interactive Hacker Lang REPL
pub fn run_interactive(env: &mut Env) -> Result<()> {
    print_banner();

    let config = Config::builder()
    .history_ignore_space(true)
    .completion_type(CompletionType::List)
    .edit_mode(EditMode::Emacs)
    .build();

    let helper = HlCompleter::new();
    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(helper));

    // Load history
    let history_path = dirs::home_dir()
    .map(|h| h.join(HISTORY_FILE))
    .unwrap_or_else(|| Path::new(HISTORY_FILE).to_path_buf());

    if history_path.exists() {
        let _ = rl.load_history(&history_path);
    }

    let prompt_renderer = Prompt::new();
    let mut multiline_buf = String::new();
    let mut in_multiline = false;

    loop {
        let prompt_str = if in_multiline {
            "  … ".bright_blue().bold().to_string()
        } else {
            prompt_renderer.render(env.last_exit)
        };

        match rl.readline(&prompt_str) {
            Ok(line) => {
                let trimmed = line.trim();

                if trimmed.is_empty() {
                    if in_multiline {
                        // Execute accumulated multiline block
                        let src = multiline_buf.clone();
                        multiline_buf.clear();
                        in_multiline = false;
                        execute_line(&src, env);
                    }
                    continue;
                }

                rl.add_history_entry(trimmed).ok();

                // Detect multiline blocks (function definitions, conditionals)
                if is_block_start(trimmed) {
                    in_multiline = true;
                }

                if in_multiline {
                    multiline_buf.push_str(trimmed);
                    multiline_buf.push('\n');

                    if trimmed == "done" {
                        let src = multiline_buf.clone();
                        multiline_buf.clear();
                        in_multiline = false;
                        execute_line(&src, env);
                    }
                    continue;
                }

                execute_line(trimmed, env);
            }

            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: cancel current input
                in_multiline = false;
                multiline_buf.clear();
                println!("{}", "^C".bright_red());
            }

            Err(ReadlineError::Eof) => {
                // Ctrl-D: exit
                println!("\n{}", "Goodbye, hacker.".bright_cyan());
                break;
            }

            Err(e) => {
                warn!("Readline error: {}", e);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

/// Execute a source string in the shell context
fn execute_line(source: &str, env: &mut Env) {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return;
    }

    // Try builtins first
    match try_builtin(trimmed, env) {
        BuiltinResult::Handled(code) => {
            env.last_exit = code;
            return;
        }
        BuiltinResult::NotBuiltin => {}
    }

    // Syntax-check before executing
    match check_source(source) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{} {}", "[hl parse]".red().bold(), e);
            env.last_exit = 2;
            return;
        }
    }

    debug!("Executing: {}", trimmed);

    match run_source(source, env) {
        Ok(result) => {
            env.last_exit = result.exit_code;
        }
        Err(e) => {
            eprintln!("{} {}", "[hl error]".red().bold(), e);
            env.last_exit = 1;
        }
    }
}

/// Run a .hl script file
pub fn run_file(path: &Path, env: &mut Env) -> Result<i32> {
    let source = std::fs::read_to_string(path)?;
    match run_source(&source, env) {
        Ok(result) => Ok(result.exit_code),
        Err(e) => {
            eprintln!("{} {}", "[hl error]".red().bold(), e);
            Ok(1)
        }
    }
}

/// Check if a line starts a block that needs `done`
fn is_block_start(line: &str) -> bool {
    // Function definition starts with `: name def`
    if line.starts_with(':') && line.ends_with("def") {
        return true;
    }
    // Conditional blocks
    if line.starts_with("? ok") || line.starts_with("? err") {
        return true;
    }
    false
}

fn print_banner() {
    println!(
        "{}",
        r#"
        ██╗  ██╗ █████╗  ██████╗██╗  ██╗███████╗██████╗
        ██║  ██║██╔══██╗██╔════╝██║ ██╔╝██╔════╝██╔══██╗
        ███████║███████║██║     █████╔╝ █████╗  ██████╔╝
        ██╔══██║██╔══██║██║     ██╔═██╗ ██╔══╝  ██╔══██╗
        ██║  ██║██║  ██║╚██████╗██║  ██╗███████╗██║  ██║
        ╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝
        L A N G  —  HackerOS Shell  v0.1.0
        "#
        .bright_cyan()
        .bold()
    );
    println!("{}", "  Type 'help' for syntax reference. Ctrl+D to exit.\n".bright_black());
}
