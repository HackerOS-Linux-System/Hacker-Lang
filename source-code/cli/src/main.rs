use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use hl_core::diagnostics::{parse_error_to_diag, DiagRenderer, DiagSummary, lint_source};
use hl_core::env::Env;
use hl_core::{check_source, run_source, cmd_lib_list, cmd_lib_install, cmd_lib_remove, cmd_clean_cache};
use hl_shell::{run_interactive, run_as_shell};
use hl_compiler::{compile, CompileOptions, CompileMode};
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, fmt};

// ── HackerOS Guard ────────────────────────────────────────────────────────────

fn check_hackeros_only() {
    if !std::path::Path::new("/usr/share/HackerOS/").exists()  { die_not_hackeros(); }
    if !std::path::Path::new("/usr/lib/HackerOS/").exists()    { die_not_hackeros(); }
    if !std::path::Path::new("/usr/bin/hacker").exists()       { die_not_hackeros(); }
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    if !os.lines().any(|l| l == r#"NAME="HackerOS""#) { die_not_hackeros(); }
}

#[cold]
#[inline(never)]
fn die_not_hackeros() -> ! {
    eprintln!("{} {}", "hl:".bright_magenta().bold(),
              "Hacker Lang działa wyłącznie na HackerOS.".white().bold());
    eprintln!("    {}", "https://github.com/HackerOS-Linux-System".bright_black());
    std::process::exit(1);
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "hl", version = "0.3", author = "HackerOS Team",
about = "Hacker Lang — język skryptowy HackerOS")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    #[arg(value_name = "ARGS", last = true)]
    script_args: Vec<String>,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short = 'c', long = "code", value_name = "CODE")]
    inline_code: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Uruchom skrypt .hl
    Run {
        file: PathBuf,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Interaktywna powłoka REPL
    Repl,
    /// HL jako powłoka systemowa
    Shell {
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,
        #[arg(short = 'c', long = "command", value_name = "CMD")]
        command: Option<String>,
    },
    /// Sprawdź składnię
    Check { file: PathBuf },
    /// Wydrukuj AST jako JSON
    Ast { file: PathBuf },
    /// Skompiluj .hl do statycznej binarki lub .so (Virus)
    Compile {
        /// Plik .hl do skompilowania
        file: PathBuf,
        /// Plik wyjściowy
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// Tryb verbose
        #[arg(short, long)]
        verbose: bool,
        /// Kompiluj jako bibliotekę .so (ekosystem Virus)
        #[arg(long)]
        shared: bool,
    },
    /// Wyczyść cache
    Clean,
    /// Menedżer bibliotek
    Lib {
        #[command(subcommand)]
        action: LibAction,
    },
    /// Informacje o wersji
    Version,
}

#[derive(Subcommand, Debug)]
enum LibAction {
    /// Lista bibliotek (std + virus)
    List,
    /// Zainstaluj bibliotekę community
    Install { name: String },
    /// Usuń bibliotekę
    Remove { name: String },
    /// Lista bibliotek Virus
    Virus,
    /// Informacje o bibliotece
    Info { name: String },
}

fn main() -> Result<()> {
    check_hackeros_only();

    let cli = Cli::parse();

    fmt().with_env_filter(
        if cli.verbose { EnvFilter::new("debug") } else { EnvFilter::new("warn") }
    ).without_time().compact().init();

    match cli.command {
        Some(Commands::Repl) => {
            let mut env = Env::new();
            run_interactive(&mut env)?;
        }

        Some(Commands::Shell { config, command }) => {
            let mut env = Env::new();
            if let Some(cmd) = command {
                std::process::exit(run_source_with_diag("<shell -c>", &cmd, &mut env));
            }
            run_as_shell(config.as_deref(), &mut env)?;
        }

        Some(Commands::Run { file, args }) => {
            let mut env = Env::new();
            inject_args(&mut env, &args);
            std::process::exit(run_file_with_diag(&file, &mut env));
        }

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
                    Ok(nodes) => println!("{} {} ({} węzłów, {} ostrzeżeń)",
                                          "✓".green().bold(), file.display().to_string().bright_white(),
                                          nodes.len(), lint_diags.len()),
                                          Err(e) => { renderer.emit(&parse_error_to_diag(&e)); exit_code = 1; }
                }
            }
            std::process::exit(exit_code);
        }

        Some(Commands::Ast { file }) => {
            let source = std::fs::read_to_string(&file)?;
            match check_source(&source) {
                Ok(nodes) => println!("{}", serde_json::to_string_pretty(&nodes)?),
                Err(e) => {
                    let fname = file.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>");
                    DiagRenderer::new(fname, &source).emit(&parse_error_to_diag(&e));
                    std::process::exit(1);
                }
            }
        }

        Some(Commands::Compile { file, output, verbose, shared }) => {
            let source = std::fs::read_to_string(&file)?;
            let fname  = file.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>");
            let renderer = DiagRenderer::new(fname, &source);

            let lint_diags = lint_source(&source);
            if !lint_diags.is_empty() {
                renderer.emit_all(&lint_diags);
                let sum = DiagSummary::from_diags(&lint_diags);
                sum.print();
                if sum.has_errors() {
                    eprintln!("{} Kompilacja przerwana.", "✗".red().bold());
                    std::process::exit(2);
                }
            }
            if let Err(e) = check_source(&source) {
                renderer.emit(&parse_error_to_diag(&e));
                std::process::exit(2);
            }

            let mode = if shared { CompileMode::Shared } else { CompileMode::Binary };
            let mode_str = match mode { CompileMode::Binary => "binarka", CompileMode::Shared => ".so (Virus)" };

            eprintln!("{} {} {} {}",
                      "hl compile:".bright_magenta().bold(),
                      file.display().to_string().bright_white(),
                      "→".bright_black(),
                      mode_str.bright_cyan());

            match compile(CompileOptions { input: file, output, verbose: verbose || cli.verbose, mode }) {
                Ok(result) => {
                    let mode_badge = match result.mode {
                        CompileMode::Binary => "[statyczna binarka x86_64]".bright_black(),
                        CompileMode::Shared => "[biblioteka .so — Virus]".bright_magenta(),
                    };
                    println!("{} {} {}",
                             "✓".green().bold(),
                             result.output_path.display().to_string().bright_green().bold(),
                             mode_badge);
                }
                Err(e) => { eprintln!("{} {}", "✗".red().bold(), e); std::process::exit(1); }
            }
        }

        Some(Commands::Clean) => cmd_clean_cache(),

        Some(Commands::Lib { action }) => match action {
            LibAction::List    => cmd_lib_list(),
            LibAction::Install { name } => cmd_lib_install(&name),
            LibAction::Remove  { name } => cmd_lib_remove(&name),
            LibAction::Virus   => {
                // Wywołaj virus_list bezpośrednio
                let base = dirs::home_dir()
                .unwrap_or_default()
                .join(".hackeros")
                .join("hacker-lang");
                println!("{}", "=== Biblioteki Virus ===".bright_magenta().bold());
                if let Ok(entries) = std::fs::read_dir(&base) {
                    let mut found = false;
                    for e in entries.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.ends_with(".so") {
                            println!("  {} {}", "◆".bright_magenta(), name.bright_white());
                            found = true;
                        }
                    }
                    if !found { println!("  {}", "(brak)".bright_black()); }
                } else {
                    println!("  {}", "(brak ~/.hackeros/hacker-lang/)".bright_black());
                }
            }
            LibAction::Info { name } => {
                eprintln!("{} '{}' — niedostępne w tej wersji.", "!".yellow(), name);
            }
        },

        Some(Commands::Version) => {
            println!("{} {}", "Hacker Lang".bright_magenta().bold(), "v0.3".bright_white());
            println!("{} hl-shell    v0.3", "Shell:    ".bright_black());
            println!("{} hl-core     v0.3", "Core:     ".bright_black());
            println!("{} hl-compiler v0.3", "Compiler: ".bright_black());
            println!("{} HackerOS (Debian-based)", "OS:       ".bright_black());
            println!();
            println!("{} # <std/net>, # <virus/lib>, # <community/repo>",
                     "Import:   ".bright_black());
        }

        None => {
            if let Some(code) = cli.inline_code {
                let mut env = Env::new();
                inject_args(&mut env, &cli.script_args);
                std::process::exit(run_source_with_diag("<inline>", &code, &mut env));
            } else if let Some(file) = cli.file {
                if !file.exists() {
                    eprintln!("{} Plik nie istnieje: {}", "✗".red().bold(), file.display());
                    std::process::exit(1);
                }
                let mut env = Env::new();
                inject_args(&mut env, &cli.script_args);
                std::process::exit(run_file_with_diag(&file, &mut env));
            } else {
                let mut env = Env::new();
                run_interactive(&mut env)?;
            }
        }
    }

    Ok(())
}

fn run_file_with_diag(file: &PathBuf, env: &mut Env) -> i32 {
    if !file.exists() {
        eprintln!("{} Plik nie istnieje: {}", "✗".red().bold(), file.display());
        return 1;
    }
    if file.extension().and_then(|e| e.to_str()) != Some("hl") {
        eprintln!("{} Ostrzeżenie: '{}' nie ma rozszerzenia .hl", "!".yellow().bold(), file.display());
    }
    env.set_var("HL_SCRIPT", hl_core::Value::String(file.display().to_string()));
    match hl_shell::run_file(file, env) {
        Ok(code) => code,
        Err(e)   => { eprintln!("{} {}", "✗".red().bold(), e); 1 }
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
        Ok(_)  => {}
    }
    match run_source(source, env) {
        Ok(r) => r.exit_code,
        Err(e) => { renderer.emit(&hl_core::Diag::error(e.to_string())); 1 }
    }
}

fn inject_args(env: &mut Env, args: &[String]) {
    env.set_var("argc", hl_core::Value::Number(args.len() as f64));
    for (i, arg) in args.iter().enumerate() {
        env.set_var(&format!("arg{}", i), hl_core::Value::String(arg.clone()));
    }
}
