#include <iostream>
#include <fstream>
#include <sstream>
#include <vector>
#include <string>
#include <algorithm>
#include <filesystem>
#include <regex>
#include <json/json.h> // jsoncpp
#include <cctype>

namespace fs = std::filesystem;

struct Token {
    std::string type;
    std::string value;
    size_t line;
    size_t col;
};

std::string trim(const std::string& str) {
    size_t first = str.find_first_not_of(" \t");
    if (first == std::string::npos) return "";
    size_t last = str.find_last_not_of(" \t");
    return str.substr(first, (last - first + 1));
}

class HackerLexer {
private:
    std::vector<Token> tokens;
    size_t current_line = 1;

    // Naprawiona funkcja – teraz przyjmuje linię i kolumnę jawnie
    void add_token(const std::string& type, const std::string& value, size_t line, size_t col) {
        tokens.push_back({type, value, line, col});
    }

    void new_line() {
        ++current_line;
    }

    std::string extract_cmd_part(const std::string& line, size_t start, size_t& excl_pos) {
        excl_pos = line.find('!', start);
        if (excl_pos != std::string::npos) {
            return trim(line.substr(start, excl_pos - start));
        }
        return trim(line.substr(start));
    }

    void lex_line(const std::string& line) {
        std::string l = line;
        size_t col = 1;
        size_t pos = 0;

        // Pomijamy leading whitespace i liczymy kolumnę
        while (pos < l.size() && std::isspace(l[pos])) {
            ++col;
            ++pos;
        }

        // Cała linia to tylko whitespace?
        if (pos >= l.size()) {
            if (!l.empty()) { // nie dodajemy tokena dla pustej linii
                add_token("WHITESPACE", l.substr(0, pos), current_line, 1);
            }
            return;
        }

        // Specjalne przypadki – cała linia
        std::string trimmed_line = trim(l);
        if (trimmed_line == "!!") {
            add_token("COMMENT_TOGGLE", "!!", current_line, col);
            return;
        }
        if (trimmed_line == "[") {
            add_token("CONFIG_START", "[", current_line, col);
            return;
        }
        if (trimmed_line == "]") {
            add_token("CONFIG_END", "]", current_line, col);
            return;
        }

        // Function definition / end
        if (l[pos] == ':') {
            if (pos + 1 < l.size()) {
                std::string func_name = trim(l.substr(pos + 1));
                add_token("FUNCTION_START", func_name, current_line, col);
            } else {
                add_token("FUNCTION_END", ":", current_line, col);
            }
            return;
        }

        // Function call
        if (l[pos] == '.') {
            std::string func_name = trim(l.substr(pos + 1));
            add_token("FUNCTION_CALL", func_name, current_line, col);
            return;
        }

        // Super prefix ^
        bool is_super = false;
        if (l[pos] == '^') {
            is_super = true;
            ++col;
            ++pos;
            while  (pos < l.size() && std::isspace(l[pos])) {
                ++col;
                ++pos;
            }
        }

        // Główna logika tokenów
        if (pos >= l.size()) {
            // nic po ^ i spacjach
            if (is_super) add_token("SUPER", "", current_line, col - 1);
            return;
        }

        char ch = l[pos];

        if (ch == '/' && pos + 1 < l.size() && l[pos + 1] == '/') {
            // Dep  //
            std::string dep = trim(l.substr(pos + 2));
            add_token("DEP", dep, current_line, col);
            return;
        }
        if (ch == '#') {
            // Lib  # lub #>
            size_t start = pos + 1;
            std::string prefix = "#";
            if (pos + 1 < l.size() && l[pos + 1] == '>') {
                prefix = "#>";
                start = pos + 2;
            }
            std::string lib_name = trim(l.substr(start));
            add_token("LIB", lib_name, current_line, col);
            if (prefix == "#>") {
                add_token("FOREIGN_LIB", "", current_line, col + 1); // marker
            }
            return;
        }
        if (ch == '@') {
            // Var
            size_t eq_pos = l.find('=', pos + 1);
            if (eq_pos != std::string::npos) {
                std::string key = trim(l.substr(pos + 1, eq_pos - pos - 1));
                std::string value = trim(l.substr(eq_pos + 1));
                add_token("VAR", key + "=" + value, current_line, col);
                return;
            }
        }
        if (ch == '$') {
            // Local var
            size_t eq_pos = l.find('=', pos + 1);
            if (eq_pos != std::string::npos) {
                std::string key = trim(l.substr(pos + 1, eq_pos - pos - 1));
                std::string value = trim(l.substr(eq_pos + 1));
                add_token("LOCAL_VAR", key + "=" + value, current_line, col);
                return;
            }
        }
        if (ch == '>') {
            // Cmd
            size_t excl_pos;
            std::string cmd = extract_cmd_part(l, pos + 1, excl_pos);
            add_token("CMD", cmd, current_line, col);
            if (is_super) add_token("SUPER", "", current_line, col - 1);
            return;
        }
        if (std::string(l.data() + pos, 2) == ">>") {
            // Cmd vars
            size_t excl_pos;
            std::string cmd = extract_cmd_part(l, pos + 2, excl_pos);
            add_token("CMD_VARS", cmd, current_line, col);
            if (is_super) add_token("SUPER", "", current_line, col - 1);
            return;
        }
        if (std::string(l.data() + pos, 3) == ">>>") {
            // Cmd separate
            size_t excl_pos;
            std::string cmd = extract_cmd_part(l, pos + 3, excl_pos);
            add_token("CMD_SEPARATE", cmd, current_line, col);
            if (is_super) add_token("SUPER", "", current_line, col - 1);
            return;
        }
        if (ch == '=') {
            // Loop  =N>cmd
            size_t gt_pos = l.find('>', pos + 1);
            if (gt_pos != std::string::npos) {
                std::string num_str = trim(l.substr(pos + 1, gt_pos - pos - 1));
                size_t excl_pos;
                std::string cmd = extract_cmd_part(l, gt_pos + 1, excl_pos);
                add_token("LOOP", num_str + ">" + cmd, current_line, col);
                if (is_super) add_token("SUPER", "", current_line, col - 1);
                return;
            }
        }
        if (ch == '?') {
            // Conditional  ?cond>cmd
            size_t gt_pos = l.find('>', pos + 1);
            if (gt_pos != std::string::npos) {
                std::string cond = trim(l.substr(pos + 1, gt_pos - pos - 1));
                size_t excl_pos;
                std::string cmd = extract_cmd_part(l, gt_pos + 1, excl_pos);
                add_token("CONDITIONAL", cond + ">" + cmd, current_line, col);
                if (is_super) add_token("SUPER", "", current_line, col - 1);
                return;
            }
        }
        if (ch == '&') {
            // Background
            size_t excl_pos;
            std::string cmd = extract_cmd_part(l, pos + 1, excl_pos);
            add_token("BACKGROUND", cmd, current_line, col);
            if (is_super) add_token("SUPER", "", current_line, col - 1);
            return;
        }
        if (ch == '\\') {
            // Plugin
            std::string plugin_name = trim(l.substr(pos + 1));
            add_token("PLUGIN", plugin_name, current_line, col);
            if (is_super) add_token("SUPER", "", current_line, col - 1);
            return;
        }
        if (ch == '!') {
            // Comment
            add_token("COMMENT", trim(l.substr(pos + 1)), current_line, col);
            return;
        }

        // Domyślnie TEXT
        std::string rest = trim(l.substr(pos));
        if (!rest.empty()) {
            add_token("TEXT", rest, current_line, col);
        }
    }

public:
    std::vector<Token> lex_file(const std::string& file_path) {
        tokens.clear();
        current_line = 1;

        std::ifstream file(file_path);
        if (!file.is_open()) {
            add_token("ERROR", "File not found: " + file_path, 0, 0);
            return tokens;
        }

        std::string line;
        while (std::getline(file, line)) {
            lex_line(line);
            new_line();
        }
        return tokens;
    }
};

Json::Value tokens_to_json(const std::vector<Token>& tokens) {
    Json::Value root(Json::arrayValue);
    for (const auto& t : tokens) {
        Json::Value tok(Json::objectValue);
        tok["type"] = t.type;
        tok["value"] = t.value;
        tok["line"] = static_cast<Json::UInt>(t.line);
        tok["col"] = static_cast<Json::UInt>(t.col);
        root.append(tok);
    }
    return root;
}

int main(int argc, char* argv[]) {
    bool verbose = false;
    std::string file_path;

    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "--verbose") {
            verbose = true;
        } else {
            file_path = arg;
        }
    }

    if (file_path.empty()) {
        std::cerr << "Usage: hacker-lexer [--verbose] <file>" << std::endl;
        return 1;
    }

    HackerLexer lexer;
    auto tokens = lexer.lex_file(file_path);

    if (verbose) {
        std::cout << "Tokens:\n";
        for (const auto& t : tokens) {
            std::cout << "[" << t.line << ":" << t.col << "] " << t.type << ": '" << t.value << "'\n";
        }
    }

    Json::Value json_tokens = tokens_to_json(tokens);
    // ładny wydruk JSON-a
    std::cout << Json::StyledWriter().write(json_tokens);
    return 0;
}

