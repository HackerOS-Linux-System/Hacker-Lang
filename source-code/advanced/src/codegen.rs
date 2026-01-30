use crate::ast_parser::Node;
use indextree::{Arena, NodeId};
use miette::Result;

pub fn generate_c(arena: &Arena<Node>, root: NodeId) -> Result<String> {
    let mut code = String::new();
    let root_data = arena.get(root).unwrap().get();

    // 1. Headers
    code.push_str("#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n");

    // 2. Memory Allocator
    if let Node::Program(mode) = root_data {
        code.push_str(&format!("// HLA Memory Mode: {}\n", mode));
        if mode == "automatic" || mode == "auto" {
            code.push_str("/* Arena Allocator Stub */\nvoid* hla_alloc(size_t size) { return malloc(size); }\n");
        }
    }

    // 3. PASS 1: Global Definitions (Includes, Functions, Structs)
    for child in root.children(arena) {
        let node = arena.get(child).unwrap().get();
        match node {
            Node::Require(file) => code.push_str(&format!("#include \"{}\"\n", file.replace(".hlc", ".h"))),
            Node::Import { source, name, .. } if source == "c" => code.push_str(&format!("#include <{}.h>\n", name)),
            Node::Func { .. } => generate_node(child, arena, &mut code, true)?,
            Node::Object { .. } => generate_node(child, arena, &mut code, true)?,
            _ => {} // Zmienne, logi i wywołania w globalnym scope są ignorowane w tym przebiegu (trafią do main)
        }
    }

    // 4. Main Function
    code.push_str("\nint main(int argc, char** argv) {\n");

    // 5. PASS 2: Main Body Statements
    for child in root.children(arena) {
        let node = arena.get(child).unwrap().get();
        match node {
            // Te elementy zostały już wygenerowane globalnie
            Node::Func { .. } | Node::Object { .. } | Node::Require(_) | Node::Import{..} => {},
            // Reszta trafia do main
            _ => generate_node(child, arena, &mut code, false)?,
        }
    }

    code.push_str("return 0;\n}\n");
    Ok(code)
}

fn generate_node(id: NodeId, arena: &Arena<Node>, code: &mut String, is_global: bool) -> Result<()> {
    let node = arena.get(id).unwrap().get();
    match node {
        Node::Func { name, args, body } => {
            if !is_global { return Ok(()); }
            code.push_str(&format!("int {}(", name));
            if args.is_empty() {
                code.push_str("void");
            } else {
                let arg_defs: Vec<String> = args.iter().map(|a| format!("int {}", a)).collect(); // Uproszczenie: wszystko to int
                code.push_str(&arg_defs.join(", "));
            }
            code.push_str(") {\n");
            for child in body.children(arena) {
                generate_node(child, arena, code, false)?;
            }
            code.push_str("return 0;\n}\n");
        },
        Node::Call { name, args_ids } => {
            code.push_str(name);
            code.push_str("(");
            for (i, arg_id) in args_ids.iter().enumerate() {
                if i > 0 { code.push_str(", "); }
                emit_expr(*arg_id, arena, code)?;
            }
            code.push_str(");\n");
        },
        Node::Log(arg_id) => {
            code.push_str("printf(\"");
            let arg_node = arena.get(*arg_id).unwrap().get();
            match arg_node {
                Node::StringLit(_) => code.push_str("%s\\n\", "),
                _ => code.push_str("%d\\n\", "), // Domyślnie traktujemy jako liczbę/identyfikator int
            }
            emit_expr(*arg_id, arena, code)?;
            code.push_str(");\n");
        },
        Node::Var { name, value_id } => {
            let val = arena.get(*value_id).unwrap().get();
            match val {
                Node::StringLit(_) => code.push_str("char* "),
                _ => code.push_str("int "),
            }
            code.push_str(name);
            code.push_str(" = ");
            emit_expr(*value_id, arena, code)?;
            code.push_str(";\n");
        },
        Node::Object { name, body: _ } => {
            if !is_global { return Ok(()); }
            code.push_str(&format!("typedef struct {} {{\n", name));
            code.push_str("    // fields placeholder\n");
            code.push_str(&format!("}} {};\n", name));
        },
        Node::Block => {
            code.push_str("{\n");
            for child in id.children(arena) {
                generate_node(child, arena, code, false)?;
            }
            code.push_str("}\n");
        },
        _ => {}
    }
    Ok(())
}

fn emit_expr(id: NodeId, arena: &Arena<Node>, code: &mut String) -> Result<()> {
    let node = arena.get(id).unwrap().get();
    match node {
        Node::StringLit(s) => code.push_str(&format!("\"{}\"", s)),
        Node::IntLit(i) => code.push_str(&i.to_string()),
        Node::Ident(s) => code.push_str(s),
        Node::Call { name, args_ids } => {
            code.push_str(name);
            code.push_str("(");
            for (i, arg_id) in args_ids.iter().enumerate() {
                if i > 0 { code.push_str(", "); }
                emit_expr(*arg_id, arena, code)?;
            }
            code.push_str(")");
        },
        _ => code.push_str("0"),
    }
    Ok(())
}
