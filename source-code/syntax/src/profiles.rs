use crate::registry::resolve_version;
use crate::version::Version;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub base: Version,
    pub provides: Vec<String>,
    pub description: String,
    pub path: PathBuf,
}

/// Nazwa profilu trafia do ścieżki pliku, więc dopuszczamy tylko bezpieczne
/// znaki — żadnych `/`, `\`, `..` (ucieczka z katalogu profili).
pub fn is_valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Katalogi przeszukiwane w poszukiwaniu profili (w kolejności priorytetu).
pub fn profile_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(p) = std::env::var_os("HL_SYNTAX_PATH") {
        for d in std::env::split_paths(&p) {
            if !d.as_os_str().is_empty() {
                dirs.push(d);
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".hackeros/hacker-lang/syntax/not-default"));
    }
    dirs.push(PathBuf::from("/usr/lib/HackerOS/Hacker-Lang/syntax/not-default"));
    dirs
}

/// Wczytaj profil z konkretnego pliku.
pub fn load_profile_file(name: &str, path: &Path) -> Result<Profile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("nie mogę odczytać profilu {}: {}", path.display(), e))?;
    parse_profile(name, &text, path)
}

pub fn parse_profile(name: &str, text: &str, path: &Path) -> Result<Profile, String> {
    let mut base: Option<Version> = None;
    let mut provides = Vec::new();
    let mut description = String::new();

    for (i, raw) in text.lines().enumerate() {
        let line = match raw.find(";;") {
            Some(p) => &raw[..p],
            None => raw,
        }
        .trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            format!("{}:{}: oczekiwano `klucz = wartość`", path.display(), i + 1)
        })?;
        let (key, value) = (key.trim(), value.trim());
        match key {
            "base" => {
                let v = Version::parse_hackeros(value)
                    .map_err(|e| format!("{}:{}: base: {}", path.display(), i + 1, e))?;
                resolve_version(v)
                    .map_err(|e| format!("{}:{}: base: {}", path.display(), i + 1, e))?;
                base = Some(v);
            }
            "provides" => {
                provides = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "description" => description = value.to_string(),
            other => {
                return Err(format!(
                    "{}:{}: nieznany klucz `{}` (dozwolone: base, provides, description)",
                    path.display(),
                    i + 1,
                    other
                ))
            }
        }
    }

    let base = base.ok_or_else(|| format!("{}: brak wymaganego klucza `base`", path.display()))?;
    Ok(Profile { name: name.to_string(), base, provides, description, path: path.to_path_buf() })
}

/// Znajdź profil w podanych katalogach. `Ok(None)` = nie ma takiego profilu.
pub fn find_profile_in(dirs: &[PathBuf], name: &str) -> Result<Option<Profile>, String> {
    if !is_valid_profile_name(name) {
        return Err(format!(
            "niepoprawna nazwa profilu `{}` (dozwolone: litery, cyfry, `-`, `_`, `.`)",
            name
        ));
    }
    for dir in dirs {
        let path = dir.join(format!("{}.syntax", name));
        if path.is_file() {
            return load_profile_file(name, &path).map(Some);
        }
    }
    Ok(None)
}

pub fn find_profile(name: &str) -> Result<Option<Profile>, String> {
    find_profile_in(&profile_dirs(), name)
}

/// Wszystkie zainstalowane profile (do `hl version`). Błędne pliki pomijane.
pub fn list_profiles() -> Vec<Profile> {
    let mut out: Vec<Profile> = Vec::new();
    for dir in profile_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        for path in files {
            if path.extension().and_then(|e| e.to_str()) != Some("syntax") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if !is_valid_profile_name(stem) || out.iter().any(|p| p.name == stem) {
                continue;
            }
            if let Ok(p) = load_profile_file(stem, &path) {
                out.push(p);
            }
        }
    }
    out
}
