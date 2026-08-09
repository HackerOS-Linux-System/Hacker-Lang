use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(value_name = "FILE")]
    pub file: Option<PathBuf>,

    #[arg(value_name = "ARGS", last = true)]
    pub script_args: Vec<String>,

    /// Włącz verbose output (debug info)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Automatycznie instaluj brakujące narzędzia przez apt (z deklaracji // narzędzie)
    #[arg(long = "install-deps", global = true)]
    pub install_deps: bool,

    #[arg(short = 'c', long = "code", value_name = "CODE")]
    pub inline_code: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
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
pub enum EnvAction {
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
pub enum LibAction {
    List,
    Install { name: String },
    Remove  { name: String },
    Bit,
    Info    { name: String },
}
