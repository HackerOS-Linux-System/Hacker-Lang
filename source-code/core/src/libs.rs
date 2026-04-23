use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use tracing::info;
use crate::env::{Env, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum ImportSource {
    Std     { lib: String, detail: Option<String>, version: Option<String> },
    Community { path: String, version: Option<String> },
    Virus   { name: String, version: Option<String> },
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
        "std" => {
            let (lib, detail) = if let Some(s) = rest.find('/') {
                (rest[..s].to_string(), Some(rest[s+1..].to_string()))
            } else { (rest.to_string(), None) };
            Some(ImportSource::Std { lib, detail, version })
        }
        "community" => Some(ImportSource::Community { path: rest.to_string(), version }),
        "virus"     => Some(ImportSource::Virus { name: rest.to_string(), version }),
        _ => None,
    }
}

pub fn resolve_import(lib: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let lib = lib.trim();
    if lib.starts_with('<') && lib.ends_with('>') {
        let spec = &lib[1..lib.len()-1];
        if let Some(src) = parse_import_spec(spec) {
            return match src {
                ImportSource::Std { lib, detail, .. }    => resolve_std(&lib, detail.as_deref(), env),
                ImportSource::Community { path, version } => resolve_community(&path, version.as_deref(), env),
                ImportSource::Virus { name, version }     => resolve_virus(&name, version.as_deref(), env),
            };
        }
    }
    match lib {
        "std/net"    => load_std_net(detail, env),
        "std/fs"     => load_std_fs(detail, env),
        "std/sys"    => load_std_sys(detail, env),
        "std/str"    => load_std_str(detail, env),
        "std/crypto" => load_std_crypto(detail, env),
        "std/proc"   => load_std_proc(detail, env),
        _ => {
            // community / github fallback
            if lib.contains('/') && !lib.starts_with("std/") {
                let lib_dir = hl_libs_dir_legacy().join("github").join(lib.replace('/', "__"));
                if !lib_dir.exists() {
                    if which::which("git").is_err() { bail!("git nie jest zainstalowany"); }
                    std::fs::create_dir_all(lib_dir.parent().unwrap_or(Path::new("/tmp")))?;
                    let url = format!("https://github.com/{}.git", lib);
                    let status = std::process::Command::new("git").args(["clone","--depth=1",&url,lib_dir.to_str().unwrap_or("/tmp/hl_lib")]).status()?;
                    if !status.success() { bail!("Nie mozna pobrac: {}", lib); }
                }
                return load_from_dir(&lib_dir, detail, env, lib);
            }
            bail!("Nieznana biblioteka: '{}'", lib)
        }
    }
}

fn resolve_std(lib: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    match lib {
        "net"    => load_std_net(detail, env),
        "fs"     => load_std_fs(detail, env),
        "sys"    => load_std_sys(detail, env),
        "str"    => load_std_str(detail, env),
        "crypto" => load_std_crypto(detail, env),
        "proc"   => load_std_proc(detail, env),
        other    => bail!("Nieznana biblioteka std: 'std/{}'", other),
    }
}

fn resolve_community(path: &str, version: Option<&str>, env: &mut Env) -> Result<()> {
    let lib_dir = virus_base_dir().join("community").join(path.replace('/', "__"));
    if !lib_dir.exists() {
        if which::which("git").is_err() { bail!("git nie jest zainstalowany"); }
        std::fs::create_dir_all(&lib_dir)?;
        let url = if path.starts_with("github.com") || path.starts_with("gitlab.com") {
            format!("https://{}", path)
        } else { format!("https://github.com/{}", path) };
        let mut cmd = std::process::Command::new("git");
        cmd.args(["clone","--depth=1"]);
        if let Some(v) = version { cmd.args(["--branch", v]); }
        cmd.args([&url, lib_dir.to_str().unwrap_or("/tmp/hl_lib")]);
        if !cmd.status()?.success() { bail!("Nie mozna pobrac community: {}", path); }
    }
    load_from_dir(&lib_dir, None, env, path)
}

fn resolve_virus(name: &str, version: Option<&str>, env: &mut Env) -> Result<()> {
    let base = virus_base_dir();
    let so_name = if let Some(v) = version { format!("{}-{}.so", name, v) } else { format!("{}.so", name) };
    let so_path  = base.join(&so_name);
    let hl_path  = base.join(format!("{}.hl", name));
    let env_path = base.join(format!("{}.hlvars", name));
    if !so_path.exists() && !hl_path.exists() && !env_path.exists() {
        bail!("Biblioteka virus '{}' nie jest zainstalowana. Zainstaluj: hpm install virus/{}", name, name);
    }
    if hl_path.exists() {
        let src = std::fs::read_to_string(&hl_path)?;
        let nodes = hl_parser::parse_source(&src)?;
        crate::executor::exec_nodes(&nodes, env)?;
    }
    if so_path.exists() {
        let prefix = name.to_uppercase().replace('-', "_");
        env.set_var(&format!("VIRUS_{}_LOADED", prefix), Value::Bool(true));
        env.set_var(&format!("VIRUS_{}_PATH", prefix), Value::String(so_path.display().to_string()));
    }
    Ok(())
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
    info!("Laduje '{}' z {:?}", name, main_file);
    let src = std::fs::read_to_string(&main_file)?;
    let nodes = hl_parser::parse_source(&src)?;
    crate::executor::exec_nodes(&nodes, env)?;
    Ok(())
}

pub fn virus_base_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join(".hackeros").join("hacker-lang")
}

fn hl_libs_dir_legacy() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join(".hl").join("libs")
}

pub fn hl_cache_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join(".hl").join("cache")
}

// ── Std loaders ───────────────────────────────────────────────────────────────

fn load_std_net(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("NET_LOCALHOST", Value::String("127.0.0.1".into()));
    env.set_var("NET_BROADCAST", Value::String("255.255.255.255".into()));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Zaladowano std/net");
    Ok(())
}

fn load_std_fs(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    let home = dirs::home_dir().map(|p| p.display().to_string()).unwrap_or_default();
    env.set_var("FS_HOME",    Value::String(home));
    env.set_var("FS_TMP",     Value::String("/tmp".into()));
    env.set_var("FS_ETC",     Value::String("/etc".into()));
    env.set_var("FS_VAR_LOG", Value::String("/var/log".into()));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Zaladowano std/fs");
    Ok(())
}

fn load_std_sys(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("SYS_ARCH", Value::String(std::env::consts::ARCH.into()));
    env.set_var("SYS_HOSTNAME", Value::String(std::fs::read_to_string("/etc/hostname").unwrap_or_default().trim().into()));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Zaladowano std/sys");
    Ok(())
}

fn load_std_str(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("STR_NEWLINE", Value::String("\n".into()));
    env.set_var("STR_TAB",     Value::String("\t".into()));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Zaladowano std/str");
    Ok(())
}

fn load_std_crypto(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("CRYPTO_SHA256_CMD", Value::String("sha256sum".into()));
    env.set_var("CRYPTO_MD5_CMD",    Value::String("md5sum".into()));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Zaladowano std/crypto");
    Ok(())
}

fn load_std_proc(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("PROC_SELF_PID", Value::Number(std::process::id() as f64));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Zaladowano std/proc");
    Ok(())
}

// ── CLI helpers ───────────────────────────────────────────────────────────────

pub fn cmd_lib_list() {
    use colored::Colorize;
    println!("{}", "=== Biblioteki standardowe (std) ===".bright_cyan().bold());
    for (name, desc) in [
        ("std/net","Siec: IP, gateway, iface, porty"),("std/fs","System plikow"),
        ("std/sys","OS, kernel, CPU, RAM"),("std/str","Stale stringowe"),
        ("std/crypto","sha256, md5, gpg"),("std/proc","PID, PPID"),
    ] {
        println!("  {} {}", format!("# <{}>", name).bright_green(), desc.bright_black());
    }
}

pub fn cmd_lib_install(repo: &str) {
    use colored::Colorize;
    let lib_dir = hl_libs_dir_legacy().join("github").join(repo.replace('/', "__"));
    if lib_dir.exists() { println!("{} '{}' juz zainstalowana.", "✓".green(), repo); return; }
    let url = format!("https://github.com/{}.git", repo);
    let status = std::process::Command::new("git").args(["clone","--depth=1",&url,lib_dir.to_str().unwrap()]).status();
    match status { Ok(s) if s.success() => println!("{} Zainstalowano: {}", "✓".green(), repo), _ => eprintln!("{} Blad instalacji: {}", "✗".red(), repo) }
}

pub fn cmd_lib_remove(name: &str) {
    use colored::Colorize;
    let p = hl_libs_dir_legacy().join("github").join(name.replace('/', "__"));
    if p.exists() { std::fs::remove_dir_all(&p).unwrap(); println!("{} Usunieto: {}", "✓".green(), name); }
    else { eprintln!("{} Nie znaleziono: {}", "✗".red(), name); }
}

pub fn cmd_clean_cache() {
    use colored::Colorize;
    let cache = hl_cache_dir();
    if cache.exists() { std::fs::remove_dir_all(&cache).unwrap_or(()); println!("{} Cache wyczyszczony.", "✓".green()); }
    else { println!("{}", "Cache jest pusty.".bright_black()); }
}
