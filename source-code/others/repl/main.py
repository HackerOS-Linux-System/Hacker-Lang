import os
import re
import subprocess
import tempfile
from pathlib import Path
from typing import Dict, List, Optional, Tuple
from prompt_toolkit import PromptSession
from prompt_toolkit.auto_suggest import AutoSuggestFromHistory
from prompt_toolkit.completion import Completer, Completion
from prompt_toolkit.document import Document
from prompt_toolkit.formatted_text import FormattedText
from prompt_toolkit.history import FileHistory
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.lexers import Lexer
from prompt_toolkit.styles import Style
from prompt_toolkit.validation import ValidationError, Validator
from rich.console import Console
from rich.panel import Panel
from rich.syntax import Syntax
from rich.table import Table
from rich.text import Text
# ─────────────────────────────────────────────────────────────
# Konfiguracja
# ─────────────────────────────────────────────────────────────
VERSION = "1.7.5"
HACKER_DIR = Path.home() / ".hackeros" / "hacker-lang"
LIBS_DIR = HACKER_DIR / "libs"
PLUGINS_DIR = HACKER_DIR / "plugins"
BIN_DIR = HACKER_DIR / "bin"
HISTORY_DIR = Path.home() / ".hackeros" / "history"
HISTORY_FILE = HISTORY_DIR / "hacker_repl_history"
console = Console()
# ─────────────────────────────────────────────────────────────
# Kolory (zgodnie z wymaganiami)
# success → jasny szary (było zielony)
# info → biały (było niebieski)
# prompt → czerwony (było fioletowy)
# ─────────────────────────────────────────────────────────────
C_SUCCESS = "bright_white"
C_ERROR = "bold red"
C_WARN = "yellow"
C_INFO = "white"
C_DIM = "bright_black"
C_HL = "red"
# ─────────────────────────────────────────────────────────────
# Słowa kluczowe i wzorce składni v9
# ─────────────────────────────────────────────────────────────
HL_KEYWORDS = [
    "spawn", "await", "assert", "match", "out", "log", "end",
    "try", "catch", "while", "for", "in", "import", "extern",
    "static", "struct", "lock", "unlock", "plugin", "if",
    "elif", "else", "true", "false", "nil",
]
HL_PREFIXES = [
    "> ", ">> ", "@", "%", "$", "=",
    "?? ", "?: ", "? ", "& ", "..", "..",
    "( ", "^ ", "\\ ", "#", "//", "==",
    "<< ", "-- ", "--|>",
]
# Komendy shell które REPL podpowiada
SHELL_CMDS = [
    "ls", "cd", "pwd", "echo", "cat", "grep", "find", "sed", "awk",
    "mkdir", "rm", "cp", "mv", "chmod", "chown", "ps", "kill",
    "curl", "wget", "git", "make", "cargo", "python3", "node",
    "docker", "systemctl", "journalctl", "tar", "zip", "unzip",
    "ssh", "scp", "rsync", "ping", "ip", "netstat", "df", "du",
    "top", "htop", "env", "export", "source", "which", "man",
]
# REPL meta-komendy
REPL_CMDS = [
    ":help", ":clear", ":verbose", ":reset", ":history",
    ":show", ":vars", ":libs", ":quit", ":exit",
    ":multiline", ":run", ":save", ":load",
]
# ─────────────────────────────────────────────────────────────
# Lexer składni HL dla kolorowania w prompt_toolkit
# ─────────────────────────────────────────────────────────────
class HackerLangLexer(Lexer):
    """Koloruje składnię Hacker Lang bezpośrednio w polu input."""
    # Token → klasa CSS używana w prompt_toolkit Style
    def lex_document(self, document: Document):
        def get_line(lineno):
            line = document.lines[lineno]
            return list(self._tokenize(line))
        return get_line
    def _tokenize(self, line: str):
        s = line
        # Komentarz
        if s.strip().startswith("!"):
            yield ("class:comment", s)
            return
        # Prefiks ^ (sudo)
        if s.startswith("^"):
            yield ("class:sudo", "^")
            s = s[1:]
        # % stała
        if s.startswith("%"):
            yield ("class:const", "%")
            rest = s[1:]
            m = re.match(r"(\w+)\s*=\s*(.*)", rest)
            if m:
                yield ("class:varname", m.group(1))
                yield ("class:op", " = ")
                yield ("class:string", m.group(2))
            else:
                yield ("class:varname", rest)
            return
        # @ env var
        if s.startswith("@"):
            yield ("class:envvar", "@")
            rest = s[1:]
            m = re.match(r"(\w+)\s*=\s*(.*)", rest)
            if m:
                yield ("class:varname", m.group(1))
                yield ("class:op", " = ")
                yield ("class:string", m.group(2))
            else:
                yield ("class:varname", rest)
            return
        # spawn / await / assert
        for kw in ["spawn", "await", "assert"]:
            if s.startswith(kw + " ") or s == kw:
                yield ("class:keyword", kw)
                yield ("class:text", s[len(kw):])
                return
        # Pipe chain .a |> .b
        if "|>" in s:
            parts = s.split("|>")
            for i, p in enumerate(parts):
                if i > 0:
                    yield ("class:pipe", " |> ")
                yield ("class:call", p.strip())
            return
        # Wywołanie funkcji .func
        if s.startswith("."):
            m = re.match(r"(.[A-Za-z*]\w*(.[A-Za-z*]\w*)*)(.*)", s)
            if m:
                yield ("class:call", m.group(1))
                yield ("class:text", m.group(3))
                return
        # Pętla = N > cmd
        if s.startswith("="):
            m = re.match(r"=\s*(\d+)\s*>\s*(.*)", s)
            if m:
                yield ("class:op", "=")
                yield ("class:number", m.group(1))
                yield ("class:op", " > ")
                yield ("class:command", m.group(2))
                return
        # Warunki
        for pref, cls in [("?? ", "elif"), ("?: ", "else"), ("? ", "if")]:
            if s.startswith(pref):
                yield ("class:keyword", pref.strip())
                yield ("class:text", " " + s[len(pref):])
                return
        # > / >> raw command
        if s.startswith(">>"):
            yield ("class:prefix", ">>")
            yield ("class:command", s[2:])
            return
        if s.startswith(">"):
            yield ("class:prefix", ">")
            yield ("class:command", s[1:])
            return
        # Słowa kluczowe na początku linii
        first = s.split()[0] if s.split() else ""
        if first in HL_KEYWORDS:
            yield ("class:keyword", first)
            yield ("class:text", s[len(first):])
            return
        # Domyślnie — zwykły tekst
        yield ("class:text", s)
# ─────────────────────────────────────────────────────────────
# Completer — autouzupełnianie składni HL
# ─────────────────────────────────────────────────────────────
class HackerLangCompleter(Completer):
    def __init__(self, repl: "HackerREPL"):
        self.repl = repl
    def get_completions(self, document: Document, complete_event):
        text = document.text_before_cursor
        word = document.get_word_before_cursor(WORD=True)
        lstrip = text.lstrip()
        # ── :repl meta-komendy ────────────────────────────────
        if text.startswith(":"):
            for cmd in REPL_CMDS:
                if cmd.startswith(text):
                    yield Completion(
                        cmd[len(text):],
                        display=cmd,
                        display_meta="repl command",
                        style="fg:red",
                    )
            return
        # ── Wywołanie funkcji HL (.func) ──────────────────────
        if lstrip.startswith("."):
            prefix = lstrip[1:]
            for fn in self.repl.functions:
                if fn.startswith(prefix):
                    display = f".{fn}"
                    yield Completion(
                        fn[len(prefix):],
                        display=display,
                        display_meta="hl function",
                        style="fg:cyan",
                    )
            return
        # ── Stałe i zmienne ($, %) ────────────────────────────
        if word.startswith("$") or word.startswith("%"):
            prefix = word[1:]
            pool = (
                list(self.repl.variables.keys()) +
                list(self.repl.constants.keys())
            )
            for v in pool:
                if v.startswith(prefix):
                    sigil = word[0]
                    yield Completion(
                        v[len(prefix):],
                        display=f"{sigil}{v}",
                        display_meta="variable" if sigil == "$" else "const",
                        style="fg:yellow",
                    )
            return
        # ── Prefiks składni HL ────────────────────────────────
        for pfx in HL_PREFIXES:
            if pfx.startswith(text) and pfx != text:
                yield Completion(
                    pfx[len(text):],
                    display=pfx,
                    display_meta="syntax",
                    style="fg:bright_white",
                )
        # ── Słowa kluczowe ────────────────────────────────────
        for kw in HL_KEYWORDS:
            if kw.startswith(word) and word:
                yield Completion(
                    kw[len(word):],
                    display=kw,
                    display_meta="keyword",
                    style="fg:white bold",
                )
        # ── Komendy shell (po > i >>) ──────────────────────────
        if lstrip.startswith(">") or lstrip.startswith(">>"):
            cmd_word = lstrip.lstrip(">").lstrip()
            for cmd in SHELL_CMDS:
                if cmd.startswith(cmd_word) and cmd_word:
                    yield Completion(
                        cmd[len(cmd_word):],
                        display=cmd,
                        display_meta="shell",
                        style="fg:bright_black",
                    )
            return
        # ── Autocompletion ścieżek plików ─────────────────────
        if word.startswith("/") or word.startswith("./") or word.startswith("~/"):
            expanded = os.path.expanduser(word)
            base = os.path.dirname(expanded) or "."
            prefix_f = os.path.basename(expanded)
            try:
                for entry in os.scandir(base):
                    if entry.name.startswith(prefix_f):
                        suffix = "/" if entry.is_dir() else ""
                        full = os.path.join(base, entry.name) + suffix
                        rel = full[len(expanded) - len(prefix_f):]
                        yield Completion(
                            rel,
                            display=entry.name + suffix,
                            display_meta="path",
                            style="fg:bright_black",
                        )
            except (PermissionError, FileNotFoundError):
                pass
        # ── Pluginy ───────────────────────────────────────────
        if lstrip.startswith("\\"):
            pword = lstrip[2:].lstrip()
            if PLUGINS_DIR.exists():
                for p in PLUGINS_DIR.iterdir():
                    if p.name.startswith(pword):
                        yield Completion(
                            p.name[len(pword):],
                            display=p.name,
                            display_meta="plugin",
                            style="fg:magenta",
                        )
# ─────────────────────────────────────────────────────────────
# Parser składni Hacker Lang v9
# ─────────────────────────────────────────────────────────────
def parse_lines(lines: List[str], verbose: bool = False) -> Dict:
    deps = []
    libs = []
    includes = []
    vars_dict = {}
    consts = {}
    cmds = []
    plugins = []
    errors = []
    config = {}
    functions = {}
    in_config = False
    in_function = None
    func_body = []
    match_stack = [] # [(cond, arms)]
    in_match = False
    def flush_match():
        """Zamknij bieżący blok match → case..esac"""
        if not match_stack:
            return
        cond, arms = match_stack[-1]
        if arms:
            sh = f"case {cond} in\n"
            for val, cmd in arms:
                v = "*" if val == "_" else val.strip('""')
                sh += f" {v}) {cmd};;\n"
            sh += "esac"
            cmds.append(sh)
        match_stack.pop()
    for line_num, raw in enumerate(lines, 1):
        line = raw.strip()
        # Puste linie i komentarze
        if not line or line.startswith("!"):
            continue
        # ── Config block ──────────────────────────────────────
        if line == "[":
            in_config = True
            continue
        if line == "]":
            in_config = False
            continue
        if in_config:
            if "=" in line:
                k, v = line.split("=", 1)
                config[k.strip()] = v.strip()
            else:
                errors.append(f"L{line_num}: Nieprawidłowy wpis config: {line}")
            continue
        # ── Definicja funkcji: fn .name ─────────────────────
        # Składnia: fn .name …linie… endfn
        if line.startswith("fn "):
            fname = line[3:].strip().lstrip(".")
            in_function = fname
            func_body = []
            continue
        if line == "endfn" and in_function:
            functions[in_function] = func_body[:]
            # Buduj sh-funkcję
            body_sh = "\n ".join(func_body) if func_body else ":"
            sh_func = f"function {in_function}() {{\n {body_sh}\n}}"
            cmds.insert(0, sh_func) # funkcje deklarujemy przed main body
            in_function = None
            func_body = []
            continue
        if in_function is not None:
            func_body.append(_line_to_sh(line, vars_dict, consts, libs, includes, plugins, errors, line_num))
            continue
        # ── Deps: //dep1 dep2 ────────────────────────────────
        if line.startswith("//"):
            deps.extend(line[2:].strip().split())
            continue
        # ── Library: #libname ─────────────────────────────────
        if line.startswith("#"):
            lib_name = line[1:].strip()
            if lib_name:
                lib_path = LIBS_DIR / lib_name / "main.hacker"
                if lib_path.exists():
                    includes.append(lib_name)
                else:
                    libs.append(lib_name)
            continue
        # ── Extern: -- [static] path ──────────────────────────
        if line.startswith("--"):
            # Metadane — ignorowane przez REPL
            continue
        # ── Import: << "path" [in ns] ─────────────────────────
        if line.startswith("<<"):
            rest = line[2:].strip()
            path = rest.strip('"').split()[0]
            if os.path.exists(path):
                with open(path) as f:
                    cmds.append(f"# import {path}")
                    cmds.extend(f.read().splitlines())
            else:
                if verbose:
                    errors.append(f"L{line_num}: Import nie istnieje: {path}")
            continue
        # ── Enum: == Name [A, B, C] ───────────────────────────
        if line.startswith("=="):
            # Metadane — brak generowanego kodu shell
            continue
        # ── Struct: struct Name [field:type] ──────────────────
        if line.startswith("struct "):
            continue
        # ── % Stała: %KEY=val ──────────────────────────────────
        if line.startswith("%"):
            rest = line[1:].strip()
            if "=" in rest:
                k, v = rest.split("=", 1)
                key = k.strip()
                val = v.strip()
                consts[key] = val
                cmds.append(f'export {key}={_sh_quote(val)}')
            else:
                errors.append(f"L{line_num}: Nieprawidłowa stała: {line}")
            continue
        # ── @ Env var: @KEY=val ───────────────────────────────
        if line.startswith("@"):
            rest = line[1:].strip()
            if "=" in rest:
                k, v = rest.split("=", 1)
                key = k.strip()
                val = v.strip()
                vars_dict[key] = val
                cmds.append(f'export {key}={_sh_quote(val)}')
            else:
                errors.append(f"L{line_num}: Nieprawidłowa zmienna: {line}")
            continue
        # ── $ Zmienna lokalna: $key=val lub $key = spawn/await ─
        if line.startswith("$"):
            rest = line[1:]
            # $key = spawn cmd
            m = re.match(r"(\w+)\s*=\s*spawn\s+(.*)", rest)
            if m:
                key, task = m.group(1), m.group(2).strip().lstrip(".")
                cmds.append(f'export {key}=$( {task} & echo $! )')
                continue
            # $key = await expr
            m = re.match(r"(\w+)\s*=\s*await\s+(.*)", rest)
            if m:
                key, expr = m.group(1), m.group(2).strip()
                if expr.startswith("$"):
                    cmds.append(f'wait {expr}; export {key}=$?')
                elif expr.startswith("."):
                    fn = expr.lstrip(".")
                    cmds.append(f'{fn}; export {key}=$_HL_OUT')
                else:
                    cmds.append(f'export {key}=$( {expr} )')
                continue
            # $key = val
            if "=" in rest:
                k, v = rest.split("=", 1)
                key = k.strip()
                val = v.strip()
                vars_dict[key] = val
                cmds.append(f'export {key}={_sh_quote(val)}')
                continue
            errors.append(f"L{line_num}: Nieprawidłowa zmienna: {line}")
            continue
        # ── lock / unlock ──────────────────────────────────────
        if line.startswith("lock "):
            # lock $key = size — metadane, brak kodu shell
            continue
        if line.startswith("unlock "):
            continue
        # ── spawn (bez przypisania) ───────────────────────────
        if re.match(r"^spawn\s+", line):
            task = line[6:].strip().lstrip(".")
            cmds.append(f"{task} &")
            continue
        # ── await (bez przypisania) ───────────────────────────
        if re.match(r"^await\s+", line):
            expr = line[6:].strip()
            if expr.startswith("."):
                fn = expr.lstrip(".")
                cmds.append(fn)
            else:
                cmds.append(f"wait {expr}")
            continue
        # ── assert cond [msg] ─────────────────────────────────
        if re.match(r"^assert\s+", line):
            rest = line[7:].strip()
            # Spróbuj wyodrębnić msg w cudzysłowach na końcu
            m = re.match(r"(.*?)\s+\"(.*?)\"\s*$", rest)
            if m:
                cond, msg = m.group(1).strip(), m.group(2)
            else:
                cond, msg = rest, f"Assertion failed: {rest}"
            cmds.append(
                f"if ! ( {cond} ) 2>/dev/null; then "
                f"echo 'assert: {msg}' >&2; exit 1; fi"
            )
            continue
        # ── match $var |> ─────────────────────────────────────
        if re.match(r"^match\s+", line) and line.endswith("|>"):
            cond = line[6:].rstrip("|>").strip()
            match_stack.append((cond, []))
            in_match = True
            continue
        # ── match arm: val > cmd ─────────────────────────────
        if in_match and match_stack:
            # Format: "value" > cmd lub _ > cmd
            m = re.match(r'^(["\']?[\w*]+["\']?)\s*>\s+(.+)$', line)
            if m:
                match_stack[-1][1].append((m.group(1), m.group(2)))
                continue
            else:
                # Koniec bloku match
                flush_match()
                in_match = bool(match_stack)
                # fall through do normalnego parsowania
        # ── Pipe chain: .a |> .b |> .c ───────────────────────
        if "|>" in line and line.startswith("."):
            steps = [s.strip() for s in line.split("|>")]
            # Sprawdź czy wszystkie to HL functions
            all_hl = all(s.startswith(".") for s in steps)
            if all_hl:
                # Sekwencja wywołań
                for s in steps:
                    fn = s.lstrip(".")
                    cmds.append(fn)
            else:
                parts = [s.lstrip(".") for s in steps]
                cmds.append(" | ".join(parts))
            continue
        # ── Pętla = N > cmd ───────────────────────────────────
        m = re.match(r"^=\s*(\d+)\s*>\s+(.+)$", line)
        if m:
            n, cmd = int(m.group(1)), m.group(2).strip()
            cmds.append(f"for _hl_i in $(seq 1 {n}); do {_sh_cmd(cmd)}; done")
            continue
        # ── while cond > cmd / while cond |> cmd ─────────────
        m = re.match(r"^while\s+(.+?)\s+[|>]>\s+(.+)$", line)
        if not m:
            m = re.match(r"^while\s+(.+?)\s+>\s+(.+)$", line)
        if m:
            cond, cmd = m.group(1).strip(), m.group(2).strip()
            cmds.append(f"while {cond}; do {_sh_cmd(cmd)}; done")
            continue
        # ── for var in expr > cmd ─────────────────────────────
        m = re.match(r"^for\s+(\w+)\s+in\s+(.+?)\s+[|>]>\s+(.+)$", line)
        if not m:
            m = re.match(r"^for\s+(\w+)\s+in\s+(.+?)\s+>\s+(.+)$", line)
        if m:
            var, expr, cmd = m.group(1), m.group(2).strip(), m.group(3).strip()
            cmds.append(f"for {var} in {expr}; do {_sh_cmd(cmd)}; done")
            continue
        # ── try cmd catch cmd ─────────────────────────────────
        m = re.match(r"^try\s+(.+?)\s+catch\s+(.+)$", line)
        if m:
            t, c = m.group(1).strip(), m.group(2).strip()
            cmds.append(f"( {_sh_cmd(t)} ) || ( {_sh_cmd(c)} )")
            continue
        # ── if: ? cond > cmd ──────────────────────────────────
        if line.startswith("? ") and ">" in line:
            rest = line[2:].strip()
            parts = rest.split(">", 1)
            if len(parts) == 2:
                cond, cmd = parts[0].strip(), parts[1].strip()
                cmds.append(f"if {cond}; then {_sh_cmd(cmd)}; fi")
                continue
            errors.append(f"L{line_num}: Nieprawidłowy if: {line}")
            continue
        # ── elif: ?? cond > cmd ───────────────────────────────
        if line.startswith("?? ") and ">" in line:
            rest = line[3:].strip()
            parts = rest.split(">", 1)
            if len(parts) == 2:
                cond, cmd = parts[0].strip(), parts[1].strip()
                # Modyfikuj ostatnią komendę if → dodaj elif
                if cmds and cmds[-1].endswith("fi"):
                    prev = cmds[-1][:-2].rstrip()
                    cmds[-1] = f"{prev} elif {cond}; then {_sh_cmd(cmd)}; fi"
                else:
                    cmds.append(f"elif {cond}; then {_sh_cmd(cmd)}; fi")
                continue
        # ── else: ?: > cmd ────────────────────────────────────
        if line.startswith("?: ") and ">" in line:
            rest = line[3:].strip()
            parts = rest.split(">", 1)
            if len(parts) == 2:
                cmd = parts[1].strip()
                if cmds and cmds[-1].endswith("fi"):
                    prev = cmds[-1][:-2].rstrip()
                    cmds[-1] = f"{prev} else {_sh_cmd(cmd)}; fi"
                else:
                    cmds.append(f"else {_sh_cmd(cmd)}; fi")
                continue
        # ── background: & cmd ─────────────────────────────────
        if line.startswith("& "):
            cmds.append(f"{line[2:].strip()} &")
            continue
        # ── plugin: \ name [args] ────────────────────────────
        if line.startswith("\\"):
            rest = line[1:].strip()
            parts = rest.split(None, 1)
            pname = parts[0]
            pargs = parts[1] if len(parts) > 1 else ""
            pbin = PLUGINS_DIR / pname
            phl = PLUGINS_DIR / f"{pname}.hacker"
            if pbin.exists():
                cmds.append(f"{pbin} {pargs}".strip())
            elif phl.exists():
                cmds.append(f"hl-runtime {phl} {pargs}".strip())
            else:
                plugins.append(rest)
                cmds.append(f"# plugin: {rest}")
            continue
        # ── log "msg" ─────────────────────────────────────────
        if re.match(r"^log\s+", line):
            msg = line[4:].strip()
            cmds.append(f"echo {msg}")
            continue
        # ── out val ───────────────────────────────────────────
        if re.match(r"^out\s+", line):
            val = line[4:].strip()
            cmds.append(f"export _HL_OUT={val}")
            continue
        # ── end [N] ───────────────────────────────────────────
        if line == "end" or re.match(r"^end\s+\d+$", line):
            code = line.split()[-1] if " " in line else "0"
            cmds.append(f"exit {code}")
            continue
        # ── Izolowana komenda: ( cmd ) ────────────────────────
        if line.startswith("(") and line.endswith(")"):
            inner = line[1:-1].strip()
            cmds.append(f"( {inner} )")
            continue
        # ── Wywołanie funkcji HL: .func [args] ────────────────
        if line.startswith("."):
            m = re.match(r"(.[A-Za-z*][\w.]*)(.*)", line)
            if m:
                fn = m.group(1).lstrip(".")
                args = m.group(2).strip()
                if args:
                    cmds.append(f'export _HL_ARGS={_sh_quote(args)}; {fn}')
                else:
                    cmds.append(fn)
                continue
        # ── >> raw no-sub ─────────────────────────────────────
        if line.startswith(">>"):
            cmds.append(line[2:].strip())
            continue
        # ── > raw sub ─────────────────────────────────────────
        if line.startswith(">"):
            cmds.append(line[1:].strip())
            continue
        # ── ^ sudo prefix ─────────────────────────────────────
        if line.startswith("^"):
            cmds.append(f"sudo {line[1:].strip()}")
            continue
        # ── Fallback: zwykła komenda ──────────────────────────
        cmds.append(line)
    # Zamknij niezamknięte bloki match
    while match_stack:
        flush_match()
    return {
        "deps": list(set(deps)),
        "libs": libs,
        "vars": vars_dict,
        "consts": consts,
        "cmds": cmds,
        "includes": includes,
        "plugins": plugins,
        "errors": errors,
        "config": config,
        "functions": functions,
    }
def _sh_quote(val: str) -> str:
    """Otocz wartość w pojedyncze cudzysłowy jeśli zawiera spacje lub specjalne znaki."""
    if any(c in val for c in ' \t\n$`"|;&<>(){}'):
        return "'" + val.replace("'", "'\''") + "'"
    return val
def _sh_cmd(cmd: str) -> str:
    """Przetłumacz komendę HL na shell inline."""
    cmd = cmd.strip()
    if cmd.startswith("."):
        return cmd.lstrip(".")
    if cmd.startswith("log "):
        return f"echo {cmd[4:].strip()}"
    if cmd.startswith("out "):
        return f"export _HL_OUT={cmd[4:].strip()}"
    if cmd.startswith("end"):
        code = cmd.split()[-1] if " " in cmd else "0"
        return f"exit {code}"
    return cmd
def _line_to_sh(line, vars_dict, consts, libs, includes, plugins, errors, line_num):
    """Uproszczone tłumaczenie pojedynczej linii HL → sh (dla funkcji)."""
    return _sh_cmd(line)
# ─────────────────────────────────────────────────────────────
# Wykonanie skryptu shell
# ─────────────────────────────────────────────────────────────
def execute_parsed(parsed: Dict, verbose: bool) -> Tuple[int, str, str]:
    """Buduje i wykonuje tymczasowy skrypt bash z parsed AST."""
    with tempfile.NamedTemporaryFile(
        mode="w+", suffix=".sh", delete=False, prefix="hl_repl_"
    ) as tmp:
        tmp.write("#!/bin/bash\n")
        tmp.write("set -euo pipefail\n\n")
        # Deps
        for dep in parsed["deps"]:
            if dep != "sudo":
                tmp.write(
                    f'command -v {dep} >/dev/null 2>&1 || '
                    f'(echo "Installing {dep}..." && sudo apt-get install -y {dep})\n'
                )
        # Includes
        for inc in parsed["includes"]:
            lib_path = LIBS_DIR / inc / "main.hacker"
            tmp.write(f"# === include: {inc} ===\n")
            with open(lib_path) as lf:
                tmp.write(lf.read())
            tmp.write("\n")
        # Env vars i stałe
        for k, v in parsed["vars"].items():
            tmp.write(f'export {k}={_sh_quote(v)}\n')
        for k, v in parsed["consts"].items():
            tmp.write(f'export {k}={_sh_quote(v)}\n')
        # Komendy
        for cmd in parsed["cmds"]:
            if verbose:
                tmp.write(f'echo "[hl] {cmd.replace(chr(39), chr(39)+chr(92)+chr(39)+chr(39))}"\n')
            tmp.write(f"{cmd}\n")
        tmp_path = tmp.name
    os.chmod(tmp_path, 0o755)
    env = {**os.environ, **parsed["vars"], **parsed["consts"]}
    try:
        result = subprocess.run(
            ["bash", tmp_path],
            env=env,
            capture_output=True,
            text=True,
            timeout=30,
        )
        return result.returncode, result.stdout.strip(), result.stderr.strip()
    except subprocess.TimeoutExpired:
        return 124, "", "Timeout: komenda trwała za długo (>30s)"
    finally:
        try:
            os.remove(tmp_path)
        except OSError:
            pass
# ─────────────────────────────────────────────────────────────
# Główna klasa REPL
# ─────────────────────────────────────────────────────────────
class HackerREPL:
    def __init__(self):
        self.verbose = False
        self.lines = []
        self.variables = {}
        self.constants = {}
        self.functions = []
        self.multiline = False
        self.ml_buffer = []
        self._load_known_functions()
    def _load_known_functions(self):
        """Wczytaj znane funkcje HL z plików w HACKER_DIR."""
        self.functions = []
        if LIBS_DIR.exists():
            for lib in LIBS_DIR.iterdir():
                self.functions.append(lib.name)
        if BIN_DIR.exists():
            for b in BIN_DIR.iterdir():
                if b.is_file() and b.stat().st_mode & 0o111:
                    self.functions.append(b.name)
    def ensure_dirs(self):
        for d in [HACKER_DIR, LIBS_DIR, PLUGINS_DIR, HISTORY_DIR]:
            d.mkdir(parents=True, exist_ok=True)
    # ── Obsługa meta-komend REPL ──────────────────────────────
    def handle_meta(self, cmd: str) -> bool:
        """Zwraca True jeśli komenda była meta-komendą."""
        c = cmd.strip()
        if c in (":exit", ":quit"):
            raise EOFError
        if c == ":help":
            self._print_help()
            return True
        if c == ":clear":
            self.lines = []
            self.variables = {}
            self.constants = {}
            console.print(Text("• Sesja wyczyszczona.", style=C_INFO))
            return True
        if c == ":verbose":
            self.verbose = not self.verbose
            state = "ON" if self.verbose else "OFF"
            console.print(Text(f"• Verbose: {state}", style=C_INFO))
            return True
        if c == ":reset":
            self.lines = []
            self.variables = {}
            self.constants = {}
            self.multiline = False
            self.ml_buffer = []
            console.print(Text("• Reset sesji.", style=C_INFO))
            return True
        if c == ":history":
            if HISTORY_FILE.exists():
                lines = HISTORY_FILE.read_text().splitlines()
                for i, l in enumerate(lines[-20:], 1):
                    console.print(Text(f" {i:3} {l}", style=C_DIM))
            return True
        if c == ":vars":
            if self.variables or self.constants:
                t = Table(show_header=True, header_style="bold white")
                t.add_column("Typ", style=C_DIM, width=8)
                t.add_column("Klucz", style="white", width=20)
                t.add_column("Wartość", style=C_DIM)
                for k, v in self.variables.items():
                    t.add_row("$var", k, v)
                for k, v in self.constants.items():
                    t.add_row("%const", k, v)
                console.print(t)
            else:
                console.print(Text("• Brak zmiennych.", style=C_DIM))
            return True
        if c == ":show":
            if self.lines:
                for i, l in enumerate(self.lines, 1):
                    console.print(Text(f" {i:3} {l}", style=C_DIM))
            else:
                console.print(Text("• Bufor pusty.", style=C_DIM))
            return True
        if c == ":libs":
            if LIBS_DIR.exists():
                for lib in sorted(LIBS_DIR.iterdir()):
                    console.print(Text(f" {lib.name}", style=C_DIM))
            return True
        if c == ":multiline":
            self.multiline = True
            console.print(Text("• Tryb multiline. Wpisz :run aby wykonać.", style=C_INFO))
            return True
        if c == ":run" and self.multiline:
            self.multiline = False
            lines_to_run = self.ml_buffer[:]
            self.ml_buffer = []
            self._execute_lines(lines_to_run)
            return True
        if c.startswith(":save "):
            path = c[6:].strip()
            try:
                Path(path).write_text("\n".join(self.lines))
                console.print(Text(f"• Zapisano do {path}", style=C_INFO))
            except OSError as e:
                console.print(Text(f"• Błąd zapisu: {e}", style=C_ERROR))
            return True
        if c.startswith(":load "):
            path = c[6:].strip()
            try:
                loaded = Path(path).read_text().splitlines()
                self.lines.extend(loaded)
                console.print(Text(f"• Załadowano {len(loaded)} linii z {path}", style=C_INFO))
                self._execute_lines(loaded)
            except OSError as e:
                console.print(Text(f"• Błąd odczytu: {e}", style=C_ERROR))
            return True
        return False
    def _execute_lines(self, lines: List[str]):
        """Parsuj i wykonaj listę linii HL."""
        parsed = parse_lines(lines, self.verbose)
        # Zaktualizuj lokalne zmienne
        self.variables.update(parsed["vars"])
        self.constants.update(parsed["consts"])
        self.functions.extend(parsed["functions"].keys())
        if parsed["errors"]:
            for e in parsed["errors"]:
                console.print(Text(f"• {e}", style=C_ERROR))
            return
        if parsed["libs"]:
            console.print(
                Text(f"• Brakujące biblioteki: {', '.join(parsed['libs'])}", style=C_WARN)
            )
        if not parsed["cmds"]:
            return
        rc, stdout, stderr = execute_parsed(parsed, self.verbose)
        if stdout:
            console.print(
                Panel(stdout, title="output", border_style="bright_black", padding=(0, 1))
            )
        if stderr:
            console.print(
                Panel(stderr, title="stderr", border_style="red", padding=(0, 1))
            )
        if rc != 0:
            console.print(Text(f"• exit {rc}", style=C_WARN))
    def _print_help(self):
        t = Table(show_header=False, box=None, padding=(0, 2))
        t.add_column(style="red bold", width=18)
        t.add_column(style="bright_black")
        rows = [
            (":help", "Ta pomoc"),
            (":clear", "Wyczyść sesję"),
            (":reset", "Pełny reset"),
            (":verbose", "Przełącz verbose"),
            (":vars", "Pokaż zmienne i stałe"),
            (":show", "Pokaż bufor"),
            (":libs", "Listuj biblioteki"),
            (":history", "Ostatnie 20 wpisów"),
            (":multiline", "Tryb wielu linii"),
            (":run", "Wykonaj bufor multiline"),
            (":save <path>","Zapisz sesję do pliku"),
            (":load <path>","Wczytaj i wykonaj plik"),
            (":exit / :quit","Wyjdź"),
            ("", ""),
            ("SKŁADNIA HL v9", ""),
            ("> cmd", "Komenda shell"),
            (">> cmd", "Komenda raw (bez $-sub)"),
            ("@KEY=val", "Zmienna env"),
            ("%KEY=val", "Stała"),
            ("$key=val", "Zmienna lokalna"),
            ("=N > cmd", "Pętla N razy"),
            ("? c > cmd", "If"),
            ("?? c > cmd", "Elif"),
            ("?: > cmd", "Else"),
            ("while c > cmd","While"),
            ("for v in e > cmd","For"),
            ("try c catch c","Try/catch"),
            ("spawn cmd", "Async fire&forget"),
            ("await expr", "Czekaj na zadanie"),
            ("$k=spawn cmd","Spawn + PID"),
            ("$k=await expr","Await + wynik"),
            ("assert c msg","Asercja"),
            ("match $v |>","Blok match"),
            ("val > cmd", "Ramię match"),
            (".a |> .b", "Pipe chain"),
            ("log msg", "Wypisz"),
            ("out val", "Zwróć wartość"),
            ("end [N]", "Zakończ"),
            ("( cmd )", "Izolowana komenda"),
            ("^ cmd", "Sudo"),
            ("\\ plugin", "Plugin"),
            ("#lib", "Biblioteka"),
            ("//dep", "Zależność"),
            ("fn .name", "Definicja funkcji"),
            ("endfn", "Koniec funkcji"),
        ]
        for k, v in rows:
            t.add_row(k, v)
        console.print(t)
    def run(self):
        self.ensure_dirs()
        history = FileHistory(str(HISTORY_FILE))
        completer = HackerLangCompleter(self)
        # Styl prompt_toolkit
        style = Style.from_dict({
            "prompt": "bold #cc0000", # czerwony
            "": "#ffffff", # biały tekst wejściowy
            # Klasy leksera
            "keyword": "bold white",
            "const": "bold yellow",
            "envvar": "yellow",
            "varname": "white",
            "call": "cyan",
            "op": "ansibrightblack",
            "prefix": "ansibrightblack",
            "command": "white",
            "string": "ansibrightblack",
            "number": "cyan",
            "comment": "ansibrightblack italic",
            "sudo": "bold red",
            "pipe": "red bold",
            "text": "white",
        })
        # Keybindings
        kb = KeyBindings()
        @kb.add("c-l")
        def _clear_screen(event):
            event.app.renderer.clear()
        session = PromptSession(
            history=history,
            auto_suggest=AutoSuggestFromHistory(),
            completer=completer,
            complete_while_typing=True,
            lexer=HackerLangLexer(),
            style=style,
            key_bindings=kb,
            mouse_support=False,
            wrap_lines=True,
        )
        # Banner
        console.print(
            Panel(
                f"Hacker Lang REPL v{VERSION}",
                border_style="bright_black",
                padding=(0, 1),
            )
        )
        console.print(
            Text(
                ":help — pomoc | :exit — wyjście | Tab — autouzupełnianie",
                style=C_DIM,
            )
        )
        while True:
            try:
                if self.multiline:
                    prompt_str = FormattedText([("class:prompt", " ... ")])
                else:
                    prompt_str = FormattedText([("class:prompt", "hl> ")])
                line = session.prompt(prompt_str)
                if not line.strip():
                    continue
                # Meta-komendy
                if line.strip().startswith(":"):
                    if self.handle_meta(line.strip()):
                        continue
                # Tryb multiline
                if self.multiline:
                    self.ml_buffer.append(line)
                    continue
                # Dodaj do bufora sesji
                self.lines.append(line)
                # Wykonaj bieżącą linię
                self._execute_lines([line])
            except KeyboardInterrupt:
                if self.multiline:
                    self.multiline = False
                    self.ml_buffer = []
                    console.print(Text("• Anulowano multiline.", style=C_DIM))
                else:
                    console.print()
                continue
            except EOFError:
                console.print(Text("\n• Do zobaczenia.", style=C_DIM))
                break
        console.print(Text("• Sesja zakończona.", style=C_DIM))
# ─────────────────────────────────────────────────────────────
# Entry point
# ─────────────────────────────────────────────────────────────
def main():
    import argparse
    import sys
    parser = argparse.ArgumentParser(
        prog="hl-repl",
        description=f"Hacker Lang REPL v{VERSION}",
    )
    parser.add_argument("--verbose", "-v", action="store_true", help="Verbose mode")
    parser.add_argument("--exec", "-e", metavar="CODE", help="Wykonaj kod i wyjdź")
    parser.add_argument("--file", "-f", metavar="FILE", help="Wczytaj i wykonaj plik")
    args = parser.parse_args()
    repl = HackerREPL()
    repl.verbose = args.verbose
    if args.exec:
        lines = args.exec.splitlines()
        parsed = parse_lines(lines, args.verbose)
        if parsed["errors"]:
            for e in parsed["errors"]:
                print(f"[błąd] {e}", file=sys.stderr)
            sys.exit(1)
        rc, out, err = execute_parsed(parsed, args.verbose)
        if out:
            print(out)
        if err:
            print(err, file=sys.stderr)
        sys.exit(rc)
    if args.file:
        path = Path(args.file)
        if not path.exists():
            print(f"[błąd] Plik nie istnieje: {path}", file=sys.stderr)
            sys.exit(1)
        lines = path.read_text().splitlines()
        parsed = parse_lines(lines, args.verbose)
        if parsed["errors"]:
            for e in parsed["errors"]:
                print(f"[błąd] {e}", file=sys.stderr)
            sys.exit(1)
        rc, out, err = execute_parsed(parsed, args.verbose)
        if out:
            print(out)
        if err:
            print(err, file=sys.stderr)
        sys.exit(rc)
    repl.ensure_dirs()
    repl.run()
if __name__ == "__main__":
    main()
