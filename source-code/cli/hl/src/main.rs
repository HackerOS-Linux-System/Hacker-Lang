use std::fs;
use std::path::Path;
use std::process::ExitCode;
use lexopt::prelude::*;
use lexopt::Parser;
use owo_colors::OwoColorize;
use owo_colors::colors::*;
use hl_compiler::compile_command;
use hl_runtime::run_command;

const VERSION: &str = "1.6.2";
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
					parser.value().unwrap().into_string().unwrap(),
				)
			}
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
			}
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
