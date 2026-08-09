use colored::Colorize;
use hl_core::{HL_DEFAULT_GEN, HL_MAX_GEN};

use crate::guard::HL_SCRIPTS_DIR;

pub fn run_docs() {
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

pub fn print_version() {
    println!("{} {}", "Hacker Lang".bright_magenta().bold(), "gen 2".bright_white());
    println!();
    println!("{}", "Komponenty:".bright_yellow());
    println!("  hl-parser    gen 2  -- Lexer, Parser, AST, Gen, Shebang");
    println!("  hl-core      gen 2  -- Executor, Env, Quick Functions, Diagnostics");
    println!("  hl-coreutils gen 2  -- Wbudowane coreutils (/> ...), jedno narzędzie = jeden plik");
    println!("  hl-compiler  gen 2  -- Bytecode compiler (AST → .bc, Cranelift)");
    println!("  hl-jit       gen 2  -- JIT engine (Cranelift, eksperymentalny)");
    println!("  hl-shell     gen 2  -- REPL, Shell, Completion");
    println!("  hl-docs      gen 2  -- Dokumentacja TUI (Go + Bubble Tea)");
    println!("  hlh          gen 2  -- Tłumacz/generator dokumentacji dla plików .hl");
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
