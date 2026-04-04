use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use tracing::info;
use crate::env::{Env, Value};

/// Resolver bibliotek — ładuje zmienne i funkcje do środowiska
pub fn resolve_import(lib: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let lib = lib.trim();

    // GitHub repo: "owner/repo" (zawiera slash, nie ma std/ prefix z wyjątkiem std/)
    if is_github_lib(lib) {
        return resolve_github(lib, detail, env);
    }

    // Biblioteki wbudowane
    match lib {
        "std/net"    => load_std_net(detail, env),
        "std/fs"     => load_std_fs(detail, env),
        "std/sys"    => load_std_sys(detail, env),
        "std/str"    => load_std_str(detail, env),
        "std/crypto" => load_std_crypto(detail, env),
        "std/proc"   => load_std_proc(detail, env),
        _ => {
            // Próba załadowania z lokalnego katalogu ~/.hl/libs/
            load_local(lib, detail, env)
        }
    }
}

fn is_github_lib(lib: &str) -> bool {
    // Wygląda jak "owner/repo" — jedno ukośnik, żaden segment nie jest "std"
    let parts: Vec<&str> = lib.splitn(2, '/').collect();
    parts.len() == 2 && parts[0] != "std" && !parts[0].is_empty() && !parts[1].is_empty()
}

/// Ładuje bibliotekę z GitHuba (owner/repo)
fn resolve_github(repo: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let lib_dir = hl_libs_dir().join("github").join(repo.replace('/', "__"));

    if !lib_dir.exists() {
        eprintln!("\x1b[36m[hl lib]\x1b[0m Pobieram bibliotekę z GitHub: {}", repo);

        // Sprawdź czy git jest dostępny
        if which::which("git").is_err() {
            bail!("git nie jest zainstalowany — wymagany do pobierania bibliotek GitHub");
        }

        std::fs::create_dir_all(lib_dir.parent().unwrap_or(Path::new("/tmp")))?;
        let url = format!("https://github.com/{}.git", repo);

        let status = std::process::Command::new("git")
        .args(["clone", "--depth=1", &url, lib_dir.to_str().unwrap_or("/tmp/hl_lib")])
        .status()?;

        if !status.success() {
            bail!("Nie można pobrać biblioteki: {}", repo);
        }

        eprintln!("\x1b[32m[hl lib]\x1b[0m Pobrano: {}", repo);
    }

    load_from_dir(&lib_dir, detail, env, repo)
}

/// Ładuje bibliotekę z lokalnego katalogu ~/.hl/libs/<name>
fn load_local(name: &str, detail: Option<&str>, env: &mut Env) -> Result<()> {
    let lib_dir = hl_libs_dir().join("local").join(name);
    if !lib_dir.exists() {
        bail!("Biblioteka '{}' nie znaleziona. Sprawdź ~/.hl/libs/ lub użyj owner/repo dla GitHub", name);
    }
    load_from_dir(&lib_dir, detail, env, name)
}

/// Ładuje .hl pliki z katalogu biblioteki
fn load_from_dir(dir: &Path, detail: Option<&str>, env: &mut Env, name: &str) -> Result<()> {
    let main_file = if let Some(d) = detail {
        // Szukaj konkretnego pliku: detail.hl lub detail/mod.hl
        let f = dir.join(format!("{}.hl", d));
        if f.exists() { f } else { dir.join(d).join("mod.hl") }
    } else {
        // Główny plik biblioteki
        let candidates = ["lib.hl", "mod.hl", "main.hl"];
        candidates.iter()
        .map(|c| dir.join(c))
        .find(|p| p.exists())
        .unwrap_or_else(|| dir.join("lib.hl"))
    };

    if !main_file.exists() {
        bail!("Brak pliku wejściowego dla biblioteki '{}' w {:?}", name, dir);
    }

    info!("Ładuję bibliotekę '{}' z {:?}", name, main_file);
    let source = std::fs::read_to_string(&main_file)?;
    let nodes = crate::parser::parse_source(&source)?;
    crate::executor::exec_nodes(&nodes, env)?;
    Ok(())
}

// ─── Biblioteki wbudowane ────────────────────────────────────────────────────

/// std/net — zmienne i helpery sieciowe
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
            eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/net <- dns");
        }
        Some("ports") => {
            inject_well_known_ports(env);
            eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/net <- ports");
        }
        Some(other) => bail!("Nieznany moduł std/net: '{}'", other),
    }
    Ok(())
}

fn inject_net_funcs(env: &mut Env) {
    use crate::ast::{Node, CommandMode};
    // Wstrzyknij funkcję `net_ping` jako wbudowaną
    let ping_body = vec![
        Node::Command {
            raw: "ping -c 1 -W 2 @_ping_target".into(),
            mode: CommandMode::WithVars,
            interpolate: true,
        }
    ];
    env.define_function("net_ping".into(), ping_body);
}

fn inject_well_known_ports(env: &mut Env) {
    let ports = [
        ("PORT_SSH", "22"), ("PORT_HTTP", "80"), ("PORT_HTTPS", "443"),
        ("PORT_FTP", "21"), ("PORT_SMTP", "25"), ("PORT_DNS", "53"),
        ("PORT_RDP", "3389"), ("PORT_MYSQL", "3306"), ("PORT_PSQL", "5432"),
    ];
    for (k, v) in ports {
        env.set_var(k, Value::String(v.into()));
    }
}

/// std/fs — stałe i helpery systemu plików
fn load_std_fs(detail: Option<&str>, env: &mut Env) -> Result<()> {
    let home = dirs::home_dir().map(|p: std::path::PathBuf| p.display().to_string()).unwrap_or_default();
    env.set_var("FS_HOME",    Value::String(home));
    env.set_var("FS_TMP",     Value::String("/tmp".into()));
    env.set_var("FS_ETC",     Value::String("/etc".into()));
    env.set_var("FS_VAR_LOG", Value::String("/var/log".into()));
    env.set_var("FS_PROC",    Value::String("/proc".into()));

    if detail.is_none() || detail == Some("all") {
        eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/fs");
    }
    Ok(())
}

/// std/sys — informacje o systemie
fn load_std_sys(detail: Option<&str>, env: &mut Env) -> Result<()> {
    // Czytaj /etc/os-release
    let os_name = read_os_release("NAME").unwrap_or_else(|| "HackerOS".into());
    let os_ver  = read_os_release("VERSION_ID").unwrap_or_else(|| "unknown".into());

    env.set_var("SYS_OS",      Value::String(os_name));
    env.set_var("SYS_VERSION", Value::String(os_ver));
    env.set_var("SYS_ARCH",    Value::String(std::env::consts::ARCH.into()));
    env.set_var("SYS_KERNEL",  Value::String(read_file("/proc/version")
    .unwrap_or_default().split_whitespace().nth(2).unwrap_or("?").into()));
    env.set_var("SYS_HOSTNAME",Value::String(read_file("/etc/hostname")
    .unwrap_or_default().trim().into()));
    env.set_var("SYS_UPTIME",  Value::String(read_uptime()));
    env.set_var("SYS_CPU",     Value::Number(num_cpus()));
    env.set_var("SYS_MEMTOTAL",Value::String(read_meminfo("MemTotal")));

    if detail.is_none() || detail == Some("all") {
        eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/sys");
    }
    Ok(())
}

/// std/str — pomocne stałe stringowe
fn load_std_str(detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("STR_NEWLINE", Value::String("\n".into()));
    env.set_var("STR_TAB",     Value::String("\t".into()));
    env.set_var("STR_EMPTY",   Value::String("".into()));
    if detail.is_none() || detail == Some("all") {
        eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/str");
    }
    Ok(())
}

/// std/crypto — stałe kryptograficzne
fn load_std_crypto(detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("CRYPTO_SHA256_CMD", Value::String("sha256sum".into()));
    env.set_var("CRYPTO_MD5_CMD",    Value::String("md5sum".into()));
    env.set_var("CRYPTO_GPG_CMD",    Value::String("gpg".into()));
    if detail.is_none() || detail == Some("all") {
        eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/crypto");
    }
    Ok(())
}

/// std/proc — zarządzanie procesami
fn load_std_proc(detail: Option<&str>, env: &mut Env) -> Result<()> {
    env.set_var("PROC_SELF_PID", Value::Number(std::process::id() as f64));
    env.set_var("PROC_PPID",     Value::String(
        read_file("/proc/self/status")
        .and_then(|s| s.lines()
        .find(|l| l.starts_with("PPid:"))
        .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string()))
        .unwrap_or_else(|| "?".into())
    ));
    if detail.is_none() || detail == Some("all") {
        eprintln!("\x1b[36m[hl lib]\x1b[0m Załadowano std/proc");
    }
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hl_libs_dir() -> PathBuf {
    dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hl")
    .join("libs")
}

pub fn hl_cache_dir() -> PathBuf {
    dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hl")
    .join("cache")
}

fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn read_os_release(key: &str) -> Option<String> {
    let content = read_file("/etc/os-release")?;
    for line in content.lines() {
        if line.starts_with(key) {
            let val = line.splitn(2, '=').nth(1)?.trim_matches('"').to_string();
            return Some(val);
        }
    }
    None
}

fn read_uptime() -> String {
    read_file("/proc/uptime")
    .and_then(|s| s.split_whitespace().next().map(|s| s.to_string()))
    .map(|secs| {
        let s: f64 = secs.parse().unwrap_or(0.0);
        let h = (s / 3600.0) as u64;
        let m = ((s % 3600.0) / 60.0) as u64;
        format!("{}h {}m", h, m)
    })
    .unwrap_or_else(|| "?".into())
}

fn read_meminfo(key: &str) -> String {
    read_file("/proc/meminfo")
    .and_then(|s| {
        s.lines()
        .find(|l| l.starts_with(key))
        .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string())
    })
    .map(|kb| {
        let n: u64 = kb.parse().unwrap_or(0);
        format!("{} MB", n / 1024)
    })
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
        s.lines().skip(1).find(|l| l.split_whitespace().nth(1) == Some("00000000"))
        .and_then(|l| l.split_whitespace().nth(2).map(|h| {
            // Parse hex gateway
            let n = u32::from_str_radix(h, 16).unwrap_or(0).to_le();
            format!("{}.{}.{}.{}", n & 0xff, (n>>8)&0xff, (n>>16)&0xff, (n>>24)&0xff)
        }))
    })
    .unwrap_or_else(|| "unknown".into())
}

fn detect_iface() -> String {
    read_file("/proc/net/route")
    .and_then(|s| {
        s.lines().skip(1).find(|l| l.split_whitespace().nth(1) == Some("00000000"))
        .and_then(|l| l.split_whitespace().next().map(|s| s.to_string()))
    })
    .unwrap_or_else(|| "eth0".into())
}

fn detect_local_ip() -> String {
    // Prosta heurystyka — weź IP z domyślnego interfejsu
    std::process::Command::new("hostname")
    .arg("-I")
    .output()
    .ok()
    .and_then(|o| String::from_utf8(o.stdout).ok())
    .map(|s| s.split_whitespace().next().unwrap_or("127.0.0.1").to_string())
    .unwrap_or_else(|| "127.0.0.1".into())
}

// ─── CLI: hl lib ─────────────────────────────────────────────────────────────

pub fn cmd_lib_list() {
    use colored::Colorize;
    println!("{}", "=== Biblioteki wbudowane (std) ===".bright_cyan().bold());
    let builtin = [
        ("std/net",    "Narzędzia sieciowe: IP, gateway, iface, porty"),
        ("std/fs",     "System plików: ścieżki home, tmp, etc, log"),
        ("std/sys",    "System: OS, kernel, CPU, RAM, hostname, uptime"),
        ("std/str",    "Stałe stringowe: newline, tab, empty"),
        ("std/crypto", "Kryptografia: sha256sum, md5sum, gpg"),
        ("std/proc",   "Procesy: PID, PPID"),
    ];
    for (name, desc) in builtin {
        println!("  {} {}", format!("# {}", name).bright_green(), desc.bright_black());
    }

    println!("\n{}", "=== Zainstalowane biblioteki ===".bright_cyan().bold());
    let libs_dir = hl_libs_dir();
    if !libs_dir.exists() {
        println!("  {}", "(brak zainstalowanych bibliotek)".bright_black());
        return;
    }
    for subdir in ["github", "local"] {
        let d = libs_dir.join(subdir);
        if let Ok(entries) = std::fs::read_dir(&d) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().replace("__", "/");
                println!("  {} {}", format!("# {}", name).yellow(), format!("[{}]", subdir).bright_black());
            }
        }
    }
}

pub fn cmd_lib_install(repo: &str) {
    use colored::Colorize;
    let lib_dir = hl_libs_dir().join("github").join(repo.replace('/', "__"));
    if lib_dir.exists() {
        println!("{} '{}' już zainstalowana.", "✓".green(), repo);
        return;
    }
    eprintln!("{} Instaluję '{}'...", "[hl lib]".bright_cyan(), repo);
    std::fs::create_dir_all(lib_dir.parent().unwrap()).unwrap();
    let url = format!("https://github.com/{}.git", repo);
    let status = std::process::Command::new("git")
    .args(["clone", "--depth=1", &url, lib_dir.to_str().unwrap()])
    .status();
    match status {
        Ok(s) if s.success() => println!("{} Zainstalowano: {}", "✓".green(), repo),
        _ => eprintln!("{} Błąd instalacji: {}", "✗".red(), repo),
    }
}

pub fn cmd_lib_remove(name: &str) {
    use colored::Colorize;
    let p = hl_libs_dir().join("github").join(name.replace('/', "__"));
    if p.exists() {
        std::fs::remove_dir_all(&p).unwrap();
        println!("{} Usunięto: {}", "✓".green(), name);
    } else {
        eprintln!("{} Nie znaleziono biblioteki: {}", "✗".red(), name);
    }
}

pub fn cmd_clean_cache() {
    use colored::Colorize;
    let cache = hl_cache_dir();
    if cache.exists() {
        std::fs::remove_dir_all(&cache).unwrap_or(());
        println!("{} Cache wyczyszczony: {:?}", "✓".green(), cache);
    } else {
        println!("{}", "Cache jest już pusty.".bright_black());
    }
}
