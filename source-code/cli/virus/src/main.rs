#![allow(dead_code)]
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use anyhow::{anyhow, Context, Result};
use dirs::home_dir;
use hex;
use lexopt::prelude::*;
use lexopt::Parser;
use owo_colors::{AnsiColors, OwoColorize, Style};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toml;
use ureq;
use std::ffi::{OsStr, OsString};
// Static links — virus can run .hl build scripts directly
// Assuming these are available; in practice, implement or import accordingly
// use hl_compiler::compile_command;
// use hl_runtime::run_command;
// For now, placeholders
fn compile_command(_src: String, _out: String, _verbose: bool) -> bool {
    // Placeholder
    true
}
fn run_command(_file: String, _verbose: bool) -> bool {
    // Placeholder
    true
}
const VERSION: &str = "0.1.0";
const HACKER_DIR: &str = ".hackeros/hacker-lang";
const LIBS_DIR: &str = "libs";
const MANIFEST_NAME: &str = "Virus.hk";
const LOCK_NAME: &str = "Virus.lock";
// --- UI / Display Functions ---
fn print_banner() {
    let line = "─".repeat(50);
    println!();
    println!(" {}", line.magenta().dimmed());
    println!(" {} {}", "VIRUS".bold().magenta(), format!("v{}", VERSION).cyan());
    println!(" {}", "Hacker Lang Package Manager".white().dimmed());
    println!(" {}", line.magenta().dimmed());
    println!();
}
fn print_help() {
    print_banner();
    let cmds = vec![
        ("init [name] [--lib]", "Create new project"),
        ("install", "Install all deps from Virus.hk"),
        ("install <pkg> [ver] [-r reg]", "Install a specific package"),
        ("remove <pkg>", "Remove a package"),
        ("update [pkg]", "Update packages"),
        ("list", "List installed packages"),
        ("search <query>", "Search registries"),
        ("info [pkg]", "Show package info or system info"),
        ("publish", "Instructions to publish"),
        ("clean", "Remove Virus.lock"),
        ("run <file.hl> [--verbose]", "Run a .hl script"),
        ("compile <file.hl> [-o out]", "Compile a .hl script"),
    ];
    println!(" {}", "AVAILABLE COMMANDS".yellow().bold());
    println!(" {}", "───────────────────".yellow().dimmed());
    for (cmd, desc) in cmds {
        println!(" {:<35} {}", cmd.green().bold(), desc.white().dimmed());
    }
    println!();
    let regs = vec![
        ("bytes", "https://raw.githubusercontent.com/Hacker-Lang/bytes-registry/main/index.hk"),
        ("virus", "https://raw.githubusercontent.com/Hacker-Lang/virus-registry/main/index.hk"),
    ];
    println!(" {}", "REGISTRIES".yellow().bold());
    println!(" {}", "──────────".yellow().dimmed());
    for (name, url) in regs {
        println!(" {:<12} {}", name.green().bold(), url.white().dimmed());
    }
    println!();
}
fn print_boxed(msg: &str, color: Option<AnsiColors>) {
    let lines: Vec<&str> = msg.lines().collect();
    let max_width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    let content_width = max_width;
    let total_inner_width = content_width + 4;
    let color = color.unwrap_or(AnsiColors::White);
    let style = Style::new().bold().color(color);
    let border_style = Style::new().dimmed().color(color);
    let top = format!("╔{}╗", "═".repeat(total_inner_width));
    let bottom = format!("╚{}╝", "═".repeat(total_inner_width));
    println!(" {}", top.style(border_style));
    for line in lines {
        let clean_line = line.trim();
        let padded = format!("║ {:<width$} ║", clean_line, width = content_width);
        println!(" {}", padded.style(style));
    }
    println!(" {}", bottom.style(border_style));
}
fn print_status(badge: &str, msg: &str, color: AnsiColors) {
    let style = Style::new().bold().color(color);
    let bracket_style = Style::new().dimmed().white();
    println!(" {}{}{} {}",
             "[".style(bracket_style),
             badge.style(style),
             "]".style(bracket_style),
             msg.white()
    );
}
fn print_success(msg: &str) {
    print_status(" OK ", msg, AnsiColors::Green);
}
fn print_error(msg: &str) {
    print_status("FAIL", msg, AnsiColors::Red);
}
fn print_info(msg: &str) {
    print_status("INFO", msg, AnsiColors::Cyan);
}
fn print_warning(msg: &str) {
    print_status("WARN", msg, AnsiColors::Yellow);
}
// System info similar to the first tool
fn system_info() {
    print_banner();
    let border_raw = "═".repeat(50);
    let sep_raw = "─".repeat(50);
    let c = Style::new().magenta().dimmed();
    println!(" ╔{}╗", border_raw.style(c));
    println!(" {} {:<48} {}", "║".style(c), "SYSTEM IDENTITY".green().bold(), "║".style(c));
    println!(" ╠{}╣", sep_raw.style(c));
    println!(" {} {:<48} {}", "║".style(c), "", "║".style(c));
    let print_row = |label: &str, val: &str| {
        let visible_len = label.len() + val.len();
        let padding = 48usize.saturating_sub(visible_len);
        let spaces = " ".repeat(padding);
        println!(" {} {}{}{} {}", "║".style(c), label.cyan().bold(), val.white(), spaces, "║".style(c));
    };
    print_row("NAME: ", "Virus - Hacker Lang Package Manager");
    print_row("VERSION: ", VERSION);
    print_row("AUTHOR: ", "HackerOS Team");
    println!(" {} {:<48} {}", "║".style(c), "", "║".style(c));
    println!(" {} {:<48} {}", "║".style(c), "DESCRIPTION:".yellow().bold(), "║".style(c));
    println!(" {} {:<48} {}", "║".style(c), "A package manager for Hacker Lang with support", "║".style(c));
    println!(" {} {:<48} {}", "║".style(c), "for installation, updates, searches, and builds.", "║".style(c));
    println!(" {} {:<48} {}", "║".style(c), "", "║".style(c));
    println!(" ╚{}╝", border_raw.style(c));
}
// ═══════════════════════════════════════════════════════════
// Manifest (Virus.hk) - Kept as TOML for simplicity, but styled similarly
// ═══════════════════════════════════════════════════════════
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Package {
    name: String,
    version: String,
    authors: Vec<String>,
    description: Option<String>,
    entry: Option<String>,
    license: Option<String>,
    repository: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Manifest {
    package: Package,
    dependencies: HashMap<String, DepSpec>,
    dev_deps: HashMap<String, DepSpec>,
    registry: Option<RegistryCfg>,
    build: Option<BuildCfg>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum DepSpec {
    Version(String),
    Full { version: String, registry: Option<String>, optional: Option<bool> },
}
impl DepSpec {
    fn version(&self) -> &str {
        match self {
            DepSpec::Version(v) => v,
            DepSpec::Full { version, .. } => version,
        }
    }
    fn registry(&self) -> Option<&str> {
        match self {
            DepSpec::Full { registry, .. } => registry.as_deref(),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryCfg {
    source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BuildCfg {
    script: String,
}
fn default_manifest(name: &str) -> Manifest {
    Manifest {
        package: Package {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            authors: vec![],
            entry: Some("src/main.hl".to_string()),
            ..Default::default()
        },
        ..Default::default()
    }
}
fn manifest_toml(m: &Manifest) -> String {
    toml::to_string_pretty(m).expect("Failed to serialize manifest")
}
fn read_manifest(dir: &Path) -> Result<Manifest> {
    let path = dir.join(MANIFEST_NAME);
    let txt = fs::read_to_string(&path).with_context(|| format!("Failed to read {:?}", path))?;
    toml::from_str(&txt).context("Failed to parse manifest")
}
fn write_manifest(dir: &Path, m: &Manifest) -> Result<()> {
    let path = dir.join(MANIFEST_NAME);
    let txt = manifest_toml(m);
    fs::write(&path, txt).with_context(|| format!("Failed to write {:?}", path))
}
// ═══════════════════════════════════════════════════════════
// Lockfile (Virus.lock)
// ═══════════════════════════════════════════════════════════
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Lockfile {
    packages: Vec<LockedPkg>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockedPkg {
    name: String,
    version: String,
    registry: String,
    checksum: String,
    so_path: Option<String>,
    hl_path: Option<String>,
}
fn read_lock(dir: &Path) -> Result<Lockfile> {
    let path = dir.join(LOCK_NAME);
    if !path.exists() {
        return Ok(Lockfile::default());
    }
    let txt = fs::read_to_string(&path).with_context(|| format!("Failed to read {:?}", path))?;
    toml::from_str(&txt).context("Failed to parse lockfile")
}
fn write_lock(dir: &Path, lock: &Lockfile) -> Result<()> {
    let path = dir.join(LOCK_NAME);
    let txt = toml::to_string_pretty(lock).context("Failed to serialize lockfile")?;
    fs::write(&path, txt).with_context(|| format!("Failed to write {:?}", path))
}
// ═══════════════════════════════════════════════════════════
// Registry
// ═══════════════════════════════════════════════════════════
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegEntry {
    version: String,
    description: Option<String>,
    so_download: Option<String>,
    hl_download: Option<String>,
    checksum: Option<String>,
}
const REGISTRIES: &[(&str, &str)] = &[
    ("bytes", "https://raw.githubusercontent.com/Hacker-Lang/bytes-registry/main/index.hk"),
    ("virus", "https://raw.githubusercontent.com/Hacker-Lang/virus-registry/main/index.hk"),
];
fn registry_url(name: &str) -> Option<&'static str> {
    REGISTRIES.iter().find(|(n, _)| *n == name).map(|(_, u)| *u)
}
fn parse_hk_index(txt: &str) -> HashMap<String, RegEntry> {
    // Keep custom parser, as hk_parser may not be directly usable for string
    let mut result: HashMap<String, RegEntry> = HashMap::new();
    let mut cur = String::new();
    let mut fields: HashMap<String, String> = HashMap::new();
    for line in txt.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('!') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if !cur.is_empty() {
                result.insert(cur.clone(), entry_from_fields(&fields));
                fields.clear();
            }
            cur = line[1..line.len() - 1].to_string();
        } else if line.starts_with("->") {
            if let Some(rest) = line.strip_prefix("->") {
                if let Some((k, v)) = rest.split_once("=>") {
                    fields.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
    }
    if !cur.is_empty() {
        result.insert(cur, entry_from_fields(&fields));
    }
    result
}
fn entry_from_fields(f: &HashMap<String, String>) -> RegEntry {
    RegEntry {
        version: f.get("version").cloned().unwrap_or_default(),
        description: f.get("description").cloned(),
        so_download: f.get("so-download").cloned(),
        hl_download: f.get(".hl-download").cloned(),
        checksum: f.get("checksum").cloned(),
    }
}
fn fetch_registry(name: &str) -> Result<HashMap<String, RegEntry>> {
    let url = registry_url(name).ok_or(anyhow!("Unknown registry: {}", name))?;
    let resp = ureq::get(url).call().context("HTTP request failed")?;
    let txt = resp.into_string().context("Failed to read response")?;
    Ok(parse_hk_index(&txt))
}
// ═══════════════════════════════════════════════════════════
// Installer
// ═══════════════════════════════════════════════════════════
fn install_dir() -> PathBuf {
    home_dir().unwrap().join(HACKER_DIR).join(LIBS_DIR)
}
fn pkg_dir(name: &str) -> PathBuf {
    install_dir().join(name)
}
fn download_bytes(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url).call().context("Download failed")?;
    let mut buf = Vec::new();
    let mut reader = resp.into_reader();
    reader.read_to_end(&mut buf).context("Failed to read download")?;
    Ok(buf)
}
fn checksum(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}
fn install_package(
    name: &str,
    version: &str,
    registry: &str,
    verbose: bool,
    force: bool,
) -> Result<LockedPkg> {
    let dir = pkg_dir(name);
    if dir.exists() && !force {
        if verbose {
            print_info(&format!("{} already installed", name));
        }
        return Ok(LockedPkg {
            name: name.to_string(),
                  version: version.to_string(),
                  registry: registry.to_string(),
                  checksum: String::new(),
                  so_path: None,
                  hl_path: None,
        });
    }
    print_info(&format!(
        "Installing {} {} from {}",
        name, version, registry
    ));
    let idx = fetch_registry(registry)?;
    let entry = idx
    .get(name)
    .ok_or(anyhow!(
        "Package '{}' not found in registry '{}'",
        name,
        registry
    ))?
    .clone();
    fs::create_dir_all(&dir).context("Failed to create package dir")?;
    let mut so_path = None;
    let mut hl_path = None;
    let mut combined = Vec::new();
    if let Some(url) = &entry.so_download {
        let data = download_bytes(url)?;
        combined.extend_from_slice(&data);
        let dest = dir.join(format!("lib{}.so", name));
        fs::write(&dest, &data).context("Failed to write .so")?;
        so_path = Some(dest.to_string_lossy().to_string());
        if verbose {
            print_info(&format!("Saved {:?}", dest));
        }
    }
    if let Some(url) = &entry.hl_download {
        let data = download_bytes(url)?;
        combined.extend_from_slice(&data);
        let dest = dir.join(format!("{}.hl", name));
        fs::write(&dest, &data).context("Failed to write .hl")?;
        hl_path = Some(dest.to_string_lossy().to_string());
        if verbose {
            print_info(&format!("Saved {:?}", dest));
        }
    }
    let cs = if let Some(expected) = &entry.checksum {
        let actual = checksum(&combined);
        if expected != &actual {
            return Err(anyhow!(
                "Checksum mismatch for {} (expected {}, got {})",
                               name,
                               expected,
                               actual
            ));
        }
        actual
    } else {
        checksum(&combined)
    };
    Ok(LockedPkg {
        name: name.to_string(),
       version: entry.version,
       registry: registry.to_string(),
       checksum: cs,
       so_path,
       hl_path,
    })
}
fn remove_package(name: &str) -> Result<()> {
    let dir = pkg_dir(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).context("Failed to remove package dir")?;
        print_info(&format!("Removed {}", name));
    } else {
        print_warning(&format!("{} not installed", name));
    }
    Ok(())
}
// ═══════════════════════════════════════════════════════════
// Build script runner
// ═══════════════════════════════════════════════════════════
fn run_build_script(script: &str, verbose: bool) {
    print_info(&format!("Running build script: {}", script));
    let ok = run_command(script.to_string(), verbose);
    if !ok {
        print_error("Build script failed");
    }
}
// ═══════════════════════════════════════════════════════════
// Commands
// ═══════════════════════════════════════════════════════════
fn cmd_init(name: Option<String>, lib: bool) -> Result<ExitCode> {
    let name = name.unwrap_or_else(|| {
        env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "my-project".to_string())
    });
    let dir = Path::new(".");
    let manifest_path = dir.join(MANIFEST_NAME);
    if manifest_path.exists() {
        print_error(&format!("{} already exists", MANIFEST_NAME));
        return Ok(ExitCode::FAILURE);
    }
    let mut manifest = default_manifest(&name);
    let src = dir.join("src");
    fs::create_dir_all(&src).context("Failed to create src dir")?;
    let entry = if lib { "src/lib.hl" } else { "src/main.hl" };
    let entry_path = dir.join(&entry);
    if !entry_path.exists() {
        let content = if lib {
            "! my-lib entry point\n:greet(name:string) [\n echo Hello @name\n]\n"
        } else {
            "! main entry point\necho Hello from Hacker Lang!\n"
        };
        fs::write(&entry_path, content).context("Failed to write entry file")?;
    }
    manifest.package.entry = Some(entry.to_string());
    write_manifest(dir, &manifest)?;
    print_success(&format!(
        "Initialised {} ({})",
                           name,
                           if lib { "library" } else { "binary" }
    ));
    print_info(&format!("{} created", MANIFEST_NAME));
    print_info(&format!("{} created", entry));
    Ok(ExitCode::SUCCESS)
}
fn cmd_install(
    pkg: Option<String>,
    ver: Option<String>,
    registry: Option<String>,
    dev: bool,
    force: bool,
        verbose: bool,
) -> Result<ExitCode> {
    let dir = Path::new(".");
    if let Some(name) = pkg {
        let reg = registry.as_deref().unwrap_or("bytes");
        let version = ver.as_deref().unwrap_or("latest").to_string();
        let locked = install_package(&name, &version, reg, verbose, force)?;
        let mut lock = read_lock(dir)?;
        lock.packages.retain(|p| p.name != name);
        lock.packages.push(locked);
        write_lock(dir, &lock)?;
        let mut manifest = read_manifest(dir)?;
        let spec = DepSpec::Full {
            version,
            registry,
            optional: None,
        };
        if dev {
            manifest.dev_deps.insert(name.clone(), spec);
        } else {
            manifest.dependencies.insert(name.clone(), spec);
        }
        write_manifest(dir, &manifest)?;
    } else {
        let manifest = read_manifest(dir)?;
        let mut lock = read_lock(dir)?;
        let reg_source = manifest
        .registry
        .as_ref()
        .map(|r| r.source.as_str())
        .unwrap_or("bytes");
        let mut all = manifest.dependencies;
        if dev {
            all.extend(manifest.dev_deps);
        }
        for (name, spec) in all {
            let reg = spec.registry().unwrap_or(reg_source);
            if let Ok(locked) = install_package(&name, spec.version(), reg, verbose, force) {
                lock.packages.retain(|p| p.name != name);
                lock.packages.push(locked);
            }
        }
        write_lock(dir, &lock)?;
        if let Some(build) = manifest.build {
            run_build_script(&build.script, verbose);
        }
    }
    print_success("Done.");
    Ok(ExitCode::SUCCESS)
}
fn cmd_remove(name: String) -> Result<ExitCode> {
    let dir = Path::new(".");
    remove_package(&name)?;
    let mut lock = read_lock(dir)?;
    lock.packages.retain(|p| p.name != name);
    write_lock(dir, &lock)?;
    if let Ok(mut manifest) = read_manifest(dir) {
        manifest.dependencies.remove(&name);
        manifest.dev_deps.remove(&name);
        write_manifest(dir, &manifest)?;
    }
    Ok(ExitCode::SUCCESS)
}
fn cmd_update(pkg: Option<String>, verbose: bool) -> Result<ExitCode> {
    let dir = Path::new(".");
    let manifest = read_manifest(dir)?;
    let reg_src = manifest
    .registry
    .as_ref()
    .map(|r| r.source.as_str())
    .unwrap_or("bytes");
    let mut lock = read_lock(dir)?;
    let to_update: Vec<(String, DepSpec)> = if let Some(name) = pkg {
        manifest
        .dependencies
        .iter()
        .chain(manifest.dev_deps.iter())
        .filter(|(n, _)| **n == name)
        .map(|(n, s)| (n.clone(), s.clone()))
        .collect()
    } else {
        manifest
        .dependencies
        .iter()
        .chain(manifest.dev_deps.iter())
        .map(|(n, s)| (n.clone(), s.clone()))
        .collect()
    };
    for (name, spec) in to_update {
        let reg = spec.registry().unwrap_or(reg_src);
        if let Ok(locked) = install_package(&name, spec.version(), reg, verbose, true) {
            lock.packages.retain(|p| p.name != name);
            lock.packages.push(locked);
        }
    }
    write_lock(dir, &lock)?;
    print_success("Update complete.");
    Ok(ExitCode::SUCCESS)
}
fn cmd_list() -> Result<ExitCode> {
    let dir = Path::new(".");
    let lock = read_lock(dir)?;
    if lock.packages.is_empty() {
        print_warning("No packages installed.");
        return Ok(ExitCode::SUCCESS);
    }
    print_info("Installed packages:");
    println!("{}", "─".repeat(60).magenta().dimmed());
    for p in &lock.packages {
        let so = p.so_path.as_deref().map(|_| " [.so]").unwrap_or("");
        let hl = p.hl_path.as_deref().map(|_| " [.hl]").unwrap_or("");
        println!(
            " {} {} ({}){}{}",
                 p.name.cyan(),
                 p.version.green(),
                 p.registry.white().dimmed(),
                 so,
                 hl
        );
    }
    Ok(ExitCode::SUCCESS)
}
fn cmd_search(query: String) -> Result<ExitCode> {
    print_info(&format!("Searching for '{}'", query));
    let q = query.to_lowercase();
    let mut found = false;
    for (reg_name, _) in REGISTRIES {
        if let Ok(idx) = fetch_registry(reg_name) {
            for (name, entry) in idx {
                let desc = entry.description.as_deref().unwrap_or("");
                if name.to_lowercase().contains(&q) || desc.to_lowercase().contains(&q) {
                    println!(
                        " {} {} {} — {}",
                        format!("[{}]", reg_name).white().dimmed(),
                            name.cyan(),
                             entry.version.green(),
                             desc,
                    );
                    found = true;
                }
            }
        }
    }
    if !found {
        print_warning("No results.");
    }
    Ok(ExitCode::SUCCESS)
}
fn cmd_pkg_info(name: String) -> Result<ExitCode> {
    let mut found = false;
    for (reg_name, _) in REGISTRIES {
        if let Ok(idx) = fetch_registry(reg_name) {
            if let Some(e) = idx.get(&name) {
                println!(
                    " {} {} {} (from {})",
                         "INFO".cyan().bold(),
                         name.cyan().bold(),
                         e.version.green(),
                         reg_name
                );
                if let Some(d) = &e.description {
                    println!(" {}", d);
                }
                if let Some(u) = &e.so_download {
                    println!(" .so {}", u.white().dimmed());
                }
                if let Some(u) = &e.hl_download {
                    println!(" .hl {}", u.white().dimmed());
                }
                found = true;
                break;
            }
        }
    }
    if !found {
        print_warning(&format!("Package '{}' not found.", name));
    }
    Ok(ExitCode::SUCCESS)
}
fn cmd_run(file: String, verbose: bool) -> Result<ExitCode> {
    print_info(&format!("Running {}", file));
    let ok = run_command(file, verbose);
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
fn cmd_compile(file: String, output: Option<String>, verbose: bool) -> Result<ExitCode> {
    let out = output.unwrap_or_else(|| {
        file.rfind('.')
        .map(|i| file[..i].to_string())
        .unwrap_or_else(|| file.clone())
    });
    print_info(&format!("Compiling {} → {}", file, out));
    let ok = compile_command(file, out, verbose);
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
fn cmd_clean() -> Result<ExitCode> {
    let dir = Path::new(".");
    let lock = dir.join(LOCK_NAME);
    if lock.exists() {
        fs::remove_file(&lock).context("Failed to remove lockfile")?;
        print_info(&format!("Removed {}", LOCK_NAME));
    }
    Ok(ExitCode::SUCCESS)
}
fn cmd_publish() -> Result<ExitCode> {
    print_info("Publishing to a virus registry:");
    let msg = r#"1. Fork the registry you want to publish to:
    bytes: https://github.com/Hacker-Lang/bytes-registry
    virus: https://github.com/Hacker-Lang/virus-registry
    2. Add an entry to index.hk:
    [my-package]
    -> version => 0.1.0
    -> description => My package description
    -> so-download => https://github.com/you/repo/releases/download/v0.1.0/lib.so
    -> .hl-download => https://github.com/you/repo/releases/download/v0.1.0/lib.hl
    -> checksum => <sha256-of-both-files-concatenated>
    3. Open a Pull Request."#;
    print_boxed(msg, Some(AnsiColors::Cyan));
    Ok(ExitCode::SUCCESS)
}
// ═══════════════════════════════════════════════════════════
// CLI parser
// ═══════════════════════════════════════════════════════════
fn main() -> ExitCode {
    if let Err(e) = fs::create_dir_all(install_dir()) {
        print_error(&e.to_string());
    }
    let mut p = Parser::from_env();
    let mut subcmd: Option<String> = None;
    let mut args_pos: Vec<String> = Vec::new();
    let mut registry: Option<String> = None;
    let mut output: Option<String> = None;
    let mut verbose = false;
    let mut dev = false;
    let mut force = false;
    let mut lib = false;
    loop {
        match p.next() {
            Err(e) => {
                print_error(&e.to_string());
                return ExitCode::FAILURE;
            }
            Ok(None) => break,
            Ok(Some(arg)) => {
                match arg {
                    Short('v') | Long("version") => {
                        println!("virus v{}", VERSION);
                        return ExitCode::SUCCESS;
                    }
                    Short('h') | Long("help") => {
                        print_help();
                        return ExitCode::SUCCESS;
                    }
                    Short('r') | Long("registry") => {
                        match p.value() {
                            Ok(v) => registry = Some(v.into_string().unwrap()),
                            Err(e) => {
                                print_error(&e.to_string());
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    Short('o') | Long("output") => {
                        match p.value() {
                            Ok(v) => output = Some(v.into_string().unwrap()),
                            Err(e) => {
                                print_error(&e.to_string());
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    Long("verbose") => verbose = true,
                    Long("dev") => dev = true,
                    Long("force") => force = true,
                    Long("lib") => lib = true,
                    Value(v) => {
                        let s = match v.into_string() {
                            Ok(s) => s,
                            Err(e) => {
                                print_error(&e.as_os_str().to_string_lossy().to_string());
                                return ExitCode::FAILURE;
                            }
                        };
                        if subcmd.is_none() {
                            subcmd = Some(s);
                        } else {
                            args_pos.push(s);
                        }
                    }
                    _ => {
                        print_error(&format!("Unexpected argument: {:?}", arg));
                        return ExitCode::FAILURE;
                    }
                }
            }
        }
    }
    let subcmd = match subcmd {
        Some(s) => s,
        None => {
            print_help();
            return ExitCode::SUCCESS;
        }
    };
    let result = match subcmd.as_str() {
        "init" => cmd_init(args_pos.into_iter().next(), lib),
        "install" => {
            let mut iter = args_pos.into_iter();
            let pkg = iter.next();
            let ver = iter.next();
            cmd_install(pkg, ver, registry, dev, force, verbose)
        }
        "remove" => {
            if let Some(n) = args_pos.into_iter().next() {
                cmd_remove(n)
            } else {
                print_error("Usage: virus remove <pkg>");
                Ok(ExitCode::FAILURE)
            }
        }
        "update" => cmd_update(args_pos.into_iter().next(), verbose),
        "list" => cmd_list(),
        "search" => {
            if let Some(q) = args_pos.into_iter().next() {
                cmd_search(q)
            } else {
                print_error("Usage: virus search <query>");
                Ok(ExitCode::FAILURE)
            }
        }
        "info" => {
            if let Some(n) = args_pos.into_iter().next() {
                cmd_pkg_info(n)
            } else {
                system_info();
                Ok(ExitCode::SUCCESS)
            }
        }
        "run" => {
            if let Some(f) = args_pos.into_iter().next() {
                cmd_run(f, verbose)
            } else {
                print_error("Usage: virus run <file.hl>");
                Ok(ExitCode::FAILURE)
            }
        }
        "compile" => {
            if let Some(f) = args_pos.into_iter().next() {
                cmd_compile(f, output, verbose)
            } else {
                print_error("Usage: virus compile <file.hl>");
                Ok(ExitCode::FAILURE)
            }
        }
        "publish" => cmd_publish(),
        "clean" => cmd_clean(),
        "help" => {
            print_help();
            Ok(ExitCode::SUCCESS)
        }
        other => {
            print_error(&format!("Unknown command: {}", other));
            print_help();
            Ok(ExitCode::FAILURE)
        }
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            print_error(&e.to_string());
            ExitCode::FAILURE
        }
    }
}

