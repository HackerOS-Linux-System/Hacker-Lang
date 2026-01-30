use crate::ast_parser::{Node, BinaryOp};
use indextree::{Arena, NodeId};

// Constant Folding: Zamienia wyrażenia typu 2 + 2 na 4 bezpośrednio w drzewie AST
pub fn optimize_ast(arena: &mut Arena<Node>, root: NodeId) {
    fold_constants(arena, root);
    // Dead Code Elimination (Proste usunięcie nieużywanych definicji, tu placeholder)
    // prune_dead_functions(arena, root);
}

fn fold_constants(arena: &mut Arena<Node>, node_id: NodeId) {
    // Najpierw zoptymalizuj dzieci (post-order traversal)
    let children: Vec<NodeId> = node_id.children(arena).collect();
    for child in children {
        fold_constants(arena, child);
    }

    // Sprawdź czy bieżący węzeł to BinaryOp
    let node_opt = arena.get(node_id).map(|n| n.get().clone());

    if let Some(Node::BinaryOp { op, lhs, rhs }) = node_opt {
        let left_val = get_int_value(arena, lhs);
        let right_val = get_int_value(arena, rhs);

        if let (Some(l), Some(r)) = (left_val, right_val) {
            let result = match op {
                BinaryOp::Add => l + r,
                BinaryOp::Sub => l - r,
                BinaryOp::Mul => l * r,
                BinaryOp::Div => if r != 0 { l / r } else { 0 }, // Unikamy paniki, zwracamy 0
            };

            // Podmień węzeł BinaryOp na IntLit
            if let Some(node_mut) = arena.get_mut(node_id) {
                println!("Optymalizacja: {} {:?} {} -> {}", l, op, r, result);
                *node_mut.get_mut() = Node::IntLit(result);
            }
            // Dzieci (lhs, rhs) zostaną w arenie, ale odłączone od rodzica (dead nodes)
            // W prawdziwej implementacji usunęlibyśmy je.
        }
    }
}

fn get_int_value(arena: &Arena<Node>, id: NodeId) -> Option<i64> {
    match arena.get(id).unwrap().get() {
        Node::IntLit(v) => Some(*v),
        _ => None,
    }
}
