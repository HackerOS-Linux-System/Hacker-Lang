use crate::ast_parser::Node;
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
            Node::Assign { name, value_id } => {
                let var_type = self.lookup_var(name).ok_or_else(|| miette!("Próba przypisania do niezdefiniowanej zmiennej: {}", name))?;
                let val_type = self.analyze(*value_id)?;

                if var_type != val_type && var_type != Type::Unknown && val_type != Type::Unknown {
                    // Wyjątek: Int i Unknown (z C) są kompatybilne w uproszczonym modelu
                    return Err(miette!("Niezgodność typów przy przypisaniu do '{}'. Oczekiwano {}, otrzymano {}", name, var_type, val_type));
                }
                Type::Void
            },
            Node::FieldAccess { object, field } => {
                let obj_type = self.lookup_var(object).ok_or_else(|| miette!("Niezdefiniowany obiekt: {}", object))?;

                if let Type::Object(struct_name) = obj_type {
                    if let Some(fields) = self.structs.get(&struct_name) {
                        if let Some(field_type) = fields.get(field) {
                            field_type.clone()
                        } else {
                            return Err(miette!("Struktura '{}' nie ma pola '{}'", struct_name, field));
                        }
                    } else {
                        return Err(miette!("Nieznana struktura '{}'", struct_name));
                    }
                } else {
                    return Err(miette!("Próba dostępu do pola '{}' na czymś co nie jest obiektem (typ: {})", field, obj_type));
                }
            },
            Node::FieldAssign { object, field, value_id } => {
                let obj_type = self.lookup_var(object).ok_or_else(|| miette!("Niezdefiniowany obiekt: {}", object))?;

                // Pobieramy typ pola i nazwę struktury przed analizą wartości, aby uniknąć konfliktu borrow checkera
                let (expected_type, struct_name) = if let Type::Object(name) = &obj_type {
                    let t = self.structs.get(name)
                    .and_then(|fields| fields.get(field))
                    .cloned();

                    match t {
                        Some(t) => (t, name.clone()),
                        None => return Err(miette!("Struktura '{}' nie ma pola '{}'", name, field)),
                    }
                } else {
                    return Err(miette!("Próba przypisania pola '{}' na czymś co nie jest obiektem (typ: {})", field, obj_type));
                };

                // Teraz możemy bezpiecznie wywołać self.analyze, bo nie trzymamy już referencji do self.structs
                let val_type = self.analyze(*value_id)?;

                if expected_type != val_type && expected_type != Type::Unknown && val_type != Type::Unknown {
                    return Err(miette!("Niezgodność typów przy przypisaniu do pola '{}.{}'. Oczekiwano {}, otrzymano {}", struct_name, field, expected_type, val_type));
                }
                Type::Void
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
            Node::MethodCall { object, method, args_ids } => {
                // 1. Znajdź obiekt i jego typ
                let obj_type = self.lookup_var(object).ok_or_else(|| miette!("Niezdefiniowany obiekt: {}", object))?;

                if let Type::Object(struct_name) = obj_type {
                    // 2. Skonstruuj nazwę funkcji manglowanej: StructName_MethodName
                    let mangled_name = format!("{}_{}", struct_name, method);

                    // 3. Sprawdź czy taka funkcja istnieje
                    if let Some((ret_ty, arg_tys)) = self.functions.get(&mangled_name).cloned() {
                        // Argumenty: oczekujemy (self, arg1, arg2...).
                        // W wywołaniu user.hello(x) mamy tylko x. Kompilator dodaje user automatycznie.
                        // Więc arg_tys.len() powinno być args_ids.len() + 1

                        if arg_tys.len() != args_ids.len() + 1 {
                            return Err(miette!("Metoda '{}' (jako {}) oczekuje {} argumentów (w tym self), podano {} + self", method, mangled_name, arg_tys.len(), args_ids.len()));
                        }

                        // Sprawdź argumenty (z pominięciem self, który jest weryfikowany przez typ obiektu)
                        for (i, arg_id) in args_ids.iter().enumerate() {
                            let t = self.analyze(*arg_id)?;
                            let expected = &arg_tys[i+1]; // +1 bo pomijamy self
                            if t == Type::String && *expected == Type::Int {
                                return Err(miette!("Argument {} metody '{}' ma zły typ. Oczekiwano {}, otrzymano {}", i+1, method, expected, t));
                            }
                        }
                        ret_ty
                    } else {
                        return Err(miette!("Obiekt typu '{}' nie posiada metody '{}' (szukano funkcji '{}')", struct_name, method, mangled_name));
                    }
                } else {
                    return Err(miette!("Notacja kropkowa dozwolona tylko dla obiektów. Zmienna '{}' ma typ {}", object, obj_type));
                }
            },
            Node::Func { args, body, .. } => {
                self.enter_scope();
                for arg in args {
                    // Tutaj bardzo prosta heurystyka dla prototypu "User_metoda"
                    // Jeśli argument nazywa się "self" lub nazwa funkcji zawiera "_",
                    // moglibyśmy zgadywać typ obiektu, ale obecny system typów AST
                    // nie ma deklaracji typów argumentów. Zakładamy Int/Pointer.
                    self.define_var(arg.clone(), Type::Int);
                }
                for child in body.children(self.arena) {
                    self.analyze(child)?;
                }
                self.exit_scope();
                Type::Void
            },
            Node::Object { name, body } => {
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
