use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use tracing::info;
use crate::env::{Env, Value};

pub const MAIN_LIBS_DIR: &str = "/usr/lib/HackerOS/Hacker-Lang/main-libs";

// Nowa ścieżka bit libs: ~/.hackeros/hacker-lang/libs/<name>/current/
// (zamiast starego /usr/lib/HackerOS/Hacker-Lang/bit/<name>.so)
pub fn bit_base_dir() -> PathBuf {
    dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hackeros/hacker-lang/libs")
}

pub fn bit_current_dir(name: &str) -> PathBuf {
    bit_base_dir().join(name).join("current")
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportSource {
    Main    { lib: String, detail: Option<String>, version: Option<String> },
    Bit     { name: String, version: Option<String> },
    GitHub  { path: String, version: Option<String> },
}

pub fn parse_import_spec(raw: &str) -> Option<ImportSource> {
    let raw = raw.trim();
    let (body, version) = if let Some(pos) = raw.rfind(':') {
        let after = &raw[pos + 1..];
        if !after.contains('/') { (&raw[..pos], Some(after.to_string())) } else { (raw, None) }
    } else { (raw, None) };

    let slash = body.find('/')?;
    let namespace = &body[..slash];
    let rest      = &body[slash + 1..];

    match namespace {
        "main" | "std" | "core" => {
            let (lib, detail) = if let Some(s) = rest.find('/') {
                (rest[..s].to_string(), Some(rest[s+1..].to_string()))
            } else { (rest.to_string(), None) };
            Some(ImportSource::Main { lib, detail, version })
        }
        "bit" | "virus" => Some(ImportSource::Bit { name: rest.to_string(), version }),
        "github" | "community" => Some(ImportSource::GitHub { path: rest.to_string(), version }),
        _ => None,
    }
}

pub fn resolve_import(lib: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let lib = lib.trim();
    if lib.starts_with('<') && lib.ends_with('>') {
        let spec = &lib[1..lib.len()-1];
        if let Some(src) = parse_import_spec(spec) {
            return dispatch_import(src, env);
        }
    }
    if let Some(src) = parse_import_spec(lib) {
        return dispatch_import(src, env);
    }
    match lib {
        "std/net"    | "main/net"    => load_main_lib("net",   detail, env),
        "std/fs"     | "main/fs"     => load_main_lib("fs",    detail, env),
        "std/sys"    | "main/sys"    => load_main_lib("sys",   detail, env),
        "std/str"    | "main/str"    => load_main_lib("str",   detail, env),
        "std/crypto" | "main/crypto" => load_main_lib("crypto",detail, env),
        "std/proc"   | "main/proc"   => load_main_lib("proc",  detail, env),
        _ => bail!("Nieznana biblioteka: '{}'", lib),
    }
}

fn dispatch_import(src: ImportSource, env: &mut Env) -> Result<()> {
    match src {
        ImportSource::Main { lib, detail, .. }  => load_main_lib(&lib, detail.as_deref(), env),
        ImportSource::Bit  { name, version }     => load_bit_lib(&name, version.as_deref(), env),
        ImportSource::GitHub { path, version }   => load_github_lib(&path, version.as_deref(), env),
    }
}

// ── Main libs — pliki .hl w MAIN_LIBS_DIR ─────────────────────────────────────

fn load_main_lib(lib: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let libs_dir = Path::new(MAIN_LIBS_DIR);
    let hl_file  = libs_dir.join(format!("{}.hl", lib));
    let dir_file = libs_dir.join(lib).join("lib.hl");

    if hl_file.exists() {
        info!("Laduje main lib '{}' z {:?}", lib, hl_file);
        let src   = std::fs::read_to_string(&hl_file)?;
        let nodes = hl_parser::parse_source(&src)?;
        crate::executor::exec_nodes(&nodes, env)?;
        eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/{}", lib);
        return Ok(());
    }
    if dir_file.exists() {
        info!("Laduje main lib '{}' z {:?}", lib, dir_file);
        let src   = std::fs::read_to_string(&dir_file)?;
        let nodes = hl_parser::parse_source(&src)?;
        crate::executor::exec_nodes(&nodes, env)?;
        eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/{}", lib);
        return Ok(());
    }

    // Builtin fallback
    match lib {
        "net"          => load_builtin_net(detail, env),
        "fs"           => load_builtin_fs(detail, env),
        "sys"          => load_builtin_sys(detail, env),
        "str"          => load_builtin_str(detail, env),
        "crypto"       => load_builtin_crypto(detail, env),
        "proc"         => load_builtin_proc(detail, env),
        "colors"       => load_builtin_colors(env),
        "cli"          => load_builtin_cli(env),
        "progress-bar" => load_builtin_progress_bar(env),
        "json"         => load_builtin_json(env),
        "hk-parser"    => load_builtin_hk_parser(env),
        "hacker"       => load_builtin_hacker(env),
        other => bail!(
            "Biblioteka main/{} nie znaleziona w {} i brak wbudowanego fallbacku",
            other, MAIN_LIBS_DIR
        ),
    }
}

// ── Bit libs — ~/.hackeros/hacker-lang/libs/<name>/current/ ──────────────────
//
// Nowa struktura (zamiast /usr/lib/HackerOS/Hacker-Lang/bit/):
//   ~/.hackeros/hacker-lang/libs/<name>/current/  ← symlink do <name>/<commit>/
//
// Szuka w kolejności:
//   1. current/lib.hl     (biblioteka HL)
//   2. current/<name>.hl  (plik z nazwą pakietu)
//   3. current/<name>.so  (biblioteka natywna)
//
// Jeśli nie zainstalowany → instrukcja instalacji przez bit

fn load_bit_lib(name: &str, _version: Option<&str>, env: &mut Env) -> Result<()> {
    let current_dir = bit_current_dir(name);

    if !current_dir.exists() {
        // Sprawdź czy pakiet istnieje w repo (online check byłby zbyt wolny — pomijamy)
        bail!(
            "Biblioteka bit '{}' nie jest zainstalowana.\n\
\n\
Aby zainstalować:\n\
\x1b[32m  bit install {}\x1b[0m\n\
\n\
Jeśli pakiet nie istnieje w repozytorium:\n\
\x1b[32m  bit search {}\x1b[0m",
name, name, name
        );
    }

    // Szukaj pliku .hl do załadowania
    let candidates = [
        current_dir.join("lib.hl"),
        current_dir.join(format!("{}.hl", name)),
        current_dir.join("main.hl"),
        current_dir.join("mod.hl"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            info!("Laduje bit lib '{}' z {:?}", name, candidate);
            let src   = std::fs::read_to_string(candidate)?;
            let nodes = hl_parser::parse_source(&src)?;
            crate::executor::exec_nodes(&nodes, env)?;
            eprintln!("\x1b[35m[hl bit]\x1b[0m Zaladowano bit/{}", name);

            // Ustaw zmienne informacyjne
            let prefix = name.to_uppercase().replace('-', "_");
            env.set_var(&format!("BIT_{}_LOADED", prefix), Value::Bool(true));
            env.set_var(&format!("BIT_{}_PATH", prefix),
                        Value::String(current_dir.display().to_string()));
            return Ok(());
        }
    }

    // Biblioteka natywna .so
    let so_path = current_dir.join(format!("{}.so", name));
    if so_path.exists() {
        let prefix = name.to_uppercase().replace('-', "_");
        env.set_var(&format!("BIT_{}_LOADED", prefix), Value::Bool(true));
        env.set_var(&format!("BIT_{}_PATH", prefix),
                    Value::String(so_path.display().to_string()));
        eprintln!("\x1b[35m[hl bit]\x1b[0m Zaladowano bit/{} (.so)", name);
        return Ok(());
    }

    bail!(
        "Biblioteka bit '{}' zainstalowana w {:?} ale brak pliku lib.hl/{}.hl/main.hl/{}.so\n\
Spróbuj: bit upgrade {}",
name, current_dir, name, name, name
    );
}

// ── GitHub libs ───────────────────────────────────────────────────────────────

fn load_github_lib(path: &str, version: Option<&str>, env: &mut Env) -> Result<()> {
    let lib_dir = github_libs_dir().join(path.replace('/', "__"));

    if !lib_dir.exists() {
        if which::which("git").is_err() { bail!("git nie jest zainstalowany"); }
        std::fs::create_dir_all(&lib_dir)?;
        let url = format!("https://github.com/{}.git", path);
        let mut cmd = std::process::Command::new("git");
        cmd.args(["clone", "--depth=1"]);
        if let Some(v) = version { cmd.args(["--branch", v]); }
        cmd.args([&url, lib_dir.to_str().unwrap_or("/tmp/hl_lib")]);
        if !cmd.status()?.success() { bail!("Nie mozna pobrac github: {}", path); }
    }

    load_from_dir(&lib_dir, None, env, path)
}

fn load_from_dir(dir: &Path, detail: Option<&str>, env: &mut Env, name: &str) -> Result<()> {
    let main_file = if let Some(d) = detail {
        let f = dir.join(format!("{}.hl", d));
        if f.exists() { f } else { dir.join(d).join("mod.hl") }
    } else {
        ["lib.hl","mod.hl","main.hl"].iter().map(|c| dir.join(c)).find(|p| p.exists())
        .unwrap_or_else(|| dir.join("lib.hl"))
    };
    if !main_file.exists() { bail!("Brak pliku wejsciowego dla '{}' w {:?}", name, dir); }
    let src   = std::fs::read_to_string(&main_file)?;
    let nodes = hl_parser::parse_source(&src)?;
    crate::executor::exec_nodes(&nodes, env)?;
    Ok(())
}

pub fn github_libs_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hl/libs/github")
}

pub fn hl_cache_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hackeros/hacker-lang/cache")
}

// ── Builtin fallbacks ─────────────────────────────────────────────────────────

fn load_builtin_net(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("NET_LOCALHOST", Value::String("127.0.0.1".into()));
    env.set_var("NET_BROADCAST", Value::String("255.255.255.255".into()));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/net (builtin fallback)");
    Ok(())
}
fn load_builtin_fs(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    let home = dirs::home_dir().map(|p| p.display().to_string()).unwrap_or_default();
    env.set_var("FS_HOME",    Value::String(home));
    env.set_var("FS_TMP",     Value::String("/tmp".into()));
    env.set_var("FS_ETC",     Value::String("/etc".into()));
    env.set_var("FS_VAR_LOG", Value::String("/var/log".into()));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/fs (builtin fallback)");
    Ok(())
}
fn load_builtin_sys(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("SYS_ARCH",     Value::String(std::env::consts::ARCH.into()));
    env.set_var("SYS_HOSTNAME", Value::String(
        std::fs::read_to_string("/etc/hostname").unwrap_or_default().trim().into()
    ));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/sys (builtin fallback)");
    Ok(())
}
fn load_builtin_str(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("STR_NEWLINE", Value::String("\n".into()));
    env.set_var("STR_TAB",     Value::String("\t".into()));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/str (builtin fallback)");
    Ok(())
}
fn load_builtin_crypto(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("CRYPTO_SHA256_CMD", Value::String("sha256sum".into()));
    env.set_var("CRYPTO_MD5_CMD",    Value::String("md5sum".into()));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/crypto (builtin fallback)");
    Ok(())
}
fn load_builtin_proc(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("PROC_SELF_PID", Value::Number(std::process::id() as f64));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/proc (builtin fallback)");
    Ok(())
}
fn load_builtin_colors(env: &mut Env) -> Result<()> {
    env.set_var("COLOR_RED",    Value::String("\x1b[31m".into()));
    env.set_var("COLOR_GREEN",  Value::String("\x1b[32m".into()));
    env.set_var("COLOR_YELLOW", Value::String("\x1b[33m".into()));
    env.set_var("COLOR_CYAN",   Value::String("\x1b[36m".into()));
    env.set_var("COLOR_RESET",  Value::String("\x1b[0m".into()));
    env.set_var("COLOR_BOLD",   Value::String("\x1b[1m".into()));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/colors (builtin fallback)");
    Ok(())
}
fn load_builtin_cli(env: &mut Env) -> Result<()> {
    env.set_var("CLI_ARGS_COUNT", Value::Number(std::env::args().count() as f64));
    env.set_var("CLI_PROG_NAME", Value::String(
        std::env::args().next().unwrap_or_else(|| "hl".into())
    ));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/cli (builtin fallback)");
    Ok(())
}
fn load_builtin_progress_bar(env: &mut Env) -> Result<()> {
    env.set_var("PROGRESS_BAR_LOADED", Value::Bool(true));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/progress-bar (builtin fallback)");
    Ok(())
}
fn load_builtin_json(env: &mut Env) -> Result<()> {
    env.set_var("JSON_LOADED", Value::Bool(true));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/json (builtin fallback)");
    Ok(())
}
fn load_builtin_hk_parser(env: &mut Env) -> Result<()> {
    env.set_var("HK_PARSER_LOADED",  Value::Bool(true));
    env.set_var("HK_PARSER_VERSION", Value::String("gen 1".into()));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/hk-parser (builtin fallback)");
    Ok(())
}
fn load_builtin_hacker(env: &mut Env) -> Result<()> {
    env.set_var("HACKER_PARSER_LOADED",  Value::Bool(true));
    env.set_var("HACKER_PARSER_VERSION", Value::String("gen 1".into()));
    eprintln!("\x1b[36m[hl main]\x1b[0m Zaladowano main/hacker (builtin fallback)");
    Ok(())
}

// ── CLI helpers ───────────────────────────────────────────────────────────────

pub fn cmd_lib_list() {
    use colored::Colorize;
    println!("{}", "=== Biblioteki standardowe (main) ===".bright_cyan().bold());
    println!("  Katalog: {}", MAIN_LIBS_DIR.bright_white());
    println!();
    for (name, desc) in [
        ("main/net","Siec: IP, gateway, iface, porty"),
        ("main/fs","System plikow: FS_HOME, FS_TMP..."),
        ("main/sys","OS, kernel, CPU, RAM"),
        ("main/str","Stale stringowe"),
        ("main/crypto","sha256, md5, gpg"),
        ("main/proc","PID, PPID"),
        ("main/colors","Kolory ANSI"),
        ("main/cli","CLI: argumenty, nazwa programu"),
        ("main/progress-bar","Pasek postepu"),
        ("main/json","Parser JSON"),
        ("main/hk-parser","Parser plikow .hk (HackerOS Config)"),
        ("main/hacker","Parser plikow .hacker (v1/v2/v3)"),
    ] {
        println!("  {} {}", format!("# <{}>", name).bright_green(), desc.bright_black());
    }
    println!();
    println!("{}", "=== Biblioteki bit ===".bright_magenta().bold());
    println!("  Instalacja: {}", "bit install <nazwa>".bright_cyan());
    println!("  Lokalizacja: {}", "~/.hackeros/hacker-lang/libs/<name>/current/".bright_white());
    println!("  Lista:      {}", "https://github.com/bit-io/repository/blob/main/bit-repo/repo-list.json".bright_black());
}

pub fn cmd_lib_install(repo: &str) {
    use colored::Colorize;
    let lib_dir = github_libs_dir().join(repo.replace('/', "__"));
    if lib_dir.exists() { println!("{} '{}' juz zainstalowana.", "✓".green(), repo); return; }
    let url = format!("https://github.com/{}.git", repo);
    let status = std::process::Command::new("git")
    .args(["clone","--depth=1",&url,lib_dir.to_str().unwrap()])
    .status();
    match status {
        Ok(s) if s.success() => println!("{} Zainstalowano: {}", "✓".green(), repo),
        _ => eprintln!("{} Blad instalacji: {}", "✗".red(), repo)
    }
}

pub fn cmd_lib_remove(name: &str) {
    use colored::Colorize;
    let p = github_libs_dir().join(name.replace('/', "__"));
    if p.exists() { std::fs::remove_dir_all(&p).unwrap(); println!("{} Usunieto: {}", "✓".green(), name); }
    else { eprintln!("{} Nie znaleziono: {}", "✗".red(), name); }
}

pub fn cmd_clean_cache() {
    use colored::Colorize;
    let cache = hl_cache_dir();
    if cache.exists() {
        std::fs::remove_dir_all(&cache).unwrap_or(());
        println!("{} Cache wyczyszczony.", "✓".green());
    } else {
        println!("{}", "Cache jest pusty.".bright_black());
    }
}
