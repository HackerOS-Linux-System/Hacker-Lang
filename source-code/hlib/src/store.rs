use std::path::{Path, PathBuf};

use crate::error::{HlibError, Result};
use crate::fsutil;
use crate::manifest::Manifest;
use crate::read::HlibArchive;
use crate::trust::{TrustPolicy, TrustStatus};
use crate::validate::validate_name;
use crate::version::{Version, VersionReq};

pub const SYSTEM_DIR: &str = "/usr/lib/HackerOS/Hacker-Lang/hlibs";

#[derive(Debug, Clone)]
pub struct StoreEntry {
    pub path: PathBuf,
    pub name: String,
    /// `None` dla plików `<name>.hlib` (wersja dopiero w manifeście).
    pub version: Option<Version>,
}

#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    pub policy: TrustPolicy,
    /// Nadpisz istniejące `<name>-<version>.hlib`.
    pub overwrite: bool,
}

#[derive(Debug, Clone)]
pub struct HlibStore {
    dirs: Vec<PathBuf>,
}

/// `name-1.2.3` → (`name`, 1.2.3); `name` → (`name`, None).
fn split_stem(stem: &str) -> (String, Option<Version>) {
    // nazwa może zawierać '-', więc próbujemy od prawej strony
    for (i, ch) in stem.char_indices().rev() {
        if ch == '-' {
            if let Ok(v) = Version::parse(&stem[i + 1..]) {
                return (stem[..i].to_string(), Some(v));
            }
        }
    }
    (stem.to_string(), None)
}

impl HlibStore {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        HlibStore { dirs }
    }

    pub fn user_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".hackeros/Hacker-Lang/hlibs"))
    }

    /// Standardowa kolejność (spec §8): `hlibs/` projektu i jego przodków,
    /// katalog użytkownika, katalog systemowy.
    pub fn standard(start: &Path) -> Self {
        let mut d: Vec<PathBuf> = start.ancestors().map(|a| a.join("hlibs")).collect();
        if let Some(u) = Self::user_dir() {
            d.push(u);
        }
        d.push(PathBuf::from(SYSTEM_DIR));
        HlibStore { dirs: d }
    }

    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Wszystkie `.hlib` we wszystkich katalogach (bez otwierania archiwów).
    pub fn list(&self) -> Vec<StoreEntry> {
        let mut out = Vec::new();
        for dir in &self.dirs {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            let mut files: Vec<PathBuf> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("hlib") && p.is_file())
                .collect();
            files.sort();
            for p in files {
                let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else { continue };
                let (name, version) = split_stem(stem);
                if validate_name(&name).is_err() {
                    continue;
                }
                out.push(StoreEntry { path: p, name, version });
            }
        }
        out
    }

    /// Najwyższa wersja `name` spełniająca `req`. Katalogi wcześniejsze na
    /// liście mają pierwszeństwo przy równych wersjach (projekt > użytkownik > system).
    pub fn find(&self, name: &str, req: &VersionReq) -> Result<StoreEntry> {
        validate_name(name)?;
        let mut best: Option<(Version, StoreEntry)> = None;
        for e in self.list().into_iter().filter(|e| e.name == name) {
            let v = match &e.version {
                Some(v) => v.clone(),
                None => match Self::peek_manifest(&e.path).and_then(|m| m.version_parsed()) {
                    Ok(v) => v,
                    Err(_) => continue, // uszkodzony plik nie blokuje innych kandydatów
                },
            };
            if !req.matches(&v) {
                continue;
            }
            // `>` a nie `>=`: przy remisie zostaje wcześniejszy katalog
            if best.as_ref().map_or(true, |(bv, _)| v > *bv) {
                best = Some((v, e));
            }
        }
        best.map(|(_, e)| e).ok_or_else(|| HlibError::NotFound {
            name: name.to_string(),
            req: req.to_string(),
            searched: self.dirs.len(),
        })
    }

    fn peek_manifest(path: &Path) -> Result<Manifest> {
        Ok(HlibArchive::open(path)?.manifest)
    }

    /// Otwiera, sprawdza nazwę/wersję względem żądania, weryfikuje sumy i politykę zaufania.
    pub fn open_verified(
        &self,
        name: &str,
        req: &VersionReq,
        policy: &TrustPolicy,
    ) -> Result<(HlibArchive, StoreEntry, TrustStatus)> {
        let entry = self.find(name, req)?;
        let archive = HlibArchive::open(&entry.path)?;
        if archive.manifest.name != name {
            return Err(HlibError::NameMismatch {
                expected: name.to_string(),
                found: archive.manifest.name.clone(),
                found_version: archive.manifest.version.clone(),
            });
        }
        let v = archive.manifest.version_parsed()?;
        if !req.matches(&v) {
            return Err(HlibError::VersionUnsatisfied { found: v.to_string(), req: req.to_string() });
        }
        let status = archive.verify(policy)?;
        Ok((archive, entry, status))
    }

    /// Sprawdza, czy zależności z manifestu są dostępne w sklepie.
    /// Zwraca listę brakujących jako `(nazwa, wymaganie)`.
    pub fn missing_dependencies(&self, manifest: &Manifest) -> Vec<(String, String)> {
        let mut missing = Vec::new();
        for d in &manifest.dependencies {
            let ok = VersionReq::parse(&d.version_req)
                .ok()
                .and_then(|r| self.find(&d.name, &r).ok())
                .is_some();
            if !ok {
                missing.push((d.name.clone(), d.version_req.clone()));
            }
        }
        missing
    }

    /// Instaluje `.hlib` do `dest_dir` jako `<name>-<version>.hlib`
    /// po pełnej weryfikacji (sumy + polityka zaufania). Zapis atomowy.
    pub fn install(archive_path: &Path, dest_dir: &Path, opts: &InstallOptions) -> Result<PathBuf> {
        let bytes = std::fs::read(archive_path)
            .map_err(|e| HlibError::Io { path: archive_path.to_path_buf(), source: e })?;
        Self::install_bytes(&bytes, archive_path, dest_dir, opts)
    }

    pub fn install_bytes(bytes: &[u8], origin: &Path, dest_dir: &Path, opts: &InstallOptions) -> Result<PathBuf> {
        let archive = HlibArchive::from_bytes(bytes, origin)?;
        archive.verify(&opts.policy)?;
        let dest = dest_dir.join(format!("{}-{}.hlib", archive.manifest.name, archive.manifest.version));
        if dest.exists() && !opts.overwrite {
            return Err(HlibError::Io {
                path: dest,
                source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "already installed (use overwrite)"),
            });
        }
        fsutil::write_atomic(&dest, bytes, 0o644)?;
        Ok(dest)
    }

    /// Usuwa `<name>-<version>.hlib` z `dest_dir`; `Ok(false)` gdy nie było pliku.
    pub fn remove(dest_dir: &Path, name: &str, version: &Version) -> Result<bool> {
        validate_name(name)?;
        let p = dest_dir.join(format!("{}-{}.hlib", name, version));
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(HlibError::Io { path: p, source: e }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems() {
        assert_eq!(split_stem("mold-1.2.3"), ("mold".into(), Some(Version::new(1, 2, 3))));
        let (n, v) = split_stem("my-lib-0.1.0-rc.1");
        assert_eq!(n, "my-lib");
        assert_eq!(v.unwrap().to_string(), "0.1.0-rc.1");
        assert_eq!(split_stem("my-lib"), ("my-lib".into(), None));
    }
}
