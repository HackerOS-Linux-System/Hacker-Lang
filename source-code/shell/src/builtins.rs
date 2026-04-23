use colored::Colorize;
use hl_core::env::Env;
use std::env as std_env;

pub enum BuiltinResult { Handled(i32), NotBuiltin }

pub fn try_builtin(line: &str, env: &mut Env) -> BuiltinResult {
    let trimmed = line.trim();
    let mut parts = trimmed.splitn(2, ' ');
    let cmd  = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();

    match cmd {
        "cd" => {
            let target = if rest.is_empty() {
                dirs::home_dir().map(|p| p.display().to_string()).unwrap_or_else(|| "/".to_string())
            } else { rest.to_string() };
            match std_env::set_current_dir(&target) {
                Ok(_)  => BuiltinResult::Handled(0),
                Err(e) => { eprintln!("{}: {}", "cd error".red(), e); BuiltinResult::Handled(1) }
            }
        }
        "exit" | "quit" => { std::process::exit(rest.parse::<i32>().unwrap_or(0)); }
        "help" => { print_help(); BuiltinResult::Handled(0) }
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
            for name in names { println!("  {} {}()", ":".yellow(), name.bright_white()); }
            BuiltinResult::Handled(0)
        }
        "clear" | "cls" => { print!("\x1b[2J\x1b[H"); BuiltinResult::Handled(0) }
        _ => BuiltinResult::NotBuiltin,
    }
}

fn print_help() {
    println!("{}", r#"
  Hacker Lang v0.4 — Referencia skladni

  PRINT:   ~> tekst          -- wypisz tekst (interpolacja @var)
  CMD:     >  komenda         -- uruchom komende
  SUDO:    ^> komenda         -- uruchom z sudo
  ISO:     -> komenda         -- uruchom w izolacji (unshare)
  ISO+SU:  ^-> komenda        -- sudo + izolacja
  VARS:    >> komenda         -- komenda z interpolacja @zmiennych
  VAR+SU:  ^>> komenda        -- vars + sudo

  ZMIENNA: % nazwa = wartosc  -- zmienna lokalna
  REF:     @nazwa             -- odwolanie do zmiennej
  EXPORT:  => nazwa = wartosc -- export do srodowiska
  EXPORT:  => nazwa [         -- export listy wartosci
           | val1
           | val2
           ]

  FUNKCJA: : nazwa def        -- definicja funkcji
           ...
           done
  CALL:    -- nazwa           -- wywolanie funkcji

  WARUNEK: ? ok               -- jesli exit 0
           ? err              -- jesli exit != 0
           done

  KOMENTARZ: ;; tekst         -- liniowy
             /// tekst        -- dokumentacyjny

  DEP:     // narzedzie       -- deklaracja zaleznosci
  IMPORT:  # <std/net>        -- import biblioteki

  BUILTINS: cd, vars, funcs, help, clear, exit
"#.bright_white());
}
