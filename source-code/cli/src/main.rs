use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use hl_core::diagnostics::{parse_error_to_diag, DiagRenderer, DiagSummary, lint_source, lint_gen};
use hl_core::env::Env;
use hl_core::{check_source, run_source, cmd_clean_cache};
use hl_core::{HL_MAX_GEN, HL_DEFAULT_GEN, parse_source_with_meta};
use hl_shell::{run_interactive, run_as_shell};
use std::path::{Path, PathBuf};
use tracing_subscriber::{EnvFilter, fmt};

// ── Katalog skryptow HackerOS ─────────────────────────────────────────────────
const HL_SCRIPTS_DIR: &str = "/usr/share/HackerOS/Scripts/Bin";

// ── Katalog bibliotek main ────────────────────────────────────────────────────
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
              "Hacker Lang dziala wylacznie na HackerOS.".white().bold());
    eprintln!("    {}", "https://github.com/HackerOS-Linux-System".bright_black());
    std::process::exit(1);
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name    = "hl",
    version = "gen 2",
    author  = "HackerOS Team",
    about   = "Hacker Lang — jezyk skryptowy HackerOS (gen 2)",
    after_help = "\
SKRYPTY SYSTEMOWE:
    hl search <nazwa>    Szukaj skryptu w /usr/share/HackerOS/Scripts/Bin/
    hl search all        Pokaz wszystkie dostepne skrypty
    hl exec <nazwa>      Uruchom skrypt z /usr/share/HackerOS/Scripts/Bin/

MANAGER PAKIETOW bit:
    bit                  Wywolaj bez argumentow — wypisze pomoc

PRZYKLADY:
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

    /// Uruchom skrypt z /usr/share/HackerOS/Scripts/Bin/ po nazwie (bez .hl)
    Exec {
        name: String,
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Szukaj skryptow w /usr/share/HackerOS/Scripts/Bin/
    Search {
        query: String,
    },

    /// Interaktywna powloka REPL
    Repl,

    /// HL jako powloka systemowa
    Shell {
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,
        #[arg(short = 'c', long = "command", value_name = "CMD")]
        command: Option<String>,
    },

    /// Sprawdz skladnie (bez uruchamiania)
    Check {
        file: PathBuf,
        #[arg(long)]
        meta: bool,
    },

    /// Wydrukuj AST jako JSON
    Ast { file: PathBuf },

    /// Wyczysc cache bibliotek
    Clean,

    /// Informacje o systemie bibliotek
    Lib {
        #[command(subcommand)]
        action: Option<LibAction>,
    },

    /// Otworz interaktywna dokumentacje Hacker Lang (TUI)
    Docs,

    /// Informacje o wersji HL i systemie genow
    Version,

    /// Informacje o genie i shebangu pliku .hl
    GenInfo { file: PathBuf },
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

        // ── hl exec ──────────────────────────────────────────────────────────
        Some(Commands::Exec { name, args }) => {
            std::process::exit(cmd_exec(&name, &args, cli.verbose));
        }

        // ── hl search ────────────────────────────────────────────────────────
        Some(Commands::Search { query }) => {
            cmd_search(&query);
        }

        // ── hl docs ──────────────────────────────────────────────────────────
        Some(Commands::Docs) => run_docs(),

        // ── hl version ───────────────────────────────────────────────────────
        Some(Commands::Version) => print_version(),

        // ── hl gen-info ──────────────────────────────────────────────────────
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
            println!("  Wezly:   {}", meta.nodes.len().to_string().bright_white());
        }

        // ── hl repl ──────────────────────────────────────────────────────────
        Some(Commands::Repl) => {
            let mut env = Env::new();
            run_interactive(&mut env)?;
        }

        // ── hl shell ─────────────────────────────────────────────────────────
        Some(Commands::Shell { config, command }) => {
            let mut env = Env::new();
            if let Some(cmd) = command {
                std::process::exit(run_source_with_diag("<shell -c>", &cmd, &mut env));
            }
            run_as_shell(config.as_deref(), &mut env)?;
        }

        // ── hl run ───────────────────────────────────────────────────────────
        Some(Commands::Run { file, args }) => {
            let mut env = Env::new();
            inject_args(&mut env, &args);
            std::process::exit(run_file_with_diag(&file, &mut env, cli.verbose));
        }

        // ── hl check ─────────────────────────────────────────────────────────
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
                        println!("{} {} ({} wezlow, gen {}, {} ostrzezen)",
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

        // ── hl ast ───────────────────────────────────────────────────────────
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

        // ── hl clean ─────────────────────────────────────────────────────────
        Some(Commands::Clean) => cmd_clean_cache(),

        // ── hl lib ────────────────────────────────────────────────────────────
        Some(Commands::Lib { .. }) => {
            println!();
            println!("{}", "  Hacker Lang — system bibliotek".bright_cyan().bold());
            println!();
            println!("  Biblioteki HL sa instalowane przez manager pakietow bit.");
            println!("  Komenda {} zostala uproszczona.", "hl lib".bright_yellow());
            println!();
            println!("  Aby zainstalowac biblioteke bit uzyj:");
            println!("    {}", "bit install <nazwa>".bright_green().bold());
            println!();
            println!("  Aby usunac biblioteke bit uzyj:");
            println!("    {}", "bit remove <nazwa>".bright_red().bold());
            println!();
            println!("  Skladnia importu w plikach .hl:");
            println!("    {}  -- biblioteka standardowa", "# <main/net>".bright_cyan());
            println!("    {}  -- biblioteka bit", "# <bit/hashlib>".bright_magenta());
            println!("    {}  -- GitHub", "# <github/user/repo>".bright_blue());
            println!();
            println!("  Biblioteki main sa plikami .hl w:");
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
                    eprintln!("{} Plik nie istnieje: {}", "BLAD".red().bold(), file.display());
                    std::process::exit(1);
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
                eprintln!("{} {}", "hl exec:".bright_magenta().bold(), path.display().to_string().bright_white());
            }
            let mut env = Env::new();
            inject_args(&mut env, args);
            env.set_var("HL_EXEC_NAME", hl_core::Value::String(name.to_string()));
            run_file_with_diag(path, &mut env, verbose)
        }
        None => {
            eprintln!("{} Skrypt '{}' nie znaleziony w {}",
                "BLAD".red().bold(), name.bright_white(), HL_SCRIPTS_DIR.bright_black());
            eprintln!();
            eprintln!("  Uzyj {} aby zobaczyc dostepne skrypty.", "hl search all".bright_cyan());
            1
        }
    }
}

// ── hl search ────────────────────────────────────────────────────────────────

fn cmd_search(query: &str) {
    let scripts_dir = Path::new(HL_SCRIPTS_DIR);

    if !scripts_dir.exists() {
        eprintln!("{} Katalog skryptow nie istnieje: {}", "BLAD".red().bold(), HL_SCRIPTS_DIR.bright_black());
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
            eprintln!("{} Nie mozna odczytac katalogu: {}", "BLAD".red().bold(), e);
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
        println!("{} Brak skryptow pasujacych do '{}'", "hl search:".bright_magenta().bold(), query.bright_yellow());
        return;
    }

    println!("{} {} — {}",
        "hl search:".bright_magenta().bold(),
        HL_SCRIPTS_DIR.bright_black(),
        if show_all {
            format!("{} skryptow", matched.len()).bright_white().to_string()
        } else {
            format!("{} wynikow dla '{}'", matched.len(), query).bright_white().to_string()
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
        eprintln!("{} Plik nie istnieje: {}", "BLAD".red().bold(), file.display());
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
        Err(e)   => { eprintln!("{} {}", "BLAD".red().bold(), e); 1 }
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

fn run_docs() {
    const DOCS_BIN: &str = "/usr/lib/HackerOS/Hacker-Lang/hl-docs";
    if !std::path::Path::new(DOCS_BIN).exists() {
        eprintln!("{} Binarka hl-docs nie znaleziona.", "hl docs:".bright_magenta().bold());
        eprintln!("  Oczekiwana sciezka: {}", DOCS_BIN.bright_white());
        eprintln!("  Zainstaluj: {}", "sudo hl-docs-install".bright_cyan());
        std::process::exit(1);
    }
    let status = std::process::Command::new(DOCS_BIN).status()
        .unwrap_or_else(|e| { eprintln!("{} {}", "BLAD".red().bold(), e); std::process::exit(1); });
    std::process::exit(status.code().unwrap_or(0));
}

fn print_version() {
    println!("{} {}", "Hacker Lang".bright_magenta().bold(), "gen 2".bright_white());
    println!();
    println!("{}", "Komponenty:".bright_yellow());
    println!("  hl-parser    gen 2  -- Lexer, Parser, AST, Gen, Shebang");
    println!("  hl-core      gen 2  -- Executor, Env, Quick Functions, Diagnostics");
    println!("  hl-shell     gen 2  -- REPL, Shell, Completion");
    println!("  hl-docs      gen 2  -- Dokumentacja TUI (Go + Bubble Tea)");
    println!();
    println!("{}", "System Genow:".bright_yellow());
    println!("  Aktualny max gen: {}", format!("gen {}", HL_MAX_GEN).bright_magenta().bold());
    println!("  Domyslny gen:     {}", format!("gen {}", HL_DEFAULT_GEN).bright_magenta());
    println!("  Deklaracja:       {}", "using <gen 2>".bright_cyan());
    println!("  ROLLING:          {}", "using <rolling>  -- najnowsze funkcje (pre-gen 3)".bright_yellow());
    println!();
    println!("{}", "Shebang:".bright_yellow());
    println!("  {}", "#!/usr/bin/env hl".bright_cyan());
    println!("  {}", "#!/usr/bin/hl".bright_cyan());
    println!();
    println!("{}", "Manager pakietow:".bright_yellow());
    println!("  {}  -- manager pakietow bit (ekosystem HL)", "bit".bright_green().bold());
    println!("  {}  -- lista paczek: https://github.com/bit-io/repository", "bit install <nazwa>".bright_cyan());
    println!();
    println!("{}", "Importy:".bright_yellow());
    println!("  {}  -- biblioteka standardowa (pliki .hl w {})", "# <main/nazwa>".bright_cyan(), HL_MAIN_LIBS_DIR.bright_black());
    println!("  {}   -- biblioteka bit", "# <bit/nazwa>".bright_magenta());
    println!("  {} -- GitHub", "# <github/user/repo>".bright_blue());
    println!();
    println!("{}", "Skrypty systemowe:".bright_yellow());
    println!("  Katalog:  {}", HL_SCRIPTS_DIR.bright_white());
    println!("  Szukaj:   {}", "hl search <nazwa> | hl search all".bright_cyan());
    println!("  Uruchom:  {}", "hl exec <nazwa>".bright_cyan());
}
