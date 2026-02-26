use anyhow::{Context, Result};
use hk_parser::{parse_hk, HkValue};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use std::{fs, env};

const REPO_URL: &str =
"https://raw.githubusercontent.com/HackerOS-Linux-System/Hacker-Lang/main/repository/virus.io";

/// Czas ważności cache: 1 godzina
const CACHE_TTL_SECS: u64 = 3600;

// ─────────────────────────────────────────────────────────────
// Wpis w repozytorium
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct RepoEntry {
    pub name:        String,
    pub version:     Option<String>,
    pub description: Option<String>,
    pub authors:     Vec<String>,
    pub so_download: Option<String>,
    pub hl_download: Option<String>,
    pub a_download:  Option<String>,
    pub archive_url: Option<String>,
}

// ─────────────────────────────────────────────────────────────
// Pobierz indeks (z cache lub sieci)
// ─────────────────────────────────────────────────────────────
pub fn fetch_repo_index() -> Result<Vec<RepoEntry>> {
    let cache_path = repo_cache_path();

    // Sprawdź czy cache jest aktualny
    if let Ok(meta) = fs::metadata(&cache_path) {
        if let Ok(modified) = meta.modified() {
            if let Ok(age) = SystemTime::now().duration_since(modified) {
                if age.as_secs() < CACHE_TTL_SECS {
                    if let Ok(content) = fs::read_to_string(&cache_path) {
                        return parse_repo_hk(&content);
                    }
                }
            }
        }
    }

    // Pobierz z sieci
    let content = download_repo_index()?;

    // Zapisz cache
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&cache_path, &content);

    parse_repo_hk(&content)
}

/// Wymuś odświeżenie cache
pub fn refresh_repo_index() -> Result<Vec<RepoEntry>> {
    let cache_path = repo_cache_path();
    let _ = fs::remove_file(&cache_path);
    fetch_repo_index()
}

// ─────────────────────────────────────────────────────────────
// Ścieżka cache
// ─────────────────────────────────────────────────────────────
fn repo_cache_path() -> PathBuf {
    dirs::cache_dir()
    .unwrap_or_else(|| dirs::home_dir().unwrap().join(".cache"))
    .join("hacker-lang")
    .join("virus-repo.hk")
}

// ─────────────────────────────────────────────────────────────
// Pobieranie HTTP
// ─────────────────────────────────────────────────────────────
fn download_repo_index() -> Result<String> {
    let client = reqwest::blocking::Client::builder()
    .user_agent(format!("virus/{}", env!("CARGO_PKG_VERSION")))
    .timeout(Duration::from_secs(30))
    .build()?;

    let resp = client.get(REPO_URL).send()
    .with_context(|| format!("Nie można pobrać indeksu z: {}", REPO_URL))?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "HTTP {} podczas pobierania indeksu repozytoriów.\n\
Sprawdź połączenie internetowe lub użyj biblioteki lokalnej.",
resp.status()
        );
    }

    resp.text().with_context(|| "Nieprawidłowa odpowiedź z serwera")
}

// ─────────────────────────────────────────────────────────────
// Parsowanie formatu .hk dla virus.io
// ─────────────────────────────────────────────────────────────
fn parse_repo_hk(content: &str) -> Result<Vec<RepoEntry>> {
    let config = parse_hk(content)
    .with_context(|| "Błąd parsowania indeksu repozytoriów virus.io")?;

    let mut entries = Vec::new();

    let libraries = config
    .get("libraries")
    .and_then(|v| v.as_map().ok())
    .ok_or_else(|| anyhow::anyhow!("Brak sekcji [libraries] w virus.io"))?;

    for (name, value) in libraries {
        let map = match value.as_map() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let entry = RepoEntry {
            name: name.clone(),

            version: map.get("version")
            .and_then(|v| v.as_string().ok()),

            description: map.get("description")
            .and_then(|v| v.as_string().ok()),

            authors: map.get("authors")
            .and_then(|v| v.as_array().ok())
            .map(|arr| arr.iter().filter_map(|v| v.as_string().ok()).collect())
            .unwrap_or_default(),

            so_download: map.get("so-download")
            .and_then(|v| v.as_string().ok()),

            hl_download: map.get(".hl-download")
            .and_then(|v| v.as_string().ok()),

            a_download: map.get("a-download")
            .and_then(|v| v.as_string().ok()),

            archive_url: map.get("archive-download")
            .and_then(|v| v.as_string().ok()),
        };

        entries.push(entry);
    }

    Ok(entries)
}

// ─────────────────────────────────────────────────────────────
// Wyświetl listę dostępnych bibliotek
// ─────────────────────────────────────────────────────────────
pub fn list_available() -> Result<()> {
    use crate::ui::{step_info, step_ok};
    use owo_colors::OwoColorize;

    let entries = fetch_repo_index()?;

    println!();
    println!(
        "  {} ({} bibliotek):",
             "Dostępne biblioteki virus.io".bright_white().bold(),
             entries.len().to_string().bright_cyan()
    );
    println!("  {}", "─".repeat(60).dimmed());

    for e in &entries {
        let ver = e.version.as_deref().unwrap_or("?");
        let desc = e.description.as_deref().unwrap_or("");
        println!(
            "  {:20} {} {}",
            e.name.bright_cyan(),
                 format!("v{}", ver).bright_yellow(),
                     desc.dimmed()
        );
    }

    println!();
    println!(
        "  {} virus ii <nazwa>",
        "Instaluj:".dimmed()
    );

    Ok(())
}
