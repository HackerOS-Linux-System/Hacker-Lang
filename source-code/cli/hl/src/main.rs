use std::fs;
use std::path::Path;
use std::process::ExitCode;

use lexopt::{Arg, Parser};
use owo_colors::OwoColorize;
use owo_colors::colors::*;

// Fix: Import run_command from hl_runtime (it was added in lib.rs)
// Note: hl_compiler::compile_command is not defined in the provided lib.rs for hl-compiler.
// I need to check hl-compiler/src/lib.rs.
// The provided hl-compiler/src/lib.rs has a main() function but no public compile_command.
// It seems hl-compiler is designed as a binary (it has main), but here it is used as a library.
// However, the user provided code for hl-compiler/src/lib.rs which has `fn main()`.
// This implies hl-compiler is a binary crate, or the user copy-pasted a binary source into lib.rs.
// If it is a library, it should expose a function.
// Given the user asked for "fixed code", and the `hl` CLI calls `hl_compiler::compile_command`,
// I should probably expose `compile_command` in `hl-compiler/src/lib.rs` instead of `main`.
// Or `hl` CLI should spawn the `hl-compiler` binary.
// But `hl/Cargo.toml` has `hl-compiler = { path = "../hl-compiler" }`, implying library usage.
// So I will modify `hl-compiler/src/lib.rs` to expose `compile_command` and rename `main` to it, or similar.
// Same for `hl-runtime`. `hl-runtime` has `run_command` which I added in the previous step.

use hl_runtime::run_command;

// I will assume I need to fix hl-compiler to expose compile_command.
// I will add a placeholder here and then fix hl-compiler.

const VERSION: &str = "1.6.2";
const HACKER_DIR: &str = ".hackeros/hacker-lang";
const BIN_DIR: &str = "bin";

fn main() -> ExitCode {
	if let Err(e) = ensure_hacker_dir() {
		eprintln!("{} {}", "Failed to create hacker directory:".red().bold(), e);
		return ExitCode::from(1);
	}

	let mut parser = Parser::from_env();
	let mut command: Option<String> = None;
	let mut file: Option<String> = None;
	let mut output: Option<String> = None;
	let mut verbose = false;
	let mut show_version = false;
	let mut show_help = false;

	while let Some(arg) = parser.next() {
		match arg {
			Ok(Arg::Short('v') | Arg::Long("version")) => show_version = true,
			Ok(Arg::Short('h') | Arg::Long("help")) => show_help = true,
			Ok(Arg::Short('o') | Arg::Long("output")) => output = Some(parser.value().unwrap().into_string().unwrap()),
			Ok(Arg::Long("verbose")) => verbose = true,
			Ok(Arg::Value(val)) => {
				if command.is_none() {
					command = Some(val.into_string().unwrap());
				} else if file.is_none() {
					file = Some(val.into_string().unwrap());
				} else {
					eprintln!("{} {}", "ERROR: Unexpected argument:".red().bold(), val.to_string_lossy());
					show_help = true;
				}
			}
			Err(e) => {
				eprintln!("{} {}", "ERROR:".red().bold(), e);
				show_help = true;
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
				let verbose_str = if verbose { " (verbose mode)" } else { "" };
				println!("{} {} {}", "INFO: Executing script:".cyan().bold(), f, verbose_str);
				let ok = run_command(f, verbose);
				if ok {
					println!("{}", "SUCCESS: Execution completed successfully.".green().bold());
				} else {
					println!("{}", "ERROR: Execution failed.".red().bold());
				}
				ok
			} else {
				eprintln!("{}", "ERROR: Expected exactly one argument: <file>".red().bold());
				println!("Usage: hl run <file> [options]");
				println!(" --verbose Enable verbose output");
				false
			}
		}
		"compile" => {
			if let Some(f) = file {
				let out = output.unwrap_or_else(|| {
					if let Some(pos) = f.rfind('.') {
						f[..pos].to_string()
					} else {
						f.clone()
					}
				});
				let verbose_str = if verbose { " (verbose mode)" } else { "" };
				println!("{} {} {} {}", "INFO: Compiling script:".cyan().bold(), f, "to", out + verbose_str);
				// Fix: call compile_command from hl_compiler
				let ok = hl_compiler::compile_command(f, out, verbose);
				if ok {
					println!("{}", "SUCCESS: Compilation completed successfully.".green().bold());
				} else {
					println!("{}", "ERROR: Compilation failed.".red().bold());
				}
				ok
			} else {
				eprintln!("{}", "ERROR: Expected exactly one argument: <file>".red().bold());
				println!("Usage: hl compile <file> [options]");
				println!(" -o, --output string Specify output file");
				println!(" --verbose Enable verbose output");
				false
			}
		}
		"help" => help_command(true),
		_ => {
			eprintln!("{} {}", "ERROR: Unknown command:".red().bold(), command);
			help_command(false);
			false
		}
	};

	if success { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

fn ensure_hacker_dir() -> std::io::Result<()> {
	if let Ok(home) = std::env::var("HOME") {
		let full_bin_dir = Path::new(&home).join(HACKER_DIR).join(BIN_DIR);
		fs::create_dir_all(full_bin_dir)
	} else {
		Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Failed to get HOME environment variable"))
	}
}

fn display_welcome() {
	println!("{}", "┌──────────────────────────────────────────────┐".bold().fg::<White>().on_bright_magenta());
	println!("{}", format!("│ Welcome to Hacker Lang CLI v{}               │", VERSION).bold().fg::<White>().on_bright_magenta());
	println!("{}", "└──────────────────────────────────────────────┘".bold().fg::<White>().on_bright_magenta());
	println!("{}", "Simplified tool for running and compiling .hacker scripts".cyan().bold());
	println!("{}", "Type 'hl help' for available commands.".white());
	println!();
	help_command(false);
}

fn help_command(show_banner: bool) -> bool {
	if show_banner {
		println!("{}", "┌──────────────────────────────────────────────┐".bold().fg::<White>().on_bright_magenta());
		println!("{}", "│ Hacker Lang CLI - Simplified Scripting Tool  │".bold().fg::<White>().on_bright_magenta());
		println!("{}", format!("│ v{}                                   │", VERSION).bold().fg::<White>().on_bright_magenta());
		println!("{}", "└──────────────────────────────────────────────┘".bold().fg::<White>().on_bright_magenta());
		println!();
	}
	println!("{}", "Available Commands:".blue().bold());
	println!("{}", "─────────────────────────────────────────────────────────────".bright_black());
	println!("{:<10} {:<30} {:<40}", "Command".dimmed(), "Description".dimmed(), "Usage".dimmed());
	println!("{:<10} {:<30} {:<40}", "run".cyan(), "Execute a .hacker script".white(), "hl run <file> [--verbose]".yellow());
	println!("{:<10} {:<30} {:<40}", "compile".cyan(), "Compile to native executable".white(), "hl compile <file> [-o output] [--verbose]".yellow());
	println!("{:<10} {:<30} {:<40}", "help".cyan(), "Show this help menu".white(), "hl help".yellow());
	println!("{}", "─────────────────────────────────────────────────────────────".bright_black());
	println!();
	println!("{}", "Global options:".bright_black().bold());
	println!("{}", "-v, --version Display version".magenta());
	println!("{}", "-h, --help Display help".magenta());
	true
}

fn version_command() {
	println!("{} {}", "INFO: Hacker Lang CLI v".cyan().bold(), VERSION);
}
