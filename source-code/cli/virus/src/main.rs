use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use clap::{Parser, Subcommand};
use anyhow::{Context, Result};
use hk_parser::{HkConfig, HkValue, load_hk_file, resolve_interpolations};
use hcl::{from_str as hcl_from_str, Body, Expression, ObjectKey, Structure};
use indexmap::IndexMap;

#[derive(Parser)]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start,
    Run { file: String },
    Compile { file: String },
    Build,
    Clean,
    Info,
    Docs,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start => start()?,
        Commands::Run { file } => run_file(&file)?,
        Commands::Compile { file } => compile_file(&file)?,
        Commands::Build => build()?,
        Commands::Clean => clean()?,
        Commands::Info => info(),
        Commands::Docs => docs()?,
    }
    Ok(())
}

fn load_config() -> Result<HkConfig> {
    let mut config: HkConfig;
    let mut is_hk = false;

    if Path::new("Virus.hk").exists() {
        config = load_hk_file("Virus.hk")
        .map_err(|e| anyhow::anyhow!("HK parse error: {}", e))?;
        is_hk = true;
    } else if Path::new("Virus.hcl").exists() {
        let contents = fs::read_to_string("Virus.hcl")?;
        let body: Body = hcl_from_str(&contents)?;
        config = body_to_hkconfig(&body)?;
    } else {
        config = HkConfig::new();
    }

    if is_hk {
        resolve_interpolations(&mut config)
        .map_err(|e| anyhow::anyhow!("Interpolation error: {}", e))?;
    }

    generate_cargo_toml(&config)?;
    Ok(config)
}

fn body_to_hkconfig(body: &Body) -> Result<HkConfig> {
    let mut config = IndexMap::new();

    for structure in body.iter() {
        match structure {
            Structure::Attribute(attr) => {
                let key = attr.key().to_string();
                let value = hcl_to_hkvalue(attr.expr())?;
                config.insert(key, value);
            }
            Structure::Block(block) => {
                let key = block.identifier().to_string();
                let sub_config = body_to_hkconfig(block.body())?;
                config.insert(key, HkValue::Map(sub_config));
            }
        }
    }
    Ok(config)
}

fn hcl_to_hkvalue(expr: &Expression) -> Result<HkValue> {
    match expr {
        Expression::String(s) => Ok(HkValue::String(s.clone())),
        Expression::Bool(b) => Ok(HkValue::Bool(*b)),
        Expression::Number(n) => Ok(HkValue::Number(n.as_f64().unwrap_or(0.0))),
        Expression::Array(a) => {
            let vec: Vec<HkValue> = a
            .iter()
            .map(hcl_to_hkvalue)
            .collect::<Result<_, _>>()?;
            Ok(HkValue::Array(vec))
        }
        Expression::Object(o) => {
            let mut map = IndexMap::new();
            for (key, val) in o.iter() {
                let key_str = match key {
                    ObjectKey::Identifier(id) => id.to_string(),
                    ObjectKey::Expression(e) => {
                        if let Expression::String(s) = e {
                            s.clone()
                        } else {
                            format!("{:?}", e)
                        }
                    }
                    _ => format!("{:?}", key),
                };
                map.insert(key_str, hcl_to_hkvalue(val)?);
            }
            Ok(HkValue::Map(map))
        }
        Expression::Null => Ok(HkValue::String("null".to_string())),
        Expression::Variable(v) => Err(anyhow::anyhow!("Variables not supported: {:?}", v)),
        _ => Err(anyhow::anyhow!("Unsupported expression type: {:?}", expr)),
    }
}

fn generate_cargo_toml(config: &HkConfig) -> Result<()> {
    let empty_map = IndexMap::new();
    let metadata = config
    .get("metadata")
    .and_then(|v| v.as_map().ok())
    .unwrap_or(&empty_map);

    let name = metadata
    .get("name")
    .and_then(|v| v.as_string().ok())
    .unwrap_or_else(|| "project".to_string());

    let version_str = metadata
    .get("version")
    .and_then(|v| {
        if let Ok(n) = v.as_number() {
            Some(n.to_string())
        } else if let Ok(s) = v.as_string() {
            Some(s)
        } else {
            None
        }
    })
    .unwrap_or_else(|| "0.1.0".to_string());

    let edition = metadata
    .get("edition")
    .and_then(|v| v.as_string().ok())
    .unwrap_or_else(|| "2021".to_string());

    let mut toml = format!(
        "[package]\nname = \"{name}\"\nversion = \"{version_str}\"\nedition = \"{edition}\"\n\n[dependencies]\n"
    );

    if let Some(deps_val) = config.get("dependencies") {
        if let Ok(deps) = deps_val.as_map() {
            for (dep_name, dep_config) in deps {
                match dep_config {
                    HkValue::String(ver) => {
                        toml.push_str(&format!("{dep_name} = \"{ver}\"\n"));
                    }
                    HkValue::Map(m) => {
                        toml.push_str(&format!("{dep_name} = {{ "));
                        let mut parts: Vec<String> = Vec::new();
                        if let Some(ver) = m.get("version").and_then(|v| {
                            if let Ok(s) = v.as_string() {
                                Some(s)
                            } else if let Ok(n) = v.as_number() {
                                Some(n.to_string())
                            } else {
                                None
                            }
                        }) {
                            parts.push(format!("version = \"{ver}\""));
                        }
                        if let Some(features) = m.get("features").and_then(|f| f.as_array().ok()) {
                            let feats: Vec<String> = features
                            .iter()
                            .map(|f| {
                                if let HkValue::String(s) = f {
                                    format!("\"{s}\"")
                                } else {
                                    format!("{:?}", f)
                                }
                            })
                            .collect();
                            parts.push(format!("features = [{}]", feats.join(", ")));
                        }
                        toml.push_str(&parts.join(", "));
                        toml.push_str(" }\n");
                    }
                    _ => {}
                }
            }
        }
    }
    fs::write("Cargo.toml", toml)?;
    Ok(())
}

fn start() -> Result<()> {
    println!("virus start not implemented yet.");
    Ok(())
}

fn run_file(_file: &str) -> Result<()> {
    println!("virus run not implemented yet.");
    Ok(())
}

fn compile_file(file: &str) -> Result<()> {
    if !Path::new("Virus.hk").exists() && !Path::new("Virus.hcl").exists() {
        // Single file mode
        let home = env::var("HOME")?;
        let transpiler_path = PathBuf::from(home.clone())
        .join(".hackeros/hacker-lang/bin/hla/hla-transpiler");
        let mut cmd = Command::new("python3");
        cmd.arg(transpiler_path)
        .arg(file)
        .arg("out.rs")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
        let status = cmd.status()?;
        if !status.success() {
            if Path::new("error.json").exists() {
                let errors_path = PathBuf::from(home)
                .join(".hackeros/hacker-lang/bin/hla/hla-errors");
                Command::new(errors_path)
                .arg("error.json")
                .status()?;
            }
            return Err(anyhow::anyhow!("Transpile failed"));
        }
        Command::new("rustc").arg("out.rs").status()?;
        println!("Compiled single file.");
    } else {
        build()?;
    }
    Ok(())
}

fn build() -> Result<()> {
    let _config = load_config()?;
    let home = env::var("HOME")?;
    let transpiler_path = PathBuf::from(home.clone())
    .join(".hackeros/hacker-lang/bin/hla/hla-transpiler");
    let mut cmd = Command::new("python3");
    cmd.arg(transpiler_path)
    .arg("src/main.hla")
    .arg("build/main.rs")
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit());
    let status = cmd.status()?;
    if !status.success() {
        if Path::new("error.json").exists() {
            let errors_path = PathBuf::from(home)
            .join(".hackeros/hacker-lang/bin/hla/hla-errors");
            Command::new(errors_path)
            .arg("error.json")
            .status()?;
        }
        return Err(anyhow::anyhow!("Transpile failed"));
    }
    if Path::new("Cargo.toml").exists() {
        Command::new("cargo")
        .arg("build")
        .status()
        .context("Cargo build failed")?;
    } else {
        Command::new("rustc")
        .arg("-o")
        .arg("build/main")
        .arg("build/main.rs")
        .status()
        .context("Compile failed")?;
    }
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
    Command::new("hla-docs").status()?;
    Ok(())
}
