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
                dirs::home_dir().map(|p| p.display().to_string()).unwrap_or_else(|| "/".into())
            } else { rest.to_string() };
            match std_env::set_current_dir(&target) {
                Ok(_)  => BuiltinResult::Handled(0),
                Err(e) => { eprintln!("{}: {}", "cd error".red(), e); BuiltinResult::Handled(1) }
            }
        }
        "exit" | "quit" => { std::process::exit(rest.parse::<i32>().unwrap_or(0)); }
        "help"          => { print_help(); BuiltinResult::Handled(0) }
        "vars"          => {
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
        _               => BuiltinResult::NotBuiltin,
    }
}

fn print_help() {
    println!("{}", r#"
  Hacker Lang gen 2 — Referencia skladni

  ── GEN 1 ────────────────────────────────────────────────────
  PRINT:     ~> tekst          -- wypisz tekst (@var interpolacja)
  CMD:       >  komenda         -- uruchom komende
  SUDO:      ^> komenda         -- uruchom z sudo
  ISO:       -> komenda         -- izolacja namespace
  ISO+SU:    ^-> komenda        -- sudo + izolacja
  VARS:      >> komenda         -- komenda z @zmiennymi
  HSH:       *> komenda         -- uruchom przez hsh -c
  BG:        & komenda          -- uruchom w tle

  VAR:       % n = val          -- zmienna lokalna
  REF:       @nazwa             -- odwolanie do zmiennej
  EXPORT:    => n = val         -- export do srodowiska
  PETLA:     _N > cmd           -- powtorz N razy
  IMPORT:    << plik.hl         -- importuj plik .hl
  GOROUTINE: :* [nazwa] def     -- goroutine (blok + done)
  CHANNEL:   :** nazwa          -- zadeklaruj channel
  CHAN-OP:   *-- nazwa          -- wyslij/odbierz channel

  ── GEN 2 ────────────────────────────────────────────────────
  TYPED VAR: % n: int = 42      -- typowana zmienna (int/float/str/bool)
  ARITH:     $( expr ) -> @var  -- arytmetyka natywna
  PIPE:      > cmd |> @var      -- pipe wyniku do zmiennej
  FOR-IN:    @ item in lista    -- for-in loop (done)
  WHILE:     ?~ warunek         -- while loop (done)
  SWITCH:    ? switch @var      -- switch/case
             | "pattern"        -- case arm
             | *                -- wildcard
             done
  HACKEROS:  || narzedzie args  -- HackerOS API (hacker/hsh/lpm/...)

  ── WSPOLNE ──────────────────────────────────────────────────
  FUNC DEF:  : nazwa def ... done
  FUNC CALL: -- nazwa
  COND:      ? ok / ? err ... done
  IMPORT:    # <main/lib> / # <bit/lib> / # <github/u/r>
  DEP:       // narzedzie
  COMMENTS:  ;; linia  ///  doc  // blok \\

  BUILTINS:  cd, vars, funcs, help, clear, exit
"#.bright_white());
}
