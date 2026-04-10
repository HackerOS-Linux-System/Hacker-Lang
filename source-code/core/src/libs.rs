use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use tracing::info;
use crate::env::{Env, Value};

// ── Import spec (inlined) ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ImportSource {
    Std     { lib: String, detail: Option<String>, version: Option<String> },
    Community { path: String, version: Option<String> },
    Virus   { name: String, version: Option<String> },
}

/// Parsuj string wewnątrz # <...>
pub fn parse_import_spec(raw: &str) -> Option<ImportSource> {
    let raw = raw.trim();

    // Rozdziel wersję — ostatni ':' jeśli po nim nie ma '/'
    let (body, version) = if let Some(pos) = raw.rfind(':') {
        let after = &raw[pos + 1..];
        if !after.contains('/') {
            (&raw[..pos], Some(after.to_string()))
        } else {
            (raw, None)
        }
    } else {
        (raw, None)
    };

    let slash = body.find('/')?;
    let namespace = &body[..slash];
    let rest      = &body[slash + 1..];

    match namespace {
        "std" => {
            let (lib, detail) = if let Some(s) = rest.find('/') {
                (rest[..s].to_string(), Some(rest[s+1..].to_string()))
            } else {
                (rest.to_string(), None)
            };
            Some(ImportSource::Std { lib, detail, version })
        }
        "community" => Some(ImportSource::Community { path: rest.to_string(), version }),
        "virus"     => Some(ImportSource::Virus { name: rest.to_string(), version }),
        _ => None,
    }
}

// ── Główny entry point (nowa składnia) ────────────────────────────────────────

pub fn resolve_import_new(spec: &str, env: &mut Env) -> Result<()> {
    let src = parse_import_spec(spec)
    .ok_or_else(|| anyhow::anyhow!("Nieprawidłowy import: '# <{}>'", spec))?;

    match src {
        ImportSource::Std { lib, detail, .. }    => resolve_std(&lib, detail.as_deref(), env),
        ImportSource::Community { path, version } => resolve_community(&path, version.as_deref(), env),
        ImportSource::Virus { name, version }     => resolve_virus(&name, version.as_deref(), env),
    }
}

/// Compat: stara składnia # lib lub # lib <- detail
pub fn resolve_import(lib: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let lib = lib.trim();

    // Nowa składnia w starym wywołaniu: # <std/net>
    if lib.starts_with('<') && lib.ends_with('>') {
        return resolve_import_new(&lib[1..lib.len()-1], env);
    }

    if is_github_lib(lib) {
        return resolve_github_legacy(lib, detail, env);
    }

    match lib {
        "std/net"    => load_std_net(detail, env),
        "std/fs"     => load_std_fs(detail, env),
        "std/sys"    => load_std_sys(detail, env),
        "std/str"    => load_std_str(detail, env),
        "std/crypto" => load_std_crypto(detail, env),
        "std/proc"   => load_std_proc(detail, env),
        _ => load_local_legacy(lib, detail, env),
    }
}

// ── Std resolver ──────────────────────────────────────────────────────────────

fn resolve_std(lib: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    match lib {
        "net"    => load_std_net(detail, env),
        "fs"     => load_std_fs(detail, env),
        "sys"    => load_std_sys(detail, env),
        "str"    => load_std_str(detail, env),
        "crypto" => load_std_crypto(detail, env),
        "proc"   => load_std_proc(detail, env),
        other => bail!(
            "Nieznana biblioteka std: 'std/{}'. Dostępne: net, fs, sys, str, crypto, proc",
            other
        ),
    }
}

// ── Community resolver ────────────────────────────────────────────────────────

fn resolve_community(path: &str, version: Option<&str>, env: &mut Env) -> Result<()> {
    if path.ends_with(".hl") {
        return load_community_file(path, env);
    }
    if path.contains("github.com") || path.contains("gitlab.com") {
        return resolve_community_git(path, version, env);
    }
    if path.contains('/') {
        let full = format!("github.com/{}", path);
        return resolve_community_git(&full, version, env);
    }
    bail!("Nie można rozwiązać importu community: '{}'", path)
}

fn load_community_file(path: &str, env: &mut Env) -> Result<()> {
    let base = virus_base_dir().join("community");
    let file = base.join(path);

    let src_path = if file.exists() {
        file
    } else {
        let local = PathBuf::from(path);
        if local.exists() { local }
        else {
            bail!("Plik community nie znaleziony: '{}'. Umieść go w ~/.hackeros/hacker-lang/community/", path);
        }
    };

    let src = std::fs::read_to_string(&src_path)?;
    let nodes = crate::parser::parse_source(&src)?;
    crate::executor::exec_nodes(&nodes, env)?;
    eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano community/{}", path);
    Ok(())
}

fn resolve_community_git(url: &str, version: Option<&str>, env: &mut Env) -> Result<()> {
    let repo_key = url.replace('/', "__").replace('.', "_");
    let lib_dir  = community_libs_dir().join(&repo_key);

    if !lib_dir.exists() {
        eprintln!("\x1b[36m[hl lib]\x1b[0m Pobieranie community/{} ...", url);

        if which::which("git").is_err() {
            bail!("git nie jest zainstalowany — wymagany do pobierania bibliotek community");
        }

        std::fs::create_dir_all(&lib_dir)?;

        let clone_url = if url.starts_with("github.com") || url.starts_with("gitlab.com") {
            format!("https://{}", url)
        } else {
            url.to_string()
        };

        let mut cmd = std::process::Command::new("git");
        cmd.args(["clone", "--depth=1"]);
        if let Some(ver) = version {
            cmd.args(["--branch", ver]);
        }
        cmd.args([&clone_url, lib_dir.to_str().unwrap_or("/tmp/hl_lib")]);

        let status = cmd.status()?;
        if !status.success() {
            std::fs::remove_dir_all(&lib_dir).ok();
            bail!("Nie można pobrać biblioteki community: {}", url);
        }

        eprintln!("\x1b[32m[hl lib]\x1b[0m Pobrano: {}", url);
    }

    load_from_dir(&lib_dir, None, env, url)
}

// ── Virus resolver ────────────────────────────────────────────────────────────

fn resolve_virus(name: &str, version: Option<&str>, env: &mut Env) -> Result<()> {
    let base = virus_base_dir();
    std::fs::create_dir_all(&base)?;

    let so_name = if let Some(ver) = version {
        format!("{}-{}.so", name, ver)
    } else {
        format!("{}.so", name)
    };

    let so_path  = base.join(&so_name);
    let hl_path  = base.join(format!("{}.hl",     name));
    let env_path = base.join(format!("{}.hlvars", name));

    if !so_path.exists() && !hl_path.exists() && !env_path.exists() {
        eprintln!(
            "\x1b[33m[hl virus]\x1b[0m Biblioteka '{}' nie znaleziona w ~/.hackeros/hacker-lang/",
            name
        );
        eprintln!("  Zainstaluj: hpm install virus/{}", name);
        bail!("Biblioteka virus '{}' nie jest zainstalowana", name);
    }

    if env_path.exists() {
        load_hlvars(&env_path, env)?;
        eprintln!("\x1b[36m[hl virus]\x1b[0m Załadowano zmienne z {}", name);
    }

    if hl_path.exists() {
        let src = std::fs::read_to_string(&hl_path)?;
        let nodes = crate::parser::parse_source(&src)?;
        crate::executor::exec_nodes(&nodes, env)?;
        eprintln!("\x1b[36m[hl virus]\x1b[0m Załadowano wrapper {}", name);
    }

    if so_path.exists() {
        let prefix = name.to_uppercase().replace('-', "_");
        env.set_var(&format!("VIRUS_{}_LOADED",  prefix), Value::Bool(true));
        env.set_var(&format!("VIRUS_{}_PATH",    prefix), Value::String(so_path.display().to_string()));
        if let Some(ver) = version {
            env.set_var(&format!("VIRUS_{}_VERSION", prefix), Value::String(ver.to_string()));
        }
        eprintln!("\x1b[36m[hl virus]\x1b[0m Załadowano .so: {}", so_name);
    }

    Ok(())
}

fn load_hlvars(path: &Path, env: &mut Env) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(";;") { continue; }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            let val = line[eq+1..].trim().to_string();
            let value = if val == "true" {
                Value::Bool(true)
            } else if val == "false" {
                Value::Bool(false)
            } else if let Ok(n) = val.parse::<f64>() {
                Value::Number(n)
            } else {
                Value::String(val.trim_matches('"').to_string())
            };
            env.set_var(&key, value);
        }
    }
    Ok(())
}

// ── Ścieżki ───────────────────────────────────────────────────────────────────

pub fn virus_base_dir() -> PathBuf {
    dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hackeros")
    .join("hacker-lang")
}

fn community_libs_dir() -> PathBuf {
    virus_base_dir().join("community")
}

pub fn hl_cache_dir() -> PathBuf {
    dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hl")
    .join("cache")
}

// ── Legacy helpers ────────────────────────────────────────────────────────────

fn is_github_lib(lib: &str) -> bool {
    let parts: Vec<&str> = lib.splitn(2, '/').collect();
    parts.len() == 2 && parts[0] != "std" && !parts[0].is_empty() && !parts[1].is_empty()
}

fn resolve_github_legacy(repo: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let lib_dir = hl_libs_dir_legacy().join("github").join(repo.replace('/', "__"));
    if !lib_dir.exists() {
        eprintln!("\x1b[36m[hl lib]\x1b[0m Pobieram z GitHub: {}", repo);
        if which::which("git").is_err() {
            bail!("git nie jest zainstalowany");
        }
        std::fs::create_dir_all(lib_dir.parent().unwrap_or(Path::new("/tmp")))?;
        let url = format!("https://github.com/{}.git", repo);
        let status = std::process::Command::new("git")
        .args(["clone", "--depth=1", &url, lib_dir.to_str().unwrap_or("/tmp/hl_lib")])
        .status()?;
        if !status.success() {
            bail!("Nie można pobrać: {}", repo);
        }
        eprintln!("\x1b[32m[hl lib]\x1b[0m Pobrano: {}", repo);
    }
    load_from_dir(&lib_dir, detail, env, repo)
}

fn load_local_legacy(name: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let lib_dir = hl_libs_dir_legacy().join("local").join(name);
    if !lib_dir.exists() {
        bail!("Biblioteka '{}' nie znaleziona", name);
    }
    load_from_dir(&lib_dir, detail, env, name)
}

fn load_from_dir(dir: &Path, detail: Option<&str>, env: &mut Env, name: &str) -> Result<()> {
    let main_file = if let Some(d) = detail {
        let f = dir.join(format!("{}.hl", d));
        if f.exists() { f } else { dir.join(d).join("mod.hl") }
    } else {
        ["lib.hl", "mod.hl", "main.hl"]
        .iter().map(|c| dir.join(c)).find(|p| p.exists())
        .unwrap_or_else(|| dir.join("lib.hl"))
    };

    if !main_file.exists() {
        bail!("Brak pliku wejściowego dla '{}' w {:?}", name, dir);
    }

    info!("Ładuję '{}' z {:?}", name, main_file);
    let src = std::fs::read_to_string(&main_file)?;
    let nodes = crate::parser::parse_source(&src)?;
    crate::executor::exec_nodes(&nodes, env)?;
    Ok(())
}

fn hl_libs_dir_legacy() -> PathBuf {
    dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hl")
    .join("libs")
}

// ── Biblioteki std ────────────────────────────────────────────────────────────

fn load_std_net(detail: Option<&str>, env: &mut Env) -> Result<()> {
    match detail {
        None | Some("all") => {
            env.set_var("NET_LOCALHOST", Value::String("127.0.0.1".into()));
            env.set_var("NET_BROADCAST", Value::String("255.255.255.255".into()));
            env.set_var("NET_GATEWAY",   Value::String(detect_gateway()));
            env.set_var("NET_IFACE",     Value::String(detect_iface()));
            env.set_var("NET_MYIP",      Value::String(detect_local_ip()));
            inject_net_funcs(env);
            eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/net");
        }
        Some("dns") => {
            env.set_var("NET_DNS1", Value::String("8.8.8.8".into()));
            env.set_var("NET_DNS2", Value::String("1.1.1.1".into()));
        }
        Some("ports") => inject_well_known_ports(env),
        Some(other)   => bail!("Nieznany moduł std/net: '{}'", other),
    }
    Ok(())
}

fn inject_net_funcs(env: &mut Env) {
    use crate::ast::{Node, CommandMode};
    let ping_body = vec![Node::Command {
        raw: "ping -c 1 -W 2 @_ping_target".into(),
        mode: CommandMode::WithVars,
        interpolate: true,
    }];
    env.define_function("net_ping".into(), ping_body);
}

fn inject_well_known_ports(env: &mut Env) {
    for (k, v) in [
        ("PORT_SSH","22"),("PORT_HTTP","80"),("PORT_HTTPS","443"),
        ("PORT_FTP","21"),("PORT_SMTP","25"),("PORT_DNS","53"),
        ("PORT_RDP","3389"),("PORT_MYSQL","3306"),("PORT_PSQL","5432"),
    ] {
        env.set_var(k, Value::String(v.into()));
    }
}

fn load_std_fs(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    let home = dirs::home_dir().map(|p| p.display().to_string()).unwrap_or_default();
    env.set_var("FS_HOME",    Value::String(home));
    env.set_var("FS_TMP",     Value::String("/tmp".into()));
    env.set_var("FS_ETC",     Value::String("/etc".into()));
    env.set_var("FS_VAR_LOG", Value::String("/var/log".into()));
    env.set_var("FS_PROC",    Value::String("/proc".into()));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/fs");
    Ok(())
}

fn load_std_sys(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    let os_name = read_os_release("NAME").unwrap_or_else(|| "HackerOS".into());
    let os_ver  = read_os_release("VERSION_ID").unwrap_or_else(|| "unknown".into());
    env.set_var("SYS_OS",       Value::String(os_name));
    env.set_var("SYS_VERSION",  Value::String(os_ver));
    env.set_var("SYS_ARCH",     Value::String(std::env::consts::ARCH.into()));
    env.set_var("SYS_KERNEL",   Value::String(
        read_file("/proc/version").unwrap_or_default()
        .split_whitespace().nth(2).unwrap_or("?").into()
    ));
    env.set_var("SYS_HOSTNAME", Value::String(
        read_file("/etc/hostname").unwrap_or_default().trim().into()
    ));
    env.set_var("SYS_UPTIME",   Value::String(read_uptime()));
    env.set_var("SYS_CPU",      Value::Number(num_cpus()));
    env.set_var("SYS_MEMTOTAL", Value::String(read_meminfo("MemTotal")));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/sys");
    Ok(())
}

fn load_std_str(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("STR_NEWLINE", Value::String("\n".into()));
    env.set_var("STR_TAB",     Value::String("\t".into()));
    env.set_var("STR_EMPTY",   Value::String("".into()));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/str");
    Ok(())
}

fn load_std_crypto(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("CRYPTO_SHA256_CMD", Value::String("sha256sum".into()));
    env.set_var("CRYPTO_MD5_CMD",    Value::String("md5sum".into()));
    env.set_var("CRYPTO_GPG_CMD",    Value::String("gpg".into()));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/crypto");
    Ok(())
}

fn load_std_proc(_detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("PROC_SELF_PID", Value::Number(std::process::id() as f64));
    env.set_var("PROC_PPID", Value::String(
        read_file("/proc/self/status")
        .and_then(|s| s.lines()
        .find(|l| l.starts_with("PPid:"))
        .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string()))
        .unwrap_or_else(|| "?".into())
    ));
    eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/proc");
    Ok(())
}

// ── CLI helpers ───────────────────────────────────────────────────────────────

pub fn cmd_lib_list() {
    use colored::Colorize;
    println!("{}", "=== Biblioteki standardowe (std) ===".bright_cyan().bold());
    for (name, desc) in [
        ("std/net",    "Siec: IP, gateway, iface, porty"),
        ("std/fs",     "System plikow: home, tmp, etc"),
        ("std/sys",    "System: OS, kernel, CPU, RAM"),
        ("std/str",    "Stale stringowe"),
        ("std/crypto", "Kryptografia: sha256, md5, gpg"),
        ("std/proc",   "Procesy: PID, PPID"),
    ] {
        println!("  {} {}", format!("# <{}>", name).bright_green(), desc.bright_black());
    }
    println!();
    println!("{}", "=== Ekosystem Virus (.so) ===".bright_magenta().bold());
    let base = virus_base_dir();
    let mut found = false;
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".so") {
                println!("  {} {}", "◆".bright_magenta(), name.replace(".so", "").bright_white());
                found = true;
            }
        }
    }
    if !found { println!("  {}", "(brak zainstalowanych bibliotek virus)".bright_black()); }
}

pub fn cmd_lib_install(repo: &str) {
    use colored::Colorize;
    let lib_dir = hl_libs_dir_legacy().join("github").join(repo.replace('/', "__"));
    if lib_dir.exists() { println!("{} '{}' juz zainstalowana.", "✓".green(), repo); return; }
    eprintln!("{} Instaluje '{}'...", "[hl lib]".bright_cyan(), repo);
    std::fs::create_dir_all(lib_dir.parent().unwrap()).unwrap();
    let url = format!("https://github.com/{}.git", repo);
    let status = std::process::Command::new("git")
    .args(["clone", "--depth=1", &url, lib_dir.to_str().unwrap()])
    .status();
    match status {
        Ok(s) if s.success() => println!("{} Zainstalowano: {}", "✓".green(), repo),
        _ => eprintln!("{} Blad instalacji: {}", "✗".red(), repo),
    }
}

pub fn cmd_lib_remove(name: &str) {
    use colored::Colorize;
    let p = hl_libs_dir_legacy().join("github").join(name.replace('/', "__"));
    if p.exists() {
        std::fs::remove_dir_all(&p).unwrap();
        println!("{} Usunieto: {}", "✓".green(), name);
    } else {
        eprintln!("{} Nie znaleziono: {}", "✗".red(), name);
    }
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

// ── System helpers ────────────────────────────────────────────────────────────

fn read_file(path: &str) -> Option<String> { std::fs::read_to_string(path).ok() }

fn read_os_release(key: &str) -> Option<String> {
    let content = read_file("/etc/os-release")?;
    for line in content.lines() {
        if line.starts_with(key) {
            return Some(line.splitn(2, '=').nth(1)?.trim_matches('"').to_string());
        }
    }
    None
}

fn read_uptime() -> String {
    read_file("/proc/uptime")
    .and_then(|s| s.split_whitespace().next().map(|s| s.to_string()))
    .map(|secs| {
        let s: f64 = secs.parse().unwrap_or(0.0);
        format!("{}h {}m", (s / 3600.0) as u64, ((s % 3600.0) / 60.0) as u64)
    })
    .unwrap_or_else(|| "?".into())
}

fn read_meminfo(key: &str) -> String {
    read_file("/proc/meminfo")
    .and_then(|s| s.lines()
    .find(|l| l.starts_with(key))
    .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string()))
    .map(|kb| format!("{} MB", kb.parse::<u64>().unwrap_or(0) / 1024))
    .unwrap_or_else(|| "?".into())
}

fn num_cpus() -> f64 {
    read_file("/proc/cpuinfo")
    .map(|s| s.lines().filter(|l| l.starts_with("processor")).count() as f64)
    .unwrap_or(1.0)
}

fn detect_gateway() -> String {
    read_file("/proc/net/route")
    .and_then(|s| {
        s.lines().skip(1)
        .find(|l| l.split_whitespace().nth(1) == Some("00000000"))
        .and_then(|l| l.split_whitespace().nth(2).map(|h| {
            let n = u32::from_str_radix(h, 16).unwrap_or(0).to_le();
            format!("{}.{}.{}.{}", n&0xff, (n>>8)&0xff, (n>>16)&0xff, (n>>24)&0xff)
        }))
    })
    .unwrap_or_else(|| "unknown".into())
}

fn detect_iface() -> String {
    read_file("/proc/net/route")
    .and_then(|s| s.lines().skip(1)
    .find(|l| l.split_whitespace().nth(1) == Some("00000000"))
    .and_then(|l| l.split_whitespace().next().map(|s| s.to_string())))
    .unwrap_or_else(|| "eth0".into())
}

fn detect_local_ip() -> String {
    std::process::Command::new("hostname").arg("-I").output().ok()
    .and_then(|o| String::from_utf8(o.stdout).ok())
    .map(|s| s.split_whitespace().next().unwrap_or("127.0.0.1").to_string())
    .unwrap_or_else(|| "127.0.0.1".into())
}
