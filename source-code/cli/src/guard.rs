use colored::Colorize;

pub const HL_SCRIPTS_DIR: &str = "/usr/share/HackerOS/Scripts/Bin";
pub const HL_MAIN_LIBS_DIR: &str = "/usr/lib/HackerOS/Hacker-Lang/main-libs";

/// Zmienna środowiskowa pozwalająca ominąć guard w środowisku deweloperskim/CI
/// bez konieczności podszywania się pod cały HackerOS. Guard produkcyjny
/// (domyślne zachowanie, bez tej zmiennej) pozostaje identyczny jak wcześniej.
const DEV_BYPASS_ENV: &str = "HL_NOT_ON_HACKEROS";

pub fn check_hackeros_only() {
    if std::env::var(DEV_BYPASS_ENV).ok().as_deref() == Some("YES") {
        return;
    }
    if !std::path::Path::new("/usr/share/HackerOS/").exists() { die_not_hackeros(); }
    if !std::path::Path::new("/usr/lib/HackerOS/").exists()   { die_not_hackeros(); }
    if !std::path::Path::new("/usr/bin/hacker").exists()      { die_not_hackeros(); }
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    if !os.lines().any(|l| l == r#"NAME="HackerOS""#) { die_not_hackeros(); }
}

#[cold]
#[inline(never)]
fn die_not_hackeros() -> ! {
    eprintln!("{} {}", "hl:".bright_magenta().bold(),
              "Hacker Lang działa wyłącznie na HackerOS.".white().bold());
    eprintln!("    {}", "https://github.com/HackerOS-Linux-System".bright_black());
    eprintln!(
        "    {} {}",
        "dev/test:".bright_black(),
        "uruchom HL_NOT_ON_HACKEROS=YES".bright_black()
    );
    std::process::exit(1);
}
