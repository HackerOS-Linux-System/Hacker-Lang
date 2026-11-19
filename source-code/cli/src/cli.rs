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

    /// Buduj / sprawdzaj / weryfikuj archiwa `.hlib` (HackerOS Lib) —
    /// natywny format bibliotek Hacker Lang, czytelny też dla H# i
    /// HackerScript. Zobacz /HLIB_FORMAT.md w katalogu głównym repo.
    /// (Nazwa `hlib`, nie `lib` — `hl lib` to już istniejący opis
    /// systemu pakietów `bit`.)
    Hlib {
        #[command(subcommand)]
        action: HlibAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum HlibAction {
    /// Zbuduj `.hlib` z pliku `.hl`: pakuje AST (bezpośrednio
    /// interpretowalne przez `hl`), opcjonalny bytecode `.bc`, oraz
    /// nagłówek eksportów (lista `FuncDef`/`ArenaFuncDef`).
    /// Zapis atomowy; `SOURCE_DATE_EPOCH` daje reprodukowalny `created_at`.
    Build {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(long, default_value = "0.1.0")]
        lib_version: String,
        /// Dołącz też skompilowany bytecode (.bc) jako dodatkowy artefakt
        #[arg(long)]
        with_bytecode: bool,
        /// Plik z kluczem prywatnym Ed25519 (hex) do podpisania archiwum
        #[arg(long)]
        sign: Option<PathBuf>,
        /// Opis biblioteki (manifest.description)
        #[arg(long)]
        description: Option<String>,
        /// Autor (można powtarzać)
        #[arg(long = "author")]
        authors: Vec<String>,
        /// Zależność `nazwa=wymaganie` (np. `mold=^1.2`), można powtarzać
        #[arg(long = "dep")]
        deps: Vec<String>,
        /// Dołącz plik jako `extra/<nazwa pliku>` (README, LICENSE, ...), można powtarzać
        #[arg(long = "extra")]
        extras: Vec<PathBuf>,
        /// Poziom kompresji zstd 1-22
        #[arg(long, default_value_t = 19)]
        level: i32,
    },
    /// Wypisz manifest i listę wpisów archiwum `.hlib`.
    Inspect {
        file: PathBuf,
        /// Wyjście JSON (maszynowo czytelne; używa go `main/hlib.hl`)
        #[arg(long)]
        json: bool,
    },
    /// Zweryfikuj sumy SHA-256 (ścisłe) i (opcjonalnie) podpis Ed25519.
    /// Kod wyjścia 0 = OK, 1 = weryfikacja nie powiodła się.
    Verify {
        file: PathBuf,
        /// Zaufany klucz publiczny (hex); można powtarzać. Bez niego sprawdzane są tylko sumy,
        /// chyba że ustawiono `HL_HLIB_TRUSTED_KEYS` / `HL_HLIB_REQUIRE_SIGNED`.
        #[arg(long = "pubkey")]
        pubkeys: Vec<String>,
        /// Wymagaj podpisu
        #[arg(long)]
        require_signed: bool,
        #[arg(long)]
        json: bool,
    },
    /// Wygeneruj nową parę kluczy Ed25519 do podpisywania `.hlib`.
    Keygen {
        #[arg(short, long, default_value = "hlib_signing.key")]
        out: PathBuf,
        /// Nadpisz istniejący plik klucza
        #[arg(long)]
        force: bool,
    },
    /// Wypisz klucz publiczny (hex) odpowiadający plikowi z kluczem prywatnym.
    Pubkey { keyfile: PathBuf },
    /// Wypisz odcisk (SHA256:...) klucza publicznego podanego jako hex.
    Fingerprint { pubkey: String },
    /// Rozpakuj `.hlib` i wygeneruj gotowy do `<< ` (file-import) plik
    /// `.hl` udostępniający jego funkcje.
    Bind {
        file: PathBuf,
        #[arg(short, long, default_value = "hlibs")]
        into: PathBuf,
    },
    /// Bezpiecznie wypakuj archiwum (po weryfikacji sum) do katalogu.
    Extract {
        file: PathBuf,
        #[arg(short, long, default_value = ".")]
        into: PathBuf,
        /// Wypakuj tylko ten wpis (ścieżka wewnątrz archiwum)
        #[arg(long)]
        entry: Option<String>,
    },
    /// Wypisz biblioteki widoczne w katalogach wyszukiwania (hlibs/, ~/.hackeros, system).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Znajdź najwyższą wersję `nazwa` spełniającą wymaganie semver i wypisz jej ścieżkę.
    Find {
        name: String,
        #[arg(long, default_value = "*")]
        req: String,
        #[arg(long)]
        json: bool,
    },
    /// Zainstaluj archiwum jako `<nazwa>-<wersja>.hlib` (po weryfikacji) do katalogu.
    Install {
        file: PathBuf,
        /// Katalog docelowy (domyślnie ~/.hackeros/Hacker-Lang/hlibs)
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long = "pubkey")]
        pubkeys: Vec<String>,
        #[arg(long)]
        require_signed: bool,
        /// Nadpisz tę samą wersję
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Usuń zainstalowaną `<nazwa>-<wersja>.hlib`.
    Remove {
        name: String,
        version: String,
        #[arg(long)]
        dir: Option<PathBuf>,
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
