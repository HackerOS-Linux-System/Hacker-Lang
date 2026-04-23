use colored::Colorize;
use std::env as std_env;

pub struct Prompt { pub show_git: bool }

impl Prompt {
    pub fn new() -> Self { Self { show_git: true } }

    fn current_dir_short() -> String {
        let cwd = std_env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "?".into());
        if let Some(home) = dirs::home_dir() {
            let hs = home.display().to_string();
            if cwd.starts_with(&hs) { return format!("~{}", &cwd[hs.len()..]); }
        }
        cwd
    }

    fn git_branch() -> Option<String> {
        let out = std::process::Command::new("git").args(["rev-parse","--abbrev-ref","HEAD"]).output().ok()?;
        if out.status.success() {
            let b = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !b.is_empty() && b != "HEAD" { return Some(b); }
        }
        None
    }

    fn is_root() -> bool {
        #[cfg(unix)] { nix::unistd::geteuid().is_root() }
        #[cfg(not(unix))] { false }
    }

    pub fn render(&self, exit_code: i32) -> String {
        let dir    = Self::current_dir_short();
        let status = if exit_code == 0 { "✓".green().bold() } else { format!("✗({})", exit_code).red().bold() };
        let git_part = if self.show_git {
            Self::git_branch().map(|b| format!(" \x1b[35m\x1b[0m {}", b.purple())).unwrap_or_default()
        } else { String::new() };
        let prompt_char = if Self::is_root() { "#".red().bold().to_string() } else { "»".cyan().bold().to_string() };
        let user = std_env::var("USER").unwrap_or_else(|_| "hacker".into());
        let host = std::fs::read_to_string("/etc/hostname").unwrap_or_else(|_| "hackeros".into()).trim().to_string();
        format!("\n{} {}@{} {} {}{}\n{} ",
            status,
            user.bright_green().bold(), host.bright_cyan(),
            dir.bright_yellow().bold(),
            "hl".bright_magenta().bold(), git_part,
            prompt_char)
    }
}

impl Default for Prompt { fn default() -> Self { Self::new() } }
