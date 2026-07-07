use anyhow::Result;
use hk_parser::{parse_hk, write_hk_file, HkConfig, HkValue};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

/// Ścieżka do pliku config.hk
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("hackeros")
        .join("hacker-lang")
        .join("config.hk")
}

/// Katalog envów
pub fn envs_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".hackeros")
        .join("hacker-lang")
        .join("envs")
}

/// Katalog libs globalny
pub fn global_libs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".hackeros")
        .join("hacker-lang")
        .join("libs")
}

/// Wrapper nad HkConfig (IndexMap) dla konfiguracji HL
#[derive(Debug, Default, Clone)]
pub struct HlConfig {
    inner: HkConfig,
}

impl HlConfig {
    pub fn new() -> Self { Self::default() }

    /// Odczytaj wartość z sekcji jako string
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let section_val = self.inner.get(section)?;
        if let HkValue::Map(map) = section_val {
            if let Some(v) = map.get(key) {
                return match v {
                    HkValue::String(s) => Some(s.as_str()),
                    _ => None,
                };
            }
        }
        None
    }

    /// Ustaw wartość string w sekcji
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let section_entry = self.inner
            .entry(section.to_string())
            .or_insert_with(|| HkValue::Map(IndexMap::new()));

        if let HkValue::Map(map) = section_entry {
            map.insert(key.to_string(), HkValue::String(value.to_string()));
        }
    }

    /// Usuń klucz z sekcji
    pub fn remove_key(&mut self, section: &str, key: &str) {
        if let Some(HkValue::Map(map)) = self.inner.get_mut(section) {
            map.remove(key);
        }
    }

    /// Aktywne środowisko (None = globalne)
    pub fn active_env(&self) -> Option<&str> {
        let name = self.get("env", "active")?;
        if name.is_empty() || name == "global" { None } else { Some(name) }
    }

    /// Ścieżka aktywnego środowiska
    pub fn active_env_path(&self) -> Option<PathBuf> {
        let path = self.get("env", "active_path")?;
        if path.is_empty() { None } else { Some(PathBuf::from(path)) }
    }

    /// Ścieżka libs (env lub globalna)
    pub fn effective_libs_dir(&self) -> PathBuf {
        if let Some(env_path) = self.active_env_path() {
            env_path.join("libs")
        } else {
            self.get("paths", "libs")
                .map(PathBuf::from)
                .unwrap_or_else(global_libs_dir)
        }
    }

    /// bit.lock (env lub globalny)
    pub fn effective_lock_path(&self) -> PathBuf {
        if let Some(env_path) = self.active_env_path() {
            env_path.join("bit.lock")
        } else {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".hackeros/hacker-lang/meta/bit.lock")
        }
    }

    pub fn python_cmd(&self) -> &str { self.get("extern", "python").unwrap_or("python3") }
    pub fn java_cmd(&self)   -> &str { self.get("extern", "java").unwrap_or("java") }
    pub fn shell_cmd(&self)  -> &str { self.get("extern", "shell").unwrap_or("bash") }

    /// Pobierz wewnętrzny HkConfig do serializacji
    pub fn hk_config(&self) -> &HkConfig { &self.inner }
}

// ── Odczyt / zapis przez hk-parser ────────────────────────────────────────────

pub fn load_config() -> HlConfig {
    let path = config_path();
    if !path.exists() {
        return default_config();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match parse_hk(&content) {
            Ok(inner) => HlConfig { inner },
            Err(_)    => default_config(),
        },
        Err(_) => default_config(),
    }
}

pub fn save_config(cfg: &HlConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_hk_file(&path, cfg.hk_config())?;
    Ok(())
}

fn default_config() -> HlConfig {
    let mut cfg = HlConfig::new();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let base = home.join(".hackeros/hacker-lang");

    cfg.set("env", "active",       "");
    cfg.set("env", "active_path",  "");

    cfg.set("paths", "libs",  base.join("libs").to_str().unwrap_or(""));
    cfg.set("paths", "cache", base.join("cache").to_str().unwrap_or(""));
    cfg.set("paths", "meta",  base.join("meta").to_str().unwrap_or(""));
    cfg.set("paths", "envs",  base.join("envs").to_str().unwrap_or(""));

    cfg.set("runtime", "default_gen", "2");
    cfg.set("runtime", "jit",         "false");

    cfg.set("extern", "python", "python3");
    cfg.set("extern", "java",   "java");
    cfg.set("extern", "shell",  "bash");

    cfg
}

/// Zapisz konfigurację pod konkretną ścieżkę (np. env.hk środowiska)
pub fn save_config_to_path(cfg: &HlConfig, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_hk_file(path, cfg.hk_config())?;
    Ok(())
}

// ── Helpers dla hl env ────────────────────────────────────────────────────────

pub fn set_active_env(name: &str, path: &Path) -> Result<()> {
    let mut cfg = load_config();
    cfg.set("env", "active",      name);
    cfg.set("env", "active_path", path.to_str().unwrap_or(""));
    save_config(&cfg)
}

pub fn clear_active_env() -> Result<()> {
    let mut cfg = load_config();
    cfg.set("env", "active",      "");
    cfg.set("env", "active_path", "");
    save_config(&cfg)
}

pub fn get_active_env() -> Option<(String, PathBuf)> {
    let cfg = load_config();
    let name = cfg.active_env()?.to_string();
    let path = cfg.active_env_path()?;
    Some((name, path))
}
