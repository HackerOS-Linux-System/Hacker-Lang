use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use hacker_core::env::Env;
use hacker_core::{check_source, run_source};
use hacker_shell::{run_file, run_interactive};
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, fmt};

/// hl - Hacker Lang interpreter and shell
#[derive(Parser, Debug)]
#[command(
name = "hl",
version = "0.1.0",
author = "HackerOS Team",
about = "Hacker Lang — the scripting language of HackerOS",
long_about = r#"
██╗  ██╗██╗
██║  ██║██║
███████║██║
██╔══██║██║
██║  ██║███████╗
╚═╝  ╚═╝╚══════╝  Hacker Lang v0.1.0

A powerful scripting language for HackerOS (Debian-based).
Files use .hl extension. Run 'hl shell' for the interactive REPL.
"#
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Script file to execute (.hl)
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Arguments passed to the script
    #[arg(value_name = "ARGS", last = true)]
    script_args: Vec<String>,

    /// Enable verbose/debug output
    #[arg(short, long)]
    verbose: bool,

    /// Execute inline Hacker Lang code
    #[arg(short = 'c', long = "code", value_name = "CODE")]
    inline_code: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the interactive Hacker Lang shell (REPL)
    Shell,

    /// Check syntax of a .hl file without running it
    Check {
        /// File to check
        file: PathBuf,
    },

    /// Print the AST of a .hl file (for debugging)
    Ast {
        /// File to parse
        file: PathBuf,
    },

    /// Print version information
    Version,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Init tracing
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("warn")
    };
    fmt().with_env_filter(filter).without_time().init();

    match cli.command {
        Some(Commands::Shell) => {
            let mut env = Env::new();
            run_interactive(&mut env)?;
        }

        Some(Commands::Check { file }) => {
            let source = std::fs::read_to_string(&file)?;
            match check_source(&source) {
                Ok(nodes) => {
                    println!(
                        "{} {} ({} nodes)",
                             "✓".green().bold(),
                             file.display().to_string().bright_white(),
                             nodes.len()
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("{} {}: {}", "✗".red().bold(), file.display(), e);
                    std::process::exit(1);
                }
            }
        }

        Some(Commands::Ast { file }) => {
            let source = std::fs::read_to_string(&file)?;
            match check_source(&source) {
                Ok(nodes) => {
                    println!("{}", serde_json::to_string_pretty(&nodes)?);
                }
                Err(e) => {
                    eprintln!("{} {}: {}", "✗".red().bold(), file.display(), e);
                    std::process::exit(1);
                }
            }
        }

        Some(Commands::Version) => {
            print_version();
        }

        None => {
            // No subcommand given
            if let Some(code) = cli.inline_code {
                // -c "code" mode
                let mut env = Env::new();
                inject_args(&mut env, &cli.script_args);
                match run_source(&code, &mut env) {
                    Ok(result) => std::process::exit(result.exit_code),
                    Err(e) => {
                        eprintln!("{} {}", "[hl]".red().bold(), e);
                        std::process::exit(1);
                    }
                }
            } else if let Some(file) = cli.file {
                // Run a .hl script file
                if !file.exists() {
                    eprintln!("{} File not found: {}", "✗".red().bold(), file.display());
                    std::process::exit(1);
                }
                let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "hl" {
                    eprintln!(
                        "{} Warning: '{}' does not have .hl extension",
                        "!".yellow().bold(),
                              file.display()
                    );
                }
                let mut env = Env::new();
                inject_args(&mut env, &cli.script_args);
                // Inject script path
                env.set_var(
                    "HL_SCRIPT",
                    hacker_core::Value::String(file.display().to_string()),
                );
                let code = run_file(&file, &mut env)?;
                std::process::exit(code);
            } else {
                // No file, no command → launch interactive shell
                let mut env = Env::new();
                run_interactive(&mut env)?;
            }
        }
    }

    Ok(())
}

/// Inject script arguments as @arg0, @arg1, ... and @argc
fn inject_args(env: &mut Env, args: &[String]) {
    env.set_var(
        "argc",
        hacker_core::Value::Number(args.len() as f64),
    );
    for (i, arg) in args.iter().enumerate() {
        env.set_var(
            &format!("arg{}", i),
                    hacker_core::Value::String(arg.clone()),
        );
    }
}

fn print_version() {
    println!(
        "{} {}",
        "Hacker Lang".bright_cyan().bold(),
             "v0.1.0".bright_white()
    );
    println!("{} {}", "Shell:".bright_black(), "hacker-shell v0.1.0");
    println!("{} {}", "Core: ".bright_black(), "hacker-core v0.1.0");
    println!("{} {}", "OS:   ".bright_black(), "HackerOS (Debian-based)");
}
