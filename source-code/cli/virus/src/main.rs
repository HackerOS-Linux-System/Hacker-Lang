use std::env;
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::os::unix::fs::PermissionsExt; // For setting executable permissions
use clap::{Parser, Subcommand};
use hk_parser::{HkConfig, load_hk_file, HkValue};
use hcl::from_str as hcl_from_str;
use hcl::Body;
use anyhow::{Context, Result, anyhow};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use crossterm::event::{self, KeyCode};
use crossterm::{execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}};
use flate2::read::GzDecoder;
use tar::Archive;

const VENV_PATH: &str = "/usr/lib/Hacker-Lang/venv";
const VENV_PYTHON: &str = "/usr/lib/Hacker-Lang/venv/bin/python3";
const VENV_PIP: &str = "/usr/lib/Hacker-Lang/venv/bin/pip";
const DOWNLOAD_URL: &str = "https://github.com/HackerOS-Linux-System/Hacker-Lang/releases/download/v1.6.3/hla.tar.gz";

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
    Init,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Build => build()?,
        Commands::Clean => clean()?,
        Commands::Info => info(),
        Commands::Docs => docs()?,
        Commands::Run => run()?,
        Commands::Init => init()?,
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

fn init() -> Result<()> {
    println!("Initializing Hacker Lang Environment...");

    // 1. Setup Virtual Environment
    if !Path::new(VENV_PATH).exists() {
        println!("Creating virtual environment at {}...", VENV_PATH);
        // Ensure parent directory exists (requires sudo/root usually)
        if let Some(parent) = Path::new(VENV_PATH).parent() {
            fs::create_dir_all(parent).context("Failed to create directory structure. Run with sudo?")?;
        }
        
        let status = Command::new("python3")
            .arg("-m")
            .arg("venv")
            .arg(VENV_PATH)
            .status()
            .context("Failed to create python venv")?;
        
        if !status.success() {
            return Err(anyhow!("Failed to create virtual environment."));
        }

        // Install dependencies needed for transpiler (e.g., lark)
        println!("Installing python dependencies...");
        let pip_status = Command::new(VENV_PIP)
            .args(["install", "lark"])
            .status()
            .context("Failed to install pip dependencies")?;
        
        if !pip_status.success() {
            return Err(anyhow!("Failed to install dependencies via pip."));
        }
    } else {
        println!("Virtual environment already exists.");
    }

    // 2. Download and Extract Binaries
    println!("Downloading release from {}...", DOWNLOAD_URL);
    let response = reqwest::blocking::get(DOWNLOAD_URL)
        .context("Failed to download HLA release")?;
    
    let bytes = response.bytes().context("Failed to read response bytes")?;
    let cursor = Cursor::new(bytes);
    let tar = GzDecoder::new(cursor);
    let mut archive = Archive::new(tar);

    println!("Extracting to /tmp/...");
    archive.unpack("/tmp/").context("Failed to unpack archive to /tmp/")?;

    // 3. Move binaries to User Home
    let home = env::var("HOME")?;
    let hla_bin_path = PathBuf::from(&home).join(".hackeros/hacker-lang/bin/hla/");
    
    println!("Setting up {}...", hla_bin_path.display());
    fs::create_dir_all(&hla_bin_path).context("Failed to create user bin directory")?;

    let files_to_move = vec!["hla-transpiler", "hla-errors"];
    
    for filename in files_to_move {
        let src = PathBuf::from("/tmp/").join(filename);
        let dest = hla_bin_path.join(filename);

        if src.exists() {
            // Copy instead of rename to avoid cross-device link errors
            fs::copy(&src, &dest).context(format!("Failed to copy {}", filename))?;
            
            // Set executable permission
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
            
            println!("Installed {}", filename);
        } else {
            eprintln!("Warning: {} not found in extracted archive.", filename);
        }
    }

    println!("Initialization complete.");
    Ok(())
}

fn check_environment() -> Result<(String, PathBuf, PathBuf)> {
    // Check for venv python
    if !Path::new(VENV_PYTHON).exists() {
        return Err(anyhow!("Environment missing. Please use 'virus init'"));
    }

    let home = env::var("HOME")?;
    let transpiler_path = PathBuf::from(&home).join(".hackeros/hacker-lang/bin/hla/hla-transpiler");
    let errors_path = PathBuf::from(&home).join(".hackeros/hacker-lang/bin/hla/hla-errors");

    if !transpiler_path.exists() || !errors_path.exists() {
        return Err(anyhow!("HLA binaries missing. Please use 'virus init'"));
    }

    Ok((VENV_PYTHON.to_string(), transpiler_path, errors_path))
}

fn build() -> Result<()> {
    // Validate environment first
    let (python_path, transpiler_path, errors_path) = check_environment()?;
    
    let _config = load_config()?;

    // Use python from venv to run the transpiler
    let mut cmd = Command::new(python_path);
    cmd.arg(transpiler_path)
       .arg("src/main.hla")
       .arg("build/main.rs")
       .stdout(Stdio::inherit())
       .stderr(Stdio::inherit());

    let status = cmd.status().context("Failed to execute transpiler")?;
    
    if !status.success() {
        if Path::new("error.json").exists() {
            Command::new(errors_path)
                .arg("error.json")
                .status()?;
        }
        return Err(anyhow!("Transpile failed"));
    }

    // Compile with rustc
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
            let _size = f.area();
            // Render docs text, placeholder
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
