use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use hl_core::diagnostics::{parse_error_to_diag, DiagRenderer, DiagSummary, lint_source};
use hl_core::env::Env;
use hl_core::{check_source, run_source, cmd_lib_list, cmd_lib_install, cmd_lib_remove, cmd_clean_cache};
use hl_shell::{run_file, run_interactive, run_as_shell};
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser, Debug)]
#[command(
name = "hl",
version = "0.3",
author = "HackerOS Team",
about = "Hacker Lang — język skryptowy HackerOS",
long_about = None,
disable_version_flag = false,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Plik .hl do uruchomienia (skrót dla `hl run`)
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Argumenty przekazane do skryptu
    #[arg(value_name = "ARGS", last = true)]
    script_args: Vec<String>,

    /// Tryb verbose (debug)
    #[arg(short, long)]
    verbose: bool,

    /// Wykonaj kod inline
    #[arg(short = 'c', long = "code", value_name = "CODE")]
    inline_code: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Uruchom skrypt .hl
    Run {
        /// Plik .hl do uruchomienia
        file: PathBuf,
        /// Argumenty skryptu
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Interaktywna powłoka REPL (do testowania kodu HL)
    Repl,

    /// Uruchom HL jako powłokę systemową (zamiennik bash/zsh)
    Shell {
        /// Plik konfiguracyjny powłoki (domyślnie ~/.hlrc)
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,
        /// Wykonaj komendę i wyjdź
        #[arg(short = 'c', long = "command", value_name = "CMD")]
        command: Option<String>,
    },

    /// Sprawdź składnię pliku .hl bez uruchamiania
    Check {
        file: PathBuf,
    },

    /// Wydrukuj AST pliku .hl (debug)
    Ast {
        file: PathBuf,
    },

    /// Wyczyść cache Hacker Lang (~/.hl/cache)
    Clean,

    /// Menedżer bibliotek HL
    Lib {
        #[command(subcommand)]
        action: LibAction,
    },

    /// Informacje o wersji
    Version,
}

#[derive(Subcommand, Debug)]
enum LibAction {
    /// Lista zainstalowanych bibliotek
    List,
    /// Zainstaluj bibliotekę (std/* lub owner/repo GitHub)
    Install {
        /// Nazwa biblioteki (np. owner/repo)
        name: String,
    },
    /// Usuń zainstalowaną bibliotekę
    Remove {
        name: String,
    },
    /// Pokaż informacje o bibliotece
    Info {
        name: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("warn")
    };
    fmt().with_env_filter(filter).without_time().compact().init();

    match cli.command {
        // ── hl repl ──────────────────────────────────────────────────────────
        Some(Commands::Repl) => {
            let mut env = Env::new();
            run_interactive(&mut env)?;
        }

        // ── hl shell ─────────────────────────────────────────────────────────
        Some(Commands::Shell { config, command }) => {
            let mut env = Env::new();
            if let Some(cmd) = command {
                // -c mode: wykonaj jedną komendę i wyjdź
                let code = run_source_with_diag("<shell -c>", &cmd, &mut env);
                std::process::exit(code);
            }
            run_as_shell(config.as_deref(), &mut env)?;
        }

        // ── hl run <plik> ────────────────────────────────────────────────────
        Some(Commands::Run { file, args }) => {
            let mut env = Env::new();
            inject_args(&mut env, &args);
            let code = run_file_with_diag(&file, &mut env);
            std::process::exit(code);
        }

        // ── hl check <plik> ──────────────────────────────────────────────────
        Some(Commands::Check { file }) => {
            let source = std::fs::read_to_string(&file)?;
            let fname  = file.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>");
            let renderer = DiagRenderer::new(fname, &source);
            let mut exit_code = 0i32;

            let lint_diags = lint_source(&source);
            if !lint_diags.is_empty() {
                renderer.emit_all(&lint_diags);
                let sum = DiagSummary::from_diags(&lint_diags);
                sum.print();
                if sum.has_errors() { exit_code = 2; }
            }

            if exit_code == 0 {
                match check_source(&source) {
                    Ok(nodes) => {
                        println!(
                            "{} {} ({} węzłów, {} ostrzeżeń)",
                                 "✓".green().bold(),
                                 file.display().to_string().bright_white(),
                                 nodes.len(),
                                 lint_diags.len()
                        );
                    }
                    Err(e) => {
                        renderer.emit(&parse_error_to_diag(&e));
                        exit_code = 1;
                    }
                }
            }
            std::process::exit(exit_code);
        }

        // ── hl ast <plik> ────────────────────────────────────────────────────
        Some(Commands::Ast { file }) => {
            let source = std::fs::read_to_string(&file)?;
            match check_source(&source) {
                Ok(nodes) => println!("{}", serde_json::to_string_pretty(&nodes)?),
                Err(e) => {
                    let fname = file.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>");
                    let renderer = DiagRenderer::new(fname, &source);
                    renderer.emit(&parse_error_to_diag(&e));
                    std::process::exit(1);
                }
            }
        }

        // ── hl clean ─────────────────────────────────────────────────────────
        Some(Commands::Clean) => {
            cmd_clean_cache();
        }

        // ── hl lib ───────────────────────────────────────────────────────────
        Some(Commands::Lib { action }) => match action {
            LibAction::List => cmd_lib_list(),
            LibAction::Install { name } => cmd_lib_install(&name),
            LibAction::Remove  { name } => cmd_lib_remove(&name),
            LibAction::Info    { name } => {
                eprintln!("{} Informacje o '{}' — niedostępne w tej wersji.", "!".yellow(), name);
            }
        },

        // ── hl version ───────────────────────────────────────────────────────
        Some(Commands::Version) => print_version(),

        // ── hl [plik] lub hl -c "kod" ────────────────────────────────────────
        None => {
            if let Some(code) = cli.inline_code {
                let mut env = Env::new();
                inject_args(&mut env, &cli.script_args);
                let c = run_source_with_diag("<inline>", &code, &mut env);
                std::process::exit(c);
            } else if let Some(file) = cli.file {
                if !file.exists() {
                    eprintln!("{} Plik nie istnieje: {}", "✗".red().bold(), file.display());
                    std::process::exit(1);
                }
                let mut env = Env::new();
                inject_args(&mut env, &cli.script_args);
                let c = run_file_with_diag(&file, &mut env);
                std::process::exit(c);
            } else {
                // Brak argumentów — uruchom REPL
                let mut env = Env::new();
                run_interactive(&mut env)?;
            }
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn run_file_with_diag(file: &PathBuf, env: &mut Env) -> i32 {
    if !file.exists() {
        eprintln!("{} Plik nie istnieje: {}", "✗".red().bold(), file.display());
        return 1;
    }
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "hl" {
        eprintln!("{} Ostrzeżenie: plik '{}' nie ma rozszerzenia .hl",
                  "!".yellow().bold(), file.display());
    }
    env.set_var("HL_SCRIPT", hl_core::Value::String(file.display().to_string()));
    match hl_shell::run_file(file, env) {
        Ok(code) => code,
        Err(e) => { eprintln!("{} {}", "✗".red().bold(), e); 1 }
    }
}

fn run_source_with_diag(fname: &str, source: &str, env: &mut Env) -> i32 {
    let renderer = DiagRenderer::new(fname, source);
    let lint_diags = lint_source(source);
    if !lint_diags.is_empty() {
        renderer.emit_all(&lint_diags);
        let sum = DiagSummary::from_diags(&lint_diags);
        sum.print();
        if sum.has_errors() { return 2; }
    }
    match check_source(source) {
        Err(e) => { renderer.emit(&parse_error_to_diag(&e)); return 2; }
        Ok(_) => {}
    }
    match run_source(source, env) {
        Ok(r) => r.exit_code,
        Err(e) => {
            let d = hl_core::Diag::error(e.to_string());
            renderer.emit(&d);
            1
        }
    }
}

fn inject_args(env: &mut Env, args: &[String]) {
    env.set_var("argc", hl_core::Value::Number(args.len() as f64));
    for (i, arg) in args.iter().enumerate() {
        env.set_var(&format!("arg{}", i), hl_core::Value::String(arg.clone()));
    }
}

fn print_version() {
    println!("{} {}", "Hacker Lang".bright_cyan().bold(), "v0.3".bright_white());
    println!("{} {}", "Shell: ".bright_black(), "hl-shell v0.3");
    println!("{} {}", "Core:  ".bright_black(), "hl-core  v0.3");
    println!("{} {}", "OS:    ".bright_black(), "HackerOS (Debian-based)");
}
