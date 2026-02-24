import os
import subprocess
import tempfile
from pathlib import Path
from typing import Dict, List
from prompt_toolkit import PromptSession
from prompt_toolkit.auto_suggest import AutoSuggestFromHistory
from prompt_toolkit.history import FileHistory
from prompt_toolkit.styles import Style
from prompt_toolkit.formatted_text import FormattedText
from rich.console import Console
from rich.panel import Panel
from rich.text import Text
VERSION = "1.4"
HACKER_DIR = Path.home() / ".hackeros" / "hacker-lang"
LIBS_DIR = HACKER_DIR / "libs"
HISTORY_DIR = Path.home() / ".hackeros" / "history"
HISTORY_FILE = HISTORY_DIR / "hacker_repl_history"
console = Console()
# Colorful styles
success_style = "bold green"
error_style = "bold red"
warning_style = "yellow"
info_style = "blue"
prompt_style = "bold purple"
def ensure_dirs():
    HACKER_DIR.mkdir(parents=True, exist_ok=True)
    LIBS_DIR.mkdir(parents=True, exist_ok=True)
    HISTORY_DIR.mkdir(parents=True, exist_ok=True)
def parse_lines(lines: List[str], verbose: bool = False) -> Dict:
    deps = []
    libs = [] # missing libs
    vars_dict = {}
    cmds = []
    includes = [] # existing libs to include
    binaries = []
    plugins = []
    errors = []
    config = {}
    in_config = False
    for line_num, line in enumerate(lines, 1):
        line = line.strip()
        if not line or line.startswith("!"):
            continue
        if line == "[":
            if in_config:
                errors.append(f"Line {line_num}: Nested config block")
            in_config = True
            continue
        if line == "]":
            if not in_config:
                errors.append(f"Line {line_num}: Unmatched ']'")
            in_config = False
            continue
        if in_config:
            if "=" in line:
                k, v = line.split("=", 1)
                config[k.strip()] = v.strip()
            else:
                errors.append(f"Line {line_num}: Invalid config entry: {line}")
            continue
        if line.startswith("//"):
            deps.extend(line[2:].strip().split())
        elif line.startswith("#"):
            lib_name = line[1:].strip()
            if lib_name:
                lib_path = LIBS_DIR / lib_name / "main.hacker"
                if lib_path.exists():
                    includes.append(lib_name)
                else:
                    libs.append(lib_name)
        elif line.startswith("@"):
            var_def = line[1:].strip()
            if "=" in var_def:
                k, v = var_def.split("=", 1)
                vars_dict[k.strip()] = v.strip()
            else:
                errors.append(f"Line {line_num}: Invalid variable: {line}")
        elif line.startswith("="):
            parts = line[1:].strip().split(">", 1)
            if len(parts) == 2:
                try:
                    n = int(parts[0].strip())
                    cmd = parts[1].strip()
                    loop_cmd = f"for i in $(seq 1 {n}); do {cmd}; done"
                    cmds.append(loop_cmd)
                except ValueError:
                    errors.append(f"Line {line_num}: Invalid loop count: {line}")
            else:
                errors.append(f"Line {line_num}: Invalid loop syntax: {line}")
        elif line.startswith("?"):
            parts = line[1:].strip().split(">", 1)
            if len(parts) == 2:
                cond = parts[0].strip()
                cmd = parts[1].strip()
                if_cmd = f"if {cond}; then {cmd}; fi"
                cmds.append(if_cmd)
            else:
                errors.append(f"Line {line_num}: Invalid if syntax: {line}")
        elif line.startswith("&"):
            plugin = line[1:].strip()
            plugins.append(plugin)
        elif line.startswith(">"):
            cmd = line[1:].strip()
            cmds.append(cmd)
        else:
            cmds.append(line)
    return {
        "deps": list(set(deps)),
        "libs": libs,
        "vars": vars_dict,
        "cmds": cmds,
        "includes": includes,
        "binaries": binaries,
        "plugins": plugins,
        "errors": errors,
        "config": config
    }
def run_repl(verbose: bool = False):
    ensure_dirs()
    history = FileHistory(str(HISTORY_FILE))
    session = PromptSession(
        history=history,
        auto_suggest=AutoSuggestFromHistory(),
        style=Style.from_dict({"prompt": prompt_style}),
    )
    lines: List[str] = []
    in_config = False
    console.print(Panel(f"Hacker Lang REPL v{VERSION} - Enhanced Interactive Mode", style=success_style))
    console.print(Text("Type 'exit' to quit, 'help' for commands, 'clear' to reset", style=info_style))
    console.print(Text("Supported: //deps, #libs, @vars, =loops, ?ifs, &bg, >cmds, [config], !comments", style=info_style))
    while True:
        try:
            prompt_text = "CONFIG> " if in_config else "hacker> "
            prompt = FormattedText([("class:prompt", prompt_text)])
            line = session.prompt(prompt)
            if not line.strip():
                continue
            if line == "exit":
                break
            if line == "help":
                console.print(Text("REPL Commands:\n- exit: Quit REPL\n- help: This menu\n- clear: Reset session\n- verbose: Toggle verbose", style=info_style))
                continue
            if line == "clear":
                lines = []
                in_config = False
                console.print(Text("Session cleared!", style=success_style))
                continue
            if line == "verbose":
                verbose = not verbose
                console.print(Text(f"Verbose mode: {verbose}", style=info_style))
                continue
            lines.append(line)
            if line == "[":
                in_config = True
                continue
            elif line == "]":
                if not in_config:
                    console.print(Text("Error: Unmatched ']'", style=error_style))
                in_config = False
                continue
            if not in_config and line and not line.startswith("!"):
                parsed = parse_lines(lines, verbose)
                if parsed["errors"]:
                    console.print(Panel("\n".join(parsed["errors"]), title="REPL Errors", style=error_style))
                else:
                    if parsed["libs"]:
                        console.print(Text(f"Warning: Missing custom libs: {', '.join(parsed['libs'])}", style=warning_style))
                    with tempfile.NamedTemporaryFile(mode="w+", suffix=".sh", delete=False) as temp_sh:
                        temp_sh.write("#!/bin/bash\nset -e\n")
                        for k, v in parsed["vars"].items():
                            temp_sh.write(f'export {k}="{v}"\n')
                        for dep in parsed["deps"]:
                            if dep != "sudo":
                                temp_sh.write(f'command -v {dep} || (sudo apt update && sudo apt install -y {dep})\n')
                        for inc in parsed["includes"]:
                            lib_path = LIBS_DIR / inc / "main.hacker"
                            with open(lib_path, "r") as lib_f:
                                temp_sh.write(f"# include {inc}\n")
                                temp_sh.write(lib_f.read())
                                temp_sh.write("\n")
                        for cmd in parsed["cmds"]:
                            temp_sh.write(f"{cmd}\n")
                        for bin in parsed["binaries"]:
                            temp_sh.write(f"{bin}\n")
                        for plugin in parsed["plugins"]:
                            temp_sh.write(f"{plugin} &\n")
                        temp_path = temp_sh.name
                    os.chmod(temp_path, 0o755)
                    env = os.environ.copy()
                    env.update(parsed["vars"])
                    result = subprocess.run(["bash", temp_path], env=env, capture_output=True, text=True)
                    os.remove(temp_path)
                    out_str = result.stdout.strip()
                    err_str = result.stderr.strip()
                    if result.returncode != 0:
                        if err_str:
                            console.print(Panel(err_str, title="REPL Error", style=error_style))
                    elif out_str:
                        console.print(Panel(out_str, title="REPL Output", style=success_style))
        except KeyboardInterrupt:
            break
    console.print(Text("REPL session ended.", style=success_style))
if __name__ == "__main__":
    import sys
    verbose = "--verbose" in sys.argv
    run_repl(verbose=verbose)
