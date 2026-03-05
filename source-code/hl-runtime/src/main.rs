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
author  = "HackerOS Team <hackeros068@gmail.com>",
version = "1.8.0",
about   = "hacker-lang runtime v1.8.0 — native numeric types, generacyjny GC, bytecode VM, DynASM JIT"
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
// Disassembler
// ─────────────────────────────────────────────────────────────
fn disassemble(prog: &BytecodeProgram) {
    let mut addr_to_func: std::collections::HashMap<usize, &str> =
    std::collections::HashMap::new();
    for (name, &addr) in &prog.functions {
        addr_to_func.insert(addr, name);
    }

    eprintln!(
        "{} Bytecode v{}: {} ops, {} funkcji, {} strings w pool",
        "[dis]".cyan(),
              prog.schema_version,
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
            OpCode::SetConst { key_id, val_id } => {
                eprintln!(
                    "{}  SCONST %{} = \"{}\"",
                    prefix.dimmed(),
                          prog.str(*key_id).yellow().bold(),
                          prog.str(*val_id)
                );
            }
            OpCode::SetOut { val_id } => {
                eprintln!("{}  OUT \"{}\"", prefix.dimmed(), prog.str(*val_id).cyan());
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
                eprintln!("{}  AWAIT {}", prefix.dimmed(), prog.str(*expr_id).blue());
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
                let cmd     = prog.str(*case_cmd_id);
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
            // ── v7: NUMERYCZNE ────────────────────────────────
            OpCode::LoadInt { dst, val } => {
                eprintln!("{}  LDI   r{} = {}", prefix.dimmed(), dst, val.to_string().green());
            }
            OpCode::LoadFloat { dst, val } => {
                eprintln!("{}  LDF   r{} = {}", prefix.dimmed(), dst, val.to_string().green());
            }
            OpCode::LoadBool { dst, val } => {
                eprintln!("{}  LDB   r{} = {}", prefix.dimmed(), dst, val.to_string().green());
            }
            OpCode::LoadStr { dst, str_id } => {
                eprintln!(
                    "{}  LDS   r{} = \"{}\"",
                    prefix.dimmed(), dst,
                          prog.str(*str_id).green()
                );
            }
            OpCode::LoadVarI { dst, var_id } => {
                eprintln!(
                    "{}  LDVI  r{} = ${}",
                    prefix.dimmed(), dst,
                          prog.str(*var_id).blue()
                );
            }
            OpCode::LoadVarF { dst, var_id } => {
                eprintln!(
                    "{}  LDVF  r{} = ${}",
                    prefix.dimmed(), dst,
                          prog.str(*var_id).blue()
                );
            }
            OpCode::StoreVarI { var_id, src } => {
                eprintln!(
                    "{}  STVI  ${} = r{}",
                    prefix.dimmed(),
                          prog.str(*var_id).blue(), src
                );
            }
            OpCode::StoreVarF { var_id, src } => {
                eprintln!(
                    "{}  STVF  ${} = r{}",
                    prefix.dimmed(),
                          prog.str(*var_id).blue(), src
                );
            }
            OpCode::AddI { dst, a, b } => {
                eprintln!("{}  ADDI  r{} = r{} + r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::SubI { dst, a, b } => {
                eprintln!("{}  SUBI  r{} = r{} - r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::MulI { dst, a, b } => {
                eprintln!("{}  MULI  r{} = r{} * r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::DivI { dst, a, b } => {
                eprintln!("{}  DIVI  r{} = r{} / r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::ModI { dst, a, b } => {
                eprintln!("{}  MODI  r{} = r{} % r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::NegI { dst, src } => {
                eprintln!("{}  NEGI  r{} = -r{}", prefix.dimmed(), dst, src);
            }
            OpCode::AddF { dst, a, b } => {
                eprintln!("{}  ADDF  r{} = r{} + r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::SubF { dst, a, b } => {
                eprintln!("{}  SUBF  r{} = r{} - r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::MulF { dst, a, b } => {
                eprintln!("{}  MULF  r{} = r{} * r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::DivF { dst, a, b } => {
                eprintln!("{}  DIVF  r{} = r{} / r{}", prefix.dimmed(), dst, a, b);
            }
            OpCode::NegF { dst, src } => {
                eprintln!("{}  NEGF  r{} = -r{}", prefix.dimmed(), dst, src);
            }
            OpCode::CmpI { a, b, op } => {
                eprintln!(
                    "{}  CMPI  r{} {} r{}  [→ flag]",
                    prefix.dimmed(), a, op.as_str().yellow(), b
                );
            }
            OpCode::CmpF { a, b, op } => {
                eprintln!(
                    "{}  CMPF  r{} {} r{}  [→ flag]",
                    prefix.dimmed(), a, op.as_str().yellow(), b
                );
            }
            OpCode::JumpIfTrue { target } => {
                eprintln!(
                    "{}  JIFT  flag → {}",
                    prefix.dimmed(),
                          target.to_string().red()
                );
            }
            OpCode::NumForExec { var_id, start, end, step, cmd_id, sudo } => {
                eprintln!(
                    "{}  NUMFOR{} ${} {}..{} step {} > \"{}\"",
                    prefix.dimmed(),
                          if *sudo { " SUDO" } else { "" }.red(),
                              prog.str(*var_id).cyan(),
                          start.to_string().green(),
                          end.to_string().green(),
                          step.to_string().yellow(),
                          prog.str(*cmd_id)
                );
            }
            OpCode::WhileExprExec { lhs_reg, op, rhs_reg, cmd_id, sudo } => {
                eprintln!(
                    "{}  WHILEE{} r{} {} r{} > \"{}\"",
                    prefix.dimmed(),
                          if *sudo { " SUDO" } else { "" }.red(),
                              lhs_reg,
                          op.as_str().yellow(),
                          rhs_reg,
                          prog.str(*cmd_id)
                );
            }
            OpCode::IntToFloat { dst, src } => {
                eprintln!("{}  I2F   r{} = (float)r{}", prefix.dimmed(), dst, src);
            }
            OpCode::FloatToInt { dst, src } => {
                eprintln!("{}  F2I   r{} = (int)r{}", prefix.dimmed(), dst, src);
            }
            OpCode::IntToStr { var_id, src } => {
                eprintln!(
                    "{}  I2S   ${} = str(r{})",
                          prefix.dimmed(),
                          prog.str(*var_id).blue(), src
                );
            }
            OpCode::FloatToStr { var_id, src } => {
                eprintln!(
                    "{}  F2S   ${} = str(r{})",
                          prefix.dimmed(),
                          prog.str(*var_id).blue(), src
                );
            }
            OpCode::ReturnI { src } => {
                eprintln!("{}  RETI  r{} → _HL_OUT", prefix.dimmed(), src);
            }
            OpCode::ReturnF { src } => {
                eprintln!("{}  RETF  r{} → _HL_OUT", prefix.dimmed(), src);
            }
            // ── ARENA ─────────────────────────────────────────
            OpCode::ArenaEnter { name_id, size_id } => {
                eprintln!(
                    "{}  ARENA ENTER :: {} [{}]",
                    prefix.dimmed(),
                          prog.str(*name_id).magenta().bold(),
                          prog.str(*size_id).yellow()
                );
            }
            OpCode::ArenaExit => {
                eprintln!("{}  ARENA EXIT", prefix.dimmed());
            }
            OpCode::ArenaAlloc { var_id, n_bytes } => {
                eprintln!(
                    "{}  ARENA ALLOC ${} {}B",
                    prefix.dimmed(),
                          prog.str(*var_id).blue(),
                          n_bytes.to_string().yellow()
                );
            }
            OpCode::ArenaReset => {
                eprintln!("{}  ARENA RESET", prefix.dimmed());
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
// Zlicz typy opcode
// ─────────────────────────────────────────────────────────────
fn count_opcodes(prog: &BytecodeProgram) {
    let mut exec_count    = 0usize;
    let mut numeric_count = 0usize;
    let mut numfor_count  = 0usize;
    let mut jit_count     = 0usize;
    let mut arena_count   = 0usize;

    for op in &prog.ops {
        match op {
            OpCode::Exec { .. } | OpCode::MatchExec { .. } | OpCode::PipeExec { .. } => {
                exec_count += 1;
            }
            OpCode::LoadInt { .. } | OpCode::LoadFloat { .. } | OpCode::LoadBool { .. }
            | OpCode::AddI { .. } | OpCode::SubI { .. } | OpCode::MulI { .. }
            | OpCode::DivI { .. } | OpCode::ModI { .. } | OpCode::NegI { .. }
            | OpCode::AddF { .. } | OpCode::SubF { .. } | OpCode::MulF { .. }
            | OpCode::DivF { .. } | OpCode::NegF { .. }
            | OpCode::CmpI { .. } | OpCode::CmpF { .. }
            | OpCode::StoreVarI { .. } | OpCode::StoreVarF { .. }
            | OpCode::IntToFloat { .. } | OpCode::FloatToInt { .. } => {
                numeric_count += 1;
            }
            OpCode::NumForExec { .. } | OpCode::WhileExprExec { .. } => {
                numfor_count += 1;
            }
            OpCode::CallFunc { .. } => {
                jit_count += 1;
            }
            OpCode::ArenaEnter { .. }
            | OpCode::ArenaExit
            | OpCode::ArenaAlloc { .. }
            | OpCode::ArenaReset => {
                arena_count += 1;
            }
            _ => {}
        }
    }

    if exec_count + numeric_count + numfor_count + arena_count > 0 {
        eprintln!("{} Profil opcode:", "[dis]".cyan());
        eprintln!("    shell exec    : {}", exec_count.to_string().yellow());
        eprintln!("    native numeric: {}", numeric_count.to_string().green());
        eprintln!("    native loops  : {}", numfor_count.to_string().cyan());
        eprintln!("    hl calls      : {}", jit_count.to_string().magenta());
        eprintln!("    arena ops     : {}", arena_count.to_string().blue());
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
        count_opcodes(&program);
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

        if !vm.typed_vars.is_empty() {
            eprintln!("{} Typed vars po wykonaniu ({}):", "[n]".green(), vm.typed_vars.len());
            let mut sorted: Vec<_> = vm.typed_vars.iter().collect();
            sorted.sort_by_key(|(k, _)| k.as_str());
            for (name, val) in sorted {
                eprintln!("    ${} = {:?}", name.cyan(), val);
            }
        }

        // Wyniki testów jednostkowych
        if vm.test_passed + vm.test_failed > 0 {
            eprintln!("{}", "━━━ Test Results ━━━━━━━━━━━━━━━━━━━━━━━".green());
            eprintln!("  passed : {}", vm.test_passed.to_string().green());
            eprintln!("  failed : {}", vm.test_failed.to_string().red());
            eprintln!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".green());
        }
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
