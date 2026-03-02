use std::path::PathBuf;

pub const PLSA_BIN_NAME: &str = "hl-plsa";

pub fn get_plsa_path() -> PathBuf {
    use colored::*;
    use std::process::exit;

    let home = dirs::home_dir().expect("HOME not set");
    let path = home
    .join(".hackeros/hacker-lang/bin")
    .join(PLSA_BIN_NAME);
    if !path.exists() {
        eprintln!(
            "{} Krytyczny błąd: {} nie znaleziony pod {:?}",
            "[x]".red(),
                  PLSA_BIN_NAME,
                  path
        );
        exit(127);
    }
    path
}

pub fn get_plugins_root() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/plugins")
}

pub fn get_libs_base() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/libs")
}

/// Domyślna nazwa wyjściowa: plik bez rozszerzenia
pub fn default_output(input: &str) -> String {
    let path = PathBuf::from(input);
    path.with_extension("")
    .to_str()
    .unwrap_or("a.out")
    .to_string()
}
