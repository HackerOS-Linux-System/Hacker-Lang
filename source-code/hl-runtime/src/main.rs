mod ast;
mod bytecode;
mod cache;
mod compiler;
mod executor;
mod gc_ffi;
mod jit;
mod optimizer;
mod vm;

use ast::AnalysisResult;
use bytecode::{BytecodeProgram, OpCode};
use cache::{cache_clean_all, cache_load, cache_save, cache_size_bytes};
use compiler::compile_to_bytecode;
use gc_ffi::{full_gc, print_gc_stats, GcStats};
use vm::{get_plsa_path, VM};

use clap::Parser;
use colored::*;
use std::process::Command;
use std::time::Instant;

// ─────────────────────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────────────────────
#[derive(Parser, Debug)]
#[command(
author  = "HackerOS",
version = "1.7.5",
about   = "hacker-lang runtime v2 — generacyjny GC, bytecode VM, DynASM JIT, persistent shell session"
)]
struct Args {
    #[arg(required_unless_present_any = ["clean_cache", "cache_stats"])]
    file: Option<String>,
    #[arg(long, short)] verbose:     bool,
    #[arg(long)]        no_cache:    bool,
    #[arg(long)]        gc_stats:    bool,
    #[arg(long)]        dry_run:     bool,
    #[arg(long)]        disasm:      bool,
    #[arg(long)]        clean_cache: bool,
    #[arg(long)]        cache_stats: bool,
    #[arg(long)]        jit_stats:   bool,
    #[arg(long)]        exec_stats:  bool,
    #[arg(long)]        no_jit:      bool,
}

// ─────────────────────────────────────────────────────────────
// Generowanie bytecode przez hl-plsa
// ─────────────────────────────────────────────────────────────
fn generate_bytecode(file_path: &str, verbose: bool) -> BytecodeProgram {
    if verbose {
        eprintln!("{} Cache miss — analizuję: {}", "[*]".yellow(), file_path);
    }

    let out = Command::new(get_plsa_path())
    .args([file_path, "--json", "--resolve-libs"])
    .output()
    .unwrap_or_else(|e| {
        eprintln!("{} hl-plsa błąd uruchomienia: {}", "[x]".red(), e);
        std::process::exit(1);
    });

    if !out.status.success() {
        eprintln!(
            "{} hl-plsa błąd (exit {}):\n{}",
                  "[x]".red(),
                  out.status.code().unwrap_or(-1),
                  String::from_utf8_lossy(&out.stderr)
        );
        std::process::exit(1);
    }

    // AnalysisResult teraz z trójką functions: (bool, Option<String>, Vec<ProgramNode>)
    let ast: AnalysisResult = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        let preview = String::from_utf8_lossy(&out.stdout);
        let preview = &preview[..preview.len().min(512)];
        eprintln!("{} Nieprawidłowy JSON z PLSA: {}\n{}", "[x]".red(), e, preview);
        std::process::exit(1);
    });

    if verbose {
        eprintln!(
            "{} AST: {} funkcji, {} węzłów, {} deps",
            "[i]".blue(),
                  ast.functions.len(),
                  ast.main_body.len(),
                  ast.deps.len()
        );
        if ast.is_potentially_unsafe {
            eprintln!("{} Komendy sudo (^):", "[!]".yellow());
            for w in &ast.safety_warnings {
                eprintln!("    {}", w.yellow());
            }
        }

        // Pokaż funkcje z sygnaturami typów (nowe v9)
        let typed: Vec<_> = ast.functions.iter()
        .filter(|(_, (_, sig, _))| sig.is_some())
        .collect();
        if !typed.is_empty() {
            eprintln!("{} Funkcje z typami:", "[t]".green());
            for (name, (_, sig, _)) in &typed {
                eprintln!("    {} {}", name.cyan(), sig.as_deref().unwrap_or("").yellow());
            }
        }
    }

    compile_to_bytecode(&ast)
}

// ─────────────────────────────────────────────────────────────
// Disassembler — obsługuje wszystkie OpCode v6
// ─────────────────────────────────────────────────────────────
fn disassemble(prog: &BytecodeProgram) {
    let mut addr_to_func: std::collections::HashMap<usize, &str> =
    std::collections::HashMap::new();
    for (name, &addr) in &prog.functions {
        addr_to_func.insert(addr, name);
    }

    eprintln!(
        "{} Bytecode: {} ops, {} funkcji, {} strings w pool",
        "[dis]".cyan(),
              prog.ops.len(),
              prog.functions.len(),
              prog.pool.strings.len()
    );
    eprintln!("{}", "─".repeat(60).dimmed());

    for (i, op) in prog.ops.iter().enumerate() {
        if let Some(fname) = addr_to_func.get(&i) {
            eprintln!("\n{} .{}:", "fn".green().bold(), fname.yellow());
        }

        let prefix = format!("{:>5}:", i);
        match op {
            // ── ISTNIEJĄCE ────────────────────────────────────
            OpCode::Exec { cmd_id, sudo } => {
                eprintln!(
                    "{}  EXEC{} \"{}\"",
                    prefix.dimmed(),
                          if *sudo { " SUDO" } else { "" }.red(),
                              prog.str(*cmd_id).cyan()
                );
            }
            OpCode::JumpIfFalse { cond_id, target } => {
                eprintln!(
                    "{}  JIF  [[ {} ]]  → {}",
                    prefix.dimmed(),
                          prog.str(*cond_id).yellow(),
                          target.to_string().red()
                );
            }
            OpCode::Jump { target } => {
                eprintln!("{}  JMP  → {}", prefix.dimmed(), target.to_string().red());
            }
            OpCode::CallFunc { func_id } => {
                eprintln!("{}  CALL .{}", prefix.dimmed(), prog.str(*func_id).green());
            }
            OpCode::Return => { eprintln!("{}  RET", prefix.dimmed()); }
            OpCode::Exit(code) => {
                eprintln!("{}  EXIT {}", prefix.dimmed(), code.to_string().red());
            }
            OpCode::SetEnv { key_id, val_id } => {
                eprintln!(
                    "{}  SENV {} = \"{}\"",
                    prefix.dimmed(),
                          prog.str(*key_id).blue(),
                          prog.str(*val_id)
                );
            }
            OpCode::SetLocal { key_id, val_id, is_raw } => {
                eprintln!(
                    "{}  SLOC{} ${} = \"{}\"",
                    prefix.dimmed(),
                          if *is_raw { " RAW" } else { "" },
                              prog.str(*key_id).blue(),
                          prog.str(*val_id)
                );
            }
            OpCode::Plugin { name_id, args_id, sudo } => {
                eprintln!(
                    "{}  PLGN{} \\{} {}",
                    prefix.dimmed(),
                          if *sudo { " SUDO" } else { "" }.red(),
                              prog.str(*name_id).magenta(),
                          prog.str(*args_id)
                );
            }
            OpCode::Lock { key_id, val_id } => {
                eprintln!(
                    "{}  LOCK {} = {}",
                    prefix.dimmed(),
                          prog.str(*key_id).magenta(),
                          prog.str(*val_id)
                );
            }
            OpCode::Unlock { key_id } => {
                eprintln!("{}  ULCK {}", prefix.dimmed(), prog.str(*key_id).magenta());
            }
            OpCode::HotLoop { loop_ip } => {
                eprintln!("{}  HOTL loop_ip={}", prefix.dimmed(), loop_ip);
            }
            OpCode::Nop => { eprintln!("{}  NOP", prefix.dimmed()); }

            // ── NOWE v6 ───────────────────────────────────────
            OpCode::SetConst { key_id, val_id } => {
                eprintln!(
                    "{}  SCONST %{} = \"{}\"",
                    prefix.dimmed(),
                          prog.str(*key_id).yellow().bold(),
                          prog.str(*val_id)
                );
            }
            OpCode::SetOut { val_id } => {
                eprintln!(
                    "{}  OUT \"{}\"",
                    prefix.dimmed(),
                          prog.str(*val_id).cyan()
                );
            }
            OpCode::SpawnBg { cmd_id, sudo } => {
                eprintln!(
                    "{}  SPAWN{} {}",
                    prefix.dimmed(),
                          if *sudo { " SUDO" } else { "" }.red(),
                              prog.str(*cmd_id).blue()
                );
            }
            OpCode::SpawnAssign { key_id, cmd_id, sudo } => {
                eprintln!(
                    "{}  SPAWNA{} {} = spawn {}",
                    prefix.dimmed(),
                          if *sudo { " SUDO" } else { "" }.red(),
                              prog.str(*key_id).blue(),
                          prog.str(*cmd_id).cyan()
                );
            }
            OpCode::AwaitPid { expr_id } => {
                eprintln!(
                    "{}  AWAIT {}",
                    prefix.dimmed(),
                          prog.str(*expr_id).blue()
                );
            }
            OpCode::AwaitAssign { key_id, expr_id } => {
                eprintln!(
                    "{}  AWAITA {} = await {}",
                    prefix.dimmed(),
                          prog.str(*key_id).blue(),
                          prog.str(*expr_id).cyan()
                );
            }
            OpCode::Assert { cond_id, msg_id } => {
                let msg = msg_id.map(|id| prog.str(id)).unwrap_or("(brak komunikatu)");
                eprintln!(
                    "{}  ASSERT {} → \"{}\"",
                    prefix.dimmed(),
                          prog.str(*cond_id).yellow(),
                          msg.red()
                );
            }
            OpCode::MatchExec { case_cmd_id, sudo } => {
                let cmd = prog.str(*case_cmd_id);
                let preview = &cmd[..cmd.len().min(50)];
                eprintln!(
                    "{}  MATCH{} {}…",
                    prefix.dimmed(),
                          if *sudo { " SUDO" } else { "" }.red(),
                              preview.cyan()
                );
            }
            OpCode::PipeExec { cmd_id, sudo } => {
                eprintln!(
                    "{}  PIPE{} {}",
                    prefix.dimmed(),
                          if *sudo { " SUDO" } else { "" }.red(),
                              prog.str(*cmd_id).magenta()
                );
            }
        }
    }

    eprintln!("{}", "─".repeat(60).dimmed());
    eprintln!("\n{} String Pool:", "[pool]".cyan());
    for (i, s) in prog.pool.strings.iter().enumerate() {
        eprintln!("  {:>4}: {:?}", i, s);
    }
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────
fn main() {
    let args = Args::parse();

    if args.clean_cache {
        cache_clean_all(true);
        return;
    }

    if args.cache_stats {
        let bytes = cache_size_bytes();
        eprintln!(
            "{} Cache: {} KB w {}",
            "[cache]".cyan(),
                  bytes / 1024,
                  dirs::home_dir()
                  .unwrap_or_default()
                  .join(".cache/hacker-lang")
                  .display()
        );
        return;
    }

    let file = match &args.file {
        Some(f) => f.clone(),
        None => {
            eprintln!("{} Brak pliku .hl do wykonania", "[x]".red());
            std::process::exit(1);
        }
    };

    // ── Załaduj lub wygeneruj bytecode ────────────────────────
    let program: BytecodeProgram = if !args.no_cache {
        match cache_load(&file, args.verbose) {
            Some(p) => p,
            None => {
                let p = generate_bytecode(&file, args.verbose);
                cache_save(&file, &p, args.verbose);
                p
            }
        }
    } else {
        generate_bytecode(&file, args.verbose)
    };

    if args.disasm {
        disassemble(&program);
        return;
    }

    if args.dry_run {
        eprintln!(
            "{} Dry run: {} ops, {} funkcji, {} strings.",
            "[✓]".green(),
                  program.ops.len(),
                  program.functions.len(),
                  program.pool.strings.len()
        );
        if args.verbose { disassemble(&program); }
        return;
    }

    // ── Wykonaj program ───────────────────────────────────────
    let mut vm    = VM::new(args.verbose, false);
    let start     = Instant::now();
    let exit_code = vm.run(&program);
    let elapsed   = start.elapsed();

    full_gc();

    if args.verbose {
        eprintln!("{} Czas wykonania: {:?}", "[INFO]".blue(), elapsed);
    }

    if args.jit_stats {
        let compiled = vm.jit.compiled.len();
        eprintln!("{}", "━━━ JIT Statistics ━━━━━━━━━━━━━━━━━━━━━".magenta());
        eprintln!("  funkcje skompilowane : {}", compiled.to_string().yellow());
        for (func_id, _) in &vm.jit.compiled {
            eprintln!("    .{} → skompilowana", program.str(*func_id).green());
        }
        eprintln!(
            "  próg JIT (HOT_THRESHOLD): {}",
                  jit::HOT_THRESHOLD.to_string().cyan()
        );
        eprintln!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".magenta());
    }

    if args.exec_stats {
        vm.session.metrics.report();
    }

    if args.gc_stats || args.verbose {
        let stats = GcStats::collect();
        eprintln!("{}", "━━━ GC Statistics ━━━━━━━━━━━━━━━━━━━━━━".cyan());
        eprintln!("  allocs total : {}", stats.total_allocs.to_string().yellow());
        eprintln!("  minor GC     : {}", stats.minor_count.to_string().green());
        eprintln!("  major GC     : {}", stats.major_count.to_string().red());
        eprintln!("  promoted     : {}", stats.promoted.to_string().magenta());
        eprintln!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".cyan());
        if args.verbose { print_gc_stats(); }
    }

    std::process::exit(exit_code);
}
