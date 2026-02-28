use std::fs;
use std::path::Path;
use std::process::ExitCode;
use lexopt::prelude::*;
use lexopt::Parser;
use owo_colors::OwoColorize;
use owo_colors::colors::*;
use hl_transpiler::compile_command;
use hl_transpiler::run_command;
const VERSION: &str = "1.7.0";
const HACKER_DIR: &str = ".hackeros/hacker-lang";
const BIN_DIR: &str = "bin";
fn main() -> ExitCode {
	if let Err(e) = ensure_hacker_dir() {
		eprintln!(
			"{} {}",
			"Failed to create hacker directory:".red().bold(),
				  e
		);
		return ExitCode::from(1);
	}
	let mut parser = Parser::from_env();
	let mut command: Option<String> = None;
	let mut file: Option<String> = None;
	let mut output: Option<String> = None;
	let mut verbose = false;
	let mut show_version = false;
	let mut show_help = false;
	while let Some(arg) = parser.next().unwrap_or(None) {
		match arg {
			Short('v') | Long("version") => show_version = true,
			Short('h') | Long("help") => show_help = true,
			Short('o') | Long("output") => {
				output = Some(
					parser.value().unwrap().into_string().unwrap()
				)
			},
			Long("verbose") => verbose = true,
			Value(val) => {
				let s = val.into_string().unwrap();
				if command.is_none() {
					command = Some(s);
				} else if file.is_none() {
					file = Some(s);
				} else {
					eprintln!(
						"{} {}",
			   "ERROR: Unexpected argument:".red().bold(),
							  s
					);
					show_help = true;
				}
			},
			_ => {}
		}
	}
	if show_version {
		version_command();
		return ExitCode::SUCCESS;
	}
	if show_help || command.is_none() {
		if command.is_none() {
			display_welcome();
		} else {
			help_command(true);
		}
		return ExitCode::SUCCESS;
	}
	let command = command.unwrap();
	let success = match command.as_str() {
		"run" => {
			if let Some(f) = file {
				let verbose_suffix = if verbose { " (verbose)" } else { "" };
				println!(
					"{} {}{}",
			 "INFO: Executing:".cyan().bold(),
						 f,
			 verbose_suffix
				);
				let ok = run_command(f, verbose);
				if ok {
					println!("{}", "SUCCESS: Execution complete.".green().bold());
				} else {
					println!("{}", "ERROR: Execution failed.".red().bold());
				}
				ok
			} else {
				eprintln!("{}", "ERROR: Expected a <file> argument.".red().bold());
				println!("Usage: hl run <file> [--verbose]");
				false
			}
		}
		"compile" => {
			if let Some(f) = file {
				let out = output.unwrap_or_else(|| {
					// Strip extension to get default output name
					if let Some(pos) = f.rfind('.') {
						f[..pos].to_string()
					} else {
						f.clone()
					}
				});
				println!(
					"{} {} → {}",
					"INFO: Compiling:".cyan().bold(),
						 f,
						 out
				);
				let ok = compile_command(f, out, verbose);
				if ok {
					println!("{}", "SUCCESS: Compilation complete.".green().bold());
				} else {
					println!("{}", "ERROR: Compilation failed.".red().bold());
				}
				ok
			} else {
				eprintln!("{}", "ERROR: Expected a <file> argument.".red().bold());
				println!("Usage: hl compile <file> [-o output] [--verbose]");
				false
			}
		}
		"check" => {
			if let Some(f) = file {
				let verbose_suffix = if verbose { " (verbose)" } else { "" };
				println!(
					"{} {}{}",
			 "INFO: Checking syntax:".cyan().bold(),
						 f,
			 verbose_suffix
				);
				let home = match std::env::var("HOME") {
					Ok(h) => h,
					Err(_) => {
						eprintln!("{}", "ERROR: HOME not set.".red().bold());
						return ExitCode::from(1);
					}
				};
				let bin_path = Path::new(&home).join(HACKER_DIR).join(BIN_DIR);
				if !bin_path.exists() {
					if let Err(e) = fs::create_dir_all(&bin_path) {
						eprintln!("{} {}", "Failed to create bin dir:".red().bold(), e);
						return ExitCode::from(1);
					}
				}
				let temp_out = bin_path.join("check_temp");
				let out_str = temp_out.to_str().unwrap().to_string();
				let ok = compile_command(f, out_str.clone(), verbose);
				if ok {
					println!("{}", "SUCCESS: Syntax is correct.".green().bold());
					let _ = fs::remove_file(&temp_out);
				} else {
					println!("{}", "ERROR: Syntax check failed.".red().bold());
					let _ = fs::remove_file(&temp_out);
				}
				ok
			} else {
				eprintln!("{}", "ERROR: Expected a <file> argument.".red().bold());
				println!("Usage: hl check <file> [--verbose]");
				false
			}
		}
		"clean" => {
			if file.is_some() || output.is_some() {
				eprintln!("{}", "ERROR: clean command takes no additional arguments.".red().bold());
				println!("Usage: hl clean [--verbose]");
				false
			} else {
				println!("{}", "INFO: Cleaning cache directory.".cyan().bold());
				let home = match std::env::var("HOME") {
					Ok(h) => h,
					Err(_) => {
						eprintln!("{}", "ERROR: HOME not set.".red().bold());
						return ExitCode::from(1);
					}
				};
				let cache_path = Path::new(&home).join(".cache").join("hacker-lang");
				if !cache_path.exists() {
					println!("{}", "INFO: Cache directory does not exist, nothing to clean.".cyan().bold());
					true
				} else {
					match fs::remove_dir_all(&cache_path) {
						Ok(_) => {
							if let Err(e) = fs::create_dir_all(&cache_path) {
								eprintln!("{} {}", "Warning: Failed to recreate cache dir:".red().bold(), e);
							}
							println!("{}", "SUCCESS: Cache cleaned.".green().bold());
							true
						}
						Err(e) => {
							eprintln!("{} {}", "ERROR: Failed to remove cache:".red().bold(), e);
							false
						}
					}
				}
			}
		}
		"help" => {
			help_command(true);
			true
		}
		other => {
			eprintln!(
				"{} {}",
			 "ERROR: Unknown command:".red().bold(),
					  other
			);
			help_command(false);
			false
		}
	};
	if success {
		ExitCode::SUCCESS
	} else {
		ExitCode::from(1)
	}
}
fn ensure_hacker_dir() -> std::io::Result<()> {
	let home = std::env::var("HOME")
	.map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))?;
	fs::create_dir_all(Path::new(&home).join(HACKER_DIR).join(BIN_DIR))
}
fn display_welcome() {
	println!(
		"{}",
		"┌──────────────────────────────────────────────┐"
		.bold()
		.fg::<White>()
		.on_bright_magenta()
	);
	println!(
		"{}",
		format!("│ Welcome to Hacker Lang CLI v{} │", VERSION)
			.bold()
			.fg::<White>()
			.on_bright_magenta()
	);
	println!(
		"{}",
		"└──────────────────────────────────────────────┘"
		.bold()
		.fg::<White>()
		.on_bright_magenta()
	);
	println!(
		"{}",
		"Simplified tool for running and compiling .hl scripts"
		.cyan()
		.bold()
	);
	println!("{}", "Type 'hl help' for available commands.".white());
	println!();
	help_command(false);
}
fn help_command(show_banner: bool) -> bool {
	if show_banner {
		println!(
			"{}",
		   "┌──────────────────────────────────────────────┐"
		   .bold()
		   .fg::<White>()
		   .on_bright_magenta()
		);
		println!(
			"{}",
		   "│ Hacker Lang CLI — Scripting Tool │"
		   .bold()
		   .fg::<White>()
		   .on_bright_magenta()
		);
		println!(
			"{}",
		   format!("│ v{} │", VERSION)
			   .bold()
			   .fg::<White>()
			   .on_bright_magenta()
		);
		println!(
			"{}",
		   "└──────────────────────────────────────────────┘"
		   .bold()
		   .fg::<White>()
		   .on_bright_magenta()
		);
		println!();
	}
	println!("{}", "Available Commands:".blue().bold());
	println!(
		"{}",
		"─────────────────────────────────────────────────────────".bright_black()
	);
	println!(
		"{:<12} {:<32} {:<40}",
		"Command".dimmed(),
			 "Description".dimmed(),
			 "Usage".dimmed()
	);
	println!(
		"{:<12} {:<32} {:<40}",
		"run".cyan(),
			 "Execute a .hl script",
		  "hl run <file> [--verbose]".yellow()
	);
	println!(
		"{:<12} {:<32} {:<40}",
		"compile".cyan(),
			 "Compile to native executable",
		  "hl compile <file> [-o out] [--verbose]".yellow()
	);
	println!(
		"{:<12} {:<32} {:<40}",
		"check".cyan(),
			 "Check syntax correctness",
		  "hl check <file> [--verbose]".yellow()
	);
	println!(
		"{:<12} {:<32} {:<40}",
		"clean".cyan(),
			 "Remove contents of cache directory",
		  "hl clean".yellow()
	);
	println!(
		"{:<12} {:<32} {:<40}",
		"help".cyan(),
			 "Show this help",
		  "hl help".yellow()
	);
	println!(
		"{}",
		"─────────────────────────────────────────────────────────".bright_black()
	);
	println!();
	println!("{}", "Global options:".bright_black().bold());
	println!("{}", "-v, --version Print version".magenta());
	println!("{}", "-h, --help Print help".magenta());
	true
}
fn version_command() {
	println!(
		"{} {}",
		"Hacker Lang CLI v".cyan().bold(),
			 VERSION
	);
}
