use anyhow::{bail, Context, Result};
use hk_parser::{load_hk_file, HkValue};
use std::path::{Path, PathBuf};

use crate::libs::LibSource;

// ─────────────────────────────────────────────────────────────
// Struktura projektu
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct VirusDependency {
    pub name:         String,
    pub source:       LibSource,
    pub version:      Option<String>,
    pub static_link:  bool,
}

#[derive(Debug, Clone)]
pub struct VirusProject {
    pub name:         String,
    pub version:      String,
    pub description:  Option<String>,
    pub authors:      Vec<String>,
    pub edition:      String,
    pub dependencies: Vec<VirusDependency>,
    pub build_output: Option<String>,
    pub optimization: Option<u8>,
    /// Ścieżka do pliku konfiguracyjnego
    pub config_path:  PathBuf,
}

impl VirusProject {
    /// Wczytaj projekt z katalogu roboczego
    /// Szuka: Virus.hk → Virus.hcl (fallback)
    pub fn load(dir: &Path) -> Result<Self> {
        // Pierwszeństwo: Virus.hk, potem Virus.hcl
        let hk_path  = dir.join("Virus.hk");
        let hcl_path = dir.join("Virus.hcl");

        if hk_path.exists() {
            return Self::load_hk(&hk_path);
        }
        if hcl_path.exists() {
            return Self::load_hcl(&hcl_path);
        }

        bail!(
            "Nie znaleziono Virus.hk ani Virus.hcl w {}\nUruchom 'virus set' żeby zainicjalizować projekt.",
            dir.display()
        );
    }

    // ── .hk parser ───────────────────────────────────────────
    fn load_hk(path: &Path) -> Result<Self> {
        let config = load_hk_file(path)
        .with_context(|| format!("Błąd parsowania {}", path.display()))?;

        // [project]
        let project = config.get("project")
        .and_then(|v| v.as_map().ok())
        .ok_or_else(|| anyhow::anyhow!("Brak sekcji [project] w Virus.hk"))?;

        let name = project.get("name")
        .and_then(|v| v.as_string().ok())
        .ok_or_else(|| anyhow::anyhow!("Brak pola 'name' w [project]"))?;

        let version = project.get("version")
        .and_then(|v| v.as_string().ok())
        .unwrap_or_else(|| "0.1.0".to_string());

        let description = project.get("description")
        .and_then(|v| v.as_string().ok());

        let authors = project.get("authors")
        .and_then(|v| v.as_array().ok())
        .map(|arr| {
            arr.iter()
            .filter_map(|v| v.as_string().ok())
            .collect()
        })
        .unwrap_or_default();

        let edition = project.get("edition")
        .and_then(|v| v.as_string().ok())
        .unwrap_or_else(|| "2024".to_string());

        // [build]
        let (build_output, optimization) = if let Some(build) = config.get("build")
        .and_then(|v| v.as_map().ok())
        {
            let out  = build.get("output").and_then(|v| v.as_string().ok());
            let opt  = build.get("optimization")
            .and_then(|v| v.as_number().ok())
            .map(|n| n as u8);
            (out, opt)
        } else {
            (None, None)
        };

        // [dependencies]
        let dependencies = parse_hk_deps(&config)?;

        Ok(VirusProject {
            name,
            version,
            description,
            authors,
            edition,
            dependencies,
            build_output,
            optimization,
            config_path: path.to_path_buf(),
        })
    }

    // ── .hcl fallback ────────────────────────────────────────
    // Prosta implementacja ręczna dla .hcl
    fn load_hcl(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
        .with_context(|| format!("Nie można odczytać {}", path.display()))?;

        // Minimalistyczny parser HCL — wystarczy dla Virus.hcl
        let mut name        = String::from("unnamed");
        let mut version     = String::from("0.1.0");
        let mut description = None::<String>;
        let mut authors     = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if let Some(val) = extract_hcl_val(line, "name") {
                name = val;
            } else if let Some(val) = extract_hcl_val(line, "version") {
                version = val;
            } else if let Some(val) = extract_hcl_val(line, "description") {
                description = Some(val);
            } else if let Some(val) = extract_hcl_val(line, "authors") {
                authors.push(val.trim_matches('[').trim_matches(']').to_string());
            }
        }

        Ok(VirusProject {
            name,
            version,
            description,
            authors,
            edition: "2024".to_string(),
           dependencies: Vec::new(), // TODO: pełny parser HCL deps
           build_output: None,
           optimization: None,
           config_path: path.to_path_buf(),
        })
    }
}

// ─────────────────────────────────────────────────────────────
// Parsowanie sekcji [dependencies] z .hk
// ─────────────────────────────────────────────────────────────
fn parse_hk_deps(config: &hk_parser::HkConfig) -> Result<Vec<VirusDependency>> {
    let mut deps = Vec::new();

    let dep_section = match config.get("dependencies").and_then(|v| v.as_map().ok()) {
        Some(s) => s,
        None => return Ok(deps),
    };

    for (name, val) in dep_section {
        let dep = if let Ok(map) = val.as_map() {
            // Forma rozbudowana:
            // -> obsidian
            // --> source  => bytes
            // --> version => 0.2
            // --> static  => false
            let source_str = map.get("source")
            .and_then(|v| v.as_string().ok())
            .unwrap_or_else(|| "bytes".to_string());

            let source = parse_lib_source(&source_str);

            let version = map.get("version")
            .and_then(|v| v.as_string().ok());

            let static_link = map.get("static")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false);

            VirusDependency {
                name: name.clone(),
                source,
                version,
                static_link,
            }
        } else {
            // Forma prosta: -> obsidian => bytes
            let source_str = val.as_string().unwrap_or_default();
            VirusDependency {
                name:        name.clone(),
                source:      parse_lib_source(&source_str),
                version:     None,
                static_link: false,
            }
        };

        deps.push(dep);
    }

    Ok(deps)
}

fn parse_lib_source(s: &str) -> LibSource {
    match s.to_lowercase().as_str() {
        "bytes"        => LibSource::Bytes,
        "virus"        => LibSource::Virus,
        "vira"         => LibSource::Vira,
        "core"         => LibSource::Core,
        "github"       => LibSource::Github,
        "source"       => LibSource::Source,
        _              => LibSource::Bytes,
    }
}

// ─────────────────────────────────────────────────────────────
// Pomocnicze HCL
// ─────────────────────────────────────────────────────────────
fn extract_hcl_val(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{} =", key);
    let prefix2 = format!("{}=", key);
    let rest = if line.starts_with(&prefix) {
        &line[prefix.len()..]
    } else if line.starts_with(&prefix2) {
        &line[prefix2.len()..]
    } else {
        return None;
    };
    Some(rest.trim().trim_matches('"').to_string())
}
