mod cli;
mod guard;
mod run;
mod compile;
mod scripts;
mod info;

use anyhow::Result;
use colored::Colorize;
use hl_core::diagnostics::{parse_error_to_diag, DiagRenderer, DiagSummary, lint_source, lint_gen};
use hl_core::env::Env;
use hl_core::{check_source, cmd_clean_cache};
use hl_core::parse_source_with_meta;
use hl_core::{
    cmd_env_create, cmd_env_enter, cmd_env_exit,
    cmd_env_remove, cmd_env_list, cmd_env_status, cmd_env_help,
};
use hl_shell::{run_interactive, run_as_shell};

use cli::{Cli, Commands, EnvAction};
use guard::{check_hackeros_only, HL_MAIN_LIBS_DIR};
use run::{inject_args, run_bc_direct, run_file_jit, run_file_with_diag, run_source_with_diag};
use scripts::{cmd_exec, cmd_search};

use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

fn main() -> Result<()> {
    check_hackeros_only();

    let cli = Cli::parse();
    // --install-deps: przekaż przez process env, zero modyfikacji struct Env
    if cli.install_deps {
        std::env::set_var("HL_INSTALL_DEPS", "1");
    }

    fmt().with_env_filter(
        if cli.verbose { EnvFilter::new("debug") } else { EnvFilter::new("warn") }
    ).without_time().compact().init();

    match cli.command {

        Some(Commands::Exec { name, args }) => {
            std::process::exit(cmd_exec(&name, &args, cli.verbose));
        }

        Some(Commands::Search { query }) => {
            cmd_search(&query);
        }

        Some(Commands::Compile { file, shared: _, output }) => {
            compile::cmd_compile(&file, output.as_deref())?;
        }

        Some(Commands::Docs) => info::run_docs(),

        Some(Commands::Version) => info::print_version(),

        Some(Commands::Env { action }) => {
            match action {
                None | Some(EnvAction::Help) => {
                    cmd_env_help();
                }
                Some(EnvAction::Create { name }) => {
                    if let Err(e) = cmd_env_create(&name) {
                        eprintln!("{} {}", "BŁĄD".red().bold(), e);
                        std::process::exit(1);
                    }
                }
                Some(EnvAction::Enter { name }) => {
                    if let Err(e) = cmd_env_enter(name.as_deref()) {
                        eprintln!("{} {}", "BŁĄD".red().bold(), e);
                        std::process::exit(1);
                    }
                }
                Some(EnvAction::Exit) => {
                    if let Err(e) = cmd_env_exit() {
                        eprintln!("{} {}", "BŁĄD".red().bold(), e);
                        std::process::exit(1);
                    }
                }
                Some(EnvAction::Remove { name }) => {
                    if let Err(e) = cmd_env_remove(&name) {
                        eprintln!("{} {}", "BŁĄD".red().bold(), e);
                        std::process::exit(1);
                    }
                }
                Some(EnvAction::List) => {
                    if let Err(e) = cmd_env_list() {
                        eprintln!("{} {}", "BŁĄD".red().bold(), e);
                        std::process::exit(1);
                    }
                }
                Some(EnvAction::Status) => {
                    if let Err(e) = cmd_env_status() {
                        eprintln!("{} {}", "BŁĄD".red().bold(), e);
                        std::process::exit(1);
                    }
                }
            }
        }

        Some(Commands::GenInfo { file }) => {
            let source = std::fs::read_to_string(&file)?;
            let meta   = parse_source_with_meta(&source)?;
            println!("{}", "=== Hacker Lang Meta ===".bright_cyan().bold());
            println!("  Plik:    {}", file.display().to_string().bright_white());
            println!("  Gen:     {}", format!("gen {}", meta.gen.number()).bright_magenta().bold());
            match &meta.shebang {
                Some(sb) => println!("  Shebang: {}", sb.raw.bright_black()),
                None     => println!("  Shebang: {}", "(brak)".bright_black()),
            }
            println!("  Węzły:   {}", meta.nodes.len().to_string().bright_white());
        }

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

        // ── hl run ───────────────────────────────────────────────────────────
        // Domyślnie: tree-walk interpreter (sprawdzony, poprawnie obsługuje @VAR)
        // --jit: eksperymentalny JIT pipeline (compile→cache→bytecode)
        Some(Commands::Run { file, jit, args }) => {
            let exit_code = if jit && file.extension().and_then(|e| e.to_str()) != Some("bc") {
                // JIT pipeline — tylko gdy jawnie włączony i plik nie jest .bc
                run_file_jit(&file, &args, cli.verbose)
            } else if file.extension().and_then(|e| e.to_str()) == Some("bc") {
                // .bc plik — zawsze przez JIT interpreter
                run_bc_direct(&file, &args)
            } else {
                // Tree-walk interpreter — domyślny, stabilny
                let mut env = Env::new();
                inject_args(&mut env, &args);
                run_file_with_diag(&file, &mut env, cli.verbose)
            };
            std::process::exit(exit_code);
        }

        Some(Commands::Check { file, meta: show_meta }) => {
            let source = std::fs::read_to_string(&file)?;
            let fname  = file.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>");
            let renderer = DiagRenderer::new(fname, &source);
            let mut exit_code = 0i32;

            let mut lint_diags = lint_source(&source);
            lint_diags.extend(lint_gen(&source));

            if !lint_diags.is_empty() {
                renderer.emit_all(&lint_diags);
                let sum = DiagSummary::from_diags(&lint_diags);
                sum.print();
                if sum.has_errors() { exit_code = 2; }
            }

            if exit_code == 0 {
                match parse_source_with_meta(&source) {
                    Ok(meta) => {
                        println!("{} {} ({} węzłów, gen {}, {} ostrzeżeń)",
                                 "OK".green().bold(),
                                 file.display().to_string().bright_white(),
                                 meta.nodes.len(),
                                 meta.gen.number(),
                                 lint_diags.len());
                        if show_meta {
                            println!("  Gen:     {}", format!("gen {}", meta.gen.number()).bright_magenta());
                            if let Some(sb) = &meta.shebang {
                                println!("  Shebang: {}", sb.raw.bright_black());
                            }
                        }
                    }
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

        Some(Commands::Clean) => {
            cmd_clean_cache();
            match hl_compiler::cache::cache_clean_all() {
                Ok(n) if n > 0 => println!("{} Usunięto {} plików .bc z cache.", "✓".green(), n),
                Ok(_)          => println!("{}", "Cache .bc jest pusty.".bright_black()),
                Err(e)         => eprintln!("{} Błąd czyszczenia cache .bc: {}", "✗".red(), e),
            }
        }

        Some(Commands::CacheInfo) => {
            hl_jit::runner::print_cache_stats();
        }

        Some(Commands::Lib { .. }) => {
            println!();
            println!("{}", "  Hacker Lang — system bibliotek".bright_cyan().bold());
            println!();
            println!("  Biblioteki HL są instalowane przez manager pakietów bit.");
            println!("  Komenda {} została uproszczona.", "hl lib".bright_yellow());
            println!();
            println!("  Aby zainstalować bibliotekę bit użyj:");
            println!("    {}", "bit install <nazwa>".bright_green().bold());
            println!();
            println!("  Aby usunąć bibliotekę bit użyj:");
            println!("    {}", "bit remove <nazwa>".bright_red().bold());
            println!();
            println!("  Składnia importu w plikach .hl:");
            println!("    {}  -- biblioteka standardowa", "# <main/net>".bright_cyan());
            println!("    {}  -- biblioteka bit", "# <bit/hashlib>".bright_magenta());
            println!("    {}  -- GitHub", "# <github/user/repo>".bright_blue());
            println!();
            println!("  Biblioteki main są plikami .hl w:");
            println!("    {}", HL_MAIN_LIBS_DIR.bright_white());
            println!();
        }

        None => {
            if let Some(code) = cli.inline_code {
                let mut env = Env::new();
                inject_args(&mut env, &cli.script_args);
                std::process::exit(run_source_with_diag("<inline>", &code, &mut env));
            } else if let Some(file) = cli.file {
                if !file.exists() {
                    eprintln!("{} Plik nie istnieje: {}", "BŁĄD".red().bold(), file.display());
                    std::process::exit(1);
                }
                // .bc → JIT, wszystko inne → tree-walk
                if file.extension().and_then(|e| e.to_str()) == Some("bc") {
                    std::process::exit(run_bc_direct(&file, &cli.script_args));
                }
                let mut env = Env::new();
                inject_args(&mut env, &cli.script_args);
                std::process::exit(run_file_with_diag(&file, &mut env, cli.verbose));
            } else {
                let mut env = Env::new();
                run_interactive(&mut env)?;
            }
        }
    }

    Ok(())
}
