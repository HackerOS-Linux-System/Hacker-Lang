use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;
use std::{fs, thread, time::Duration};

use crate::ui::{download_bar, progress_bar, step_info, step_ok, step_warn};
use crate::repo::{fetch_repo_index, RepoEntry};

// ─────────────────────────────────────────────────────────────
// Typy źródeł
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
pub enum LibSource {
    Bytes,
    Virus,
    Vira,
    Core,
    Github,
    Source,
}

impl LibSource {
    /// Ścieżka bazowa dla danego źródła
    pub fn base_dir(&self) -> PathBuf {
        let home = dirs::home_dir().expect("HOME not set");
        let base = home.join(".hackeros/hacker-lang/libs");
        match self {
            LibSource::Bytes  => base.join("bytes"),
            LibSource::Virus  => base.join(".virus"),
            LibSource::Vira   => base.join(".vira"),
            LibSource::Core   => base.join("core"),
            LibSource::Github => base.join(".github-cache"),
            LibSource::Source => base.join("sources"),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            LibSource::Bytes  => "bytes",
            LibSource::Virus  => "virus (tymczasowy)",
            LibSource::Vira   => "vira",
            LibSource::Core   => "core",
            LibSource::Github => "github",
            LibSource::Source => "source",
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Instalacja
// ─────────────────────────────────────────────────────────────
pub fn install_lib(name: &str, source: LibSource) -> Result<()> {
    match source {
        LibSource::Bytes  => install_bytes(name),
        LibSource::Virus  => install_virus(name),
        LibSource::Vira   => install_vira(name),
        LibSource::Core   => install_core(name),
        LibSource::Github => install_github(name),
        LibSource::Source => install_source(name),
    }
}

// ── bytes — pobieranie .so ────────────────────────────────────
fn install_bytes(name: &str) -> Result<()> {
    let target_dir = LibSource::Bytes.base_dir().join(name);

    // Sprawdź czy już zainstalowany
    let so_path = target_dir.join(format!("{}.so", name));
    if so_path.exists() {
        step_info(&format!("'{}' już zainstalowany", name));
        return Ok(());
    }

    // Pobierz indeks repozytoriów
    let entries = fetch_repo_index()
    .with_context(|| "Nie można pobrać indeksu repozytoriów virus.io")?;

    let entry = entries.iter()
    .find(|e| e.name == name)
    .ok_or_else(|| anyhow::anyhow!(
        "Biblioteka '{}' nie znaleziona w repozytorium.\n  Sprawdź dostępne: virus docs",
        name
    ))?;

    let so_url = entry.so_download.as_ref()
    .ok_or_else(|| anyhow::anyhow!(
        "Brak URL do pobrania .so dla biblioteki '{}'", name
    ))?;

    // Utwórz katalog
    fs::create_dir_all(&target_dir)
    .with_context(|| format!("Nie można utworzyć: {}", target_dir.display()))?;

    // Pobierz .so
    step_info(&format!("Pobieram {} z {}", name, so_url));
    download_file(so_url, &so_path, &format!("{}.so", name))?;
    step_ok(&format!("Zainstalowano: {}", so_path.display()));

    // Pobierz .hl jeśli dostępny
    if let Some(hl_url) = &entry.hl_download {
        let hl_path = target_dir.join(format!("{}.hl", name));
        step_info(&format!("Pobieram plik .hl: {}", hl_url));
        download_file(hl_url, &hl_path, &format!("{}.hl", name))
        .unwrap_or_else(|_| step_warn("Nie udało się pobrać pliku .hl — tylko .so"));
    }

    Ok(())
}

// ── virus — tymczasowe .a ──────────────────────────────────────
fn install_virus(name: &str) -> Result<()> {
    let target_dir = LibSource::Virus.base_dir().join(name);

    // Sprawdź czy już zainstalowany
    let a_path = target_dir.join(format!("{}.a", name));
    if a_path.exists() {
        step_info(&format!("'{}' już zainstalowany (tymczasowy)", name));
        return Ok(());
    }

    // Pobierz indeks
    let entries = fetch_repo_index()?;
    let entry = entries.iter()
    .find(|e| e.name == name)
    .ok_or_else(|| anyhow::anyhow!("Biblioteka '{}' nie znaleziona", name))?;

    let a_url = entry.a_download.as_ref()
    .ok_or_else(|| anyhow::anyhow!("Brak URL do .a dla '{}'", name))?;

    fs::create_dir_all(&target_dir)?;
    download_file(a_url, &a_path, &format!("{}.a", name))?;

    step_ok(&format!(
        "Zainstalowano (tymczasowy, zniknie po reboot): {}",
                     a_path.display()
    ));

    // Stwórz marker usunięcia przy reboot
    let marker = LibSource::Virus.base_dir().join(format!(".{}.reboot-clean", name));
    fs::write(&marker, name)?;

    Ok(())
}

// ── vira — placeholder ──────────────────────────────────────
fn install_vira(name: &str) -> Result<()> {
    step_warn(&format!(
        "Repozytorium 'vira' jest w przygotowaniu.\n  \
'{}' nie może być zainstalowany tą metodą.",
name
    ));
    step_info("Spróbuj: virus ii <lib> --bytes lub --github");
    Ok(())
}

// ── core — pliki .hl ────────────────────────────────────────
fn install_core(name: &str) -> Result<()> {
    let target_dir = LibSource::Core.base_dir();
    fs::create_dir_all(&target_dir)?;

    let hl_path = target_dir.join(format!("{}.hl", name));
    if hl_path.exists() {
        step_info(&format!("Core lib '{}' już zainstalowana", name));
        return Ok(());
    }

    let entries = fetch_repo_index()?;
    let entry = entries.iter()
    .find(|e| e.name == name)
    .ok_or_else(|| anyhow::anyhow!("Core lib '{}' nie znaleziona", name))?;

    if let Some(hl_url) = &entry.hl_download {
        download_file(hl_url, &hl_path, &format!("{}.hl", name))?;
        step_ok(&format!("Zainstalowano core lib: {}", hl_path.display()));
    } else {
        bail!("Brak URL do .hl dla core lib '{}'", name);
    }

    Ok(())
}

// ── github — klonowanie repo ─────────────────────────────────
fn install_github(name: &str) -> Result<()> {
    // Format: virus ii owner/repo --github
    let target_dir = LibSource::Github.base_dir().join(
        name.replace('/', "_")
    );

    if target_dir.exists() {
        step_info(&format!("Repo '{}' już sklonowane", name));
        return Ok(());
    }

    fs::create_dir_all(LibSource::Github.base_dir())?;

    let url = if name.starts_with("https://") || name.starts_with("http://") {
        name.to_string()
    } else {
        format!("https://github.com/{}", name)
    };

    step_info(&format!("Klonuję: {}", url));

    let pb = progress_bar(1, &format!("git clone {}", name));
    pb.set_message("Klonowanie...");

    let status = Command::new("git")
    .args(["clone", "--depth=1", &url, target_dir.to_str().unwrap()])
    .status()
    .with_context(|| "Nie można uruchomić git — czy git jest zainstalowany?")?;

    pb.inc(1);
    pb.finish_with_message(format!(
        "{} {}",
        if status.success() { "✓" } else { "✗" },
            if status.success() { "Sklonowano" } else { "Błąd klonowania" }
    ));

    if !status.success() {
        bail!("git clone nieudany dla '{}'", url);
    }

    step_ok(&format!("Repo zainstalowane: {}", target_dir.display()));
    Ok(())
}

// ── source — gotowe projekty ─────────────────────────────────
fn install_source(name: &str) -> Result<()> {
    let target_dir = LibSource::Source.base_dir().join(name);

    if target_dir.exists() {
        step_info(&format!("Source lib '{}' już zainstalowana", name));
        return Ok(());
    }

    let entries = fetch_repo_index()?;
    let entry = entries.iter()
    .find(|e| e.name == name)
    .ok_or_else(|| anyhow::anyhow!("Source lib '{}' nie znaleziona", name))?;

    let archive_url = entry.archive_url.as_ref()
    .ok_or_else(|| anyhow::anyhow!("Brak URL archiwum dla '{}'", name))?;

    fs::create_dir_all(&target_dir)?;

    let archive_path = target_dir.join(format!("{}.tar.gz", name));
    step_info(&format!("Pobieram archiwum: {}", archive_url));
    download_file(archive_url, &archive_path, &format!("{}.tar.gz", name))?;

    // Rozpakuj
    let pb = progress_bar(1, "Rozpakowywanie");
    pb.set_message("Rozpakowuję archiwum...");

    Command::new("tar")
    .args(["-xzf", archive_path.to_str().unwrap(), "-C", target_dir.to_str().unwrap()])
    .status()
    .ok();

    let _ = fs::remove_file(&archive_path);
    pb.inc(1);
    pb.finish_with_message(format!("{} Zainstalowano", "✓".to_string()));

    step_ok(&format!("Source lib: {}", target_dir.display()));
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Usuwanie
// ─────────────────────────────────────────────────────────────
pub fn remove_lib(name: &str, source: LibSource) -> Result<()> {
    let lib_dir = source.base_dir().join(name);

    if !lib_dir.exists() {
        // Spróbuj jako plik .hl (core)
        if source == LibSource::Core {
            let hl_path = source.base_dir().join(format!("{}.hl", name));
            if hl_path.exists() {
                fs::remove_file(&hl_path)?;
                step_ok(&format!("Usunięto core lib: {}", name));
                return Ok(());
            }
        }
        bail!(
            "Biblioteka '{}' ({}) nie jest zainstalowana",
              name, source.display_name()
        );
    }

    let pb = progress_bar(1, &format!("Usuwanie {}", name));
    pb.set_message(format!("Usuwam {}...", name));

    fs::remove_dir_all(&lib_dir)
    .with_context(|| format!("Nie można usunąć: {}", lib_dir.display()))?;

    pb.inc(1);
    pb.finish_with_message(format!("{} Usunięto", "✓".to_string()));

    step_ok(&format!("Usunięto '{}' [{}]", name, source.display_name()));
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Pobieranie pliku HTTP z paskiem postępu
// ─────────────────────────────────────────────────────────────
pub fn download_file(url: &str, dest: &std::path::Path, label: &str) -> Result<()> {
    use std::io::Write;

    // Użyj reqwest blocking
    let client = reqwest::blocking::Client::builder()
    .user_agent(format!("virus/{}", env!("CARGO_PKG_VERSION")))
    .timeout(Duration::from_secs(120))
    .build()?;

    let mut response = client.get(url).send()
    .with_context(|| format!("Nie można pobrać: {}", url))?;

    if !response.status().is_success() {
        bail!("HTTP {} dla URL: {}", response.status(), url);
    }

    let total = response.content_length().unwrap_or(0);
    let pb    = if total > 0 {
        download_bar(total, label)
    } else {
        // Nieznany rozmiar — spinner
        let p = indicatif::ProgressBar::new_spinner();
        p.set_style(
            indicatif::ProgressStyle::with_template(
                "  {spinner:.cyan} {msg} {bytes}"
            ).unwrap()
            .tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"])
        );
        p.set_message(format!("Pobieram {}...", label));
        p.enable_steady_tick(Duration::from_millis(80));
        p
    };

    let mut file = fs::File::create(dest)
    .with_context(|| format!("Nie można utworzyć pliku: {}", dest.display()))?;

    let mut downloaded: u64 = 0;
    let mut buf = vec![0u8; 8192];

    loop {
        use std::io::Read;
        let n = response.read(&mut buf)?;
        if n == 0 { break; }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        pb.set_position(downloaded);
    }

    pb.finish_with_message(format!("{} Pobrano {}", "✓", label));
    Ok(())
}
