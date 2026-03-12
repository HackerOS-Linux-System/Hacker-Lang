use colored::*;
use miette::Diagnostic;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::ast::{AnalysisResult, CommandType, FuncAttr, ParseError, Visibility};
use crate::lib_resolver::plugins_root;

// ─────────────────────────────────────────────────────────────
// Błędy
// ─────────────────────────────────────────────────────────────
pub fn print_errors(errors: &[ParseError], file: &str) {
    let total = errors.len();
    eprintln!(
        "\n{} {} {} {}\n",
        "✗".red().bold(),
              total.to_string().red().bold(),
              if total == 1 { "błąd składni" } else { "błędów składni" },
                  format!("w {}", file).dimmed(),
    );
    let handler = miette::GraphicalReportHandler::new()
    .with_context_lines(2)
    .with_cause_chain();
    for e in errors.iter().take(20) {
        let mut out = String::new();
        let _ = handler.render_report(&mut out, e as &dyn Diagnostic);
        eprint!("{}", out);
    }
    if total > 20 {
        eprintln!(
            "  {} ... i {} więcej (pokazano pierwsze 20)\n",
                  "~".yellow(),
                  (total - 20).to_string().yellow()
        );
    }

    // ── Wskazówki — deduplikowane, bez URL (URL raz na końcu) ──
    let mut seen_adv = std::collections::HashSet::new();
    let unique: Vec<&str> = errors
    .iter()
    .filter_map(|e| match e {
        ParseError::SyntaxError { advice, .. } => Some(advice.as_str()),
                _ => None,
    })
    .filter(|a| seen_adv.insert(*a))
    .collect();
    if !unique.is_empty() {
        eprintln!("{}", "━━━ Wskazówki ━━━━━━━━━━━━━━━━━━━━━━━━━━".yellow());
        for a in &unique {
            // advice już nie zawiera URL — wypisz bezpośrednio
            eprintln!("  {} {}", "→".yellow(), a);
        }
        eprintln!(
            "  {} dokumentacja: {}",
            "→".dimmed(),
                  "https://hackeros-linux-system.github.io/HackerOS-Website/hacker-lang/docs.html".dimmed()
        );
        eprintln!();
    }
}

// ─────────────────────────────────────────────────────────────
// Pluginy
// ─────────────────────────────────────────────────────────────
pub fn check_plugins(res: &AnalysisResult) {
    let root = plugins_root();
    let nodes: Vec<_> = res
    .main_body
    .iter()
    .chain(res.functions.values().flat_map(|m| m.body.iter()))
    .filter(|n| matches!(&n.content, CommandType::Plugin { .. }))
    .collect();
    if nodes.is_empty() { return; }
    eprintln!("{}", "━━━ Pluginy ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".cyan());
    for node in nodes {
        if let CommandType::Plugin { name, args, is_super } = &node.content {
            let bin    = root.join(name);
            let hl     = PathBuf::from(format!("{}.hl", bin.display()));
            let exists = bin.exists() || hl.exists();
            let sym    = if exists { "✓".green() } else { "✗".red() };
            let stag   = if *is_super { " [sudo]".yellow().to_string() } else { String::new() };
            let atag   = if !args.is_empty() { format!(" {}", args.dimmed()) } else { String::new() };
            eprintln!("  {} linia {:>4} — \\\\{}{}{}", sym, node.line_num, name.cyan(), atag, stag);
            if !exists {
                eprintln!("           {} brak: {}", "→".yellow(), bin.display().to_string().yellow());
            }
        }
    }
    eprintln!();
}

// ─────────────────────────────────────────────────────────────
// Helper: all nodes (main + all functions)
// ─────────────────────────────────────────────────────────────
fn all_nodes(res: &AnalysisResult) -> impl Iterator<Item = &crate::ast::ProgramNode> {
    res.main_body
    .iter()
    .chain(res.functions.values().flat_map(|m| m.body.iter()))
}

// ─────────────────────────────────────────────────────────────
// Verbose summary
// ─────────────────────────────────────────────────────────────
pub fn print_summary(res: &AnalysisResult) {
    eprintln!("{}", "═══════════════════════════════════════════".cyan());
    eprintln!("{}", "  hacker-lang PLSA".cyan().bold());
    eprintln!("{}", "═══════════════════════════════════════════".cyan());
    eprintln!("  Funkcje    : {}", res.functions.len().to_string().yellow());
    eprintln!("  Moduły     : {}", res.modules.len().to_string().yellow());
    eprintln!("  Main nodes : {}", res.main_body.len().to_string().yellow());
    eprintln!("  Sys deps   : {}", res.deps.len().to_string().yellow());

    // ── Biblioteki pogrupowane po typie ───────────────────────
    let mut by_type: HashMap<&str, Vec<String>> = HashMap::new();
    for lib in &res.libs {
        let mut label = match &lib.version {
            Some(v) => format!("{}:{}", lib.name, v),
            None    => lib.name.clone(),
        };
        if let Some(syms) = &lib.use_symbols {
            label = format!("{} use [{}]", label, syms.join(", "));
        }
        by_type.entry(lib.lib_type.as_str()).or_default().push(label);
    }
    let mut types: Vec<&str> = by_type.keys().copied().collect();
    types.sort();
    for t in types {
        eprintln!("  lib/{:<8}: {}", t.magenta(), by_type[t].join(", "));
    }

    // ── Moduły ────────────────────────────────────────────────
    if !res.modules.is_empty() {
        eprintln!("\n{} Moduły (=;):", "[M]".cyan().bold());
        let mut mods: Vec<_> = res.modules.iter().collect();
        mods.sort_by_key(|(n, _)| n.as_str());
        for (name, meta) in &mods {
            let vis = if meta.visibility == Visibility::Public { "pub ".green().to_string() } else { String::new() };
            eprintln!("    {}{}", vis, name.cyan().bold());
            if !meta.submodules.is_empty() {
                eprintln!("      sub: {}", meta.submodules.join(", ").yellow());
            }
            if !meta.functions.is_empty() {
                eprintln!("      fn:  {}", meta.functions.join(", ").blue());
            }
        }
    }

    // ── Funkcje z widocznością i atrybutami ───────────────────
    let pub_fns: Vec<_> = res.functions.iter()
    .filter(|(_, m)| m.visibility == Visibility::Public)
    .collect();
    let attr_fns: Vec<_> = res.functions.iter()
    .filter(|(_, m)| !m.attrs.is_empty())
    .collect();

    if !pub_fns.is_empty() {
        eprintln!("\n{} Funkcje publiczne (pub):", "[P]".green().bold());
        let mut sorted: Vec<_> = pub_fns;
        sorted.sort_by_key(|(n, _)| n.as_str());
        for (name, meta) in sorted {
            let sig = meta.sig.as_deref().unwrap_or("");
            eprintln!("    pub {} {}", name.cyan(), sig.yellow());
        }
    }

    if !attr_fns.is_empty() {
        eprintln!("\n{} Funkcje z atrybutami (|]):", "[A]".magenta().bold());
        let mut sorted: Vec<_> = attr_fns;
        sorted.sort_by_key(|(n, _)| n.as_str());
        for (name, meta) in sorted {
            let attrs: Vec<String> = meta.attrs.iter().map(|a| {
                if let Some(arg) = &a.arg { format!("|] {} \"{}\"", a.name, arg) }
                else { format!("|] {}", a.name) }
            }).collect();
            eprintln!("    {} — {}", name.cyan(), attrs.join(", ").yellow());
        }
    }

    // ── Kompilacja warunkowa ───────────────────────────────────
    let cond_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::CondComp { .. }))
    .collect();
    if !cond_nodes.is_empty() {
        eprintln!("\n{} Kompilacja warunkowa (*{{}}):", "[*]".yellow().bold());
        for node in &cond_nodes {
            if let CommandType::CondComp { flag } = &node.content {
                eprintln!("    linia {:>4} — *{{{}}}",  node.line_num, flag.yellow());
            }
        }
    }

    // ── Bloki scope =: ─────────────────────────────────────────
    let scope_block_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::ScopeBlock { .. }))
    .collect();
    if !scope_block_nodes.is_empty() {
        eprintln!("\n{} Bloki scope (=:): {}", "[=:]".blue().bold(), scope_block_nodes.len().to_string().yellow());
        for node in &scope_block_nodes {
            if let CommandType::ScopeBlock { body } = &node.content {
                eprintln!("    linia {:>4} — =: ({} węzłów)", node.line_num, body.len().to_string().cyan());
            }
        }
    }

    // ── Przypisania z typem numerycznym ───────────────────────
    let typed_assign_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::AssignTyped { .. }))
    .collect();
    if !typed_assign_nodes.is_empty() {
        eprintln!("\n{} Zmienne z typem numerycznym:", "[T]".green().bold());
        for node in &typed_assign_nodes {
            if let CommandType::AssignTyped { key, expr, type_ann, .. } = &node.content {
                eprintln!("    linia {:>4} — {} = {} [{}]",
                          node.line_num, key.yellow(), expr.cyan(), type_ann.magenta());
            }
        }
    }

    // ── Fat-arrow match ────────────────────────────────────────
    let fat_match_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::MatchFat { .. }))
    .collect();
    if !fat_match_nodes.is_empty() {
        eprintln!("\n{} Fat-arrow match (=>):", "[=>]".cyan().bold());
        for node in &fat_match_nodes {
            if let CommandType::MatchFat { cond } = &node.content {
                eprintln!("    linia {:>4} — match {} =>", node.line_num, cond.cyan());
            }
        }
    }
    let fat_arm_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content,
                         CommandType::MatchArmFat { .. } | CommandType::MatchArmDestructFat { .. }))
    .collect();
    if !fat_arm_nodes.is_empty() {
        eprintln!("\n{} Ramiona fat-arrow:", "[=>arm]".cyan().bold());
        for node in &fat_arm_nodes {
            match &node.content {
                CommandType::MatchArmFat { variant, fields, cmd } => {
                    let fstr = if fields.is_empty() { String::new() } else { format!(" [{}]", fields.join(", ")) };
                    eprintln!("    linia {:>4} — {}{} => {}", node.line_num, variant.yellow(), fstr.blue(), cmd.cyan());
                }
                CommandType::MatchArmDestructFat { fields, cmd } => {
                    eprintln!("    linia {:>4} — {{{}}} => {}",
                              node.line_num, fields.join(", ").yellow(), cmd.cyan());
                }
                _ => {}
            }
        }
    }

    // ── Atrybuty funkcji (standalone |]) ──────────────────────
    let attr_decl_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::FuncAttrDecl { .. }))
    .collect();
    if !attr_decl_nodes.is_empty() {
        eprintln!("\n{} Deklaracje atrybutów (|]):", "[|]]".magenta().bold());
        for node in &attr_decl_nodes {
            if let CommandType::FuncAttrDecl { attr } = &node.content {
                let a = format_attr(attr);
                eprintln!("    linia {:>4} — {}", node.line_num, a.yellow());
            }
        }
    }

    // ── Interfejsy ────────────────────────────────────────────
    let iface_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Interface { .. }))
    .collect();
    if !iface_nodes.is_empty() {
        eprintln!("\n{} Interfejsy (==interface):", "[i]".cyan().bold());
        for node in &iface_nodes {
            if let CommandType::Interface { name, methods } = &node.content {
                eprintln!("    {} [{}]", name.cyan(), methods.join(", ").yellow());
            }
        }
    }

    // ── Implementacje ─────────────────────────────────────────
    let impl_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::ImplDef { .. }))
    .collect();
    if !impl_nodes.is_empty() {
        eprintln!("\n{} Implementacje (impl):", "[I]".cyan().bold());
        for node in &impl_nodes {
            if let CommandType::ImplDef { class, interface } = &node.content {
                eprintln!("    {} impl {}", class.cyan(), interface.yellow());
            }
        }
    }

    // ── Arena allocator ───────────────────────────────────────
    let arena_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::ArenaDef { .. }))
    .collect();
    if !arena_nodes.is_empty() {
        eprintln!("\n{} Arena allocator (::):", "[A]".magenta().bold());
        for node in &arena_nodes {
            if let CommandType::ArenaDef { name, size } = &node.content {
                eprintln!("    linia {:>4} — :: {} [{}]", node.line_num, name.cyan(), size.yellow());
            }
        }
    }

    // ── Stałe % ───────────────────────────────────────────────
    let const_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Const { .. }))
    .collect();
    if !const_nodes.is_empty() {
        eprintln!("\n{} Stałe (%):", "[%]".yellow().bold());
        for node in &const_nodes {
            if let CommandType::Const { key, val } = &node.content {
                eprintln!("    %{} = {}", key.yellow(), val);
            }
        }
    }

    // ── Wyrażenia ─────────────────────────────────────────────
    let expr_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::AssignExpr { .. }))
    .collect();
    if !expr_nodes.is_empty() {
        eprintln!("\n{} Wyrażenia (=expr):", "[=]".green().bold());
        for node in &expr_nodes {
            if let CommandType::AssignExpr { key, expr, .. } = &node.content {
                eprintln!("    linia {:>4} — {} = {}", node.line_num, key.yellow(), expr.cyan());
            }
        }
    }

    // ── Mutacje kolekcji ──────────────────────────────────────
    let col_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::CollectionMut { .. }))
    .collect();
    if !col_nodes.is_empty() {
        eprintln!("\n{} Mutacje kolekcji:", "[c]".blue().bold());
        for node in &col_nodes {
            if let CommandType::CollectionMut { var, method, args } = &node.content {
                eprintln!("    linia {:>4} — ${}.{} {}", node.line_num, var.cyan(), method.yellow(), args);
            }
        }
    }

    // ── Result unwrap ?! ──────────────────────────────────────
    let unwrap_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::ResultUnwrap { .. }))
    .collect();
    if !unwrap_nodes.is_empty() {
        eprintln!("\n{} Result unwrap (?!):", "[?]".red().bold());
        for node in &unwrap_nodes {
            if let CommandType::ResultUnwrap { expr, msg } = &node.content {
                eprintln!("    linia {:>4} — {} ?! \"{}\"", node.line_num, expr.cyan(), msg.yellow());
            }
        }
    }

    // ── Match (stary styl) ────────────────────────────────────
    let match_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Match { .. }))
    .collect();
    if !match_nodes.is_empty() {
        eprintln!("\n{} Match statements: {}", "[m]".cyan().bold(), match_nodes.len().to_string().yellow());
    }

    // ── Assert ────────────────────────────────────────────────
    let assert_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Assert { .. }))
    .collect();
    if !assert_nodes.is_empty() {
        eprintln!("\n{} Assert statements:", "[a]".green().bold());
        for node in &assert_nodes {
            if let CommandType::Assert { cond, msg } = &node.content {
                let m = msg.as_deref().unwrap_or("(brak komunikatu)");
                eprintln!("    linia {:>4} — {} → \"{}\"", node.line_num, cond.cyan(), m);
            }
        }
    }

    // ── Async (spawn/await) ───────────────────────────────────
    let async_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content,
                         CommandType::Spawn(_) | CommandType::Await(_) |
                         CommandType::AssignSpawn { .. } | CommandType::AssignAwait { .. }))
    .collect();
    if !async_nodes.is_empty() {
        eprintln!("\n{} Async (spawn/await): {}", "[~]".blue().bold(), async_nodes.len().to_string().yellow());
        for node in &async_nodes {
            match &node.content {
                CommandType::AssignSpawn { key, task } =>
                eprintln!("    linia {:>4} — {} = spawn {}", node.line_num, key.yellow(), task.cyan()),
                CommandType::AssignAwait { key, expr } =>
                eprintln!("    linia {:>4} — {} = await {}", node.line_num, key.yellow(), expr.cyan()),
                CommandType::Spawn(r) =>
                eprintln!("    linia {:>4} — spawn {}", node.line_num, r.cyan()),
                CommandType::Await(r) =>
                eprintln!("    linia {:>4} — await {}", node.line_num, r.cyan()),
                _ => {}
            }
        }
    }

    // ── Pipe chains ───────────────────────────────────────────
    let pipe_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Pipe(_)))
    .collect();
    if !pipe_nodes.is_empty() {
        eprintln!("\n{} Pipe chains:", "[|]".magenta().bold());
        for node in &pipe_nodes {
            if let CommandType::Pipe(steps) = &node.content {
                eprintln!("    linia {:>4} — {} kroków: {}",
                          node.line_num, steps.len().to_string().yellow(), steps.join(" |> ").cyan());
            }
        }
    }

    // ── Pluginy ───────────────────────────────────────────────
    let plugin_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Plugin { .. }))
    .collect();
    if !plugin_nodes.is_empty() {
        let root = plugins_root();
        eprintln!("\n{} Pluginy (\\\\):", "[p]".cyan().bold());
        for node in &plugin_nodes {
            if let CommandType::Plugin { name, args, .. } = &node.content {
                let p = root.join(name);
                let e = p.exists() || PathBuf::from(format!("{}.hl", p.display())).exists();
                let a = if args.is_empty() { String::new() } else { format!(" {}", args) };
                eprintln!("    {} \\\\{}{}", if e { "✓".green() } else { "?".yellow() }, name.cyan(), a);
            }
        }
    }

    // ── Extern ────────────────────────────────────────────────
    let extern_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Extern { .. }))
    .collect();
    if !extern_nodes.is_empty() {
        eprintln!("\n{} Extern (--):", "[e]".cyan().bold());
        for node in &extern_nodes {
            if let CommandType::Extern { path, static_link, use_symbols } = &node.content {
                let kind = if *static_link { "static".yellow() } else { "dynamic".blue() };
                let syms = use_symbols.as_ref()
                .map(|s| format!(" use [{}]", s.join(", ")))
                .unwrap_or_default();
                eprintln!("    [{}] {}{}", kind, path, syms.green());
            }
        }
    }

    // ── Funkcje arena (::) ────────────────────────────────────
    let mut arena_fns: Vec<&String> = res.functions.iter()
    .filter(|(_, m)| m.is_arena)
    .map(|(n, _)| n)
    .collect();
    arena_fns.sort();
    if !arena_fns.is_empty() {
        eprintln!("\n{} Funkcje arena (::):", "[A]".magenta().bold());
        for n in arena_fns { eprintln!("    {}", n.magenta()); }
    }

    // ── Funkcje z sygnaturami typów ───────────────────────────
    let typed_fns: Vec<_> = res.functions.iter()
    .filter(|(_, m)| m.sig.is_some())
    .collect();
    if !typed_fns.is_empty() {
        eprintln!("\n{} Funkcje z typami:", "[t]".green().bold());
        let mut sorted: Vec<_> = typed_fns;
        sorted.sort_by_key(|(n, _)| n.as_str());
        for (name, meta) in sorted {
            eprintln!("    {} {}", name.cyan(), meta.sig.as_deref().unwrap_or("").yellow());
        }
    }

    // ── Sudo ──────────────────────────────────────────────────
    if res.is_potentially_unsafe {
        eprintln!("\n{} Komendy sudo (^):", "[!]".red().bold());
        for w in &res.safety_warnings { eprintln!("    {}", w.yellow()); }
    }

    // ── Lambdy ────────────────────────────────────────────────
    let lambda_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Lambda { .. } | CommandType::AssignLambda { .. }))
    .collect();
    if !lambda_nodes.is_empty() {
        eprintln!("\n{} Lambdy / domknięcia:", "[λ]".magenta().bold());
        for node in &lambda_nodes {
            match &node.content {
                CommandType::AssignLambda { key, params, body, .. } =>
                eprintln!("    linia {:>4} — {} = {{ {} -> {} }}",
                          node.line_num, key.yellow(), params.join(", ").cyan(), body.dimmed()),
                          CommandType::Lambda { params, body } =>
                          eprintln!("    linia {:>4} — {{ {} -> {} }}",
                                    node.line_num, params.join(", ").cyan(), body.dimmed()),
                                    _ => {}
            }
        }
    }

    // ── Rekurencja ogonowa ────────────────────────────────────
    let recur_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Recur { .. }))
    .collect();
    if !recur_nodes.is_empty() {
        eprintln!("\n{} Rekurencja ogonowa (recur): {}", "[r]".cyan().bold(), recur_nodes.len().to_string().yellow());
        for node in &recur_nodes {
            if let CommandType::Recur { args } = &node.content {
                eprintln!("    linia {:>4} — recur {}", node.line_num, args.cyan());
            }
        }
    }

    // ── Destrukturyzacja ──────────────────────────────────────
    let destruct_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::DestructList { .. } | CommandType::DestructMap { .. }))
    .collect();
    if !destruct_nodes.is_empty() {
        eprintln!("\n{} Destrukturyzacja:", "[d]".blue().bold());
        for node in &destruct_nodes {
            match &node.content {
                CommandType::DestructList { head, tail, source } =>
                eprintln!("    linia {:>4} — [{} | {}] = {}",
                          node.line_num, head.yellow(), tail.yellow(), source.cyan()),
                          CommandType::DestructMap { fields, source } =>
                          eprintln!("    linia {:>4} — {{{}}} = {}",
                                    node.line_num, fields.join(", ").yellow(), source.cyan()),
                                    _ => {}
            }
        }
    }

    // ── Zasięgi leksykalne ────────────────────────────────────
    let scope_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::ScopeDef))
    .collect();
    if !scope_nodes.is_empty() {
        eprintln!("\n{} Zasięgi leksykalne (;;scope): {}",
                  "[s]".green().bold(), scope_nodes.len().to_string().yellow());
    }

    // ── ADT ───────────────────────────────────────────────────
    let adt_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::AdtDef { .. }))
    .collect();
    if !adt_nodes.is_empty() {
        eprintln!("\n{} Typy algebraiczne (==type):", "[T]".magenta().bold());
        for node in &adt_nodes {
            if let CommandType::AdtDef { name, variants } = &node.content {
                let vs: Vec<String> = variants.iter().map(|(vn, fields)| {
                    if fields.is_empty() {
                        vn.clone()
                    } else {
                        let fs: Vec<String> = fields.iter()
                        .map(|(f, t)| format!("{}: {}", f, t))
                        .collect();
                        format!("{}[{}]", vn, fs.join(", "))
                    }
                }).collect();
                eprintln!("    {} → {}", name.cyan().bold(), vs.join(" | ").yellow());
            }
        }
    }

    // ── Do-bloki ──────────────────────────────────────────────
    let do_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::DoBlock { .. }))
    .collect();
    if !do_nodes.is_empty() {
        eprintln!("\n{} Do-bloki:", "[do]".cyan().bold());
        for node in &do_nodes {
            if let CommandType::DoBlock { key, body } = &node.content {
                eprintln!("    linia {:>4} — {} = do ({} kroków)",
                          node.line_num, key.yellow(), body.len().to_string().cyan());
            }
        }
    }

    // ── Wieloliniowe pipe ─────────────────────────────────────
    let pline_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::PipeLine { .. }))
    .collect();
    if !pline_nodes.is_empty() {
        eprintln!("\n{} Kroki potoku (|):", "[»]".blue().bold());
        for node in &pline_nodes {
            if let CommandType::PipeLine { step } = &node.content {
                eprintln!("    linia {:>4} — | {}", node.line_num, step.cyan());
            }
        }
    }

    // ── Testy jednostkowe ─────────────────────────────────────
    let test_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::TestBlock { .. }))
    .collect();
    if !test_nodes.is_empty() {
        eprintln!("\n{} Testy jednostkowe (==test):", "[✓]".green().bold());
        for node in &test_nodes {
            if let CommandType::TestBlock { desc, body } = &node.content {
                let asserts = body.iter()
                .filter(|n| matches!(&n.content, CommandType::Assert { .. }))
                .count();
                eprintln!("    linia {:>4} — \"{}\" ({} assert{})",
                          node.line_num, desc.yellow(), asserts.to_string().cyan(),
                          if asserts == 1 { "" } else { "y" });
            }
        }
    }

    // ── Defer ─────────────────────────────────────────────────
    let defer_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::Defer { .. }))
    .collect();
    if !defer_nodes.is_empty() {
        eprintln!("\n{} Defer (cleanup):", "[↩]".yellow().bold());
        for node in &defer_nodes {
            if let CommandType::Defer { expr } = &node.content {
                eprintln!("    linia {:>4} — defer {}", node.line_num, expr.cyan());
            }
        }
    }

    // ── Generics z constraints ─────────────────────────────────
    let generic_nodes: Vec<_> = all_nodes(res)
    .filter(|n| matches!(&n.content, CommandType::FuncDefGeneric { .. }))
    .collect();
    if !generic_nodes.is_empty() {
        eprintln!("\n{} Generics z constraints:", "[G]".cyan().bold());
        for node in &generic_nodes {
            if let CommandType::FuncDefGeneric { name, sig } = &node.content {
                eprintln!("    linia {:>4} — :{} {}", node.line_num, name.cyan(), sig.yellow());
            }
        }
    }

    eprintln!("{}", "═══════════════════════════════════════════".cyan());
}

fn format_attr(attr: &FuncAttr) -> String {
    if let Some(arg) = &attr.arg {
        format!("|] {} \"{}\"", attr.name, arg)
    } else {
        format!("|] {}", attr.name)
    }
}
