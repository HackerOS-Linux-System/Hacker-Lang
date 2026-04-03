pub mod builtins;
pub mod completion;
pub mod prompt;

use anyhow::Result;
use colored::Colorize;
use hl_core::diagnostics::{
    parse_error_to_diag, DiagRenderer, DiagSummary, lint_source,
};
use hl_core::env::Env;
use hl_core::{check_source, run_source};
use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Config, EditMode, Editor};
use std::path::Path;
use tracing::{debug, warn};

use builtins::{try_builtin, BuiltinResult};
use completion::HlCompleter;
use prompt::Prompt;

const HISTORY_FILE: &str = ".hl_history";

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
                        let src = multiline_buf.clone();
                        multiline_buf.clear();
                        in_multiline = false;
                        execute_line(&src, "<repl>", env);
                    }
                    continue;
                }

                rl.add_history_entry(trimmed).ok();

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
                        execute_line(&src, "<repl>", env);
                    }
                    continue;
                }

                execute_line(trimmed, "<repl>", env);
            }

            Err(ReadlineError::Interrupted) => {
                in_multiline = false;
                multiline_buf.clear();
                println!("{}", "^C".bright_red());
            }

            Err(ReadlineError::Eof) => {
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

fn execute_line(source: &str, filename: &str, env: &mut Env) {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return;
    }

    match try_builtin(trimmed, env) {
        BuiltinResult::Handled(code) => {
            env.last_exit = code;
            return;
        }
        BuiltinResult::NotBuiltin => {}
    }

    let renderer = DiagRenderer::new(filename, source);

    let lint_diags = lint_source(source);
    if !lint_diags.is_empty() {
        renderer.emit_all(&lint_diags);
        let summary = DiagSummary::from_diags(&lint_diags);
        if summary.has_errors() {
            summary.print();
            env.last_exit = 2;
            return;
        }
        summary.print();
    }

    match check_source(source) {
        Ok(_) => {}
        Err(e) => {
            let diag = parse_error_to_diag(&e);
            renderer.emit(&diag);
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
            let diag = hl_core::diagnostics::Diag::error(e.to_string())
            .with_note("błąd w trakcie wykonania skryptu");
            renderer.emit(&diag);
            env.last_exit = 1;
        }
    }
}

pub fn run_file(path: &Path, env: &mut Env) -> Result<i32> {
    let source = std::fs::read_to_string(path)?;
    let filename = path.file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("<unknown>");

    let renderer = DiagRenderer::new(filename, &source);

    let lint_diags = lint_source(&source);
    if !lint_diags.is_empty() {
        renderer.emit_all(&lint_diags);
        let summary = DiagSummary::from_diags(&lint_diags);
        summary.print();
        if summary.has_errors() {
            return Ok(2);
        }
    }

    match check_source(&source) {
        Ok(_) => {}
        Err(e) => {
            let diag = parse_error_to_diag(&e);
            renderer.emit(&diag);
            return Ok(2);
        }
    }

    match run_source(&source, env) {
        Ok(result) => Ok(result.exit_code),
        Err(e) => {
            let diag = hl_core::diagnostics::Diag::error(e.to_string())
            .with_note(format!("błąd runtime w pliku `{}`", filename));
            renderer.emit(&diag);
            Ok(1)
        }
    }
}

fn is_block_start(line: &str) -> bool {
    if line.starts_with(':') && line.ends_with("def") {
        return true;
    }
    line.starts_with("? ok") || line.starts_with("? err")
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
