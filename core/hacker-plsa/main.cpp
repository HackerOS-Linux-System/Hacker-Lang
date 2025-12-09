#include <iostream>
#include <fstream>
#include <vector>
#include <unordered_map>
#include <string>
#include <optional>
#include <algorithm>
#include <filesystem>
#include <sys/stat.h>
#include <cstdlib>
#include <cctype>
#include <iomanip>
#include <sstream>  // Added for stringstream

namespace fs = std::filesystem;

const std::string HACKER_DIR_SUFFIX = "/.hackeros/hacker-lang";

struct Plugin {
    std::string path;
    bool is_super;
};

struct ParseResult {
    std::unordered_map<std::string, int> deps;
    std::unordered_map<std::string, int> libs;  // Default/bytes libs
    std::unordered_map<std::string, int> rust_libs;
    std::unordered_map<std::string, int> python_libs;
    std::unordered_map<std::string, int> java_libs;
    std::unordered_map<std::string, std::string> vars_dict;
    std::unordered_map<std::string, std::string> local_vars;
    std::vector<std::string> cmds;
    std::vector<std::string> cmds_with_vars;
    std::vector<std::string> cmds_separate;
    std::vector<std::string> includes;
    std::vector<std::string> binaries;
    std::vector<Plugin> plugins;
    std::unordered_map<std::string, std::vector<std::string>> functions;
    std::vector<std::string> errors;
    std::unordered_map<std::string, std::string> config_data;
};

void merge_maps(std::unordered_map<std::string, int>& dest, const std::unordered_map<std::string, int>& src) {
    for (const auto& p : src) {
        dest[p.first] = p.second;
    }
}

void merge_string_maps(std::unordered_map<std::string, std::string>& dest, const std::unordered_map<std::string, std::string>& src) {
    for (const auto& p : src) {
        dest[p.first] = p.second;
    }
}

void merge_function_maps(std::unordered_map<std::string, std::vector<std::string>>& dest, const std::unordered_map<std::string, std::vector<std::string>>& src) {
    for (const auto& p : src) {
        dest[p.first] = p.second;
    }
}

std::string trim(const std::string& str) {
    size_t first = str.find_first_not_of(" \t");
    if (first == std::string::npos) return "";
    size_t last = str.find_last_not_of(" \t");
    return str.substr(first, last - first + 1);
}

bool starts_with(const std::string& s, const std::string& prefix) {
    return s.size() >= prefix.size() && s.compare(0, prefix.size(), prefix) == 0;
}

void write_json_string(std::ostream& w, const std::string& s) {
    w << "\"";
    for (char c : s) {
        switch (c) {
            case '\"': w << "\\\""; break;
            case '\\': w << "\\\\"; break;
            case '\b': w << "\\b"; break;
            case '\f': w << "\\f"; break;
            case '\n': w << "\\n"; break;
            case '\r': w << "\\r"; break;
            case '\t': w << "\\t"; break;
            default:
                if (std::iscntrl(static_cast<unsigned char>(c))) {
                    w << "\\u" << std::hex << std::setw(4) << std::setfill('0') << static_cast<int>(static_cast<unsigned char>(c));
                } else {
                    w << c;
                }
                break;
        }
    }
    w << "\"";
}

void output_json(const ParseResult& res) {
    std::ostream& out = std::cout;
    out << "{";
    out << "\"deps\":[";
    bool first = true;
    for (const auto& p : res.deps) {
        if (!first) out << ",";
        write_json_string(out, p.first);
        first = false;
    }
    out << "],";
    out << "\"libs\":[";
    first = true;
    for (const auto& p : res.libs) {
        if (!first) out << ",";
        write_json_string(out, p.first);
        first = false;
    }
    out << "],";
    // Added new fields
    out << "\"rust_libs\":[";
    first = true;
    for (const auto& p : res.rust_libs) {
        if (!first) out << ",";
        write_json_string(out, p.first);
        first = false;
    }
    out << "],";
    out << "\"python_libs\":[";
    first = true;
    for (const auto& p : res.python_libs) {
        if (!first) out << ",";
        write_json_string(out, p.first);
        first = false;
    }
    out << "],";
    out << "\"java_libs\":[";
    first = true;
    for (const auto& p : res.java_libs) {
        if (!first) out << ",";
        write_json_string(out, p.first);
        first = false;
    }
    out << "],";
    out << "\"vars\":{";
    first = true;
    for (const auto& p : res.vars_dict) {
        if (!first) out << ",";
        write_json_string(out, p.first);
        out << ":";
        write_json_string(out, p.second);
        first = false;
    }
    out << "},";
    out << "\"local_vars\":{";
    first = true;
    for (const auto& p : res.local_vars) {
        if (!first) out << ",";
        write_json_string(out, p.first);
        out << ":";
        write_json_string(out, p.second);
        first = false;
    }
    out << "},";
    out << "\"cmds\":[";
    first = true;
    for (const auto& c : res.cmds) {
        if (!first) out << ",";
        write_json_string(out, c);
        first = false;
    }
    out << "],";
    out << "\"cmds_with_vars\":[";
    first = true;
    for (const auto& c : res.cmds_with_vars) {
        if (!first) out << ",";
        write_json_string(out, c);
        first = false;
    }
    out << "],";
    out << "\"cmds_separate\":[";
    first = true;
    for (const auto& c : res.cmds_separate) {
        if (!first) out << ",";
        write_json_string(out, c);
        first = false;
    }
    out << "],";
    out << "\"includes\":[";
    first = true;
    for (const auto& i : res.includes) {
        if (!first) out << ",";
        write_json_string(out, i);
        first = false;
    }
    out << "],";
    out << "\"binaries\":[";
    first = true;
    for (const auto& b : res.binaries) {
        if (!first) out << ",";
        write_json_string(out, b);
        first = false;
    }
    out << "],";
    out << "\"plugins\":[";
    first = true;
    for (const auto& p : res.plugins) {
        if (!first) out << ",";
        out << "{";
        out << "\"path\":";
        write_json_string(out, p.path);
        out << ",\"super\":" << (p.is_super ? "true" : "false");
        out << "}";
        first = false;
    }
    out << "],";
    out << "\"functions\":{";
    first = true;
    for (const auto& f : res.functions) {
        if (!first) out << ",";
        write_json_string(out, f.first);
        out << ":[";
        bool first2 = true;
        for (const auto& c : f.second) {
            if (!first2) out << ",";
            write_json_string(out, c);
            first2 = false;
        }
        out << "]";
        first = false;
    }
    out << "},";
    out << "\"errors\":[";
    first = true;
    for (const auto& e : res.errors) {
        if (!first) out << ",";
        write_json_string(out, e);
        first = false;
    }
    out << "],";
    out << "\"config\":{";
    first = true;
    for (const auto& p : res.config_data) {
        if (!first) out << ",";
        write_json_string(out, p.first);
        out << ":";
        write_json_string(out, p.second);
        first = false;
    }
    out << "}";
    out << "}" << std::endl;
}

ParseResult parse_hacker_file(const std::string& file_path, bool verbose) {
    ParseResult res;
    bool in_config = false;
    bool in_comment = false;
    std::optional<std::string> in_function = std::nullopt;
    uint32_t line_num = 0;
    char* home_env = std::getenv("HOME");
    std::string home = home_env ? home_env : "";
    fs::path hacker_dir = fs::path(home) / HACKER_DIR_SUFFIX;
    std::ifstream file(file_path);
    if (!file.is_open()) {
        if (verbose) {
            std::cout << "File " << file_path << " not found" << std::endl;
        }
        res.errors.push_back("File " + file_path + " not found");
        return res;
    }
    std::string line_slice;
    while (std::getline(file, line_slice)) {
        line_num++;
        std::string line_trimmed = trim(line_slice);
        if (line_trimmed.empty()) continue;
        std::string line = line_trimmed;
        if (line == "!!") {
            in_comment = !in_comment;
            continue;
        }
        if (in_comment) continue;
        bool is_super = false;
        if (line[0] == '^') {
            is_super = true;
            line = line.substr(1);
            line = trim(line);
        }
        if (line == "[") {
            if (in_config) res.errors.push_back("Line " + std::to_string(line_num) + ": Nested config section");
            if (in_function.has_value()) res.errors.push_back("Line " + std::to_string(line_num) + ": Config in function");
            in_config = true;
            continue;
        } else if (line == "]") {
            if (!in_config) res.errors.push_back("Line " + std::to_string(line_num) + ": Closing ] without [");
            in_config = false;
            continue;
        }
        if (in_config) {
            size_t eq_pos = line.find('=');
            if (eq_pos != std::string::npos) {
                std::string key = trim(line.substr(0, eq_pos));
                std::string value = trim(line.substr(eq_pos + 1));
                res.config_data[key] = value;
            }
            continue;
        }
        if (line == ":") {
            if (in_function.has_value()) {
                in_function = std::nullopt;
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Ending function without start");
            }
            continue;
        } else if (line[0] == ':') {
            std::string func_name = trim(line.substr(1));
            if (func_name.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty function name");
                continue;
            }
            if (in_function.has_value()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Nested function");
            }
            res.functions[func_name] = {};
            in_function = func_name;
            continue;
        } else if (line[0] == '.') {
            std::string func_name = trim(line.substr(1));
            if (func_name.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty function call");
                continue;
            }
            auto it = res.functions.find(func_name);
            if (it != res.functions.end()) {
                const auto& func_cmds = it->second;
                if (in_function.has_value()) {
                    auto& target = res.functions[in_function.value()];
                    target.insert(target.end(), func_cmds.begin(), func_cmds.end());
                } else {
                    res.cmds.insert(res.cmds.end(), func_cmds.begin(), func_cmds.end());
                }
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Unknown function " + func_name);
            }
            continue;
        }
        if (in_function.has_value()) {
            if (! (line[0] == '>' || line[0] == '=' || line[0] == '?' || line[0] == '&' || line[0] == '!' || line[0] == '@' || line[0] == '$' || line[0] == '\\' || starts_with(line, ">>") || starts_with(line, ">>>"))) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid in function");
                continue;
            }
        }
        bool parsed = false;
        if (starts_with(line, "//")) {
            parsed = true;
            if (in_function.has_value()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Deps not allowed in function");
                continue;
            }
            std::string dep = trim(line.substr(2));
            if (!dep.empty()) {
                res.deps[dep] = 1;
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty system dependency");
            }
        } else if (starts_with(line, "#")) {
            parsed = true;
            if (in_function.has_value()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Libs not allowed in function");
                continue;
            }
            std::string full_lib = trim(line.substr(1));
            if (full_lib.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty library/include");
                continue;
            }
            // Parse prefix if any
            std::string prefix = "";
            std::string lib_name = full_lib;
            size_t colon_pos = full_lib.find(':');
            if (colon_pos != std::string::npos) {
                prefix = trim(full_lib.substr(0, colon_pos));
                lib_name = trim(full_lib.substr(colon_pos + 1));
            } else {
                // Default to "bytes" if no prefix (or assume hacker-lang/bytes)
                prefix = "bytes";
            }
            if (lib_name.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty library name after prefix");
                continue;
            }
            // Handle based on prefix
            if (prefix == "rust") {
                res.rust_libs[lib_name] = 1;
            } else if (prefix == "python") {
                res.python_libs[lib_name] = 1;
            } else if (prefix == "java") {
                res.java_libs[lib_name] = 1;
            } else if (prefix == "bytes" || prefix.empty()) {  // Handle default or explicit bytes
                // Existing logic for bytes/hacker-lang libs
                fs::path lib_dir = hacker_dir / "libs" / lib_name;
                fs::path lib_hacker_path = lib_dir / "main.hacker";
                fs::path lib_bin_path = hacker_dir / "libs" / lib_name;
                if (fs::exists(lib_hacker_path)) {
                    res.includes.push_back(lib_name);
                    ParseResult sub = parse_hacker_file(lib_hacker_path.string(), verbose);
                    merge_maps(res.deps, sub.deps);
                    merge_maps(res.libs, sub.libs);
                    merge_maps(res.rust_libs, sub.rust_libs);  // Merge new fields
                    merge_maps(res.python_libs, sub.python_libs);
                    merge_maps(res.java_libs, sub.java_libs);
                    merge_string_maps(res.vars_dict, sub.vars_dict);
                    merge_string_maps(res.local_vars, sub.local_vars);
                    res.cmds.insert(res.cmds.end(), sub.cmds.begin(), sub.cmds.end());
                    res.cmds_with_vars.insert(res.cmds_with_vars.end(), sub.cmds_with_vars.begin(), sub.cmds_with_vars.end());
                    res.cmds_separate.insert(res.cmds_separate.end(), sub.cmds_separate.begin(), sub.cmds_separate.end());
                    res.includes.insert(res.includes.end(), sub.includes.begin(), sub.includes.end());
                    res.binaries.insert(res.binaries.end(), sub.binaries.begin(), sub.binaries.end());
                    res.plugins.insert(res.plugins.end(), sub.plugins.begin(), sub.plugins.end());
                    merge_function_maps(res.functions, sub.functions);
                    for (const auto& sub_err : sub.errors) {
                        res.errors.push_back("In " + lib_name + ": " + sub_err);
                    }
                }
                struct stat st;
                if (stat(lib_bin_path.c_str(), &st) == 0) {
                    if (st.st_mode & (S_IXUSR | S_IXGRP | S_IXOTH)) {
                        res.binaries.push_back(lib_bin_path.string());
                    } else {
                        res.libs[lib_name] = 1;
                    }
                } else {
                    res.libs[lib_name] = 1;
                }
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Unknown library prefix: " + prefix);
            }
        } else if (starts_with(line, ">>>")) {
            parsed = true;
            std::string cmd = trim(line.substr(3));
            size_t excl = cmd.find('!');
            if (excl != std::string::npos) {
                cmd = trim(cmd.substr(0, excl));
            }
            std::string mut_cmd = cmd;
            if (is_super) mut_cmd = "sudo " + mut_cmd;
            if (!mut_cmd.empty()) {
                auto& target = in_function.has_value() ? res.functions[in_function.value()] : res.cmds_separate;
                target.push_back(mut_cmd);
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty separate file command");
            }
        } else if (starts_with(line, ">>")) {
            parsed = true;
            std::string cmd = trim(line.substr(2));
            size_t excl = cmd.find('!');
            if (excl != std::string::npos) {
                cmd = trim(cmd.substr(0, excl));
            }
            std::string mut_cmd = cmd;
            if (is_super) mut_cmd = "sudo " + mut_cmd;
            if (!mut_cmd.empty()) {
                auto& target = in_function.has_value() ? res.functions[in_function.value()] : res.cmds_with_vars;
                target.push_back(mut_cmd);
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty command with vars");
            }
        } else if (starts_with(line, ">")) {
            parsed = true;
            std::string cmd = trim(line.substr(1));
            size_t excl = cmd.find('!');
            if (excl != std::string::npos) {
                cmd = trim(cmd.substr(0, excl));
            }
            std::string mut_cmd = cmd;
            if (is_super) mut_cmd = "sudo " + mut_cmd;
            if (!mut_cmd.empty()) {
                auto& target = in_function.has_value() ? res.functions[in_function.value()] : res.cmds;
                target.push_back(mut_cmd);
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty command");
            }
        } else if (starts_with(line, "@")) {
            parsed = true;
            size_t pos = 1;
            while (pos < line.size() && (std::isalnum(line[pos]) || line[pos] == '_')) ++pos;
            std::string key = line.substr(1, pos - 1);
            if (key.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid variable");
                continue;
            }
            std::string after = trim(line.substr(pos));
            if (!starts_with(after, "=")) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid variable");
                continue;
            }
            std::string value = trim(after.substr(1));
            if (value.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid variable");
                continue;
            }
            res.vars_dict[key] = value;
        } else if (starts_with(line, "$")) {
            parsed = true;
            size_t pos = 1;
            while (pos < line.size() && (std::isalnum(line[pos]) || line[pos] == '_')) ++pos;
            std::string key = line.substr(1, pos - 1);
            if (key.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid local variable");
                continue;
            }
            std::string after = trim(line.substr(pos));
            if (!starts_with(after, "=")) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid local variable");
                continue;
            }
            std::string value = trim(after.substr(1));
            if (value.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid local variable");
                continue;
            }
            res.local_vars[key] = value;
        } else if (starts_with(line, "\\")) {
            parsed = true;
            std::string plugin_name = trim(line.substr(1));
            if (plugin_name.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty plugin name");
                continue;
            }
            fs::path plugin_dir = hacker_dir / "plugins" / plugin_name;
            struct stat st;
            if (stat(plugin_dir.c_str(), &st) == 0 && (st.st_mode & (S_IXUSR | S_IXGRP | S_IXOTH))) {
                res.plugins.push_back({plugin_dir.string(), is_super});
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Plugin " + plugin_name + " not found or not executable");
            }
        } else if (starts_with(line, "=")) {
            parsed = true;
            size_t gt_pos = line.find('>');
            if (gt_pos == std::string::npos) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid loop syntax");
                continue;
            }
            std::string num_str = trim(line.substr(1, gt_pos - 1));
            std::string cmd_part = trim(line.substr(gt_pos + 1));
            size_t excl = cmd_part.find('!');
            if (excl != std::string::npos) {
                cmd_part = trim(cmd_part.substr(0, excl));
            }
            int num;
            try {
                num = std::stoi(num_str);
            } catch (...) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid loop count");
                continue;
            }
            if (num < 0) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Negative loop count");
                continue;
            }
            if (cmd_part.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty loop command");
                continue;
            }
            std::string cmd_base = cmd_part;
            if (is_super) cmd_base = "sudo " + cmd_base;
            auto& target = in_function.has_value() ? res.functions[in_function.value()] : res.cmds;
            for (int i = 0; i < num; ++i) {
                target.push_back(cmd_base);
            }
        } else if (starts_with(line, "?")) {
            parsed = true;
            size_t gt_pos = line.find('>');
            if (gt_pos == std::string::npos) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid conditional");
                continue;
            }
            std::string condition = trim(line.substr(1, gt_pos - 1));
            std::string cmd_part = trim(line.substr(gt_pos + 1));
            size_t excl = cmd_part.find('!');
            if (excl != std::string::npos) {
                cmd_part = trim(cmd_part.substr(0, excl));
            }
            if (condition.empty() || cmd_part.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid conditional");
                continue;
            }
            std::string cmd = cmd_part;
            if (is_super) cmd = "sudo " + cmd;
            std::string if_cmd = "if " + condition + "; then " + cmd + "; fi";
            auto& target = in_function.has_value() ? res.functions[in_function.value()] : res.cmds;
            target.push_back(if_cmd);
        } else if (starts_with(line, "&")) {
            parsed = true;
            std::string cmd_part = trim(line.substr(1));
            size_t excl = cmd_part.find('!');
            if (excl != std::string::npos) {
                cmd_part = trim(cmd_part.substr(0, excl));
            }
            if (cmd_part.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty background command");
                continue;
            }
            std::string cmd = cmd_part + " &";
            if (is_super) cmd = "sudo " + cmd;
            auto& target = in_function.has_value() ? res.functions[in_function.value()] : res.cmds;
            target.push_back(cmd);
        } else if (starts_with(line, "!")) {
            parsed = true;
            // ignore
        }
        if (!parsed) {
            res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid syntax");
        }
    }
    if (in_config) res.errors.push_back("Unclosed config section");
    if (in_comment) res.errors.push_back("Unclosed comment block");
    if (in_function.has_value()) res.errors.push_back("Unclosed function block");
    if (verbose) {
        if (!res.errors.empty()) {
            std::cout << "\n\033[31m\033[1mErrors:\033[0m" << std::endl;
            for (const auto& e : res.errors) {
                std::cout << " \033[31m✖ \033[0m" << e << std::endl;
            }
            std::cout << "" << std::endl;
        } else {
            std::cout << "\033[32mNo errors found.\033[0m" << std::endl;
        }
        std::cout << "System Deps: [";
        bool first = true;
        for (const auto& p : res.deps) {
            if (!first) std::cout << ", ";
            std::cout << p.first;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Custom Libs (Bytes): [";
        first = true;
        for (const auto& p : res.libs) {
            if (!first) std::cout << ", ";
            std::cout << p.first;
            first = false;
        }
        std::cout << "]" << std::endl;
        // Verbose for new libs
        std::cout << "Rust Libs: [";
        first = true;
        for (const auto& p : res.rust_libs) {
            if (!first) std::cout << ", ";
            std::cout << p.first;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Python Libs: [";
        first = true;
        for (const auto& p : res.python_libs) {
            if (!first) std::cout << ", ";
            std::cout << p.first;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Java Libs: [";
        first = true;
        for (const auto& p : res.java_libs) {
            if (!first) std::cout << ", ";
            std::cout << p.first;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Vars: {";
        first = true;
        for (const auto& p : res.vars_dict) {
            if (!first) std::cout << ", ";
            std::cout << p.first << ": " << p.second;
            first = false;
        }
        std::cout << "}" << std::endl;
        std::cout << "Local Vars: {";
        first = true;
        for (const auto& p : res.local_vars) {
            if (!first) std::cout << ", ";
            std::cout << p.first << ": " << p.second;
            first = false;
        }
        std::cout << "}" << std::endl;
        std::cout << "Cmds: [";
        first = true;
        for (const auto& c : res.cmds) {
            if (!first) std::cout << ", ";
            std::cout << c;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Cmds with Vars: [";
        first = true;
        for (const auto& c : res.cmds_with_vars) {
            if (!first) std::cout << ", ";
            std::cout << c;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Separate Cmds: [";
        first = true;
        for (const auto& c : res.cmds_separate) {
            if (!first) std::cout << ", ";
            std::cout << c;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Includes: [";
        first = true;
        for (const auto& i : res.includes) {
            if (!first) std::cout << ", ";
            std::cout << i;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Binaries: [";
        first = true;
        for (const auto& b : res.binaries) {
            if (!first) std::cout << ", ";
            std::cout << b;
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Plugins: [";
        first = true;
        for (const auto& p : res.plugins) {
            if (!first) std::cout << ", ";
            std::cout << "{path: " << p.path << ", super: " << (p.is_super ? "true" : "false") << "}";
            first = false;
        }
        std::cout << "]" << std::endl;
        std::cout << "Functions: {";
        first = true;
        for (const auto& f : res.functions) {
            if (!first) std::cout << ", ";
            std::cout << f.first << ": [";
            bool first2 = true;
            for (const auto& c : f.second) {
                if (!first2) std::cout << ", ";
                std::cout << c;
                first2 = false;
            }
            std::cout << "]";
            first = false;
        }
        std::cout << "}" << std::endl;
        std::cout << "Config: {";
        first = true;
        for (const auto& p : res.config_data) {
            if (!first) std::cout << ", ";
            std::cout << p.first << ": " << p.second;
            first = false;
        }
        std::cout << "}" << std::endl;
    }
    return res;
}

int main(int argc, char* argv[]) {
    bool verbose = false;
    std::string file_path;
    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "--verbose") {
            verbose = true;
        } else if (file_path.empty()) {
            file_path = arg;
        } else {
            std::cerr << "Usage: hacker-plsa [--verbose] <file>" << std::endl;
            return 1;
        }
    }
    if (file_path.empty()) {
        std::cerr << "Usage: hacker-plsa [--verbose] <file>" << std::endl;
        return 1;
    }
    ParseResult res = parse_hacker_file(file_path, verbose);
    output_json(res);
    return 0;
}
