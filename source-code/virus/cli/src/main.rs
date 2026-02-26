use std::path::PathBuf;
use std::process::{Command, exit};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use anyhow::{bail, Context, Result};
// ProgressBar/Style używane w ui.rs
use owo_colors::OwoColorize;
// Term używane w ui.rs

// Statyczny link do hl-transpiler
use hl_transpiler::transpile_project;

mod config;
mod ui;
mod libs;
mod repo;

use config::VirusProject;
use ui::{banner, box_msg, step_ok, step_err, step_warn, step_info, progress_bar};
use libs::{install_lib, remove_lib, LibSource};


// ─────────────────────────────────────────────────────────────
// Ścieżki globalnych narzędzi
// ─────────────────────────────────────────────────────────────
fn hl_bin(name: &str) -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/bin")
    .join(name)
}

fn hl_libs_dir() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/libs")
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────
fn main() {
    if let Err(e) = run() {
        step_err(&format!("{}", e));
        exit(1);
    }
}

fn run() -> Result<()> {
    // Parsuj argumenty przez lexopt
    // OsString nie implementuje std::error::Error — używamy unwrap_or_default()
    let mut parser = lexopt::Parser::from_env();
    let mut args: Vec<String> = Vec::new();

    while let Some(arg) = parser.next()? {
        match arg {
            lexopt::Arg::Value(v) => {
                // into_string() zwraca Result<String, OsString> — OsString nie jest Error
                // więc używamy map_err z własnym komunikatem
                let s = v.into_string()
                .map_err(|os| anyhow::anyhow!(
                    "Argument zawiera nieprawidłowe znaki UTF-8: {:?}", os
                ))?;
                args.push(s);
            }
            lexopt::Arg::Short('h') | lexopt::Arg::Long("help") => {
                args.insert(0, "help".to_string());
                break;
            }
            lexopt::Arg::Short('v') | lexopt::Arg::Long("version") => {
                println!("virus {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => {}
        }
    }

    // Dispatcher komend
    match args.get(0).map(|s| s.as_str()) {
        Some("set")  => cmd_set(&args[1..]),
        Some("bb")   => cmd_bb(&args[1..]),
        Some("tt")   => cmd_tt(&args[1..]),
        Some("ss")   => cmd_ss(&args[1..]),
        Some("ii")   => cmd_ii(&args[1..]),
        Some("rr")   => cmd_rr(&args[1..]),
        Some("cc")   => cmd_cc(&args[1..]),
        Some("docs") => cmd_docs(),
        Some("help") | None => { cmd_help(); Ok(()) }
        Some(unknown) => {
            step_err(&format!("Nieznana komenda: '{}'", unknown));
            println!();
            cmd_help();
            exit(1);
        }
    }
}

// ═════════════════════════════════════════════════════════════
// virus set — inicjalizacja projektu
// ═════════════════════════════════════════════════════════════
fn cmd_set(args: &[String]) -> Result<()> {
    banner();

    let name = args.get(0).cloned().unwrap_or_else(|| {
        env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "my-project".to_string())
    });

    let cwd = env::current_dir()?;
    let project_dir = if args.is_empty() { cwd.clone() } else { cwd.join(&name) };

    box_msg(&format!("Tworzę projekt: {}", name.bright_cyan().bold()));

    // Struktura katalogów
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)
    .with_context(|| format!("Nie można utworzyć src/ w {:?}", src_dir))?;

    // src/main.hl
    let main_hl = src_dir.join("main.hl");
    if !main_hl.exists() {
        fs::write(&main_hl, format!(
            "!! Projekt: {}\n!! Wygenerowano przez virus set\n\n@log \"Witaj w hacker-lang!\"\n\n@log \"Projekt: {}\"\n",
            name, name
        ))?;
        step_ok(&format!("Utworzono {}", "src/main.hl".bright_green()));
    }

    // Virus.hk
    let virus_hk = project_dir.join("Virus.hk");
    if !virus_hk.exists() {
        fs::write(&virus_hk, format!(
            "! Virus.hk — konfiguracja projektu hacker-lang\n\
! Wygenerowano przez virus set\n\n\
[project]\n\
-> name => {}\n\
-> version => 0.1.0\n\
-> description => Nowy projekt hacker-lang\n\
-> authors => [\"\"]\n\
-> edition => 2024\n\n\
[dependencies]\n\
! Przykłady:\n\
! -> obsidian\n\
! --> source => bytes\n\
! --> version => 0.2\n\n\
[build]\n\
-> output => target/\n\
-> optimization => 2\n",
name
        ))?;
        step_ok(&format!("Utworzono {}", "Virus.hk".bright_green()));
    }

    // .gitignore
    let gitignore = project_dir.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, "target/\n.virus-cache/\n*.o\n*.ll\n")?;
        step_ok(&format!("Utworzono {}", ".gitignore".bright_green()));
    }

    println!();
    box_msg(&format!(
        "{} Projekt '{}' gotowy!\n\n  Uruchom: {} | Skompiluj: {}",
        "✓".bright_green().bold(),
                     name.bright_cyan(),
                     "virus ss".bright_yellow(),
                     "virus bb".bright_yellow()
    ));

    Ok(())
}

// ═════════════════════════════════════════════════════════════
// virus bb — kompilacja przez hl-compiler
// ═════════════════════════════════════════════════════════════
fn cmd_bb(args: &[String]) -> Result<()> {
    banner();

    let cwd = env::current_dir()?;
    let project = VirusProject::load(&cwd)?;

    // Flagi
    let flag = args.get(0).map(|s| s.as_str());
    match flag {
        Some("==") => return cmd_bb_release(&project),
        Some("=")  => return cmd_bb_test(&project),
        Some(",,") => return cmd_bb_publish(&project),
        Some(",")  => return cmd_bb_login(),
        None | Some(_) => {}
    }

    box_msg(&format!(
        "Kompiluję projekt {} {}",
        project.name.bright_cyan().bold(),
                     project.version.dimmed()
    ));

    // Instaluj zależności
    install_project_deps(&project)?;

    // Znajdź src/main.hl
    let main_hl = cwd.join("src").join("main.hl");
    if !main_hl.exists() {
        bail!("Nie znaleziono src/main.hl");
    }

    let compiler = hl_bin("hl-compiler");
    if !compiler.exists() {
        bail!("hl-compiler nie znaleziony pod {:?}\nZainstaluj przez: hackeros install hl-compiler", compiler);
    }

    let output_dir = cwd.join(project.build_output.as_deref().unwrap_or("target"));
    fs::create_dir_all(&output_dir)?;
    let output_bin = output_dir.join(&project.name);

    let pb = progress_bar(5, &format!("Kompilacja {}", project.name.bright_cyan()));

    // Krok 1 — analiza PLSA
    pb.set_message(format!("{}  Analiza AST...", "⟳".bright_blue()));
    thread::sleep(Duration::from_millis(50)); // Krótka pauza dla animacji
    pb.inc(1);

    // Krok 2 — generowanie IR
    pb.set_message(format!("{}  Generowanie LLVM IR...", "⟳".bright_blue()));

    let opt = project.optimization.unwrap_or(2);
    let mut compiler_cmd = Command::new(&compiler);
    compiler_cmd
    .arg(&main_hl)
    .arg("-o").arg(&output_bin)
    .arg("--opt").arg(opt.to_string());

    if flag == Some("-v") || flag == Some("--verbose") {
        compiler_cmd.arg("--verbose");
    }

    pb.inc(1);

    // Krok 3 — kompilacja
    pb.set_message(format!("{}  Kompilacja...", "⟳".bright_blue()));

    let status = compiler_cmd.status()
    .with_context(|| format!("Nie można uruchomić hl-compiler pod {:?}", compiler))?;

    pb.inc(2);

    // Krok 4 — gotowe
    pb.set_message(format!("{}  Linkowanie...", "⟳".bright_blue()));
    thread::sleep(Duration::from_millis(30));
    pb.inc(1);
    pb.finish_with_message(format!("{} Kompilacja zakończona", "✓".bright_green()));

    if !status.success() {
        step_err(&format!("Kompilacja nieudana (kod: {})", status));
        exit(1);
    }

    println!();
    step_ok(&format!(
        "Binarka: {} ({})",
                     output_bin.display().to_string().bright_green(),
                     human_size(fs::metadata(&output_bin).map(|m| m.len()).unwrap_or(0))
    ));

    Ok(())
}

fn cmd_bb_release(project: &VirusProject) -> Result<()> {
    box_msg(&format!("{}  Release build: {}", "🚀".bright_yellow(), project.name.bright_cyan().bold()));

    let cwd = env::current_dir()?;
    let main_hl = cwd.join("src").join("main.hl");
    if !main_hl.exists() { bail!("Nie znaleziono src/main.hl"); }

    install_project_deps(project)?;

    let compiler = hl_bin("hl-compiler");
    let output_dir = cwd.join(project.build_output.as_deref().unwrap_or("target")).join("release");
    fs::create_dir_all(&output_dir)?;
    let output_bin = output_dir.join(&project.name);

    let pb = progress_bar(4, "Release build");

    pb.set_message("Kompilacja z optymalizacją O3...");
    let status = Command::new(&compiler)
    .arg(&main_hl)
    .arg("-o").arg(&output_bin)
    .arg("--opt").arg("3")
    .status()?;
    pb.inc(4);
    pb.finish_with_message(format!("{} Release gotowy", "✓".bright_green()));

    if !status.success() {
        bail!("Release build nieudany");
    }

    step_ok(&format!("Release: {}", output_bin.display().to_string().bright_green()));
    Ok(())
}

fn cmd_bb_test(project: &VirusProject) -> Result<()> {
    box_msg(&format!("{}  Test build: {}", "🧪".bright_yellow(), project.name.bright_cyan()));

    let cwd = env::current_dir()?;
    let main_hl = cwd.join("src").join("main.hl");
    if !main_hl.exists() { bail!("Nie znaleziono src/main.hl"); }

    let compiler = hl_bin("hl-compiler");
    let runtime  = hl_bin("hl-runtime");
    let output_bin = cwd.join("target").join(format!("{}-test", project.name));

    fs::create_dir_all(output_bin.parent().unwrap())?;

    let pb = progress_bar(3, "Test build");

    pb.set_message("Kompilacja testowa...");
    let status = Command::new(&compiler)
    .arg(&main_hl)
    .arg("-o").arg(&output_bin)
    .arg("--opt").arg("0")
    .status()?;
    pb.inc(2);

    if !status.success() { bail!("Test build nieudany"); }

    pb.set_message("Uruchamiam testy...");
    let run_status = Command::new(&output_bin).status()
    .unwrap_or_else(|_| {
        // Fallback przez hl-runtime
        Command::new(&runtime)
        .arg(&main_hl)
        .status()
        .expect("Nie można uruchomić")
    });
    pb.inc(1);
    pb.finish_with_message(format!("{} Testy zakończone (kod: {})", "✓".bright_green(), run_status));

    let _ = fs::remove_file(&output_bin);
    Ok(())
}

fn cmd_bb_publish(project: &VirusProject) -> Result<()> {
    box_msg(&format!("{}  Publikowanie: {}", "📦".bright_yellow(), project.name.bright_cyan()));
    step_warn("Publikowanie do repozytoriów virus.io — funkcja w przygotowaniu");
    step_info(&format!("Projekt: {} v{}", project.name, project.version));
    step_info("Aby opublikować, utwórz PR na: github.com/HackerOS-Linux-System/Hacker-Lang");
    Ok(())
}

fn cmd_bb_login() -> Result<()> {
    box_msg("Logowanie do virus.io");
    step_warn("Logowanie — funkcja w przygotowaniu");
    step_info("Token będzie przechowywany w: ~/.hackeros/hacker-lang/credentials");
    Ok(())
}

// ═════════════════════════════════════════════════════════════
// virus tt — transpilacja → Rust
// ═════════════════════════════════════════════════════════════
fn cmd_tt(args: &[String]) -> Result<()> {
    banner();

    let cwd     = env::current_dir()?;
    let project = VirusProject::load(&cwd)?;

    // Cel transpilacji (na razie tylko rust)
    let target = args.get(0).map(|s| s.as_str()).unwrap_or("rust");
    if target != "rust" {
        bail!("Nieobsługiwany cel transpilacji: '{}'\nDostępne: rust", target);
    }

    box_msg(&format!(
        "Transpilacja {} → {}",
        project.name.bright_cyan().bold(),
                     "Rust".bright_yellow().bold()
    ));

    let src_dir = cwd.join("src");
    if !src_dir.exists() {
        bail!("Katalog src/ nie istnieje — uruchom 'virus set'");
    }

    let out_dir = cwd.join("target").join("transpiled").join("rust").join("src");

    // Zbierz pliki
    let hl_files: Vec<_> = walkdir::WalkDir::new(&src_dir)
    .into_iter()
    .filter_map(|e| e.ok())
    .filter(|e| e.path().extension().map_or(false, |x| x == "hl"))
    .collect();

    if hl_files.is_empty() {
        bail!("Nie znaleziono żadnych plików .hl w src/");
    }

    let pb = progress_bar(hl_files.len() as u64, "Transpilacja");

    let start = Instant::now();
    let results = transpile_project(&src_dir, &out_dir)
    .with_context(|| "Błąd transpilacji")?;

    for r in &results {
        pb.set_message(format!(
            "{}  {}",
            "⟳".bright_blue(),
                               r.output_path.file_name().unwrap_or_default().to_string_lossy()
        ));
        pb.inc(1);
    }

    pb.finish_with_message(format!("{} Transpilacja zakończona", "✓".bright_green()));

    println!();
    let total_lines: usize = results.iter().map(|r| r.lines).sum();
    step_ok(&format!(
        "Przetransponowano {} plików → {} linii Rust ({:.1}s)",
                     results.len().to_string().bright_cyan(),
                     total_lines.to_string().bright_cyan(),
                     start.elapsed().as_secs_f64()
    ));
    step_ok(&format!("Wyjście: {}", out_dir.display().to_string().bright_green()));

    // Ostrzeżenia
    for r in &results {
        for w in &r.warnings {
            step_warn(w);
        }
    }

    println!();
    step_info(&format!(
        "Aby skompilować wygenerowany Rust:\n  cd {}\n  cargo build",
        cwd.join("target/transpiled/rust").display()
    ));

    Ok(())
}

// ═════════════════════════════════════════════════════════════
// virus ss — uruchamianie przez hl-runtime
// ═════════════════════════════════════════════════════════════
fn cmd_ss(args: &[String]) -> Result<()> {
    banner();

    let cwd     = env::current_dir()?;
    let project = VirusProject::load(&cwd)?;

    let runtime = hl_bin("hl-runtime");
    if !runtime.exists() {
        bail!("hl-runtime nie znaleziony pod {:?}", runtime);
    }

    let main_hl = cwd.join("src").join("main.hl");
    if !main_hl.exists() {
        bail!("Nie znaleziono src/main.hl");
    }

    box_msg(&format!(
        "{}  Uruchamiam preview: {}",
        "▶".bright_green().bold(),
                     project.name.bright_cyan().bold()
    ));

    println!("{}", "─".repeat(60).dimmed());

    let mut cmd = Command::new(&runtime);
    cmd.arg(&main_hl);

    // Przekaż dodatkowe argumenty
    for arg in args {
        cmd.arg(arg);
    }

    let status = cmd.status()
    .with_context(|| "Nie można uruchomić hl-runtime")?;

    println!("{}", "─".repeat(60).dimmed());
    println!();

    if status.success() {
        step_ok(&format!("Program zakończył się z kodem: {}", "0".bright_green()));
    } else {
        step_warn(&format!(
            "Program zakończył się z kodem: {}",
            status.code().unwrap_or(-1).to_string().bright_red()
        ));
    }

    Ok(())
}

// ═════════════════════════════════════════════════════════════
// virus ii — instalacja biblioteki
// ═════════════════════════════════════════════════════════════
fn cmd_ii(args: &[String]) -> Result<()> {
    banner();

    let lib_name = args.get(0)
    .ok_or_else(|| anyhow::anyhow!("Podaj nazwę biblioteki: virus ii <nazwa>"))?;

    // Opcjonalne źródło: virus ii obsidian --bytes
    let source = args.iter().find_map(|a| match a.as_str() {
        "--bytes"  | "-b" => Some(LibSource::Bytes),
                                      "--virus"  | "-V" => Some(LibSource::Virus),
                                      "--vira"   | "-r" => Some(LibSource::Vira),
                                      "--core"   | "-c" => Some(LibSource::Core),
                                      "--github" | "-g" => Some(LibSource::Github),
                                      "--source" | "-s" => Some(LibSource::Source),
                                      _ => None,
    }).unwrap_or(LibSource::Bytes); // domyślnie bytes

    box_msg(&format!(
        "{}  Instaluję: {} [{}]",
        "⬇".bright_green().bold(),
                     lib_name.bright_cyan().bold(),
                     format!("{:?}", source).to_lowercase().bright_yellow()
    ));

    install_lib(lib_name, source)?;

    Ok(())
}

// ═════════════════════════════════════════════════════════════
// virus rr — usunięcie biblioteki
// ═════════════════════════════════════════════════════════════
fn cmd_rr(args: &[String]) -> Result<()> {
    banner();

    let lib_name = args.get(0)
    .ok_or_else(|| anyhow::anyhow!("Podaj nazwę biblioteki: virus rr <nazwa>"))?;

    let source = args.iter().find_map(|a| match a.as_str() {
        "--bytes"  | "-b" => Some(LibSource::Bytes),
                                      "--virus"  | "-V" => Some(LibSource::Virus),
                                      "--vira"   | "-r" => Some(LibSource::Vira),
                                      "--core"   | "-c" => Some(LibSource::Core),
                                      _ => None,
    }).unwrap_or(LibSource::Bytes);

    box_msg(&format!(
        "{}  Usuwam: {} [{}]",
        "✗".bright_red().bold(),
                     lib_name.bright_cyan().bold(),
                     format!("{:?}", source).to_lowercase().bright_yellow()
    ));

    remove_lib(lib_name, source)?;

    Ok(())
}

// ═════════════════════════════════════════════════════════════
// virus cc — czyszczenie
// ═════════════════════════════════════════════════════════════
fn cmd_cc(_args: &[String]) -> Result<()> {
    banner();

    let cwd = env::current_dir()?;
    let project = VirusProject::load(&cwd).ok();

    let name = project.as_ref().map(|p| p.name.as_str()).unwrap_or("projekt");

    box_msg(&format!("{}  Czyszczę: {}", "🧹".bright_yellow(), name.bright_cyan()));

    let target_dir = cwd.join("target");

    if !target_dir.exists() {
        step_info("Katalog target/ nie istnieje — nic do czyszczenia");
        return Ok(());
    }

    // Zbierz pliki do usunięcia
    let mut total_size: u64 = 0;
    let mut file_count: u64 = 0;

    let entries: Vec<_> = walkdir::WalkDir::new(&target_dir)
    .into_iter()
    .filter_map(|e| e.ok())
    .collect();

    for e in &entries {
        if let Ok(meta) = e.metadata() {
            if meta.is_file() {
                total_size += meta.len();
                file_count += 1;
            }
        }
    }

    let pb = progress_bar(3, "Czyszczenie");

    pb.set_message("Usuwam pliki obiektowe...");
    thread::sleep(Duration::from_millis(100));
    pb.inc(1);

    pb.set_message("Usuwam katalog target/...");
    fs::remove_dir_all(&target_dir)
    .with_context(|| "Nie można usunąć katalogu target/")?;
    pb.inc(1);

    pb.set_message("Czyszczę cache...");
    let cache_dir = cwd.join(".virus-cache");
    if cache_dir.exists() {
        let _ = fs::remove_dir_all(&cache_dir);
    }
    pb.inc(1);

    pb.finish_with_message(format!("{} Czyszczenie zakończone", "✓".bright_green()));

    println!();
    step_ok(&format!(
        "Usunięto {} plików ({})",
                     file_count.to_string().bright_cyan(),
                     human_size(total_size)
    ));

    Ok(())
}

// ═════════════════════════════════════════════════════════════
// virus docs
// ═════════════════════════════════════════════════════════════
fn cmd_docs() -> Result<()> {
    banner();

    let docs = format!(
        r#"
        {} — Menedżer projektów hacker-lang
        {}
        Wersja: {}

        {}
        {}    Inicjalizacja nowego projektu
        {}    Kompilacja projektu do binarki
        {}    Transpilacja .hl → Rust
        {}    Uruchomienie preview (hl-runtime)
    {}    Instalacja biblioteki
    {}    Usunięcie biblioteki
    {}    Czyszczenie projektu
    {}    Dokumentacja
    {}    Pomoc

    {} virus bb
    {}   Release build (optymalizacja O3)
    {}   Test build (uruchamia po kompilacji)
    {}   Publikowanie projektu
    {}   Logowanie do virus.io

    {}
    {}
    {}

    {}
    {}
    {}
    {}
    {}
    {}
    {}

    {}
    {}
    {}
    {}
    {}

    {}
    {} — Repozytorium bibliotek
    {} — HackerOS Team
    "#,
    "virus".bright_cyan().bold(),
                       "════════════════════════════".bright_cyan().dimmed(),
                       env!("CARGO_PKG_VERSION").bright_yellow(),

                       "KOMENDY:".bright_white().bold(),
                       "virus set".bright_yellow(),
                       "virus bb".bright_yellow(),
                       "virus tt [rust]".bright_yellow(),
                       "virus ss".bright_yellow(),
                       "virus ii <lib>".bright_yellow(),
                       "virus rr <lib>".bright_yellow(),
                       "virus cc".bright_yellow(),
                       "virus docs".bright_yellow(),
                       "virus help".bright_yellow(),

                       "FLAGI dla".bright_white().bold(),
                       "virus bb ==".bright_yellow(),
                       "virus bb =".bright_yellow(),
                       "virus bb ,,".bright_yellow(),
                       "virus bb ,".bright_yellow(),

                       "STRUKTURA PROJEKTU:".bright_white().bold(),
                       "my-project/Virus.hk".bright_green(),
                       "my-project/src/main.hl".bright_green(),

                       "TYPY BIBLIOTEK (virus ii):".bright_white().bold(),
                       "--bytes  (-b)".bright_yellow(),
                       "--virus  (-V)".bright_yellow(),
                       "--vira   (-r)".bright_yellow(),
                       "--core   (-c)".bright_yellow(),
                       "--github (-g)".bright_yellow(),
                       "--source (-s)".bright_yellow(),

                       "OPIS TYPÓW BIBLIOTEK:".bright_white().bold(),
                       "bytes  — skompilowane .so w ~/.hackeros/hacker-lang/libs/bytes/".dimmed(),
                       "virus  — tymczasowe .a w ~/.hackeros/hacker-lang/libs/.virus/ (czyszczone po reboot)".dimmed(),
                       "vira   — .hlib (jak cargo) w ~/.hackeros/hacker-lang/libs/.vira/ [placeholder]".dimmed(),
                       "core   — pliki .hl w ~/.hackeros/hacker-lang/libs/core/".dimmed(),

                       "LINKI:".bright_white().bold(),
                       "https://github.com/HackerOS-Linux-System/Hacker-Lang/blob/main/repository/virus.io".bright_blue(),
                       "hackeros068@gmail.com".dimmed()
    );

    println!("{}", docs);
    Ok(())
}

// ═════════════════════════════════════════════════════════════
// virus help
// ═════════════════════════════════════════════════════════════
fn cmd_help() {
    banner();

    println!("{}", format!(
        "  {}\n",
        "Użycie: virus <komenda> [opcje]".bright_white().bold()
    ));

    let cmds = [
        ("set",         "Inicjalizacja nowego projektu hacker-lang"),
        ("bb",          "Kompilacja projektu do binarki"),
        ("bb ==",       "Release build (optymalizacja pełna)"),
        ("bb =",        "Test build (kompilacja + uruchomienie)"),
        ("bb ,,",       "Publikowanie projektu"),
        ("bb ,",        "Logowanie do rejestru"),
        ("tt [rust]",   "Transpilacja .hl → język docelowy"),
        ("ss",          "Uruchomienie preview przez hl-runtime"),
        ("ii <lib>",    "Instalacja biblioteki"),
        ("rr <lib>",    "Usunięcie biblioteki"),
        ("cc",          "Czyszczenie projektu (target/)"),
        ("docs",        "Pełna dokumentacja"),
        ("help",        "Ta pomoc"),
    ];

    for (cmd, desc) in &cmds {
        println!(
            "  {:30} {}",
            format!("virus {}", cmd).bright_yellow(),
                desc.dimmed()
        );
    }

    println!();
    println!("  {} {}", "Więcej:".dimmed(), "virus docs".bright_cyan());
    println!();
}

// ═════════════════════════════════════════════════════════════
// Pomocnicze: instalacja zależności projektu
// ═════════════════════════════════════════════════════════════
fn install_project_deps(project: &VirusProject) -> Result<()> {
    if project.dependencies.is_empty() {
        return Ok(());
    }

    step_info(&format!(
        "Sprawdzam {} zależności...",
        project.dependencies.len()
    ));

    let pb = progress_bar(
        project.dependencies.len() as u64,
                          "Zależności"
    );

    for dep in &project.dependencies {
        pb.set_message(format!("{}  {}...", "⬇".bright_blue(), dep.name.bright_cyan()));
        install_lib(&dep.name, dep.source.clone())?;
        pb.inc(1);
    }

    pb.finish_with_message(format!("{} Zależności zainstalowane", "✓".bright_green()));
    Ok(())
}

/// Rozmiar w czytelnej postaci
fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

