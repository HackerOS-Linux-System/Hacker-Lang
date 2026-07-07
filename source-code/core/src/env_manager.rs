use anyhow::{bail, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};
use crate::config::{
    config_path, envs_base_dir, load_config,
    set_active_env, clear_active_env, get_active_env,
};

// ── Struktura środowiska ──────────────────────────────────────────────────────

pub struct HlEnv {
    pub name:     String,
    pub path:     PathBuf,
    pub libs_dir: PathBuf,
    pub meta_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub lock_file: PathBuf,
    pub env_meta:  PathBuf,
}

impl HlEnv {
    pub fn from_path(path: &Path) -> Self {
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        Self {
            name:      name.clone(),
            path:      path.to_path_buf(),
            libs_dir:  path.join("libs"),
            meta_dir:  path.join("meta"),
            cache_dir: path.join("cache"),
            lock_file: path.join("meta").join("bit.lock"),
            env_meta:  path.join("env.hk"),
        }
    }

    pub fn from_name(name: &str) -> Self {
        let path = envs_base_dir().join(name);
        Self::from_path(&path)
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn is_active(&self) -> bool {
        if let Some((_, active_path)) = get_active_env() {
            active_path == self.path
        } else {
            false
        }
    }
}

// ── hl env create ─────────────────────────────────────────────────────────────

pub fn cmd_env_create(name_or_path: &str) -> Result<()> {
    let (name, env_path) = resolve_env_location(name_or_path);
    let env = HlEnv::from_path(&env_path);

    if env.exists() {
        bail!(
            "Środowisko '{}' już istnieje w {:?}",
            name.bright_yellow(),
            env_path
        );
    }

    print_env_header("hl env create");
    println!("  Nazwa:     {}", name.bright_cyan().bold());
    println!("  Lokalizacja: {}", env_path.display().to_string().bright_white());
    println!();

    // Utwórz strukturę katalogów
    std::fs::create_dir_all(&env.libs_dir)?;
    std::fs::create_dir_all(&env.meta_dir)?;
    std::fs::create_dir_all(&env.cache_dir)?;

    // Utwórz bit.lock (pusty JSON)
    std::fs::write(&env.lock_file, "{}\n")?;

    // Utwórz env.hk — metadane środowiska (format .hk przez hk-parser)
    let created_at = chrono_now();
    {
        use crate::config::HlConfig;
        let mut env_cfg = HlConfig::new();
        env_cfg.set("env",   "name",    &name);
        env_cfg.set("env",   "path",    env_path.to_str().unwrap_or(""));
        env_cfg.set("env",   "created", &created_at);
        env_cfg.set("paths", "libs",    env.libs_dir.to_str().unwrap_or(""));
        env_cfg.set("paths", "meta",    env.meta_dir.to_str().unwrap_or(""));
        env_cfg.set("paths", "cache",   env.cache_dir.to_str().unwrap_or(""));
        env_cfg.set("paths", "lock",    env.lock_file.to_str().unwrap_or(""));
        crate::config::save_config_to_path(&env_cfg, &env.env_meta)?;
    }

    println!("  {} Środowisko utworzone.", "✓".green().bold());
    println!();
    println!("  Aby wejść:   {}", format!("hl env enter {}", name).bright_cyan().bold());
    println!("  Po wejściu bit install działa w izolowanym środowisku.");
    println!();
    print_env_hr();
    Ok(())
}

// ── hl env enter ─────────────────────────────────────────────────────────────

pub fn cmd_env_enter(name_or_path: Option<&str>) -> Result<()> {
    let (name, env_path) = match name_or_path {
        Some(s) => resolve_env_location(s),
        None    => {
            // Wejdź do środowiska w bieżącym katalogu (szuka env.hk)
            let cwd = std::env::current_dir().unwrap_or_default();
            if cwd.join("env.hk").exists() {
                let n = cwd.file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("current")
                    .to_string();
                (n, cwd)
            } else {
                bail!(
                    "Nie podano nazwy środowiska i brak env.hk w bieżącym katalogu.\n\
                     Użycie: hl env enter <nazwa> lub hl env enter <ścieżka>"
                );
            }
        }
    };

    let env = HlEnv::from_path(&env_path);

    if !env.exists() {
        bail!(
            "Środowisko '{}' nie istnieje.\n  Utwórz je: {}",
            name.bright_yellow(),
            format!("hl env create {}", name).bright_cyan()
        );
    }

    // Zapisz aktywne środowisko do config.hk
    set_active_env(&name, &env_path)?;

    // Ustaw zmienne środowiskowe dla bieżącego procesu
    // (bit czyta config.hk, więc to wystarczy dla nowych procesów)
    std::env::set_var("HL_ENV_NAME",    &name);
    std::env::set_var("HL_ENV_PATH",    env_path.to_str().unwrap_or(""));
    std::env::set_var("HL_ENV_LIBS",    env.libs_dir.to_str().unwrap_or(""));
    std::env::set_var("HL_ENV_LOCK",    env.lock_file.to_str().unwrap_or(""));
    std::env::set_var("HL_ENV_ACTIVE",  "1");

    // Aktualizuj BIT_HOME i BIT_LOCK_FILE żeby bit automatycznie
    // korzystał z izolowanego środowiska
    std::env::set_var("BIT_HOME",      env.libs_dir.to_str().unwrap_or(""));
    std::env::set_var("BIT_LOCK_FILE", env.lock_file.to_str().unwrap_or(""));
    std::env::set_var("BIT_CACHE_DIR", env.cache_dir.to_str().unwrap_or(""));
    std::env::set_var("BIT_META_DIR",  env.meta_dir.to_str().unwrap_or(""));

    print_env_header("hl env enter");
    println!("  {} Wchodzę do środowiska: {}", "→".bright_cyan(), name.bright_cyan().bold());
    println!("  Lokalizacja: {}", env_path.display().to_string().bright_white());
    println!("  Libs:        {}", env.libs_dir.display().to_string().bright_white());
    println!("  Lock:        {}", env.lock_file.display().to_string().bright_white());
    println!("  Config:      {}", config_path().display().to_string().bright_black());
    println!();
    println!("  {} Środowisko aktywne. bit install instaluje teraz do tego env.", "✓".green().bold());
    println!();

    // Pokaż zainstalowane paczki w tym środowisku
    show_env_packages(&env);

    println!();
    println!("  Aby opuścić:  {}", "hl env exit".bright_yellow());
    println!("  Aby zobaczyć: {}", "hl env status".bright_cyan());
    print_env_hr();

    // Uruchom subshell z ustawionymi zmiennymi
    // tak żeby użytkownik mógł pracować w tym środowisku
    launch_env_shell(&name, &env)?;

    Ok(())
}

// ── hl env exit ───────────────────────────────────────────────────────────────

pub fn cmd_env_exit() -> Result<()> {
    let current = get_active_env();

    if current.is_none() {
        println!("{}", "Nie jesteś w żadnym środowisku (już globalne).".bright_black());
        return Ok(());
    }

    let (name, _) = current.unwrap();
    clear_active_env()?;

    // Wyczyść zmienne środowiskowe (tylko dla bieżącego procesu)
    std::env::remove_var("HL_ENV_NAME");
    std::env::remove_var("HL_ENV_PATH");
    std::env::remove_var("HL_ENV_LIBS");
    std::env::remove_var("HL_ENV_LOCK");
    std::env::remove_var("HL_ENV_ACTIVE");
    std::env::remove_var("BIT_HOME");
    std::env::remove_var("BIT_LOCK_FILE");
    std::env::remove_var("BIT_CACHE_DIR");
    std::env::remove_var("BIT_META_DIR");

    println!("{} Opuszczono środowisko '{}'.", "✓".green().bold(), name.bright_cyan());
    println!("  Wróciłeś do globalnego kontekstu bit.");
    Ok(())
}

// ── hl env remove ─────────────────────────────────────────────────────────────

pub fn cmd_env_remove(name_or_path: &str) -> Result<()> {
    let (name, env_path) = resolve_env_location(name_or_path);
    let env = HlEnv::from_path(&env_path);

    if !env.exists() {
        bail!("Środowisko '{}' nie istnieje: {:?}", name, env_path);
    }

    // Jeśli to aktywne środowisko — wyczyść najpierw
    if env.is_active() {
        clear_active_env()?;
        println!("{} Deaktywowano środowisko '{}'.", "→".bright_yellow(), name.bright_cyan());
    }

    print_env_header("hl env remove");
    println!("  Usuwam środowisko: {}", name.bright_red().bold());
    println!("  Lokalizacja: {}", env_path.display().to_string().bright_white());

    // Pokaż co będzie usunięte
    if let Ok(size) = dir_size(&env_path) {
        println!("  Rozmiar:     {} KB", (size / 1024).to_string().bright_yellow());
    }

    std::fs::remove_dir_all(&env_path)?;

    println!();
    println!("  {} Środowisko '{}' usunięte.", "✓".green().bold(), name.bright_cyan());
    print_env_hr();
    Ok(())
}

// ── hl env list ──────────────────────────────────────────────────────────────

pub fn cmd_env_list() -> Result<()> {
    let envs_dir = envs_base_dir();
    let active = get_active_env();

    print_env_header("hl env list");

    if !envs_dir.exists() {
        println!("  {}", "Brak środowisk. Utwórz pierwsze:".bright_black());
        println!("    {}", "hl env create <nazwa>".bright_cyan());
        print_env_hr();
        return Ok(());
    }

    let mut envs: Vec<PathBuf> = std::fs::read_dir(&envs_dir)?
        .flatten()
        .filter(|e| e.path().is_dir() && e.path().join("env.hk").exists())
        .map(|e| e.path())
        .collect();
    envs.sort();

    if envs.is_empty() {
        println!("  {}", "Brak środowisk.".bright_black());
        println!("    {}", "hl env create <nazwa>".bright_cyan());
        print_env_hr();
        return Ok(());
    }

    for env_path in &envs {
        let env = HlEnv::from_path(env_path);
        let is_active = active.as_ref()
            .map(|(_, p)| p == env_path)
            .unwrap_or(false);

        let marker = if is_active {
            "▶ ".bright_green().bold().to_string()
        } else {
            "  ".to_string()
        };

        let pkg_count = count_installed_pkgs(&env);
        let size_str = dir_size(env_path)
            .map(|s| format!("{} KB", s / 1024))
            .unwrap_or_else(|_| "?".to_string());

        if is_active {
            println!(
                "{}{} {} pkgs, {}",
                marker,
                env.name.bright_cyan().bold(),
                pkg_count.to_string().bright_white(),
                size_str.bright_black(),
            );
            println!("     {}", env_path.display().to_string().bright_black());
        } else {
            println!(
                "{}{} {} pkgs, {}",
                marker,
                env.name.bright_white(),
                pkg_count.to_string().bright_black(),
                size_str.bright_black(),
            );
        }
    }

    println!();
    if let Some((name, _)) = &active {
        println!("  Aktywne: {}", name.bright_cyan().bold());
    } else {
        println!("  Aktywne: {}", "(globalne)".bright_black());
    }
    print_env_hr();
    Ok(())
}

// ── hl env status ─────────────────────────────────────────────────────────────

pub fn cmd_env_status() -> Result<()> {
    let cfg = load_config();

    print_env_header("hl env status");

    match get_active_env() {
        Some((name, path)) => {
            let env = HlEnv::from_path(&path);
            println!("  Status:    {} AKTYWNE", "●".bright_green());
            println!("  Środowisko: {}", name.bright_cyan().bold());
            println!("  Ścieżka:   {}", path.display().to_string().bright_white());
            println!("  Libs:      {}", env.libs_dir.display().to_string().bright_white());
            println!("  Lock:      {}", env.lock_file.display().to_string().bright_white());
            println!("  Cache:     {}", env.cache_dir.display().to_string().bright_white());
            println!();
            show_env_packages(&env);
        }
        None => {
            println!("  Status:    {} Globalne (brak aktywnego środowiska)", "●".bright_black());
            println!("  Libs:      {}", cfg.effective_libs_dir().display().to_string().bright_white());
            println!("  Lock:      {}", cfg.effective_lock_path().display().to_string().bright_white());
        }
    }

    println!();
    println!("  Config:    {}", config_path().display().to_string().bright_black());
    println!();

    // Pokaż extern runtime paths
    println!("  {} Extern runtimes:", "⚙".bright_yellow());
    println!("    python → {}", cfg.python_cmd().bright_white());
    println!("    java   → {}", cfg.java_cmd().bright_white());
    println!("    shell  → {}", cfg.shell_cmd().bright_white());

    print_env_hr();
    Ok(())
}

// ── hl env help ───────────────────────────────────────────────────────────────

pub fn cmd_env_help() {
    print_env_header("hl env — manager izolowanych środowisk");
    println!("  Środowisko = katalog z własnymi libs, bit.lock i cache.");
    println!("  Zero konfliktów zależności — każdy projekt ma swoje wersje paczek.");
    println!("  Rust libs systemowe zawsze dostępne (niekopiowane).");
    println!();
    println!("  {}:", "Komendy".bright_yellow().bold());
    println!(
        "    {:<40} {}",
        "hl env create <nazwa>".bright_cyan(),
        "Utwórz nowe środowisko"
    );
    println!(
        "    {:<40} {}",
        "hl env create <pełna/ścieżka/nazwa>".bright_cyan(),
        "Utwórz w konkretnej lokalizacji"
    );
    println!(
        "    {:<40} {}",
        "hl env enter".bright_cyan(),
        "Wejdź do środowiska w bieżącym katalogu"
    );
    println!(
        "    {:<40} {}",
        "hl env enter <nazwa>".bright_cyan(),
        "Wejdź do środowiska po nazwie"
    );
    println!(
        "    {:<40} {}",
        "hl env enter <pełna/ścieżka>".bright_cyan(),
        "Wejdź do środowiska po ścieżce"
    );
    println!(
        "    {:<40} {}",
        "hl env exit".bright_cyan(),
        "Opuść środowisko (wróć do globalnego)"
    );
    println!(
        "    {:<40} {}",
        "hl env remove <nazwa>".bright_cyan(),
        "Usuń środowisko (z wszystkimi paczkami)"
    );
    println!(
        "    {:<40} {}",
        "hl env list".bright_cyan(),
        "Lista wszystkich środowisk"
    );
    println!(
        "    {:<40} {}",
        "hl env status".bright_cyan(),
        "Status aktywnego środowiska"
    );
    println!(
        "    {:<40} {}",
        "hl env help".bright_cyan(),
        "Ta wiadomość"
    );
    println!();
    println!("  {}:", "Jak działa izolacja".bright_yellow().bold());
    println!("    • bit install wewnątrz env → instaluje do env/libs/");
    println!("    • bit install poza env     → instaluje globalnie");
    println!("    • Każdy env ma własny bit.lock → zero konfliktów");
    println!("    • Stan zapisywany w config.hk → bit zawsze wie gdzie instalować");
    println!();
    println!("  {}:", "Config".bright_yellow().bold());
    println!(
        "    {}",
        format!("~/.config/hackeros/hacker-lang/config.hk").bright_black()
    );
    println!("    Sekcja [env] przechowuje aktywne środowisko.");
    println!("    bit i hl czytają tę sekcję automatycznie.");
    println!();
    println!("  {}:", "Przykład użycia".bright_yellow().bold());
    println!("    {}", "hl env create mojprojekt".bright_cyan());
    println!("    {}", "hl env enter mojprojekt".bright_cyan());
    println!("    {}    ← instaluje do mojprojekt/libs/", "bit install hashlib".bright_green());
    println!("    {}    ← instaluje do mojprojekt/libs/", "bit install httplib".bright_green());
    println!("    {}", "hl env exit".bright_cyan());
    println!("    {}   ← instaluje globalnie", "bit install hashlib".bright_black());
    print_env_hr();
}

// ── Subshell z ustawionymi zmiennymi ─────────────────────────────────────────

fn launch_env_shell(name: &str, env: &HlEnv) -> Result<()> {
    // Ustal shell
    let shell = std::env::var("SHELL")
        .unwrap_or_else(|_| "/bin/bash".to_string());

    // Prompt z nazwą środowiska
    let env_prompt = format!("(hl-env:{}) ", name);

    println!("  {} Uruchamiam shell: {}", "→".bright_cyan(), shell.bright_white());
    println!("  Wpisz {} aby opuścić środowisko.", "exit".bright_yellow());
    print_env_hr();
    println!();

    let _status = std::process::Command::new(&shell)
        .env("HL_ENV_NAME",    name)
        .env("HL_ENV_PATH",    env.path.to_str().unwrap_or(""))
        .env("HL_ENV_LIBS",    env.libs_dir.to_str().unwrap_or(""))
        .env("HL_ENV_LOCK",    env.lock_file.to_str().unwrap_or(""))
        .env("HL_ENV_CACHE",   env.cache_dir.to_str().unwrap_or(""))
        .env("HL_ENV_ACTIVE",  "1")
        .env("BIT_HOME",       env.libs_dir.to_str().unwrap_or(""))
        .env("BIT_LOCK_FILE",  env.lock_file.to_str().unwrap_or(""))
        .env("BIT_CACHE_DIR",  env.cache_dir.to_str().unwrap_or(""))
        .env("BIT_META_DIR",   env.meta_dir.to_str().unwrap_or(""))
        // Prompt z nazwą środowiska (bash/zsh)
        .env("PS1", format!("{}\\u@\\h:\\w\\$ ", env_prompt))
        .env("PROMPT", format!("{}%n@%m:%~%% ", env_prompt))
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    // Po wyjściu z shella wyczyść aktywne środowisko
    clear_active_env()?;

    println!();
    println!("{} Opuszczono środowisko '{}'.", "✓".green().bold(), name.bright_cyan());
    println!("  Wróciłeś do globalnego kontekstu bit.");

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Rozwiąż lokalizację środowiska z nazwy lub ścieżki
fn resolve_env_location(name_or_path: &str) -> (String, PathBuf) {
    let p = Path::new(name_or_path);
    if p.is_absolute() || name_or_path.contains('/') {
        let name = p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name_or_path)
            .to_string();
        (name, p.to_path_buf())
    } else {
        (name_or_path.to_string(), envs_base_dir().join(name_or_path))
    }
}

fn show_env_packages(env: &HlEnv) {
    if !env.lock_file.exists() {
        println!("  Paczki: (brak)");
        return;
    }
    let content = match std::fs::read_to_string(&env.lock_file) {
        Ok(c) => c,
        Err(_) => return,
    };
    let Ok(json): Result<serde_json::Value, _> = serde_json::from_str(&content) else {
        return;
    };
    let obj = match json.as_object() {
        Some(o) => o,
        None => return,
    };
    if obj.is_empty() {
        println!("  Paczki: {}", "(brak zainstalowanych)".bright_black());
        return;
    }
    println!("  Paczki: {} zainstalowanych", obj.len().to_string().bright_white().bold());
    let mut names: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
    names.sort();
    for name in names {
        let commit = obj[name].get("commit")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let typ = obj[name].get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("hl");
        println!(
            "    {} {} [{}]",
            "•".bright_black(),
            name.bright_white(),
            format!("{}@{}", typ, &commit[..commit.len().min(8)]).bright_black()
        );
    }
}

fn count_installed_pkgs(env: &HlEnv) -> usize {
    if !env.lock_file.exists() { return 0; }
    let content = std::fs::read_to_string(&env.lock_file).unwrap_or_default();
    let Ok(json): Result<serde_json::Value, _> = serde_json::from_str(&content) else {
        return 0;
    };
    json.as_object().map(|o| o.len()).unwrap_or(0)
}

fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    if path.is_file() {
        return Ok(path.metadata()?.len());
    }
    for entry in std::fs::read_dir(path)?.flatten() {
        let p = entry.path();
        if p.is_file() {
            total += p.metadata().map(|m| m.len()).unwrap_or(0);
        } else if p.is_dir() {
            total += dir_size(&p).unwrap_or(0);
        }
    }
    Ok(total)
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Prosta konwersja bez chrono — format ISO-like
    let days_since_epoch = secs / 86400;
    let year = 1970 + days_since_epoch / 365;
    format!("{}-created", year)
}

fn print_env_header(title: &str) {
    let hr = "─".repeat(56);
    println!("{}", hr.bright_black());
    println!("  {} {}", "hl".bright_magenta().bold(), title.bright_white().bold());
    println!("{}", hr.bright_black());
}

fn print_env_hr() {
    println!("{}", "─".repeat(56).bright_black());
}
