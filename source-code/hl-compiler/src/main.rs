mod args;
mod ast;
mod codegen;
mod codegen_emit;
mod ir;
mod linker;
mod passes;
mod paths;
mod plsa;

use colored::*;
use inkwell::context::Context;
use inkwell::targets::{InitializationConfig, Target};
use std::process::exit;
use args::Args;
use clap::Parser;

fn main() {
    let args = Args::parse();
    let output_name = args
    .output
    .clone()
    .unwrap_or_else(|| paths::default_output(&args.file));

    // ── 1. Analiza przez hl-plsa ──────────────────────────────
    let ast = plsa::run_plsa(&args.file, args.verbose);

    if args.verbose {
        eprintln!(
            "{} AST: {} funkcji {} węzłów {} libs",
            "[i]".blue(),
                  ast.functions.len(),
                  ast.main_body.len(),
                  ast.libs.len()
        );
        if ast.is_potentially_unsafe {
            eprintln!("{} Skrypt zawiera sudo (^):", "[!]".yellow());
            for w in &ast.safety_warnings {
                eprintln!(" {}", w.yellow());
            }
        }
    }

    // ── 2. LLVM init ──────────────────────────────────────────
    Target::initialize_native(&InitializationConfig::default()).unwrap_or_else(|e| {
        eprintln!("{} LLVM init nieudana: {}", "[x]".red(), e);
        exit(1);
    });

    // ── 3. IR lowering ────────────────────────────────────────
    if args.verbose {
        eprintln!("{} Obniżam AST → IR...", "[*]".cyan());
    }
    let ir_module = ir::IrBuilder::new(args.verbose).lower(&ast);

    // ── 4. Codegen ────────────────────────────────────────────
    if args.verbose {
        eprintln!("{} Generuję LLVM IR...", "[*]".green());
    }
    let context = Context::create();
    let mut cg = codegen::Codegen::new(&context, args.verbose);
    cg.emit_module(&ir_module);

    // Zbierz extern_libs z ciał funkcji AST
    for (_, (_, _, nodes)) in &ast.functions {
        for node in nodes {
            if let ast::CommandType::Extern { path, static_link } = &node.content {
                cg.extern_libs.push((path.clone(), *static_link));
            }
        }
    }
    // Zbierz extern_libs z main_body AST
    for node in &ast.main_body {
        if let ast::CommandType::Extern { path, static_link } = &node.content {
            cg.extern_libs.push((path.clone(), *static_link));
        }
    }

    // ── 5. Weryfikacja IR ─────────────────────────────────────
    if let Err(e) = cg.module.verify() {
        eprintln!("{} Błąd weryfikacji IR:\n{}", "[x]".red(), e);
        if args.verbose || args.emit_ir {
            cg.module.print_to_stderr();
        }
        exit(1);
    }

    // ── 6. Optymalizacje LLVM pass pipeline ───────────────────
    let tm = passes::build_target_machine(args.opt, args.pie, args.verbose);
    passes::run_passes(&cg.module, &tm, args.opt, args.verbose);

    // ── 7. Emituj .ll (opcjonalnie) ───────────────────────────
    if args.emit_ir {
        let ll = format!("{}.ll", output_name);
        cg.module
        .print_to_file(std::path::Path::new(&ll))
        .ok();
        eprintln!("{} IR: {}", "[*]".green(), ll);
    }

    // ── 8. Emituj .o ─────────────────────────────────────────
    use inkwell::targets::FileType;
    let obj_path = format!("{}.o", output_name);
    tm.write_to_file(&cg.module, FileType::Object, std::path::Path::new(&obj_path))
    .unwrap_or_else(|e| {
        eprintln!("{} Błąd zapisu .o: {}", "[x]".red(), e);
        exit(1);
    });

    if args.verbose {
        eprintln!("{} Obiekt: {}", "[*]".green(), obj_path);
    }

    if args.emit_obj {
        eprintln!("{} Gotowy: {}", "[+]".green(), obj_path);
        return;
    }

    // ── 9. Linkowanie ─────────────────────────────────────────
    linker::link(&obj_path, &output_name, &ast, args.pie, args.verbose);
}
