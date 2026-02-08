# hla-transpiler/main.py
import sys
import os
import json
from lark import Lark, Transformer, v_args, LarkError
from lark.indenter import Indenter

# Updated grammar with new features
hla_grammar = r"""
    ?start: program

    program: (directive | import | alloc_mode | link_mode | global_decl | declaration | function | class | statement)*

    directive: /---/ MODE /---/  -> alloc_directive
             | /===/ LINK_MODE /===/  -> link_directive

    MODE: "auto" | "automatic" | "fast" | "manual" | "safe" | "gc"
    LINK_MODE: "full-static" | "static" | "dynamic"

    import: "#" "<" IMPORT_TYPE ":" NAME ">"  -> library_import

    IMPORT_TYPE: "rust" | "python" | "core" | "bytes" | "virus"

    alloc_mode: directive
    link_mode: directive

    global_decl: "global" ("def" | "mut") NAME ":" type "=" expression ";"  -> global_var

    declaration: ("def" | "mut") NAME ":" type "=" expression ";"  -> var_decl

    function: "sub" NAME "(" params? ")" ("->" type)? "[" statement* "]"  -> func_def

    class: "obj" NAME "[" (field | method)* "]"  -> class_def

    field: ("def" | "mut") NAME ":" type ";"  -> class_field

    method: function  -> class_method

    statement: assignment
             | log_stmt
             | if_stmt
             | loop_stmt
             | return_stmt
             | del_stmt
             | expression ";" 

    assignment: NAME "=" expression ";"  -> assign

    log_stmt: "log" expression ";"  -> log

    if_stmt: "if" expression "[" statement* "]" ("else" "[" statement* "]")?

    loop_stmt: "loop" "[" statement* "]"

    return_stmt: "return" expression? ";"  -> ret

    del_stmt: "del" NAME ";"  -> del_var

    params: param ("," param)*

    param: NAME ":" type

    expression: atom
              | expression "+" expression
              | expression "-" expression
              | expression "*" expression
              | expression "/" expression
              | expression "!"  -> propagate_error
              | "(" expression ")"

    atom: NUMBER -> number
        | STRING -> string
        | NAME -> var
        | "true" -> true
        | "false" -> false
        | func_call

    func_call: NAME "." NAME "(" (expression ("," expression)*)? ")"

    type: "i32" | "u32" | "f64" | "bool" | "String" | NAME

    COMMENT: "!" /[^\n]/*
           | "!!" /.+?/ "!!"

    %import common.CNAME -> NAME
    %import common.INT -> NUMBER
    %import common.ESCAPED_STRING -> STRING
    %import common.WS_INLINE
    %ignore WS_INLINE
    %ignore COMMENT

"""

class HLAIndenter(Indenter):
    NL_type = '_NEWLINE'
    OPEN_PAREN_types = ['LPAR', 'LSQB', 'LBRACE']
    CLOSE_PAREN_types = ['RPAR', 'RSQB', 'RBRACE']
    INDENT_type = '_INDENT'
    DEDENT_type = '_DEDENT'
    tab_len = 4

class RustTransformer(Transformer):
    def __init__(self):
        self.alloc_mode = "auto"
        self.link_mode = "full-static"
        self.imports = []
        self.python_imports = []
        self.core_imports = []
        self.has_errors = False
        self.error_type = "std::error::Error"
        self.use_arena = False
        self.use_safe = False
        self.use_manual = False
        self.use_gc = False  # Placeholder

    @v_args(inline=True)
    def program(self, *statements):
        rust_imports = "\n".join(self.imports)
        prelude = ""
        if self.python_imports:
            rust_imports += "\nuse pyo3::prelude::*;\n"
            prelude += "Python::with_gil(|py| { py.import(\"sys\"); });\n"  # Init Python
        if self.core_imports:
            for core in self.core_imports:
                rust_imports += f"\nextern crate {core};\n"
        if self.use_arena:
            prelude += "let arena = bumpalo::Bump::new();\n"
        if self.use_safe:
            prelude = "#![forbid(unsafe_code)]\n" + prelude
        if self.has_errors:
            rust_imports += "\nuse std::error::Error;\n"
        body = "\n".join(str(s) for s in statements if s)
        main_ret = " -> Result<(), Box<dyn Error>>" if self.has_errors else ""
        return f"{prelude}{rust_imports}\nfn main(){main_ret} {{\n{body}\n}}\n"

    @v_args(inline=True)
    def alloc_directive(self, mode):
        self.alloc_mode = mode.lower()
        if self.alloc_mode in ["auto", "automatic"]:
            self.alloc_mode = "arc"
        elif self.alloc_mode == "fast":
            self.use_arena = True
            self.imports.append("use bumpalo::Bump;")
        elif self.alloc_mode == "safe":
            self.use_safe = True
        elif self.alloc_mode == "manual":
            self.use_manual = True
        elif self.alloc_mode == "gc":
            self.use_gc = True  # Future
        return None

    @v_args(inline=True)
    def link_directive(self, mode):
        self.link_mode = mode.lower()
        return None

    @v_args(inline=True)
    def library_import(self, imp_type, name):
        if imp_type == "rust":
            self.imports.append(f"use {name}::*;")
        elif imp_type == "python":
            self.python_imports.append(name)
            self.imports.append(f"// Python bridge for {name}")
        elif imp_type == "core":
            self.core_imports.append(name)
            # Assume virus handles compilation, here just extern
        return None

    @v_args(inline=True)
    def global_var(self, mutability, name, typ, expr):
        mut = "mut " if mutability == "mut" else ""
        self.imports.append("use lazy_static::lazy_static;")
        return f"lazy_static! {{ static ref {mut}{name}: {typ} = {expr}; }}"

    @v_args(inline=True)
    def var_decl(self, mutability, name, typ, expr):
        mut = "mut " if mutability == "mut" else ""
        if self.alloc_mode == "arc":
            if typ in ["String", "Vec"]:  # Assume complex
                return f"let {mut}{name}: std::sync::Arc<{typ}> = std::sync::Arc::new({expr});"
        if self.use_arena:
            return f"let {mut}{name}: &{typ} = arena.alloc({expr});"
        return f"let {mut}{name}: {typ} = {expr};"

    @v_args(inline=True)
    def func_def(self, name, params, ret_type=None, *body):
        params_str = str(params) if params else ""
        ret_str = f" -> {ret_type}" if ret_type else ""
        if self.has_errors:
            ret_str = " -> Result<(), Box<dyn Error>>" if not ret_type else f" -> Result<{ret_type}, Box<dyn Error>>"
        body_str = "\n    ".join(str(s) for s in body if s)
        return f"fn {name}({params_str}){ret_str} {{\n    {body_str}\n}}"

    @v_args(inline=True)
    def class_def(self, name, *members):
        fields = []
        methods = []
        for m in members:
            if isinstance(m, str) and m.startswith("pub"):  
                fields.append(m)
            else:
                methods.append(str(m))
        fields_str = "\n    ".join(fields)
        methods_str = "\n".join(methods)
        wrapper = ""
        if self.alloc_mode == "arc":
            wrapper = "use std::sync::{Arc, Mutex};\nstruct {name}Wrapper(Arc<Mutex<{name}>>);"
        return f"struct {name} {{\n    {fields_str}\n}}\nimpl {name} {{\n{methods_str}\n}}{wrapper}"

    @v_args(inline=True)
    def class_field(self, mutability, name, typ):
        return f"pub {name}: {typ},"

    @v_args(inline=True)
    def assign(self, name, expr):
        return f"{name} = {expr};"

    @v_args(inline=True)
    def log(self, expr):
        return f'println!(\"{{}}\", {expr});'

    @v_args(inline=True)
    def if_stmt(self, cond, *body_else):
        body = "\n    ".join(str(s) for s in body_else[:len(body_else)//2] if s)
        else_body = ""
        if len(body_else) % 2 == 1:
            else_body = " else {\n    " + "\n    ".join(str(s) for s in body_else[len(body_else)//2:]) + "\n}"
        return f"if {cond} {{\n    {body}\n}}{else_body}"

    @v_args(inline=True)
    def loop_stmt(self, *body):
        body_str = "\n    ".join(str(s) for s in body if s)
        return f"loop {{\n    {body_str}\n}}"

    @v_args(inline=True)
    def ret(self, expr=None):
        return f"return {expr}.ok()?;" if expr and self.has_errors else f"return {expr};" if expr else "return;"

    @v_args(inline=True)
    def del_var(self, name):
        if self.use_manual:
            return f"drop({name});"
        return "// del ignored"

    @v_args(inline=True)
    def propagate_error(self, expr):
        self.has_errors = True
        return f"{expr}?"

    @v_args(inline=True)
    def func_call(self, mod, func, *args):
        args_str = ", ".join(str(a) for a in args)
        if mod in self.python_imports:
            return f"Python::with_gil(|py| {{ py.import(\"{mod}\")?.getattr(\"{func}\")?.call(({args_str},))?.extract()? }})"
        return f"{mod}::{func}({args_str})"

    @v_args(inline=True)
    def params(self, *params):
        return ", ".join(str(p) for p in params)

    @v_args(inline=True)
    def param(self, name, typ):
        return f"{name}: {typ}"

    def number(self, n):
        return str(n)

    def string(self, s):
        return s

    def var(self, v):
        return str(v)

    def true(self, _):
        return "true"

    def false(self, _):
        return "false"

    def type(self, t):
        return str(t)

    def expression(self, children):
        if len(children) == 1:
            return str(children[0])
        return " ".join(str(c) for c in children)

    def atom(self, children):
        return str(children[0])

# Parser
parser = Lark(hla_grammar, parser='lalr', postlex=HLAIndenter())

def transpile_hla_to_rust(file_path, output_path):
    with open(file_path, 'r') as f:
        source = f.read()
    
    try:
        tree = parser.parse(source)
        transformer = RustTransformer()
        rust_code = transformer.transform(tree)
        with open(output_path, 'w') as f:
            f.write(rust_code)
        return True
    except LarkError as e:
        # Generate error.json
        context = e.get_context(source, 50)
        line = source[:e.pos_in_stream].count('\n') + 1
        col = e.column
        error_data = {
            "line": line,
            "column": col,
            "message": str(e),
            "context": context
        }
        with open("error.json", 'w') as f:
            json.dump(error_data, f)
        return False

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: hla-transpiler <input.hla> [output.rs]")
        sys.exit(1)
    
    input_file = sys.argv[1]
    output_file = sys.argv[2] if len(sys.argv) > 2 else input_file.replace('.hla', '.rs')
    
    success = transpile_hla_to_rust(input_file, output_file)
    if success:
        print(f"Transpiled {input_file} to {output_file}")
    else:
        print("Transpilation failed, see error.json")
        sys.exit(1)
