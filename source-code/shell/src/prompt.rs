use colored::Colorize;
use std::env as std_env;

pub struct Prompt {
    pub show_git: bool,
}

impl Prompt {
    pub fn new() -> Self {
        Self { show_git: true }
    }

    fn current_dir_short() -> String {
        let cwd = std_env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".to_string());

        // Shorten home dir to ~
        if let Some(home) = dirs::home_dir() {
            let home_str = home.display().to_string();
            if cwd.starts_with(&home_str) {
                return format!("~{}", &cwd[home_str.len()..]);
            }
        }
        cwd
    }

    fn git_branch() -> Option<String> {
        let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() && branch != "HEAD" {
                return Some(branch);
            }
        }
        None
    }

    fn is_root() -> bool {
        #[cfg(unix)]
        {
            nix::unistd::geteuid().is_root()
        }
        #[cfg(not(unix))]
        false
    }

    pub fn render(&self, exit_code: i32) -> String {
        let dir = Self::current_dir_short();
        let root = Self::is_root();

        // Status indicator
        let status = if exit_code == 0 {
            "✓".green().bold()
        } else {
            format!("✗({})", exit_code).red().bold()
        };

        // Git branch
        let git_part = if self.show_git {
            Self::git_branch()
            .map(|b| format!(" \x1b[35m\x1b[0m {}", b.purple()))
            .unwrap_or_default()
        } else {
            String::new()
        };

        // Prompt char
        let prompt_char = if root {
            "#".red().bold().to_string()
        } else {
            "»".cyan().bold().to_string()
        };

        // User@host
        let user = std_env::var("USER").unwrap_or_else(|_| "hacker".to_string());
        let host = hostname();

        format!(
            "\n{} {}@{} {} {}{}\n{} ",
            status,
            user.bright_green().bold(),
                host.bright_cyan(),
                dir.bright_yellow().bold(),
                "hl".bright_magenta().bold(),
                git_part,
                prompt_char,
        )
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
    .unwrap_or_else(|_| "hackeros".to_string())
    .trim()
    .to_string()
}

impl Default for Prompt {
    fn default() -> Self {
        Self::new()
    }
}
