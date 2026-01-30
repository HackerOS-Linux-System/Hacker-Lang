use crate::ast_parser::{Node, BinaryOp};
use indextree::{Arena, NodeId};
use miette::{miette, Result};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    String,
    Void,
    Object(String), // np. Object("User")
    Unknown,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int => write!(f, "Int"),
            Type::String => write!(f, "String"),
            Type::Void => write!(f, "Void"),
            Type::Object(s) => write!(f, "Struct {}", s),
            Type::Unknown => write!(f, "Unknown"),
        }
    }
}

pub type TypeMap = HashMap<NodeId, Type>;

struct Analyzer<'a> {
    arena: &'a Arena<Node>,
    types: TypeMap,
    // Scope: nazwa zmiennej -> typ
    scopes: Vec<HashMap<String, Type>>,
    // Funkcje: nazwa -> (typ zwracany, typy argumentów)
    functions: HashMap<String, (Type, Vec<Type>)>,
    // Structy: nazwa -> pola
    structs: HashMap<String, HashMap<String, Type>>,
}

impl<'a> Analyzer<'a> {
    fn new(arena: &'a Arena<Node>) -> Self {
        let mut global_scope = HashMap::new();
        // Wbudowane
        global_scope.insert("true".to_string(), Type::Int);
        global_scope.insert("false".to_string(), Type::Int);

        Self {
            arena,
            types: HashMap::new(),
            scopes: vec![global_scope],
            functions: HashMap::new(),
            structs: HashMap::new(),
        }
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_var(&mut self, name: String, ty: Type) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    fn lookup_var(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    fn analyze(&mut self, node_id: NodeId) -> Result<Type> {
        let node = self.arena.get(node_id).unwrap().get();
        let ty = match node {
            Node::Program(_) => {
                for child in node_id.children(self.arena) {
                    // Pre-scan for functions and structs to handle forward references (basic)
                    if let Node::Func { name, args, .. } = self.arena.get(child).unwrap().get() {
                        let arg_types = vec![Type::Int; args.len()]; // Zakładamy Int dla argumentów funkcji póki co
                        self.functions.insert(name.clone(), (Type::Int, arg_types));
                    }
                    if let Node::Object { name, .. } = self.arena.get(child).unwrap().get() {
                        self.structs.insert(name.clone(), HashMap::new());
                    }
                }

                for child in node_id.children(self.arena) {
                    self.analyze(child)?;
                }
                Type::Void
            },
            Node::IntLit(_) => Type::Int,
            Node::StringLit(_) => Type::String,
            Node::Ident(name) => {
                self.lookup_var(name).ok_or_else(|| miette!("Niezdefiniowana zmienna: {}", name))?
            },
            Node::Var { name, value_id, .. } => {
                let val_type = self.analyze(*value_id)?;
                if val_type == Type::Void {
                    return Err(miette!("Nie można przypisać Void do zmiennej '{}'", name));
                }
                self.define_var(name.clone(), val_type.clone());
                val_type
            },
            Node::BinaryOp { lhs, rhs, .. } => {
                let l_ty = self.analyze(*lhs)?;
                let r_ty = self.analyze(*rhs)?;
                if l_ty != Type::Int || r_ty != Type::Int {
                    return Err(miette!("Operacje matematyczne dozwolone tylko na Int. Otrzymano: {} i {}", l_ty, r_ty));
                }
                Type::Int
            },
            Node::Call { name, args_ids } => {
                // Sprawdź czy funkcja istnieje
                if let Some((ret_ty, arg_tys)) = self.functions.get(name).cloned() {
                    if args_ids.len() != arg_tys.len() {
                        return Err(miette!("Funkcja '{}' oczekuje {} argumentów, podano {}", name, arg_tys.len(), args_ids.len()));
                    }
                    for (i, arg_id) in args_ids.iter().enumerate() {
                        let t = self.analyze(*arg_id)?;
                        if t != arg_tys[i] {
                            // Uproszczenie: Na razie C przyjmuje głównie Int, ale rzucamy błąd dla String->Int
                            if t == Type::String && arg_tys[i] == Type::Int {
                                return Err(miette!("Argument {} funkcji '{}' ma zły typ. Oczekiwano {}, otrzymano {}", i+1, name, arg_tys[i], t));
                            }
                        }
                    }
                    ret_ty
                } else {
                    // Zakładamy, że zewnętrzne funkcje C (z importów) zwracają Int
                    for arg in args_ids { self.analyze(*arg)?; }
                    Type::Int
                }
            },
            Node::Func { args, body, .. } => {
                self.enter_scope();
                for arg in args {
                    self.define_var(arg.clone(), Type::Int); // Domyślnie argumenty to Int
                }
                for child in body.children(self.arena) {
                    self.analyze(child)?;
                }
                self.exit_scope();
                Type::Void
            },
            Node::Object { name, body } => {
                // Analizuj pola structa
                // W tym uproszczonym modelu, Var wewnątrz Object traktujemy jako definicję pola
                let mut fields = HashMap::new();
                for child in body.children(self.arena) {
                    if let Node::Var { name: field_name, value_id, .. } = self.arena.get(child).unwrap().get() {
                        let field_type = self.analyze(*value_id)?;
                        fields.insert(field_name.clone(), field_type);
                    }
                }
                self.structs.insert(name.clone(), fields);
                Type::Void
            },
            Node::Block => {
                // Blok nie tworzy scope w tym prostym parserze, chyba że dodamy obsługę
                for child in node_id.children(self.arena) {
                    self.analyze(child)?;
                }
                Type::Void
            },
            Node::Log(val_id) => {
                self.analyze(*val_id)?;
                Type::Void
            },
            _ => Type::Void,
        };

        self.types.insert(node_id, ty.clone());
        Ok(ty)
    }
}

pub fn analyze_semantics(arena: &Arena<Node>, root: NodeId) -> Result<TypeMap> {
    let mut analyzer = Analyzer::new(arena);
    analyzer.analyze(root)?;
    Ok(analyzer.types)
}
