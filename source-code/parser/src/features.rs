use crate::ast::{Node, VarType};

fn visit(nodes: &[Node], out: &mut Vec<&'static str>) {
    fn add(out: &mut Vec<&'static str>, f: &'static str) {
        if !out.contains(&f) { out.push(f); }
    }
    for node in nodes {
        match node {
            Node::PipeToVar { .. }        => add(out, "pipe-var"),
            Node::BuiltinCmd { .. } | Node::BuiltinPipeToVar { .. } => add(out, "coreutils"),
            Node::Arithmetic { .. }       => add(out, "arith"),
            Node::DirImport { .. }        => add(out, "import-dir"),
            Node::HackerOsApi { .. }      => add(out, "hackeros-api"),
            Node::VarDecl { typ, .. } if *typ != VarType::Any => add(out, "typed-vars"),
            Node::ForIn { body, .. }      => { add(out, "for-in");  visit(body, out); }
            Node::WhileLoop { body, .. }  => { add(out, "while");   visit(body, out); }
            Node::MatchExpr { arms, .. }  => {
                add(out, "switch");
                for arm in arms { visit(&arm.body, out); }
            }
            Node::ArenaFuncDef { body, .. } => { add(out, "arena"); visit(body, out); }
            Node::ArenaFuncCall { .. }    => add(out, "arena"),
            Node::ExternDef { body, .. }  => { add(out, "extern"); visit(body, out); }
            Node::Goroutine { name, body } => {
                if name.is_some() { add(out, "named-goroutines"); }
                visit(body, out);
            }
            Node::RepeatN { body, .. }
            | Node::FuncDef { body, .. }
            | Node::Conditional { body, .. } => visit(body, out),
            Node::Block(body)             => visit(body, out),
            _ => {}
        }
    }
}

/// Elementy gen 2 użyte w `nodes`, bez powtórzeń, w kolejności pierwszego wystąpienia.
pub fn gen2_features_used(nodes: &[Node]) -> Vec<&'static str> {
    let mut out = Vec::new();
    visit(nodes, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    #[test]
    fn detects_nested_and_dedups() {
        let src = ":: f <4k> def\n> hostname |> @h\ndone\n:: f\n:: f\n";
        let used = gen2_features_used(&parse_source(src).unwrap());
        assert_eq!(used, vec!["arena", "pipe-var"]);
    }

    #[test]
    fn plain_gen1_code_uses_nothing() {
        let used = gen2_features_used(&parse_source("~> hej\n> ls\n").unwrap());
        assert!(used.is_empty(), "{:?}", used);
    }
}
