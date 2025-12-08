#include <iostream>
#include <fstream>
#include <sstream>
#include <vector>
#include <map>
#include <set>
#include <string>
#include <algorithm>
#include <filesystem>
#include <regex>
#include <optional>
#include <json/json.h> // Assume jsoncpp library
#include <unistd.h> // For getuid
#include <sys/stat.h>
#include <cstdlib>   // For std::getenv, std::stoi

namespace fs = std::filesystem;

const std::string HACKER_DIR = []() -> std::string {
    const char* home = std::getenv("HOME");
    return home ? std::string(home) + "/.hackeros/hacker-lang" : "~/.hackeros/hacker-lang";
}();

const std::string LIBS_DIR = HACKER_DIR + "/libs";

struct Plugin {
    std::string path;
    bool is_super;
};

struct ParseResult {
    std::set<std::string> deps;
    std::set<std::string> libs;
    std::map<std::string, std::string> vars;
    std::map<std::string, std::string> local_vars;
    std::vector<std::string> cmds;
    std::vector<std::string> cmds_with_vars;
    std::vector<std::string> cmds_separate;
    std::vector<std::string> includes;
    std::vector<std::string> binaries;
    std::vector<Plugin> plugins;
    std::map<std::string, std::vector<std::string>> functions;
    std::vector<std::string> errors;
    std::map<std::string, std::string> config;
};

std::string expand_home(const std::string& path) {
    if (path.rfind("~", 0) == 0) {
        const char* home = std::getenv("HOME");
        if (home) {
            return std::string(home) + path.substr(1);
        }
    }
    return path;
}

std::string trim(const std::string& str) {
    size_t first = str.find_first_not_of(" \t\n\r\f\v");
    if (first == std::string::npos) return "";
    size_t last = str.find_last_not_of(" \t\n\r\f\v");
    return str.substr(first, (last - first + 1));
}

std::pair<std::string, std::string> split_once(const std::string& s, char delim) {
    size_t pos = s.find(delim);
    if (pos == std::string::npos) {
        return {s, ""};
    }
    return {s.substr(0, pos), s.substr(pos + 1)};
}

enum LineType {
    DEP, LIB, CMD, CMD_VARS, CMD_SEPARATE, VAR, LOCAL_VAR, PLUGIN,
    LOOP, CONDITIONAL, BACKGROUND, IGNORE, FUNCTION_START, FUNCTION_END,
    FUNCTION_CALL, CONFIG_START, CONFIG_END, COMMENT_TOGGLE
};

LineType classify_line(const std::string& l) {
    if (l.empty()) return IGNORE;
    if (l == "!!") return COMMENT_TOGGLE;
    if (l == "[") return CONFIG_START;
    if (l == "]") return CONFIG_END;
    if (l == ":") return FUNCTION_END;
    if (l.rfind(":", 0) == 0) return FUNCTION_START;
    if (l.rfind(".", 0) == 0) return FUNCTION_CALL;
    if (l.rfind("//", 0) == 0) return DEP;
        if (l[0] == '#') return LIB;
        if (l.rfind(">>>", 0) == 0) return CMD_SEPARATE;
        if (l.rfind(">>", 0) == 0) return CMD_VARS;
        if (l.rfind(">", 0) == 0) return CMD;
        if (l.rfind("@", 0) == 0) return VAR;
        if (l.rfind("$", 0) == 0) return LOCAL_VAR;
        if (l.rfind("\\", 0) == 0) return PLUGIN;
        if (l.rfind("=", 0) == 0) return LOOP;
        if (l.rfind("?", 0) == 0) return CONDITIONAL;
        if (l.rfind("&", 0) == 0) return BACKGROUND;
        if (l.rfind("!", 0) == 0) return IGNORE;
        return IGNORE; // unknown → will be reported as invalid syntax
}

std::string parse_cmd_part(const std::string& line) {
    size_t excl_pos = line.find('!');
    if (excl_pos != std::string::npos) {
        return trim(line.substr(0, excl_pos));
    }
    return trim(line);
}

bool parse_foreign_lib(const std::string& line, std::string& lib_name) {
    if (line.rfind("#>", 0) == 0) {
        lib_name = trim(line.substr(2));
        return true; // foreign
    }
    if (line.rfind("#", 0) == 0) {
        lib_name = trim(line.substr(1));
        return false;
    }
    lib_name = "";
    return false;
}

void handle_foreign_lib(const std::string& lib_name, const std::string& mode, ParseResult& res) {
    std::string cache_dir = (mode == "hli") ? "./.cache" : "/tmp/hacker_cache_" + std::to_string(getuid());
    fs::create_directories(cache_dir);
    std::string lib_path = cache_dir + "/" + lib_name;

    if (!fs::exists(lib_path)) {
        std::cout << "Downloading foreign lib: " << lib_name << " to " << lib_path << std::endl;
        // Real implementation would use curl/libcurl here
    }
    res.includes.push_back(lib_path);
    res.libs.insert(lib_name);
}

void merge_results(ParseResult& target, const ParseResult& src, const std::string& lib_name) {
    for (const auto& d : src.deps) target.deps.insert(d);
    for (const auto& l : src.libs) target.libs.insert(l);
    for (const auto& [k, v] : src.vars) target.vars[k] = v;
    for (const auto& [k, v] : src.local_vars) target.local_vars[k] = v;
    target.cmds.insert(target.cmds.end(), src.cmds.begin(), src.cmds.end());
    target.cmds_with_vars.insert(target.cmds_with_vars.end(), src.cmds_with_vars.begin(), src.cmds_with_vars.end());
    target.cmds_separate.insert(target.cmds_separate.end(), src.cmds_separate.begin(), src.cmds_separate.end());
    target.includes.insert(target.includes.end(), src.includes.begin(), src.includes.end());
    target.binaries.insert(target.binaries.end(), src.binaries.begin(), src.binaries.end());
    target.plugins.insert(target.plugins.end(), src.plugins.begin(), src.plugins.end());
    for (const auto& [k, vec] : src.functions) {
        target.functions[k].insert(target.functions[k].end(), vec.begin(), vec.end());
    }
    for (const auto& e : src.errors) {
        target.errors.push_back("In " + lib_name + ": " + e);
    }
    for (const auto& [k, v] : src.config) target.config[k] = v;
}

ParseResult parse_hacker_file(const std::string& file_path, bool verbose, bool bytes_mode, const std::string& mode = "hli") {
    ParseResult res;
    bool in_config = false;
    bool in_comment = false;
    std::optional<std::string> in_function = std::nullopt;
    size_t line_num = 0;

    std::ifstream file(file_path);
    if (!file.is_open()) {
        res.errors.push_back("File " + file_path + " not found");
        return res;
    }

    std::string line;
    while (std::getline(file, line)) {
        ++line_num;
        std::string raw_line = line;
        line = trim(line);
        if (line.empty()) continue;

        // Handle super user prefix ^
        bool is_super = false;
        std::string content = line;
        if (content.rfind("^", 0) == 0) {
            is_super = true;
            content = trim(content.substr(1));
            if (content.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Lone ^ is invalid");
                continue;
            }
        }

        LineType lt = classify_line(content);
        std::string clean_line = content;

        if (lt == COMMENT_TOGGLE) {
            in_comment = !in_comment;
            continue;
        }
        if (in_comment) continue;

        if (lt == CONFIG_START) {
            if (in_config || in_function.has_value()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Config block cannot be nested");
            }
            in_config = true;
            continue;
        }
        if (lt == CONFIG_END) {
            if (!in_config) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Unmatched ]");
            }
            in_config = false;
            continue;
        }

        if (in_config) {
            auto [key, value] = split_once(clean_line, '=');
            key = trim(key);
            value = trim(value);
            if (!key.empty()) {
                res.config[key] = value;
            }
            continue;
        }

        // Function handling
        if (lt == FUNCTION_END) {
            if (in_function.has_value()) {
                in_function = std::nullopt;
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Unmatched function end ':'");
            }
            continue;
        }

        if (lt == FUNCTION_START) {
            std::string func_name = trim(clean_line.substr(1));
            if (func_name.empty() || in_function.has_value()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid function definition");
                continue;
            }
            res.functions[func_name] = {};
            in_function = func_name;
            continue;
        }

        if (lt == FUNCTION_CALL) {
            std::string func_name = trim(clean_line.substr(1));
            if (func_name.empty()) {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Empty function call");
                continue;
            }
            auto it = res.functions.find(func_name);
            if (it != res.functions.end()) {
                auto& target = in_function.has_value() ? res.functions.at(*in_function) : res.cmds;
                target.insert(target.end(), it->second.begin(), it->second.end());
            } else {
                res.errors.push_back("Line " + std::to_string(line_num) + ": Unknown function '" + func_name + "'");
            }
            continue;
        }

        // Restrict certain lines inside functions
        if (in_function.has_value() && lt != CMD && lt != CMD_VARS && lt != CMD_SEPARATE &&
            lt != LOOP && lt != CONDITIONAL && lt != BACKGROUND && lt != VAR && lt != LOCAL_VAR && lt != PLUGIN) {
            res.errors.push_back("Line " + std::to_string(line_num) + ": This line type is not allowed inside a function");
        continue;
            }

            switch (lt) {
                case DEP: {
                    std::string dep = trim(clean_line.substr(2));
                    if (dep.empty()) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Empty dependency");
                    } else if (in_function.has_value()) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Dependencies cannot be inside functions");
                    } else {
                        res.deps.insert(dep);
                    }
                    break;
                }
                case LIB: {
                    std::string lib_name;
                    bool is_foreign = parse_foreign_lib(clean_line, lib_name);
                    if (lib_name.empty() || in_function.has_value()) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid or misplaced library declaration");
                        break;
                    }

                    std::string lib_dir = expand_home(LIBS_DIR + "/" + lib_name);
                    std::string lib_hacker = lib_dir + "/main.hacker";

                    if (fs::exists(lib_hacker)) {
                        res.includes.push_back(lib_name);
                        ParseResult sub = parse_hacker_file(lib_hacker, verbose, bytes_mode, mode);
                        merge_results(res, sub, lib_name);
                    }

                    struct stat st;
                    std::string lib_bin_path = lib_dir;
                    if (stat(lib_bin_path.c_str(), &st) == 0 && (st.st_mode & S_IXUSR)) {
                        if (bytes_mode) {
                            std::cout << "Embedding binary lib: " << lib_bin_path << std::endl;
                        }
                        res.binaries.push_back(lib_bin_path);
                    } else {
                        res.libs.insert(lib_name);
                    }

                    if (is_foreign) {
                        handle_foreign_lib(lib_name, mode, res);
                    }
                    break;
                }
                case CMD: {
                    std::string cmd = parse_cmd_part(clean_line.substr(1));
                    if (is_super) cmd = "sudo " + cmd;
                    if (!cmd.empty()) {
                        auto& target = in_function.has_value() ? res.functions.at(*in_function) : res.cmds;
                        target.push_back(cmd);
                    } else {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Empty command");
                    }
                    break;
                }
                case CMD_VARS: {
                    std::string cmd = parse_cmd_part(clean_line.substr(2));
                    if (is_super) cmd = "sudo " + cmd;
                    if (!cmd.empty()) {
                        auto& target = in_function.has_value() ? res.functions.at(*in_function) : res.cmds_with_vars;
                        target.push_back(cmd);
                    } else {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Empty >> command");
                    }
                    break;
                }
                case CMD_SEPARATE: {
                    std::string cmd = parse_cmd_part(clean_line.substr(3));
                    if (is_super) cmd = "sudo " + cmd;
                    if (!cmd.empty()) {
                        auto& target = in_function.has_value() ? res.functions.at(*in_function) : res.cmds_separate;
                        target.push_back(cmd);
                    } else {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Empty >>> command");
                    }
                    break;
                }
                case VAR: {
                    size_t eq = clean_line.find('=', 1);
                    if (eq != std::string::npos) {
                        std::string key = trim(clean_line.substr(1, eq - 1));
                        std::string value = trim(clean_line.substr(eq + 1));
                        if (!key.empty()) {
                            res.vars[key] = value;
                        } else {
                            res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid global variable syntax");
                        }
                    } else {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Missing = in global variable");
                    }
                    break;
                }
                case LOCAL_VAR: {
                    size_t eq = clean_line.find('=', 1);
                    if (eq != std::string::npos) {
                        std::string key = trim(clean_line.substr(1, eq - 1));
                        std::string value = trim(clean_line.substr(eq + 1));
                        if (!key.empty()) {
                            res.local_vars[key] = value;
                        } else {
                            res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid local variable syntax");
                        }
                    } else {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Missing = in local variable");
                    }
                    break;
                }
                case PLUGIN: {
                    std::string plugin_name = trim(clean_line.substr(1));
                    if (plugin_name.empty()) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Empty plugin name");
                        break;
                    }
                    std::string plugin_path = expand_home(HACKER_DIR + "/plugins/" + plugin_name);
                    struct stat st;
                    if (stat(plugin_path.c_str(), &st) == 0 && (st.st_mode & S_IXUSR)) {
                        res.plugins.push_back({plugin_path, is_super});
                        if (verbose) std::cout << "Loaded plugin: " << plugin_name << std::endl;
                    } else {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Plugin '" + plugin_name + "' not found or not executable");
                    }
                    break;
                }
                case LOOP: {
                    size_t gt = clean_line.find('>', 1);
                    if (gt == std::string::npos) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid loop syntax (missing >)");
                        break;
                    }
                    std::string num_str = trim(clean_line.substr(1, gt - 1));
                    std::string cmd_part = parse_cmd_part(clean_line.substr(gt + 1));
                    try {
                        int num = std::stoi(num_str);
                        if (num > 0 && !cmd_part.empty()) {
                            std::string cmd = is_super ? "sudo " + cmd_part : cmd_part;
                            auto& target = in_function.has_value() ? res.functions.at(*in_function) : res.cmds;
                            for (int i = 0; i < num; ++i) {
                                target.push_back(cmd);
                            }
                        } else {
                            res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid loop parameters");
                        }
                    } catch (...) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid loop count");
                    }
                    break;
                }
                case CONDITIONAL: {
                    size_t gt = clean_line.find('>', 1);
                    if (gt == std::string::npos) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid conditional syntax (missing >)");
                        break;
                    }
                    std::string condition = trim(clean_line.substr(1, gt - 1));
                    std::string cmd_part = parse_cmd_part(clean_line.substr(gt + 1));
                    if (condition.empty() || cmd_part.empty()) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Empty condition or command in conditional");
                        break;
                    }
                    std::string cmd = is_super ? "sudo " + cmd_part : cmd_part;
                    std::string if_cmd = "if " + condition + "; then " + cmd + "; fi";
                    auto& target = in_function.has_value() ? res.functions.at(*in_function) : res.cmds;
                    target.push_back(if_cmd);
                    break;
                }
                case BACKGROUND: {
                    std::string cmd_part = parse_cmd_part(clean_line.substr(1));
                    if (cmd_part.empty()) {
                        res.errors.push_back("Line " + std::to_string(line_num) + ": Empty background command");
                        break;
                    }
                    std::string cmd = (is_super ? "sudo " : "") + cmd_part + " &";
                    auto& target = in_function.has_value() ? res.functions.at(*in_function) : res.cmds;
                    target.push_back(cmd);
                    break;
                }
                case IGNORE:
                    break;
                default:
                    res.errors.push_back("Line " + std::to_string(line_num) + ": Invalid syntax");
                    break;
            }
    }

    if (in_config) res.errors.push_back("Unclosed config block");
    if (in_comment) res.errors.push_back("Unclosed comment block");
    if (in_function.has_value()) res.errors.push_back("Unclosed function '" + *in_function + "'");

    if (verbose && !res.errors.empty()) {
        std::cout << "Errors:\n";
        for (const auto& e : res.errors) std::cout << "  " << e << "\n";
    }

    return res;
}

Json::Value to_json(const ParseResult& res) {
    Json::Value root(Json::objectValue);

    Json::Value deps(Json::arrayValue);
    for (const auto& d : res.deps) deps.append(d);
    root["deps"] = deps;

    Json::Value libs(Json::arrayValue);
    for (const auto& l : res.libs) libs.append(l);
    root["libs"] = libs;

    Json::Value vars(Json::objectValue);
    for (const auto& [k, v] : res.vars) vars[k] = v;
    root["vars"] = vars;

    Json::Value local_vars(Json::objectValue);
    for (const auto& [k, v] : res.local_vars) local_vars[k] = v;
    root["local_vars"] = local_vars;

    Json::Value cmds(Json::arrayValue);
    for (const auto& c : res.cmds) cmds.append(c);
    root["cmds"] = cmds;

    Json::Value cmds_wv(Json::arrayValue);
    for (const auto& c : res.cmds_with_vars) cmds_wv.append(c);
    root["cmds_with_vars"] = cmds_wv;

    Json::Value cmds_sep(Json::arrayValue);
    for (const auto& c : res.cmds_separate) cmds_sep.append(c);
    root["cmds_separate"] = cmds_sep;

    Json::Value includes(Json::arrayValue);
    for (const auto& i : res.includes) includes.append(i);
    root["includes"] = includes;

    Json::Value binaries(Json::arrayValue);
    for (const auto& b : res.binaries) binaries.append(b);
    root["binaries"] = binaries;

    Json::Value plugins(Json::arrayValue);
    for (const auto& p : res.plugins) {
        Json::Value pl(Json::objectValue);
        pl["path"] = p.path;
        pl["super"] = p.is_super;
        plugins.append(pl);
    }
    root["plugins"] = plugins;

    Json::Value functions(Json::objectValue);
    for (const auto& [k, vec] : res.functions) {
        Json::Value fvec(Json::arrayValue);
        for (const auto& c : vec) fvec.append(c);
        functions[k] = fvec;
    }
    root["functions"] = functions;

    Json::Value errors(Json::arrayValue);
    for (const auto& e : res.errors) errors.append(e);
    root["errors"] = errors;

    Json::Value config(Json::objectValue);
    for (const auto& [k, v] : res.config) config[k] = v;
    root["config"] = config;

    return root;
}

int main(int argc, char* argv[]) {
    bool verbose = false;
    std::string file_path;
    std::string mode = "hli";

    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "--verbose") {
            verbose = true;
        } else if (arg == "--mode" && i + 1 < argc) {
            mode = argv[++i];
        } else if (file_path.empty()) {
            file_path = arg;
        }
    }

    if (file_path.empty()) {
        std::cerr << "Usage: hacker-parser [--verbose] [--mode hli|hackerc] <file.hacker>\n";
        return 1;
    }

    ParseResult res = parse_hacker_file(file_path, verbose, false, mode);
    Json::Value json_res = to_json(res);

    Json::StreamWriterBuilder builder;
    builder["indentation"] = "  ";
    std::cout << Json::writeString(builder, json_res) << std::endl;

    return res.errors.empty() ? 0 : 1;
}
