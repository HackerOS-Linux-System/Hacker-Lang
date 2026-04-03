use colored::Colorize;
use hl_core::env::Env;
use std::env as std_env;

pub enum BuiltinResult {
    Handled(i32),
    NotBuiltin,
}

/// Try to execute a line as a shell builtin
/// Returns Handled if it was a builtin, NotBuiltin if we should fall through to HL/exec
pub fn try_builtin(line: &str, env: &mut Env) -> BuiltinResult {
    let trimmed = line.trim();
    let mut parts = trimmed.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();

    match cmd {
        "cd" => {
            let target = if rest.is_empty() {
                dirs::home_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "/".to_string())
            } else {
                rest.to_string()
            };
            match std_env::set_current_dir(&target) {
                Ok(_) => BuiltinResult::Handled(0),
                Err(e) => {
                    eprintln!("{}: {}", "cd error".red(), e);
                    BuiltinResult::Handled(1)
                }
            }
        }

        "exit" | "quit" => {
            let code = rest.parse::<i32>().unwrap_or(0);
            std::process::exit(code);
        }

        "help" => {
            print_help();
            BuiltinResult::Handled(0)
        }

        "vars" => {
            println!("{}", "=== Hacker Lang Variables ===".cyan().bold());
            let mut names: Vec<&String> = env.vars.keys().collect();
            names.sort();
            for name in names {
                let val = env.get_var(name);
                println!("  {} {} = {}", "%".yellow(), name.bright_white(), val.to_string_val().green());
            }
            BuiltinResult::Handled(0)
        }

        "funcs" => {
            println!("{}", "=== Defined Functions ===".cyan().bold());
            let mut names: Vec<&String> = env.functions.keys().collect();
            names.sort();
            for name in names {
                println!("  {} {}()", ":".yellow(), name.bright_white());
            }
            BuiltinResult::Handled(0)
        }

        "clear" | "cls" => {
            print!("\x1b[2J\x1b[H");
            BuiltinResult::Handled(0)
        }

        "source" => {
            // source a .hl file — handled in shell main loop
            BuiltinResult::NotBuiltin
        }

        _ => BuiltinResult::NotBuiltin,
    }
}

fn print_help() {
    println!(
        r#"
        {}  v{}

        {}
        {}     Print message (with @var interpolation)
    {}      Run command (echo is FORBIDDEN, use ::)
    {}     Run command with sudo
    {}      Run in isolated namespace
    {}    Run isolated + sudo
    {}     Run command with @var interpolation
    {}    Run with vars + sudo

    {}
    {}       Declare variable: % name = value
    {}       Reference variable: @name

    {}
    {}       Declare function: : name def ... done
    {}      Call function: -- name

    {}
    {}     If last command succeeded (exit 0)
    {}    If last command failed (exit != 0)
    {}    Close a block

    {}
    {}       Line comment
    {}      Documentation comment
    {}  Block comment

    {}
    {}      Declare system dependency (auto-installs if missing)

    {}
    {}    Change directory
    {}    List variables
    {}   List functions
    {}      Print this help
    {}      Exit shell
    "#,
    "Hacker Lang Shell".bright_cyan().bold(),
             "0.1.0",
             "COMMANDS:".bright_yellow().bold(),
             ":: msg".bright_green(),
             ">  cmd".bright_blue(),
             "^> cmd".bright_blue(),
             "-> cmd".bright_magenta(),
             "^-> cmd".bright_magenta(),
             ">> cmd".bright_blue(),
             "^>> cmd".bright_blue(),
             "VARIABLES:".bright_yellow().bold(),
             "%  name = val".bright_yellow(),
             "@name".bright_yellow(),
             "FUNCTIONS:".bright_yellow().bold(),
             ": name def".bright_cyan(),
             "-- name".bright_cyan(),
             "LOGIC:".bright_yellow().bold(),
             "? ok".bright_green(),
             "? err".bright_red(),
             "done".bright_white(),
             "COMMENTS:".bright_yellow().bold(),
             ";; text".bright_black(),
             "/// text".bright_black(),
             "// text \\\\".bright_black(),
             "DEPENDENCIES:".bright_yellow().bold(),
             "// pkg".bright_cyan(),
             "BUILTINS:".bright_yellow().bold(),
             "cd [dir]".bright_white(),
             "vars".bright_white(),
             "funcs".bright_white(),
             "help".bright_white(),
             "exit [n]".bright_white(),
    );
}
