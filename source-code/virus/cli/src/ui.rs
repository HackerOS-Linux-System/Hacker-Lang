use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────
// Banner
// ─────────────────────────────────────────────────────────────
pub fn banner() {
    println!();
    println!(
        "  {}{}{}",
        "▓▒░ ".bright_red().dimmed(),
             " virus ".on_black().bright_cyan().bold(),
             " ░▒▓".bright_red().dimmed()
    );
    println!(
        "  {}",
        format!("  hacker-lang package manager v{}", env!("CARGO_PKG_VERSION"))
            .bright_black()
    );
    println!();
}

// ─────────────────────────────────────────────────────────────
// Box message — ramka wokół komunikatu
// ─────────────────────────────────────────────────────────────
pub fn box_msg(msg: &str) {
    let lines: Vec<&str> = msg.lines().collect();
    let max_len = lines.iter().map(|l| visible_len(l)).max().unwrap_or(0);
    let width = max_len.max(40);

    println!(
        "  {}{}{}",
        "╭".bright_cyan(),
             "─".repeat(width + 2).bright_cyan(),
             "╮".bright_cyan()
    );
    for line in &lines {
        let pad = width - visible_len(line);
        println!(
            "  {}  {}{}  {}",
            "│".bright_cyan(),
                 line,
                 " ".repeat(pad),
                 "│".bright_cyan()
        );
    }
    println!(
        "  {}{}{}",
        "╰".bright_cyan(),
             "─".repeat(width + 2).bright_cyan(),
             "╯".bright_cyan()
    );
    println!();
}

// ─────────────────────────────────────────────────────────────
// Progress bar — [=======>.................................................]  42%
//
// Format koloru w indicatif: "kolor_wypełnienia/kolor_tła"
//   magenta = fioletowy
//   white   = białe kropki widoczne na każdym tle
//
// WAŻNE: {bar:55.magenta/white} — oba kolory muszą być podane
//   bez tego tło jest niewidoczne (dziedziczy kolor terminala)
// ─────────────────────────────────────────────────────────────
pub fn progress_bar(len: u64, label: &str) -> ProgressBar {
    let pb = ProgressBar::new(len);

    pb.set_style(
        ProgressStyle::with_template(
            &format!(
                "  {{spinner:.magenta}} {label} [{{bar:55.magenta/white}}] {{pos:>3}}/{{len:3}}  {{percent:>3}}%  {{msg}}",
                label = label.bright_white().bold()
            )
        )
        .unwrap()
        // = wypełnienie, > głowica, . tło (białe przez /white powyżej)
        .progress_chars("=>.")
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
    );

    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

// ─────────────────────────────────────────────────────────────
// Download bar
// ─────────────────────────────────────────────────────────────
pub fn download_bar(total_bytes: u64, label: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);

    pb.set_style(
        ProgressStyle::with_template(
            &format!(
                "  {{spinner:.magenta}} {label} [{{bar:40.magenta/white}}] {{bytes:>9}}/{{total_bytes:>9}}  {{binary_bytes_per_sec:>13}}  eta {{eta:>3}}s",
                label = label.bright_white().bold()
            )
        )
        .unwrap()
        .progress_chars("=>.")
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
    );

    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

// ─────────────────────────────────────────────────────────────
// Spinner — nieznany rozmiar
// ─────────────────────────────────────────────────────────────
pub fn spinner(label: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();

    pb.set_style(
        ProgressStyle::with_template("  {spinner:.magenta}  {msg}")
        .unwrap()
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
    );

    pb.set_message(label.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

// ─────────────────────────────────────────────────────────────
// Komunikaty krokowe
// ─────────────────────────────────────────────────────────────
pub fn step_ok(msg: &str) {
    println!("  {} {}", "✓".bright_green().bold(), msg);
}

pub fn step_err(msg: &str) {
    eprintln!("  {} {}", "✗".bright_red().bold(), msg.bright_red());
}

pub fn step_warn(msg: &str) {
    println!("  {} {}", "!".bright_yellow().bold(), msg.bright_yellow());
}

pub fn step_info(msg: &str) {
    println!("  {} {}", "→".bright_blue(), msg.dimmed());
}

// ─────────────────────────────────────────────────────────────
// Oblicz widzialną długość stringa — pomija kody ANSI \x1b[...m
// ─────────────────────────────────────────────────────────────
fn visible_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut len = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
        } else if bytes[i] < 0x80 {
            len += 1;
        } else if bytes[i] >= 0xC0 {
            len += 1;
            while i + 1 < bytes.len() && bytes[i + 1] & 0xC0 == 0x80 {
                i += 1;
            }
        }
        i += 1;
    }
    len
}
