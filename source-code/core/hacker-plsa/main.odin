package main
import "core:fmt"
import "core:os"
import "core:strings"
import "core:slice"
import "core:strconv"
import "core:encoding/json"
import "core:path/filepath"

HACKER_DIR_SUFFIX :: "/.hackeros/hacker-lang"

Param :: struct {
    name: string,
    type_: string `json:"type"`,
    default: Maybe(string),
}

Function :: struct {
    params: [dynamic]Param,
    body: [dynamic]string,
}

Plugin :: struct {
    path: string,
    is_super: bool `json:"super"`,
}

ParseResult :: struct {
    deps: [dynamic]string,
    libs: [dynamic]string,
    rust_libs: [dynamic]string,
    python_libs: [dynamic]string,
    java_libs: [dynamic]string,
    vars_dict: map[string]string `json:"vars"`,
    local_vars: map[string]string,
    cmds: [dynamic]string,
    cmds_with_vars: [dynamic]string,
    cmds_separate: [dynamic]string,
    includes: [dynamic]string,
    binaries: [dynamic]string,
    plugins: [dynamic]Plugin,
    functions: map[string]Function,
    errors: [dynamic]string,
    config_data: map[string]string `json:"config"`,
}

trim :: proc(s: string) -> string {
    return strings.trim_space(s)
}

is_ascii_alphanumeric :: proc(c: byte) -> bool {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')
}

parse_hacker_file :: proc(file_path: string, verbose: bool) -> ParseResult {
    res: ParseResult
    res.vars_dict = make(map[string]string)
    res.local_vars = make(map[string]string)
    res.functions = make(map[string]Function)
    res.config_data = make(map[string]string)
    in_config := false
    in_comment := false
    in_function: Maybe(string)
    line_num: u32 = 0
    home := os.get_env("HOME")
    hacker_dir := filepath.join({home, HACKER_DIR_SUFFIX[1:]}) // strip leading /
    data, ok := os.read_entire_file(file_path)
    if !ok {
        if verbose {
            fmt.printf("File %s not found\n", file_path)
        }
        append(&res.errors, fmt.tprintf("File %s not found", file_path))
        return res
    }
    defer delete(data)
    lines := strings.split_lines(string(data))
    defer delete(lines)
    line_loop: for line_slice in lines {
        line_num += 1
        line_trimmed := trim(line_slice)
        if line_trimmed == "" { continue }
        line := line_trimmed // copy
        is_super := false
        if strings.has_prefix(line, "^") {
            is_super = true
            line = trim(line[1:])
        }
        if line == "!!" {
            in_comment = !in_comment
            continue
        }
        if in_comment { continue }
        if line == "[" {
            if in_config {
                append(&res.errors, fmt.tprintf("Line %d: Nested config section", line_num))
            }
            if in_function != nil {
                append(&res.errors, fmt.tprintf("Line %d: Config in function", line_num))
            }
            in_config = true
            continue
        } else if line == "]" {
            if !in_config {
                append(&res.errors, fmt.tprintf("Line %d: Closing ] without [", line_num))
            }
            in_config = false
            continue
        }
        if in_config {
            eq_pos := strings.index(line, "=")
            if eq_pos != -1 {
                key := trim(line[:eq_pos])
                value := trim(line[eq_pos+1:])
                res.config_data[key] = value
            }
            continue
        }
        if line == ":" {
            if in_function != nil {
                in_function = nil
            } else {
                append(&res.errors, fmt.tprintf("Line %d: Ending function without start", line_num))
            }
            continue
        } else if strings.has_prefix(line, ":") {
            rest := trim(line[1:])
            func_name: string
            params_str_opt: Maybe(string)
            pos := strings.index(rest, "(")
            if pos != -1 {
                func_name = trim(rest[:pos])
                params_str_opt = trim(rest[pos+1:])
            } else {
                func_name = rest
            }
            if func_name == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty function name", line_num))
                continue
            }
            if in_function != nil {
                append(&res.errors, fmt.tprintf("Line %d: Nested function", line_num))
                continue
            }
            params: [dynamic]Param
            if params_str, ok := params_str_opt.?; ok {
                if !strings.has_suffix(params_str, ")") {
                    append(&res.errors, fmt.tprintf("Line %d: Missing ) in function definition", line_num))
                    continue
                }
                params_str = trim(params_str[:len(params_str)-1])
                for p in strings.split(params_str, ",") {
                    p := trim(p)
                    name: string
                    rest: string
                    col_pos := strings.index(p, ":")
                    if col_pos != -1 {
                        name = trim(p[:col_pos])
                        rest = trim(p[col_pos+1:])
                    } else {
                        name = p
                        rest = ""
                    }
                    type_: string
                    default: Maybe(string)
                    eq_pos := strings.index(rest, "=")
                    if eq_pos != -1 {
                        type_ = trim(rest[:eq_pos])
                        default = trim(rest[eq_pos+1:])
                    } else {
                        type_ = rest
                    }
                    if type_ == "" { type_ = "str" }
                    append(&params, Param{name = name, type_ = type_, default = default})
                }
            }
            res.functions[func_name] = Function{params = params, body = make([dynamic]string)}
            in_function = func_name
            continue
        } else if strings.has_prefix(line, ".") {
            rest := trim(line[1:])
            func_name: string
            args_str_opt: Maybe(string)
            pos := strings.index(rest, "(")
            if pos != -1 {
                func_name = trim(rest[:pos])
                args_str_opt = trim(rest[pos+1:])
            } else {
                func_name = rest
            }
            if func_name == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty function call", line_num))
                continue
            }
            func_opt, func_ok := res.functions[func_name]
            if !func_ok {
                append(&res.errors, fmt.tprintf("Line %d: Unknown function %s", line_num, func_name))
                continue
            }
            func_ := copy_function(func_opt)
            args: [dynamic]string
            if args_str, ok := args_str_opt.?; ok {
                if !strings.has_suffix(args_str, ")") {
                    append(&res.errors, fmt.tprintf("Line %d: Missing ) in function call", line_num))
                    continue
                }
                args_str = trim(args_str[:len(args_str)-1])
                for a in strings.split(args_str, ",") {
                    append(&args, trim(a))
                }
            }
            params := func_.params
            if len(args) > len(params) {
                append(&res.errors, fmt.tprintf("Line %d: Too many arguments for %s", line_num, func_name))
                continue
            }
            sub_map: map[string]string
            defer delete(sub_map)
            for param, i in params {
                val: string
                if i < len(args) {
                    val = args[i]
                } else if def, ok := param.default.?; ok {
                    val = def
                } else {
                    append(&res.errors, fmt.tprintf("Line %d: Missing argument %s for %s", line_num, param.name, func_name))
                    continue line_loop
                }
                valid: bool
                switch param.type_ {
                case "int": _, valid = strconv.parse_i64(val)
                case "bool": valid = val == "true" || val == "false"
                case "str": valid = true
                case "list": valid = strings.contains(val, " ") // rough check
                case "dict": valid = strings.contains(val, "=")
                case: valid = true
                }
                if !valid {
                    append(&res.errors, fmt.tprintf("Line %d: Type mismatch for %s: expected %s, got %s", line_num, param.name, param.type_, val))
                    continue line_loop
                }
                sub_map[param.name] = val
            }
            body := func_.body
            sub_body: [dynamic]string
            defer delete(sub_body)
            for cmd in body {
                new_cmd := cmd
                for name, val in sub_map {
                    new_cmd, _ = strings.replace_all(new_cmd, fmt.tprintf("${%s}", name), val)
                }
                append(&sub_body, new_cmd)
            }
            if in_func, ok := in_function.?; ok {
                target_func := &res.functions[in_func]
                append(&target_func.body, ..sub_body[:])
            } else {
                append(&res.cmds, ..sub_body[:])
            }
            continue
        }
        if in_function != nil {
            if !(strings.has_prefix(line, ">") ||
                strings.has_prefix(line, "=") ||
                strings.has_prefix(line, "?") ||
                strings.has_prefix(line, "&") ||
                strings.has_prefix(line, "!") ||
                strings.has_prefix(line, "@") ||
                strings.has_prefix(line, "$") ||
                strings.has_prefix(line, "\\") ||
                strings.has_prefix(line, ">>") ||
                strings.has_prefix(line, ">>>") ||
                strings.has_prefix(line, "%") ||
                strings.has_prefix(line, "T>")) {
                append(&res.errors, fmt.tprintf("Line %d: Invalid in function", line_num))
                continue
            }
        }
        parsed := false
        if strings.has_prefix(line, "//") {
            parsed = true
            if in_function != nil {
                append(&res.errors, fmt.tprintf("Line %d: Deps not allowed in function", line_num))
                continue
            }
            dep := trim(line[2:])
            if dep != "" {
                append(&res.deps, dep)
            } else {
                append(&res.errors, fmt.tprintf("Line %d: Empty system dependency", line_num))
            }
        } else if strings.has_prefix(line, "#") {
            parsed = true
            if in_function != nil {
                append(&res.errors, fmt.tprintf("Line %d: Libs not allowed in function", line_num))
                continue
            }
            full_lib := trim(line[1:])
            if full_lib == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty library/include", line_num))
                continue
            }
            prefix: string = "bytes"
            lib_name: string
            colon_pos := strings.index(full_lib, ":")
            if colon_pos != -1 {
                prefix = trim(full_lib[:colon_pos])
                lib_name = trim(full_lib[colon_pos+1:])
            } else {
                lib_name = full_lib
            }
            if lib_name == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty library name after prefix", line_num))
                continue
            }
            switch prefix {
            case "rust": append(&res.rust_libs, lib_name)
            case "python": append(&res.python_libs, lib_name)
            case "java": append(&res.java_libs, lib_name)
            case "bytes":
                lib_dir := filepath.join({hacker_dir, "libs", lib_name})
                lib_hacker_path := filepath.join({lib_dir, "main.hacker"})
                lib_bin_path := filepath.join({hacker_dir, "libs", lib_name})
                if os.exists(lib_hacker_path) {
                    append(&res.includes, lib_name)
                    sub := parse_hacker_file(lib_hacker_path, verbose)
                    append(&res.deps, ..sub.deps[:])
                    append(&res.libs, ..sub.libs[:])
                    append(&res.rust_libs, ..sub.rust_libs[:])
                    append(&res.python_libs, ..sub.python_libs[:])
                    append(&res.java_libs, ..sub.java_libs[:])
                    for k, v in sub.vars_dict {
                        res.vars_dict[k] = v
                    }
                    for k, v in sub.local_vars {
                        res.local_vars[k] = v
                    }
                    append(&res.cmds, ..sub.cmds[:])
                    append(&res.cmds_with_vars, ..sub.cmds_with_vars[:])
                    append(&res.cmds_separate, ..sub.cmds_separate[:])
                    append(&res.includes, ..sub.includes[:])
                    append(&res.binaries, ..sub.binaries[:])
                    append(&res.plugins, ..sub.plugins[:])
                    for k, v in sub.functions {
                        res.functions[k] = v
                    }
                    for sub_err in sub.errors {
                        append(&res.errors, fmt.tprintf("In %s: %s", lib_name, sub_err))
                    }
                }
                file_info, err := os.stat(lib_bin_path)
                if err == 0 {
                    mode_raw := u32(file_info.mode)
                    if mode_raw & 0o111 != 0 {
                        append(&res.binaries, lib_bin_path)
                    } else {
                        append(&res.libs, lib_name)
                    }
                } else {
                    append(&res.libs, lib_name)
                }
            case:
                append(&res.errors, fmt.tprintf("Line %d: Unknown library prefix: %s", line_num, prefix))
            }
        } else if strings.has_prefix(line, ">>>") {
            parsed = true
            cmd := trim(line[3:])
            excl := strings.index(cmd, "!")
            if excl != -1 {
                cmd = trim(cmd[:excl])
            }
            mut_cmd := cmd
            if is_super {
                mut_cmd = fmt.tprintf("sudo %s", mut_cmd)
            }
            if mut_cmd == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty separate file command", line_num))
            } else {
                if in_func, ok := in_function.?; ok {
                    target_func := &res.functions[in_func]
                    append(&target_func.body, mut_cmd)
                } else {
                    append(&res.cmds_separate, mut_cmd)
                }
            }
        } else if strings.has_prefix(line, ">>") {
            parsed = true
            cmd := trim(line[2:])
            excl := strings.index(cmd, "!")
            if excl != -1 {
                cmd = trim(cmd[:excl])
            }
            mut_cmd := cmd
            if is_super {
                mut_cmd = fmt.tprintf("sudo %s", mut_cmd)
            }
            if mut_cmd == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty command with vars", line_num))
            } else {
                if in_func, ok := in_function.?; ok {
                    target_func := &res.functions[in_func]
                    append(&target_func.body, mut_cmd)
                } else {
                    append(&res.cmds_with_vars, mut_cmd)
                }
            }
        } else if strings.has_prefix(line, ">") {
            parsed = true
            cmd := trim(line[1:])
            excl := strings.index(cmd, "!")
            if excl != -1 {
                cmd = trim(cmd[:excl])
            }
            mut_cmd := cmd
            if is_super {
                mut_cmd = fmt.tprintf("sudo %s", mut_cmd)
            }
            if mut_cmd == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty command", line_num))
            } else {
                if in_func, ok := in_function.?; ok {
                    target_func := &res.functions[in_func]
                    append(&target_func.body, mut_cmd)
                } else {
                    append(&res.cmds, mut_cmd)
                }
            }
        } else if strings.has_prefix(line, "@") {
            parsed = true
            pos := 1
            for pos < len(line) && (is_ascii_alphanumeric(line[pos]) || line[pos] == '_') {
                pos += 1
            }
            key_str := line[1:pos]
            key: string
            type_: string = "str"
            col_pos := strings.index(key_str, ":")
            if col_pos != -1 {
                key = key_str[:col_pos]
                type_ = key_str[col_pos+1:]
            } else {
                key = key_str
            }
            after := trim(line[pos:])
            if !strings.has_prefix(after, "=") {
                append(&res.errors, fmt.tprintf("Line %d: Invalid variable", line_num))
                continue
            }
            value := trim(after[1:])
            if value == "" {
                append(&res.errors, fmt.tprintf("Line %d: Invalid variable", line_num))
                continue
            }
            // Handle list and dict
            if type_ == "list" {
                if strings.has_prefix(value, "[") && strings.has_suffix(value, "]") {
                    inner := value[1:len(value)-1]
                    items := strings.split(inner, ",")
                    defer delete(items)
                    value_items: [dynamic]string
                    for item in items {
                        append(&value_items, trim(item))
                    }
                    value = strings.join(value_items[:], " ")
                } else {
                    append(&res.errors, fmt.tprintf("Line %d: Invalid list format for %s", line_num, key))
                    continue
                }
            } else if type_ == "dict" {
                if strings.has_prefix(value, "{") && strings.has_suffix(value, "}") {
                    inner := value[1:len(value)-1]
                    pairs := strings.split(inner, ",")
                    defer delete(pairs)
                    value_pairs: [dynamic]string
                    for p in pairs {
                        pp := strings.split_n(p, ":", 2)
                        defer delete(pp)
                        if len(pp) == 2 {
                            append(&value_pairs, fmt.tprintf("%s=%s", trim(pp[0]), trim(pp[1])))
                        }
                    }
                    value = strings.join(value_pairs[:], " ")
                } else {
                    append(&res.errors, fmt.tprintf("Line %d: Invalid dict format for %s", line_num, key))
                    continue
                }
            }
            // Validate type
            valid: bool
            switch type_ {
            case "int": _, valid = strconv.parse_i64(value)
            case "bool": valid = value == "true" || value == "false"
            case "str": valid = true
            case "list": valid = true
            case "dict": valid = true
            case:
                append(&res.errors, fmt.tprintf("Line %d: Unknown type %s for variable %s", line_num, type_, key))
                valid = false
            }
            if !valid {
                append(&res.errors, fmt.tprintf("Line %d: Type validation failed for %s: %s", line_num, key, value))
                continue
            }
            res.vars_dict[key] = value
        } else if strings.has_prefix(line, "$") {
            parsed = true
            pos := 1
            for pos < len(line) && (is_ascii_alphanumeric(line[pos]) || line[pos] == '_') {
                pos += 1
            }
            key_str := line[1:pos]
            key: string
            type_: string = "str"
            col_pos := strings.index(key_str, ":")
            if col_pos != -1 {
                key = key_str[:col_pos]
                type_ = key_str[col_pos+1:]
            } else {
                key = key_str
            }
            after := trim(line[pos:])
            if !strings.has_prefix(after, "=") {
                append(&res.errors, fmt.tprintf("Line %d: Invalid local variable", line_num))
                continue
            }
            value := trim(after[1:])
            if value == "" {
                append(&res.errors, fmt.tprintf("Line %d: Invalid local variable", line_num))
                continue
            }
            // Handle list and dict
            if type_ == "list" {
                if strings.has_prefix(value, "[") && strings.has_suffix(value, "]") {
                    inner := value[1:len(value)-1]
                    items := strings.split(inner, ",")
                    defer delete(items)
                    value_items: [dynamic]string
                    for item in items {
                        append(&value_items, trim(item))
                    }
                    value = strings.join(value_items[:], " ")
                } else {
                    append(&res.errors, fmt.tprintf("Line %d: Invalid list format for %s", line_num, key))
                    continue
                }
            } else if type_ == "dict" {
                if strings.has_prefix(value, "{") && strings.has_suffix(value, "}") {
                    inner := value[1:len(value)-1]
                    pairs := strings.split(inner, ",")
                    defer delete(pairs)
                    value_pairs: [dynamic]string
                    for p in pairs {
                        pp := strings.split_n(p, ":", 2)
                        defer delete(pp)
                        if len(pp) == 2 {
                            append(&value_pairs, fmt.tprintf("%s=%s", trim(pp[0]), trim(pp[1])))
                        }
                    }
                    value = strings.join(value_pairs[:], " ")
                } else {
                    append(&res.errors, fmt.tprintf("Line %d: Invalid dict format for %s", line_num, key))
                    continue
                }
            }
            // Validate type
            valid: bool
            switch type_ {
            case "int": _, valid = strconv.parse_i64(value)
            case "bool": valid = value == "true" || value == "false"
            case "str": valid = true
            case "list": valid = true
            case "dict": valid = true
            case:
                append(&res.errors, fmt.tprintf("Line %d: Unknown type %s for local variable %s", line_num, type_, key))
                valid = false
            }
            if !valid {
                append(&res.errors, fmt.tprintf("Line %d: Type validation failed for local %s: %s", line_num, key, value))
                continue
            }
            res.local_vars[key] = value
        } else if strings.has_prefix(line, "\\") {
            parsed = true
            plugin_name := trim(line[1:])
            if plugin_name == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty plugin name", line_num))
                continue
            }
            plugin_dir := filepath.join({hacker_dir, "plugins", plugin_name})
            file_info, err := os.stat(plugin_dir)
            if err == 0 {
                mode_raw := u32(file_info.mode)
                if mode_raw & 0o111 != 0 {
                    append(&res.plugins, Plugin{path = plugin_dir, is_super = is_super})
                } else {
                    append(&res.errors, fmt.tprintf("Line %d: Plugin %s not found or not executable", line_num, plugin_name))
                }
            } else {
                append(&res.errors, fmt.tprintf("Line %d: Plugin %s not found or not executable", line_num, plugin_name))
            }
        } else if strings.has_prefix(line, "=") {
            parsed = true
            gt_pos := strings.index(line, ">")
            if gt_pos != -1 {
                num_str := trim(line[1:gt_pos])
                cmd_part := trim(line[gt_pos+1:])
                excl := strings.index(cmd_part, "!")
                if excl != -1 {
                    cmd_part = trim(cmd_part[:excl])
                }
                num, num_ok := strconv.parse_i64(num_str)
                if !num_ok || num < 0 {
                    append(&res.errors, fmt.tprintf("Line %d: Invalid loop count", line_num))
                    continue
                }
                if cmd_part == "" {
                    append(&res.errors, fmt.tprintf("Line %d: Empty loop command", line_num))
                    continue
                }
                cmd_base := cmd_part
                if is_super {
                    cmd_base = fmt.tprintf("sudo %s", cmd_base)
                }
                if in_func, ok := in_function.?; ok {
                    target_func := &res.functions[in_func]
                    for _ in 0..<num {
                        append(&target_func.body, cmd_base)
                    }
                } else {
                    for _ in 0..<num {
                        append(&res.cmds, cmd_base)
                    }
                }
            } else {
                append(&res.errors, fmt.tprintf("Line %d: Invalid loop syntax", line_num))
            }
        } else if strings.has_prefix(line, "?") {
            parsed = true
            gt_pos := strings.index(line, ">")
            if gt_pos != -1 {
                condition := trim(line[1:gt_pos])
                cmd_part := trim(line[gt_pos+1:])
                excl := strings.index(cmd_part, "!")
                if excl != -1 {
                    cmd_part = trim(cmd_part[:excl])
                }
                if condition == "" || cmd_part == "" {
                    append(&res.errors, fmt.tprintf("Line %d: Invalid conditional", line_num))
                    continue
                }
                cmd := cmd_part
                if is_super {
                    cmd = fmt.tprintf("sudo %s", cmd)
                }
                if_cmd := fmt.tprintf("if %s; then %s; fi", condition, cmd)
                if in_func, ok := in_function.?; ok {
                    target_func := &res.functions[in_func]
                    append(&target_func.body, if_cmd)
                } else {
                    append(&res.cmds, if_cmd)
                }
            } else {
                append(&res.errors, fmt.tprintf("Line %d: Invalid conditional", line_num))
            }
        } else if strings.has_prefix(line, "&") {
            parsed = true
            cmd_part := trim(line[1:])
            excl := strings.index(cmd_part, "!")
            if excl != -1 {
                cmd_part = trim(cmd_part[:excl])
            }
            if cmd_part == "" {
                append(&res.errors, fmt.tprintf("Line %d: Empty background command", line_num))
                continue
            }
            cmd := fmt.tprintf("%s &", cmd_part)
            if is_super {
                cmd = fmt.tprintf("sudo %s", cmd)
            }
            if in_func, ok := in_function.?; ok {
                target_func := &res.functions[in_func]
                append(&target_func.body, cmd)
            } else {
                append(&res.cmds, cmd)
            }
        } else if strings.has_prefix(line, "!") {
            parsed = true
            // ignore
        } else if strings.has_prefix(line, "%") {
            parsed = true
            rest := trim(line[1:])
            gt_pos := strings.index(rest, ">")
            if gt_pos != -1 {
                list_var := trim(rest[:gt_pos])
                cmd_part := trim(rest[gt_pos+1:])
                if list_var == "" || cmd_part == "" {
                    append(&res.errors, fmt.tprintf("Line %d: Invalid foreach syntax", line_num))
                    continue
                }
                foreach_cmd := fmt.tprintf("for item in %s; do %s; done", list_var, cmd_part)
                if is_super {
                    foreach_cmd = fmt.tprintf("sudo %s", foreach_cmd)
                }
                if in_func, ok := in_function.?; ok {
                    target_func := &res.functions[in_func]
                    append(&target_func.body, foreach_cmd)
                } else {
                    append(&res.cmds, foreach_cmd)
                }
            } else {
                append(&res.errors, fmt.tprintf("Line %d: Invalid foreach syntax", line_num))
            }
        } else if strings.has_prefix(line, "T>") {
            parsed = true
            c_pos := strings.index(line, "C>")
            if c_pos != -1 {
                try_cmd := trim(line[2:c_pos])
                rest_after_c := line[c_pos+2:]
                f_pos := strings.index(rest_after_c, "F>")
                catch_cmd: string
                finally_cmd: string
                if f_pos != -1 {
                    catch_cmd = trim(rest_after_c[:f_pos])
                    finally_cmd = trim(rest_after_c[f_pos+2:])
                } else {
                    catch_cmd = trim(rest_after_c)
                    finally_cmd = ""
                }
                try_catch_cmd := fmt.tprintf("( %s ) || %s;", try_cmd, catch_cmd)
                if finally_cmd != "" {
                    try_catch_cmd = fmt.tprintf("%s %s", try_catch_cmd, finally_cmd)
                }
                if is_super {
                    try_catch_cmd = fmt.tprintf("sudo %s", try_catch_cmd)
                }
                if in_func, ok := in_function.?; ok {
                    target_func := &res.functions[in_func]
                    append(&target_func.body, try_catch_cmd)
                } else {
                    append(&res.cmds, try_catch_cmd)
                }
            } else {
                append(&res.errors, fmt.tprintf("Line %d: Invalid try-catch syntax", line_num))
            }
        }
        if !parsed {
            append(&res.errors, fmt.tprintf("Line %d: Invalid syntax", line_num))
        }
    }
    if in_config {
        append(&res.errors, "Unclosed config section")
    }
    if in_comment {
        append(&res.errors, "Unclosed comment block")
    }
    if in_function != nil {
        append(&res.errors, "Unclosed function block")
    }
    // Sort
    slice.sort(res.deps[:])
    slice.sort(res.libs[:])
    slice.sort(res.rust_libs[:])
    slice.sort(res.python_libs[:])
    slice.sort(res.java_libs[:])
    return res
}

copy_function :: proc(f: Function) -> Function {
    res: Function
    res.params = make([dynamic]Param, len(f.params))
    copy(res.params[:], f.params[:])
    res.body = make([dynamic]string, len(f.body))
    copy(res.body[:], f.body[:])
    return res
}

main :: proc() {
    args := os.args[1:]
    verbose := false
    file: string
    for arg in args {
        if arg == "--verbose" {
            verbose = true
        } else if strings.has_prefix(arg, "--") {
            // ignore other flags for simplicity
        } else {
            file = arg
        }
    }
    if file == "" {
        fmt.println("Usage: program [--verbose] <file>")
        os.exit(1)
    }
    res := parse_hacker_file(file, verbose)
    defer {
        delete(res.deps)
        delete(res.libs)
        delete(res.rust_libs)
        delete(res.python_libs)
        delete(res.java_libs)
        delete(res.cmds)
        delete(res.cmds_with_vars)
        delete(res.cmds_separate)
        delete(res.includes)
        delete(res.binaries)
        delete(res.plugins)
        delete(res.errors)
        for _, f in res.functions {
            delete(f.params)
            delete(f.body)
        }
        delete(res.vars_dict)
        delete(res.local_vars)
        delete(res.functions)
        delete(res.config_data)
    }
    if verbose {
        RED :: "\e[31m"
        GREEN :: "\e[32m"
        BOLD :: "\e[1m"
        RESET :: "\e[0m"
        if len(res.errors) > 0 {
            fmt.printf("\n%sErrors:%s\n", RED + BOLD, RESET)
            for e in res.errors {
                fmt.printf(" %s%s %s\n", RED, "✖", e)
            }
            fmt.println()
        } else {
            fmt.printf("%sNo errors found.%s\n", GREEN, RESET)
        }
        fmt.printf("System Deps: [%s]\n", strings.join(res.deps[:], ", "))
        fmt.printf("Custom Libs (Bytes): [%s]\n", strings.join(res.libs[:], ", "))
        fmt.printf("Rust Libs: [%s]\n", strings.join(res.rust_libs[:], ", "))
        fmt.printf("Python Libs: [%s]\n", strings.join(res.python_libs[:], ", "))
        fmt.printf("Java Libs: [%s]\n", strings.join(res.java_libs[:], ", "))
        vars_str: [dynamic]string
        defer delete(vars_str)
        for k, v in res.vars_dict {
            append(&vars_str, fmt.tprintf("%s: %s", k, v))
        }
        slice.sort(vars_str[:])
        fmt.printf("Vars: {%s}\n", strings.join(vars_str[:], ", "))
        local_vars_str: [dynamic]string
        defer delete(local_vars_str)
        for k, v in res.local_vars {
            append(&local_vars_str, fmt.tprintf("%s: %s", k, v))
        }
        slice.sort(local_vars_str[:])
        fmt.printf("Local Vars: {%s}\n", strings.join(local_vars_str[:], ", "))
        fmt.printf("Cmds: [%s]\n", strings.join(res.cmds[:], ", "))
        fmt.printf("Cmds with Vars: [%s]\n", strings.join(res.cmds_with_vars[:], ", "))
        fmt.printf("Separate Cmds: [%s]\n", strings.join(res.cmds_separate[:], ", "))
        fmt.printf("Includes: [%s]\n", strings.join(res.includes[:], ", "))
        fmt.printf("Binaries: [%s]\n", strings.join(res.binaries[:], ", "))
        plugins_str: [dynamic]string
        defer delete(plugins_str)
        for p in res.plugins {
            append(&plugins_str, fmt.tprintf("{path: %s, super: %v}", p.path, p.is_super))
        }
        fmt.printf("Plugins: [%s]\n", strings.join(plugins_str[:], ", "))
        functions_str: [dynamic]string
        defer delete(functions_str)
        func_names: [dynamic]string
        defer delete(func_names)
        for k in res.functions {
            append(&func_names, k)
        }
        slice.sort(func_names[:])
        for k in func_names {
            f := res.functions[k]
            params_str: [dynamic]string
            for p in f.params {
                append(&params_str, fmt.tprintf("%s:%s=%?", p.name, p.type_, p.default))
            }
            append(&functions_str, fmt.tprintf("%s: params[%s] body[%s]", k, strings.join(params_str[:], ","), strings.join(f.body[:], ", ")))
            delete(params_str)
        }
        fmt.printf("Functions: {%s}\n", strings.join(functions_str[:], ", "))
        config_str: [dynamic]string
        defer delete(config_str)
        for k, v in res.config_data {
            append(&config_str, fmt.tprintf("%s: %s", k, v))
        }
        slice.sort(config_str[:])
        fmt.printf("Config: {%s}\n", strings.join(config_str[:], ", "))
    }
    json_data, json_err := json.marshal(res)
    if json_err != nil {
        fmt.printf("JSON marshal error: %v\n", json_err)
        os.exit(1)
    }
    defer delete(json_data)
    fmt.println(string(json_data))
}
