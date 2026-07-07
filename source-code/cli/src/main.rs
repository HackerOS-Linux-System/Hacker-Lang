use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use hl_core::diagnostics::{parse_error_to_diag, DiagRenderer, DiagSummary, lint_source, lint_gen};
use hl_core::env::Env;
use hl_core::{check_source, run_source, cmd_clean_cache};
use hl_core::{HL_MAX_GEN, HL_DEFAULT_GEN, parse_source_with_meta};
use hl_core::{
    cmd_env_create, cmd_env_enter, cmd_env_exit,
    cmd_env_remove, cmd_env_list, cmd_env_status, cmd_env_help,
    load_config, config_path, get_active_env,
};
use hl_shell::{run_interactive, run_as_shell};
use std::path::{Path, PathBuf};
use tracing_subscriber::{EnvFilter, fmt};

const HL_SCRIPTS_DIR: &str = "/usr/share/HackerOS/Scripts/Bin";
const HL_MAIN_LIBS_DIR: &str = "/usr/lib/HackerOS/Hacker-Lang/main-libs";

// ── HackerOS Guard ────────────────────────────────────────────────────────────

fn check_hackeros_only() {
    if !std::path::Path::new("/usr/share/HackerOS/").exists()  { die_not_hackeros(); }
    if !std::path::Path::new("/usr/lib/HackerOS/").exists()    { die_not_hackeros(); }
    if !std::path::Path::new("/usr/bin/hacker").exists()       { die_not_hackeros(); }
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    if !os.lines().any(|l| l == r#"NAME="HackerOS""#) { die_not_hackeros(); }
}

#[cold] #[inline(never)]
fn die_not_hackeros() -> ! {
    eprintln!("{} {}", "hl:".bright_magenta().bold(),
              "Hacker Lang działa wyłącznie na HackerOS.".white().bold());
    eprintln!("    {}", "https://github.com/HackerOS-Linux-System".bright_black());
    std::process::exit(1);
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
name    = "hl",
version = "gen 2",
author  = "HackerOS Team",
about   = "Hacker Lang — język skryptowy HackerOS (gen 2)",
          after_help = "\
SKRYPTY SYSTEMOWE:
hl search <nazwa>    Szukaj skryptu w /usr/share/HackerOS/Scripts/Bin/
hl search all        Pokaż wszystkie dostępne skrypty
hl exec <nazwa>      Uruchom skrypt z /usr/share/HackerOS/Scripts/Bin/

BYTECODE / JIT:
hl run plik.hl       Uruchom skrypt (domyślnie: tree-walk interpreter)
hl run --jit plik.hl Uruchom przez JIT pipeline (eksperymentalny)
hl run plik.bc       Uruchom bytecode bezpośrednio przez JIT
hl compile plik.hl   Kompiluj .hl → .bc (do katalogu źródłowego)
hl clean             Wyczyść cache .bc (~/.hackeros/hacker-lang/cache/)

PRZYKŁADY:
hl run skrypt.hl
hl exec update-system
hl search update
hl repl"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    #[arg(value_name = "ARGS", last = true)]
    script_args: Vec<String>,

    /// Włącz verbose output (debug info)
    #[arg(short, long, global = true)]
    verbose: bool,

    #[arg(short = 'c', long = "code", value_name = "CODE")]
    inline_code: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Uruchom skrypt .hl lub .bc
    Run {
        file: PathBuf,
        /// Użyj JIT pipeline zamiast tree-walk (eksperymentalny)
        #[arg(long)]
        jit: bool,
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Kompiluj .hl → .bc
    Compile {
        file: PathBuf,
        #[arg(long)]
        shared: bool,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Uruchom skrypt z /usr/share/HackerOS/Scripts/Bin/ po nazwie (bez .hl)
    Exec {
        name: String,
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Szukaj skryptów w /usr/share/HackerOS/Scripts/Bin/
    Search { query: String },

    /// Interaktywna powłoka REPL
    Repl,

    /// HL jako powłoka systemowa
    Shell {
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,
        #[arg(short = 'c', long = "command", value_name = "CMD")]
        command: Option<String>,
    },

    /// Sprawdź składnię (bez uruchamiania)
    Check {
        file: PathBuf,
        #[arg(long)]
        meta: bool,
    },

    /// Wydrukuj AST jako JSON
    Ast { file: PathBuf },

    /// Wyczyść cache bytecode + bibliotek
    Clean,

    /// Informacje o cache bytecode
    CacheInfo,

    /// Informacje o systemie bibliotek
    Lib {
        #[command(subcommand)]
        action: Option<LibAction>,
    },

    /// Otwórz interaktywną dokumentację Hacker Lang (TUI)
    Docs,

    /// Informacje o wersji HL i systemie genów
    Version,

    /// Informacje o genie i shebangu pliku .hl
    GenInfo { file: PathBuf },

    /// Manager izolowanych środowisk
    #[command(subcommand_required = false)]
    Env {
        #[command(subcommand)]
        action: Option<EnvAction>,
    },
}

#[derive(Subcommand, Debug)]
enum EnvAction {
    /// Utwórz nowe środowisko
    Create { name: String },
    /// Wejdź do środowiska (uruchamia subshell)
    Enter  { name: Option<String> },
    /// Opuść aktywne środowisko
    Exit,
    /// Usuń środowisko
    Remove { name: String },
    /// Lista wszystkich środowisk
    List,
    /// Status aktywnego środowiska
    Status,
    /// Pomoc
    Help,
}

#[derive(Subcommand, Debug)]
enum LibAction {
    List,
    Install { name: String },
    Remove  { name: String },
    Bit,
    Info    { name: String },
}

fn main() -> Result<()> {
    check_hackeros_only();

    let cli = Cli::parse();

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
            cmd_compile(&file, output.as_deref())?;
        }

        Some(Commands::Docs) => run_docs(),

        Some(Commands::Version) => print_version(),

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

// ── hl compile ────────────────────────────────────────────────────────────────

fn cmd_compile(file: &Path, output: Option<&Path>) -> Result<()> {
    if !file.exists() {
        eprintln!("{} Plik nie istnieje: {}", "BŁĄD".red().bold(), file.display());
        std::process::exit(1);
    }

    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "hl" => {
            eprintln!("{} {} → .bc",
                      "hl compile:".bright_magenta().bold(),
                      file.display().to_string().bright_white());

            let t0 = std::time::Instant::now();
            match hl_compiler::compile_hl_to_bc(file, output) {
                Ok(bc_path) => {
                    let elapsed = t0.elapsed();
                    println!("{} {} ({:.1}ms)",
                             "✓".green().bold(),
                             bc_path.display().to_string().bright_white(),
                             elapsed.as_secs_f64() * 1000.0);
                }
                Err(e) => {
                    eprintln!("{} {}", "BŁĄD kompilacji:".red().bold(), e);
                    std::process::exit(1);
                }
            }
        }
        "bc" => {
            eprintln!("{} Kompilacja .bc → ELF nie jest jeszcze dostępna w gen 2.",
                      "hl compile:".bright_magenta().bold());
            eprintln!("  Użyj {} aby uruchomić bytecode.",
                      "hl run plik.bc".bright_cyan());
            std::process::exit(1);
        }
        other => {
            eprintln!("{} Nieznane rozszerzenie: .{}", "BŁĄD".red().bold(), other);
            std::process::exit(1);
        }
    }

    Ok(())
}

// ── Uruchamianie plików ───────────────────────────────────────────────────────

/// Uruchom plik .bc bezpośrednio przez JIT (bez kompilacji)
fn run_bc_direct(file: &Path, args: &[String]) -> i32 {
    match hl_jit::run_bc_file(file, args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {}", "BŁĄD .bc:".red().bold(), e);
            1
        }
    }
}

/// Uruchom plik przez JIT pipeline (eksperymentalny)
fn run_file_jit(file: &Path, args: &[String], _verbose: bool) -> i32 {
    match hl_jit::run_file(file, args) {
        Ok(code) => code,
        Err(e) => {
            // JIT zawiódł — fallback do tree-walk
            tracing::warn!("JIT error: {}, fallback do interpretera", e);
            let mut env = Env::new();
            inject_args_env(&mut env, args);
            run_file_with_diag(file, &mut env, false)
        }
    }
}

// ── hl exec ───────────────────────────────────────────────────────────────────

fn cmd_exec(name: &str, args: &[String], verbose: bool) -> i32 {
    let scripts_dir = Path::new(HL_SCRIPTS_DIR);

    let candidates = [
        scripts_dir.join(format!("{}.hl", name)),
        scripts_dir.join(name),
    ];

    let script_path = candidates.iter().find(|p| p.exists());

    match script_path {
        Some(path) => {
            if verbose {
                eprintln!("{} {}", "hl exec:".bright_magenta().bold(),
                          path.display().to_string().bright_white());
            }
            let mut env = Env::new();
            inject_args(&mut env, args);
            env.set_var("HL_EXEC_NAME", hl_core::Value::String(name.to_string()));
            run_file_with_diag(path, &mut env, verbose)
        }
        None => {
            eprintln!("{} Skrypt '{}' nie znaleziony w {}",
                      "BŁĄD".red().bold(), name.bright_white(), HL_SCRIPTS_DIR.bright_black());
            eprintln!("  Użyj {} aby zobaczyć dostępne skrypty.", "hl search all".bright_cyan());
            1
        }
    }
}

// ── hl search ────────────────────────────────────────────────────────────────

fn cmd_search(query: &str) {
    let scripts_dir = Path::new(HL_SCRIPTS_DIR);

    if !scripts_dir.exists() {
        eprintln!("{} Katalog skryptów nie istnieje: {}", "BŁĄD".red().bold(), HL_SCRIPTS_DIR.bright_black());
        return;
    }

    let mut scripts: Vec<(String, PathBuf)> = match std::fs::read_dir(scripts_dir) {
        Ok(entries) => entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) == Some("hl") {
                let name = path.file_stem()?.to_str()?.to_string();
                Some((name, path))
            } else {
                None
            }
        })
        .collect(),
        Err(e) => {
            eprintln!("{} Nie można odczytać katalogu: {}", "BŁĄD".red().bold(), e);
            return;
        }
    };

    scripts.sort_by(|a, b| a.0.cmp(&b.0));

    let show_all = query.eq_ignore_ascii_case("all");
    let query_lc = query.to_lowercase();

    let matched: Vec<&(String, PathBuf)> = if show_all {
        scripts.iter().collect()
    } else {
        scripts.iter().filter(|(name, _)| name.to_lowercase().contains(&query_lc)).collect()
    };

    if matched.is_empty() {
        println!("{} Brak skryptów pasujących do '{}'",
                 "hl search:".bright_magenta().bold(), query.bright_yellow());
        return;
    }

    println!("{} {} — {}",
             "hl search:".bright_magenta().bold(),
             HL_SCRIPTS_DIR.bright_black(),
             if show_all {
                 format!("{} skryptów", matched.len()).bright_white().to_string()
             } else {
                 format!("{} wyników dla '{}'", matched.len(), query).bright_white().to_string()
             });
    println!();

    for (name, path) in &matched {
        let description = read_script_description(path);
        let exec_hint = format!("hl exec {}", name).bright_cyan().to_string();
        println!("  {} {}", format!("{:<35}", name).bright_white().bold(), exec_hint.bright_black());
        if let Some(desc) = description {
            println!("  {}  {}", " ".repeat(35), desc.bright_black().italic());
        }
        println!();
    }
}

fn read_script_description(path: &Path) -> Option<String> {
    let source = std::fs::read_to_string(path).ok()?;
    for line in source.lines().take(8) {
        let t = line.trim();
        if t.starts_with("///") {
            let desc = t.trim_start_matches('/').trim().to_string();
            if !desc.is_empty() { return Some(desc); }
        }
        if t.starts_with(";;") {
            let desc = t.trim_start_matches(';').trim().to_string();
            if !desc.is_empty() && !desc.starts_with('=') { return Some(desc); }
        }
        if !t.is_empty() && !t.starts_with('#') && !t.starts_with(';') && !t.starts_with("using") { break; }
    }
    None
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn run_file_with_diag(file: &Path, env: &mut Env, verbose: bool) -> i32 {
    if !file.exists() {
        eprintln!("{} Plik nie istnieje: {}", "BŁĄD".red().bold(), file.display());
        return 1;
    }

    if verbose {
        if let Ok(source) = std::fs::read_to_string(file) {
            if let Ok(meta) = parse_source_with_meta(&source) {
                eprintln!("  Gen: {}  Shebang: {}",
                          format!("gen {}", meta.gen.number()).bright_magenta(),
                              meta.shebang.map(|s| s.raw).unwrap_or_else(|| "(brak)".into()).bright_black());
            }
        }
    }

    env.set_var("HL_SCRIPT", hl_core::Value::String(file.display().to_string()));

    match hl_shell::run_file(file, env) {
        Ok(code) => code,
        Err(e)   => { eprintln!("{} {}", "BŁĄD".red().bold(), e); 1 }
    }
}

fn run_source_with_diag(fname: &str, source: &str, env: &mut Env) -> i32 {
    let renderer = DiagRenderer::new(fname, source);
    let mut lint_diags = lint_source(source);
    lint_diags.extend(lint_gen(source));
    if !lint_diags.is_empty() {
        renderer.emit_all(&lint_diags);
        let sum = DiagSummary::from_diags(&lint_diags);
        sum.print();
        if sum.has_errors() { return 2; }
    }
    if let Err(e) = check_source(source) { renderer.emit(&parse_error_to_diag(&e)); return 2; }
    match run_source(source, env) {
        Ok(r)  => r.exit_code,
        Err(e) => { renderer.emit(&hl_core::Diag::error(e.to_string())); 1 }
    }
}

fn inject_args(env: &mut Env, args: &[String]) {
    env.set_var("argc", hl_core::Value::Number(args.len() as f64));
    for (i, arg) in args.iter().enumerate() {
        env.set_var(&format!("arg{}", i), hl_core::Value::String(arg.clone()));
    }
}

fn inject_args_env(env: &mut Env, args: &[String]) {
    inject_args(env, args);
}

fn run_docs() {
    const DOCS_BIN: &str = "/usr/lib/HackerOS/Hacker-Lang/hl-docs";
    if !std::path::Path::new(DOCS_BIN).exists() {
        eprintln!("{} Binarka hl-docs nie znaleziona.", "hl docs:".bright_magenta().bold());
        eprintln!("  Oczekiwana ścieżka: {}", DOCS_BIN.bright_white());
        eprintln!("  Zainstaluj: {}", "sudo hl-docs-install".bright_cyan());
        std::process::exit(1);
    }
    let status = std::process::Command::new(DOCS_BIN).status()
    .unwrap_or_else(|e| { eprintln!("{} {}", "BŁĄD".red().bold(), e); std::process::exit(1); });
    std::process::exit(status.code().unwrap_or(0));
}

fn print_version() {
    println!("{} {}", "Hacker Lang".bright_magenta().bold(), "gen 2".bright_white());
    println!();
    println!("{}", "Komponenty:".bright_yellow());
    println!("  hl-parser    gen 2  -- Lexer, Parser, AST, Gen, Shebang");
    println!("  hl-core      gen 2  -- Executor, Env, Quick Functions, Diagnostics");
    println!("  hl-compiler  gen 2  -- Bytecode compiler (AST → .bc, Cranelift)");
    println!("  hl-jit       gen 2  -- JIT engine (Cranelift, eksperymentalny)");
    println!("  hl-shell     gen 2  -- REPL, Shell, Completion");
    println!("  hl-docs      gen 2  -- Dokumentacja TUI (Go + Bubble Tea)");
    println!();
    println!("{}", "Tryby wykonania:".bright_yellow());
    println!("  {} (domyślny)  -- stabilny, pełna obsługa @VAR",
             "tree-walk".bright_green().bold());
    println!("  {} (hl run --jit)  -- kompilacja .hl→.bc→JIT, eksperymentalny",
             "JIT pipeline".bright_yellow());
    println!("  {} (hl run plik.bc) -- bezpośrednie wykonanie bytecode",
             ".bc execute".bright_cyan());
    println!();
    println!("{}", "System Genów:".bright_yellow());
    println!("  Aktualny max gen: {}", format!("gen {}", HL_MAX_GEN).bright_magenta().bold());
    println!("  Domyślny gen:     {}", format!("gen {}", HL_DEFAULT_GEN).bright_magenta());
    println!("  Deklaracja:       {}", "using <gen 2>".bright_cyan());
    println!();
    println!("{}", "Shebang:".bright_yellow());
    println!("  {}", "#!/usr/bin/env hl".bright_cyan());
    println!("  {}", "#!/usr/bin/hl".bright_cyan());
    println!();
    println!("{}", "Bytecode:".bright_yellow());
    println!("  hl compile plik.hl    -- .hl → .bc");
    println!("  hl run plik.bc        -- uruchom .bc przez JIT");
    println!("  hl run --jit plik.hl  -- JIT pipeline (eksperymentalny)");
    println!("  hl clean              -- wyczyść cache .bc");
    println!("  hl cache-info         -- statystyki cache .bc");
    println!();
    println!("{}", "Arena Functions (gen 2):".bright_yellow());
    println!("  {}  -- zdefiniuj z areną 4k", ":: fn <4k> def ... done".bright_cyan());
    println!("  {}  -- wywołaj", ":: fn".bright_cyan());
    println!();
    println!("{}", "Manager pakietów:".bright_yellow());
    println!("  {}  -- manager pakietów bit", "bit".bright_green().bold());
    println!();
    println!("{}", "Importy:".bright_yellow());
    println!("  {}  -- biblioteka standardowa", "# <main/nazwa>".bright_cyan());
    println!("  {}   -- biblioteka bit", "# <bit/nazwa>".bright_magenta());
    println!("  {} -- GitHub", "# <github/user/repo>".bright_blue());
    println!();
    println!("{}", "Skrypty systemowe:".bright_yellow());
    println!("  Katalog:  {}", HL_SCRIPTS_DIR.bright_white());
    println!("  Szukaj:   {}", "hl search <nazwa> | hl search all".bright_cyan());
    println!("  Uruchom:  {}", "hl exec <nazwa>".bright_cyan());
}
