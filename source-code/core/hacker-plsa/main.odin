package main

import "core:fmt"
import "core:os"
import "core:strings"
import "core:slice"
import "core:strconv"
import "core:encoding/json"
import "core:path/filepath"

HACKER_DIR_SUFFIX :: "/.hackeros/hacker-lang"

Plugin :: struct {
    name: string,
    is_super: bool,
}

ParseResult :: struct {
    deps: [dynamic]string,
    libs: [dynamic]string,
    vars: map[string]string,
    local_vars: map[string]string,
    cmds: [dynamic]string,
    cmds_with_vars: [dynamic]string,
    cmds_separate: [dynamic]string,
    includes: [dynamic]string,
    binaries: map[string]string,
    plugins: [dynamic]Plugin,
    functions: map[string][dynamic]string,
    errors: [dynamic]string,
}

trim :: proc(s: string) -> string {
    return strings.trim_space(s)
}

parse_hacker_file :: proc(file_path: string, verbose: bool) -> ParseResult {
    res: ParseResult
    res.vars = make(map[string]string)
    res.local_vars = make(map[string]string)
    res.functions = make(map[string][dynamic]string)
    res.binaries = make(map[string]string)
    in_comment := false
    in_function: Maybe(string)
    home := os.get_env("HOME")
    hacker_dir := filepath.join({home, HACKER_DIR_SUFFIX[1:]})
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
    for line_slice in lines {
        line_trimmed := trim(line_slice)
        if line_trimmed == "" { continue }
        line := line_trimmed
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
        if line == ":" {
            if in_function != nil {
                in_function = nil
            } else {
                append(&res.errors, "Ending function without start")
            }
            continue
        } else if strings.has_prefix(line, ":") {
            func_name := trim(line[1:])
            if func_name == "" {
                append(&res.errors, "Empty function name")
                continue
            }
            if in_function != nil {
                append(&res.errors, "Nested function")
                continue
            }
            res.functions[func_name] = make([dynamic]string)
            in_function = func_name
            continue
        } else if strings.has_prefix(line, ".") {
            func_name := trim(line[1:])
            if func_name == "" {
                append(&res.errors, "Empty function call")
                continue
            }
            if body, ok := res.functions[func_name]; ok {
                if in_func, iok := in_function.?; iok {
                    append(&res.functions[in_func], ..body[:])
                } else {
                    append(&res.cmds, ..body[:])
                }
            } else {
                append(&res.errors, fmt.tprintf("Unknown function %s", func_name))
            }
            continue
        }
        if in_function != nil {
            valid := strings.has_prefix(line, ">") || strings.has_prefix(line, ">>") || strings.has_prefix(line, ">>>") ||
            strings.has_prefix(line, "=") || strings.has_prefix(line, "?") || strings.has_prefix(line, "&") ||
            strings.has_prefix(line, "!") || strings.has_prefix(line, "@") || strings.has_prefix(line, "$") ||
            strings.has_prefix(line, "\\")
            if !valid {
                append(&res.errors, "Invalid in function")
                continue
            }
        }
        if strings.has_prefix(line, "//") {
            if in_function != nil {
                append(&res.errors, "Deps not allowed in function")
                continue
            }
            dep := trim(line[2:])
            if dep != "" {
                append(&res.deps, dep)
            } else {
                append(&res.errors, "Empty system dependency")
            }
        } else if strings.has_prefix(line, "#") {
            if in_function != nil {
                append(&res.errors, "Libs not allowed in function")
                continue
            }
            lib_name := trim(line[1:])
            if lib_name == "" {
                append(&res.errors, "Empty library name")
                continue
            }
            lib_dir := filepath.join({hacker_dir, "libs", lib_name})
            lib_hacker_path := filepath.join({lib_dir, "main.hacker"})
            lib_bin_path := filepath.join({lib_dir, lib_name})
            lib_a_path := filepath.join({lib_dir, fmt.tprintf("%s.a", lib_name)})
            if os.exists(lib_hacker_path) {
                append(&res.includes, lib_name)
                sub := parse_hacker_file(lib_hacker_path, verbose)
                append(&res.deps, ..sub.deps[:])
                append(&res.libs, ..sub.libs[:])
                for k, v in sub.vars {
                    res.vars[k] = v
                }
                for k, v in sub.local_vars {
                    res.local_vars[k] = v
                }
                append(&res.cmds, ..sub.cmds[:])
                append(&res.cmds_with_vars, ..sub.cmds_with_vars[:])
                append(&res.cmds_separate, ..sub.cmds_separate[:])
                append(&res.includes, ..sub.includes[:])
                for name, path in sub.binaries {
                    if name in res.binaries {
                        append(&res.errors, fmt.tprintf("Duplicate binary name %s", name))
                    } else {
                        res.binaries[name] = path
                    }
                }
                append(&res.plugins, ..sub.plugins[:])
                for fname, fbody in sub.functions {
                    res.functions[fname] = fbody
                }
                for sub_err in sub.errors {
                    append(&res.errors, fmt.tprintf("In %s: %s", lib_name, sub_err))
                }
            }
            file_info, stat_err := os.stat(lib_bin_path)
            if stat_err == 0 && (file_info.mode & os.S_IFREG != 0) {
                mode_raw := u32(file_info.mode)
                if mode_raw & 0o111 != 0 {
                    if lib_name in res.binaries {
                        append(&res.errors, fmt.tprintf("Duplicate binary name %s", lib_name))
                    } else {
                        res.binaries[lib_name] = lib_bin_path
                    }
                }
            }
            _, a_err := os.stat(lib_a_path)
            if a_err == 0 {
                append(&res.libs, lib_a_path)
            }
        } else if strings.has_prefix(line, ">>>") {
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
                append(&res.errors, "Empty separate command")
            } else {
                if in_func, ok := in_function.?; ok {
                    append(&res.functions[in_func], mut_cmd)
                } else {
                    append(&res.cmds_separate, mut_cmd)
                }
            }
        } else if strings.has_prefix(line, ">>") {
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
                append(&res.errors, "Empty command with vars")
            } else {
                if in_func, ok := in_function.?; ok {
                    append(&res.functions[in_func], mut_cmd)
                } else {
                    append(&res.cmds_with_vars, mut_cmd)
                }
            }
        } else if strings.has_prefix(line, ">") {
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
                append(&res.errors, "Empty command")
            } else {
                if in_func, ok := in_function.?; ok {
                    append(&res.functions[in_func], mut_cmd)
                } else {
                    append(&res.cmds, mut_cmd)
                }
            }
        } else if strings.has_prefix(line, "@") {
            rest := line[1:]
            eq_pos := strings.index(rest, "=")
            if eq_pos != -1 {
                var := trim(rest[:eq_pos])
                value := trim(rest[eq_pos+1:])
                if var == "" || value == "" {
                    append(&res.errors, "Invalid variable")
                } else {
                    res.vars[var] = value
                }
            } else {
                append(&res.errors, "Invalid @ syntax")
            }
        } else if strings.has_prefix(line, "$") {
            rest := line[1:]
            eq_pos := strings.index(rest, "=")
            if eq_pos != -1 {
                var := trim(rest[:eq_pos])
                value := trim(rest[eq_pos+1:])
                if var == "" || value == "" {
                    append(&res.errors, "Invalid local variable")
                } else {
                    res.local_vars[var] = value
                }
            } else {
                append(&res.errors, "Invalid $ syntax")
            }
        } else if strings.has_prefix(line, "\\") {
            plugin_name := trim(line[1:])
            if plugin_name == "" {
                append(&res.errors, "Empty plugin name")
                continue
            }
            plugin_path := filepath.join({hacker_dir, "plugins", plugin_name})
            file_info, err := os.stat(plugin_path)
            if err == 0 {
                mode_raw := u32(file_info.mode)
                if mode_raw & 0o111 != 0 {
                    append(&res.plugins, Plugin{name = plugin_name, is_super = is_super})
                    if verbose {
                        fmt.printf("Loaded plugin: %s\n", plugin_name)
                    }
                } else {
                    append(&res.errors, fmt.tprintf("Plugin %s not found or not executable", plugin_name))
                }
            } else {
                append(&res.errors, fmt.tprintf("Plugin %s not found or not executable", plugin_name))
            }
        } else if strings.has_prefix(line, "=") {
            gt_pos := strings.index(line, ">")
            if gt_pos != -1 {
                num_str := trim(line[1:gt_pos])
                cmd_part := trim(line[gt_pos+1:])
                excl := strings.index(cmd_part, "!")
                if excl != -1 {
                    cmd_part = trim(cmd_part[:excl])
                }
                num, num_ok := strconv.parse_u64(num_str)
                if !num_ok {
                    append(&res.errors, "Invalid loop count")
                    continue
                }
                mut_cmd := cmd_part
                if is_super {
                    mut_cmd = fmt.tprintf("sudo %s", mut_cmd)
                }
                if mut_cmd == "" {
                    append(&res.errors, "Empty loop command")
                } else {
                    if in_func, ok := in_function.?; ok {
                        for _ in 0..<num {
                            append(&res.functions[in_func], mut_cmd)
                        }
                    } else {
                        for _ in 0..<num {
                            append(&res.cmds, mut_cmd)
                        }
                    }
                }
            } else {
                append(&res.errors, "Invalid loop syntax")
            }
        } else if strings.has_prefix(line, "?") {
            gt_pos := strings.index(line, ">")
            if gt_pos != -1 {
                condition := trim(line[1:gt_pos])
                cmd_part := trim(line[gt_pos+1:])
                excl := strings.index(cmd_part, "!")
                if excl != -1 {
                    cmd_part = trim(cmd_part[:excl])
                }
                mut_cmd := cmd_part
                if is_super {
                    mut_cmd = fmt.tprintf("sudo %s", mut_cmd)
                }
                if condition == "" || mut_cmd == "" {
                    append(&res.errors, "Invalid conditional")
                } else {
                    if_cmd := fmt.tprintf("if %s; then %s; fi", condition, mut_cmd)
                    if in_func, ok := in_function.?; ok {
                        append(&res.functions[in_func], if_cmd)
                    } else {
                        append(&res.cmds, if_cmd)
                    }
                }
            } else {
                append(&res.errors, "Invalid conditional syntax")
            }
        } else if strings.has_prefix(line, "&") {
            cmd_part := trim(line[1:])
            excl := strings.index(cmd_part, "!")
            if excl != -1 {
                cmd_part = trim(cmd_part[:excl])
            }
            mut_cmd := fmt.tprintf("%s &", cmd_part)
            if is_super {
                mut_cmd = fmt.tprintf("sudo %s", mut_cmd)
            }
            if cmd_part == "" {
                append(&res.errors, "Empty background command")
            } else {
                if in_func, ok := in_function.?; ok {
                    append(&res.functions[in_func], mut_cmd)
                } else {
                    append(&res.cmds, mut_cmd)
                }
            }
        } else if strings.has_prefix(line, "!") {
            // Comment, ignore
        } else {
            append(&res.errors, "Invalid syntax")
        }
    }
    if in_comment {
        append(&res.errors, "Unclosed comment block")
    }
    if in_function != nil {
        append(&res.errors, "Unclosed function block")
    }
    if verbose {
        fmt.printf("System Deps: %v\n", res.deps)
        fmt.printf("Libs (.a paths): %v\n", res.libs)
        fmt.printf("Vars: %v\n", res.vars)
        fmt.printf("Local Vars: %v\n", res.local_vars)
        fmt.printf("Cmds (direct): %v\n", res.cmds)
        fmt.printf("Cmds (with vars): %v\n", res.cmds_with_vars)
        fmt.printf("Cmds (separate): %v\n", res.cmds_separate)
        fmt.printf("Includes: %v\n", res.includes)
        fmt.printf("Binaries: %v\n", res.binaries)
        fmt.printf("Plugins: %v\n", res.plugins)
        fmt.printf("Functions: %v\n", res.functions)
        if len(res.errors) > 0 {
            fmt.printf("Errors: %v\n", res.errors)
        }
    }
    return res
}

main :: proc() {
    args := os.args[1:]
    verbose := false
    file: string
    for arg in args {
        if arg == "--verbose" {
            verbose = true
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
        delete(res.cmds)
        delete(res.cmds_with_vars)
        delete(res.cmds_separate)
        delete(res.includes)
        delete(res.plugins)
        delete(res.errors)
        delete(res.vars)
        delete(res.local_vars)
        delete(res.binaries)
        for _, f in res.functions {
            delete(f)
        }
        delete(res.functions)
    }
    if len(res.errors) > 0 {
        fmt.printf("\x1b[31m\x1b[1mErrors:\x1b[0m\n")
        for e in res.errors {
            fmt.printf(" \x1b[31m✖ \x1b[0m%s\n", e)
        }
        fmt.println()
        os.exit(1)
    }
    json_data, json_err := json.marshal(res)
    if json_err != nil {
        fmt.printf("JSON marshal error: %v\n", json_err)
        os.exit(1)
    }
    defer delete(json_data)
    fmt.println(string(json_data))
}
