pub mod builtins;
pub mod completion;
pub mod prompt;

use anyhow::Result;
use colored::Colorize;
use hl_core::diagnostics::{parse_error_to_diag, DiagRenderer, DiagSummary, lint_source};
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
const HLRC_FILE:    &str = ".hlrc";

pub fn run_interactive(env: &mut Env) -> Result<()> {
    print_banner();
    run_editor_loop(env, "<repl>", true)
}

pub fn run_as_shell(config: Option<&Path>, env: &mut Env) -> Result<()> {
    let rc_path = config.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        dirs::home_dir().unwrap_or_default().join(HLRC_FILE)
    });
    if rc_path.exists() {
        let rc_src  = std::fs::read_to_string(&rc_path).unwrap_or_default();
        let fname   = rc_path.to_string_lossy().into_owned();
        execute_source(&rc_src, &fname, env);
    }
    if let Ok(exe) = std::env::current_exe() {
        env.set_var("SHELL", hl_core::Value::String(exe.display().to_string()));
    }
    env.set_var("HL_SHELL_MODE", hl_core::Value::Bool(true));
    run_editor_loop(env, "<shell>", false)
}

fn run_editor_loop(env: &mut Env, ctx: &str, show_hint: bool) -> Result<()> {
    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .build();

    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(HlCompleter::new()));

    let history_path = dirs::home_dir().unwrap_or_default().join(HISTORY_FILE);
    if history_path.exists() { let _ = rl.load_history(&history_path); }

    if show_hint {
        println!("{}", "  Wpisz 'help' aby zobaczyc skladnie. Ctrl+D aby wyjsc.\n".bright_black());
    }

    let prompt_renderer = Prompt::new();
    let mut multiline_buf = String::new();
    let mut in_multiline  = false;

    loop {
        let prompt_str = if in_multiline {
            format!("  {} ", "...".bright_blue().bold())
        } else {
            prompt_renderer.render(env.last_exit)
        };

        match rl.readline(&prompt_str) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    if in_multiline {
                        let src = multiline_buf.clone();
                        multiline_buf.clear(); in_multiline = false;
                        execute_source(&src, ctx, env);
                    }
                    continue;
                }
                rl.add_history_entry(trimmed).ok();
                if is_block_start(trimmed) && !in_multiline { in_multiline = true; }
                if in_multiline {
                    multiline_buf.push_str(trimmed);
                    multiline_buf.push('\n');
                    if trimmed == "done" {
                        let src = multiline_buf.clone();
                        multiline_buf.clear(); in_multiline = false;
                        execute_source(&src, ctx, env);
                    }
                    continue;
                }
                execute_source(trimmed, ctx, env);
            }
            Err(ReadlineError::Interrupted) => {
                in_multiline = false; multiline_buf.clear();
                println!("{}", "^C".bright_red());
            }
            Err(ReadlineError::Eof) => {
                println!("\n{}", "Goodbye, hacker.".bright_cyan()); break;
            }
            Err(e) => { warn!("Readline error: {}", e); break; }
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

pub fn execute_source(source: &str, filename: &str, env: &mut Env) {
    let trimmed = source.trim();
    if trimmed.is_empty() { return; }

    match try_builtin(trimmed, env) {
        BuiltinResult::Handled(code) => { env.last_exit = code; return; }
        BuiltinResult::NotBuiltin    => {}
    }

    let renderer = DiagRenderer::new(filename, source);
    let mut lint_diags = lint_source(source);
    lint_diags.extend(hl_core::lint_gen(source));
    if !lint_diags.is_empty() {
        renderer.emit_all(&lint_diags);
        let sum = DiagSummary::from_diags(&lint_diags);
        if sum.has_errors() { sum.print(); env.last_exit = 2; return; }
        sum.print();
    }

    if let Err(e) = check_source(source) {
        renderer.emit(&parse_error_to_diag(&e));
        env.last_exit = 2; return;
    }

    debug!("exec: {}", trimmed);
    match run_source(source, env) {
        Ok(r)  => env.last_exit = r.exit_code,
        Err(e) => {
            renderer.emit(&hl_core::Diag::error(e.to_string()).with_note("blad runtime"));
            env.last_exit = 1;
        }
    }
}

pub fn run_file(path: &Path, env: &mut Env) -> Result<i32> {
    let source   = std::fs::read_to_string(path)?;
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>");
    let renderer = DiagRenderer::new(filename, &source);

    let mut lint_diags = lint_source(&source);
    lint_diags.extend(hl_core::lint_gen(&source));
    if !lint_diags.is_empty() {
        renderer.emit_all(&lint_diags);
        let sum = DiagSummary::from_diags(&lint_diags);
        sum.print();
        if sum.has_errors() { return Ok(2); }
    }

    if let Err(e) = check_source(&source) {
        renderer.emit(&parse_error_to_diag(&e)); return Ok(2);
    }

    match run_source(&source, env) {
        Ok(r)  => Ok(r.exit_code),
        Err(e) => {
            let d = hl_core::Diag::error(e.to_string())
                .with_note(format!("blad runtime w '{}'", filename));
            renderer.emit(&d); Ok(1)
        }
    }
}

/// Wykryj poczatek bloku (wiele linii) — gen 1 + gen 2
fn is_block_start(line: &str) -> bool {
    // gen 1
    let is_func_def = line.starts_with(':') && !line.starts_with("::") && !line.starts_with(":*") && line.ends_with("def");
    let is_goroutine = line.starts_with(":*");
    let is_cond = line.starts_with("? ok") || line.starts_with("? err");
    // gen 2
    let is_for_in  = line.starts_with('@') && line.contains(" in ");
    let is_while   = line.starts_with("?~");
    let is_switch  = line.starts_with("? switch");

    is_func_def || is_goroutine || is_cond || is_for_in || is_while || is_switch
}

fn print_banner() {
    println!("{}", r#"
  ██╗  ██╗ █████╗  ██████╗██╗  ██╗███████╗██████╗
  ██║  ██║██╔══██╗██╔════╝██║ ██╔╝██╔════╝██╔══██╗
  ███████║███████║██║     █████╔╝ █████╗  ██████╔╝
  ██╔══██║██╔══██║██║     ██╔═██╗ ██╔══╝  ██╔══██╗
  ██║  ██║██║  ██║╚██████╗██║  ██╗███████╗██║  ██║
  ╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝
  L A N G  gen 2  --  REPL"#.bright_cyan().bold());
}
