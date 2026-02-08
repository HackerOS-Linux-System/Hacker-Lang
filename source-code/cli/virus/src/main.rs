use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use clap::{Parser, Subcommand};
use hk_parser::{HkConfig, load_hk_file, HkValue};
use hcl::from_str as hcl_from_str;
use hcl::Body;
use anyhow::{Context, Result};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use crossterm::event::{self, KeyCode};
use crossterm::{execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}};

#[derive(Parser)]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Build,
    Clean,
    Info,
    Docs,
    Run,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Build => build()?,
        Commands::Clean => clean()?,
        Commands::Info => info(),
        Commands::Docs => docs()?,
        Commands::Run => run()?,
    }

    Ok(())
}

fn load_config() -> Result<HkConfig> {
    if Path::new("Virus.hk").exists() {
        load_hk_file("Virus.hk").map_err(|e| anyhow::anyhow!("HK parse error: {}", e))
    } else if Path::new("Virus.hcl").exists() {
        let contents = fs::read_to_string("Virus.hcl")?;
        let body: Body = hcl_from_str(&contents)?;
        // Convert HCL Body to HkConfig (simplified)
        let mut config = HkConfig::new();
        for structure in body.into_iter() {
            if let hcl::Structure::Attribute(attr) = structure {
                config.insert(attr.key.to_string(), HkValue::String(attr.expr.to_string()));
            } else if let hcl::Structure::Block(block) = structure {
                let mut map = HkConfig::new();
                for sub in block.body.into_iter() {
                    if let hcl::Structure::Attribute(sub_attr) = sub {
                        map.insert(sub_attr.key.to_string(), HkValue::String(sub_attr.expr.to_string()));
                    }
                }
                config.insert(block.identifier.to_string(), HkValue::Map(map));
            }
        }
        Ok(config)
    } else {
        Ok(HkConfig::new())
    }
}

fn build() -> Result<()> {
    let _config = load_config()?;  // Use for versions, etc.

    // Assume src/main.hla
    let home = env::var("HOME")?;
    let transpiler_path = PathBuf::from(home).join(".hackeros/hacker-lang/bin/hla/hla-transpiler");

    let mut cmd = Command::new("python3");
    cmd.arg(transpiler_path)
       .arg("src/main.hla")
       .arg("build/main.rs")
       .stdout(Stdio::inherit())
       .stderr(Stdio::inherit());

    let status = cmd.status()?;
    if !status.success() {
        if Path::new("error.json").exists() {
            let errors_path = PathBuf::from(home).join(".hackeros/hacker-lang/bin/hla/hla-errors");
            Command::new(errors_path)
                .arg("error.json")
                .status()?;
        }
        return Err(anyhow::anyhow!("Transpile failed"));
    }

    // Compile with rustc or cargo
    // Assume simple rustc for now
    Command::new("rustc")
        .arg("-o")
        .arg("build/main")
        .arg("build/main.rs")
        .status()
        .context("Compile failed")?;

    println!("Build complete.");
    Ok(())
}

fn clean() -> Result<()> {
    fs::remove_dir_all(".cache").ok();
    fs::remove_dir_all("build").ok();
    println!("Cleaned.");
    Ok(())
}

fn info() {
    println!("Hacker Lang Advanced (formerly H#)");
    println!("A language transpiled to Rust with advanced memory modes.");
}

fn docs() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Simple TUI
    loop {
        terminal.draw(|f| {
            let size = f.area();
            // Render docs text, placeholder
            // Use ratatui widgets to show docs
        })?;

        if let event::Event::Key(key) = event::read()? {
            if key.code == KeyCode::Esc {
                break;
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn run() -> Result<()> {
    build()?;
    Command::new("./build/main")
        .status()
        .context("Run failed")?;
    Ok(())
}
