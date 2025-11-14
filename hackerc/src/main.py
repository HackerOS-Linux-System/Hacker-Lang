import json
import os
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path
from typing import Optional

import typer
from prompt_toolkit import PromptSession
from prompt_toolkit.auto_suggest import AutoSuggestFromHistory
from prompt_toolkit.completion import Completer, Completion
from prompt_toolkit.history import FileHistory
from prompt_toolkit.styles import Style
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text
import yaml

VERSION = "0.0.9"
HACKER_DIR = Path.home() / ".hackeros" / "hacker-lang"
BIN_DIR = HACKER_DIR / "bin"
LIBS_DIR = HACKER_DIR / "libs"
HISTORY_DIR = Path.home() / ".hackeros" / "history"
HISTORY_FILE = HISTORY_DIR / "hacker_repl_history"

console = Console()

# Colorful styles
title_style = "bold magenta underline"
header_style = "bold yellow"
example_style = "cyan on grey3"
success_style = "bold green"
error_style = "bold red"
warning_style = "yellow"
info_style = "blue"
prompt_style = "bold purple"

def expand_home(path: str) -> str:
    return str(Path(path).expanduser())

def ensure_hacker_dir():
    HACKER_DIR.mkdir(parents=True, exist_ok=True)
    BIN_DIR.mkdir(parents=True, exist_ok=True)
    LIBS_DIR.mkdir(parents=True, exist_ok=True)
    HISTORY_DIR.mkdir(parents=True, exist_ok=True)

def display_welcome():
    console.print(Panel.fit(f"Welcome to Hacker Lang CLI v{VERSION}\nAdvanced scripting for Debian-based Linux systems", title="Hacker Lang", style=title_style))
    console.print(Text("Type 'hackerc help' for commands or 'hackerc repl' to start interactive mode.", style=info_style))
    help_command(show_banner=False)

def parse_lines(lines: list[str], verbose: bool = False) -> dict:
    deps = []
    libs = []  # missing libs
    vars_dict = {}
    cmds = []
    includes = []  # existing libs to include
    binaries = []  # TODO: if needed
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
            # Assume plain command
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

def run_command(file: str, verbose: bool) -> bool:
    try:
        with open(file, "r") as f:
            lines = f.readlines()
        parsed = parse_lines(lines, verbose)
        if parsed["errors"]:
            console.print(Panel("\n".join(parsed["errors"]), title="Syntax Errors", style=error_style))
            return False
        if parsed["libs"]:
            console.print(Text(f"Warning: Missing custom libs: {', '.join(parsed['libs'])}", style=warning_style))
            console.print(Text("Please install them using bytes install <lib>", style=warning_style))
        with tempfile.NamedTemporaryFile(mode="w+", suffix=".sh", delete=False) as temp_sh:
            temp_sh.write("#!/bin/bash\n")
            temp_sh.write("set -e\n")
            for k, v in parsed["vars"].items():
                temp_sh.write(f'export {k}="{v}"\n')
            for dep in parsed["deps"]:
                if dep != "sudo":
                    temp_sh.write(f'command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})\n')
            for inc in parsed["includes"]:
                lib_path = LIBS_DIR / inc / "main.hacker"
                with open(lib_path, "r") as lib_f:
                    temp_sh.write(f"# Included from {inc}\n")
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
        console.print(Text(f"Executing script: {file}", style=info_style))
        console.print(Text(f"Config: {parsed['config']}", style=info_style))
        console.print(Text("Running...", style=success_style))
        env = os.environ.copy()
        env.update(parsed["vars"])
        result = subprocess.run(["bash", temp_path], env=env, capture_output=False)
        os.remove(temp_path)
        if result.returncode != 0:
            console.print(Text("Execution failed", style=error_style))
            return False
        console.print(Text("Execution completed successfully!", style=success_style))
        return True
    except Exception as e:
        console.print(Text(f"Error: {e}", style=error_style))
        return False

def compile_command(file: str, output: str, verbose: bool) -> bool:
    # Simplified: Generate bash executable instead of native
    console.print(Text(f"Compiling {file} to {output} (simplified to bash executable)", style=info_style))
    try:
        with open(file, "r") as f:
            lines = f.readlines()
        parsed = parse_lines(lines, verbose)
        if parsed["errors"]:
            console.print(Panel("\n".join(parsed["errors"]), title="Syntax Errors", style=error_style))
            return False
        with open(output, "w") as out_sh:
            out_sh.write("#!/bin/bash\n")
            out_sh.write("set -e\n")
            # Similar to run_command
            for k, v in parsed["vars"].items():
                out_sh.write(f'export {k}="{v}"\n')
            for dep in parsed["deps"]:
                if dep != "sudo":
                    out_sh.write(f'command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})\n')
            for inc in parsed["includes"]:
                lib_path = LIBS_DIR / inc / "main.hacker"
                with open(lib_path, "r") as lib_f:
                    out_sh.write(f"# Included from {inc}\n")
                    out_sh.write(lib_f.read())
                    out_sh.write("\n")
            for cmd in parsed["cmds"]:
                out_sh.write(f"{cmd}\n")
            for bin in parsed["binaries"]:
                out_sh.write(f"{bin}\n")
            for plugin in parsed["plugins"]:
                out_sh.write(f"{plugin} &\n")
        os.chmod(output, 0o755)
        console.print(Text("Compilation successful!", style=success_style))
        return True
    except Exception as e:
        console.print(Text(f"Compilation failed: {e}", style=error_style))
        return False

def check_command(file: str, verbose: bool) -> bool:
    try:
        with open(file, "r") as f:
            lines = f.readlines()
        parsed = parse_lines(lines, verbose)
        if parsed["errors"]:
            console.print(Panel("\n".join(parsed["errors"]), title="Syntax Errors", style=error_style))
            return False
        console.print(Text("Syntax validation passed!", style=success_style))
        return True
    except Exception as e:
        console.print(Text(f"Error: {e}", style=error_style))
        return False

def init_command(file: str, verbose: bool) -> bool:
    if Path(file).exists():
        console.print(Text(f"File {file} already exists!", style=error_style))
        return False
    template = """! Hacker Lang advanced template
// sudo ! Privileged operations
// curl ! For downloads
# network-utils ! Custom library example
@APP_NAME=HackerApp ! Application name
@LOG_LEVEL=debug
=3 > echo "Iteration: $APP_NAME" ! Loop example
? [ -f /etc/os-release ] > cat /etc/os-release | grep PRETTY_NAME ! Conditional
& ping -c 1 google.com ! Background task
# logging ! Include logging library
> echo "Starting update..."
> sudo apt update && sudo apt upgrade -y ! System update
[
Author=Advanced User
Version=1.0
Description=System maintenance script
]
"""
    try:
        with open(file, "w") as f:
            f.write(template)
        console.print(Text(f"Initialized template at {file}", style=success_style))
        if verbose:
            console.print(Panel(template, title="Template", style=example_style))
        return True
    except Exception as e:
        console.print(Text(f"Initialization failed: {e}", style=error_style))
        return False

def clean_command(verbose: bool) -> bool:
    temp_dir = Path(tempfile.gettempdir())
    count = 0
    for f in temp_dir.glob("*.sh"):
        if f.name.startswith("tmp"):
            f.unlink()
            count += 1
            if verbose:
                console.print(Text(f"Removed: {f}", style=warning_style))
    console.print(Text(f"Removed {count} temporary files", style=success_style))
    return True

def unpack_bytes(verbose: bool) -> bool:
    bytes_path1 = Path.home() / "hackeros" / "hacker-lang" / "bin" / "bytes"
    bytes_path2 = Path("/usr/bin/bytes")
    if bytes_path1.exists():
        console.print(Text(f"Bytes already installed at {bytes_path1}.", style=success_style))
        return True
    if bytes_path2.exists():
        console.print(Text(f"Bytes already installed at {bytes_path2}.", style=success_style))
        return True
    url = "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes"
    try:
        with urllib.request.urlopen(url) as resp:
            if resp.status != 200:
                console.print(Text(f"Error: status code {resp.status}", style=error_style))
                return False
            bytes_path1.parent.mkdir(parents=True, exist_ok=True)
            with open(bytes_path1, "wb") as f:
                f.write(resp.read())
        bytes_path1.chmod(0o755)
        if verbose:
            console.print(Text(f"Downloaded and installed bytes from {url} to {bytes_path1}", style=success_style))
        console.print(Text("Bytes installed successfully!", style=success_style))
        return True
    except Exception as e:
        console.print(Text(f"Error installing bytes: {e}", style=error_style))
        return False

def editor_command(file: Optional[str] = None) -> bool:
    # For simplicity, use nano or vim; assume hacker-editor is nano with syntax if needed
    editor = "nano"  # or "vim"
    args = [editor]
    if file:
        args.append(file)
    console.print(Text(f"Launching editor: {' '.join(args)}", style=info_style))
    try:
        subprocess.run(args, check=True)
        console.print(Text("Editor session completed.", style=success_style))
        return True
    except Exception as e:
        console.print(Text(f"Editor failed: {e}", style=error_style))
        return False

def run_repl(verbose: bool) -> bool:
    history = FileHistory(str(HISTORY_FILE))
    session = PromptSession(history=history, auto_suggest=AutoSuggestFromHistory(), style=Style.from_dict({
        'prompt': prompt_style,
    }))
    lines = []
    in_config = False
    output_lines = [
        Text(f"Hacker Lang REPL v{VERSION} - Enhanced Interactive Mode", style=success_style),
        Text("Type 'exit' to quit, 'help' for commands, 'clear' to reset", style=info_style),
        Text("Supported: //deps, #libs, @vars, =loops, ?ifs, &bg, >cmds, [config], !comments", style=info_style)
    ]
    while True:
        try:
            prompt = "CONFIG> " if in_config else "hacker> "
            line = session.prompt(Text(prompt, style=prompt_style))
            if not line.strip():
                continue
            if line == "exit":
                break
            if line == "help":
                output_lines.append(Text("REPL Commands:\n- exit: Quit REPL\n- help: This menu\n- clear: Reset session\n- verbose: Toggle verbose", style=header_style))
            elif line == "clear":
                lines = []
                in_config = False
                output_lines.append(Text("Session cleared!", style=success_style))
            elif line == "verbose":
                verbose = not verbose
                output_lines.append(Text(f"Verbose mode: {verbose}", style=info_style))
            else:
                if line == "[":
                    in_config = True
                elif line == "]":
                    if not in_config:
                        output_lines.append(Text("Error: Unmatched ']'", style=error_style))
                    in_config = False
                lines.append(line)
                if not in_config and line and not line.startswith("!"):
                    parsed = parse_lines(lines, verbose)
                    if parsed["errors"]:
                        output_lines.append(Text("REPL Errors:\n" + "\n".join(parsed["errors"]), style=error_style))
                    else:
                        if parsed["libs"]:
                            output_lines.append(Text(f"Warning: Missing custom libs: {', '.join(parsed['libs'])}", style=warning_style))
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
                        out_str = result.stdout + result.stderr
                        if result.returncode != 0:
                            output_lines.append(Text("REPL Error:\n" + out_str.strip(), style=error_style))
                        elif out_str.strip():
                            output_lines.append(Text("REPL Output:\n" + out_str.strip(), style=success_style))
            for out in output_lines:
                console.print(out)
            output_lines = []  # Clear after print? No, accumulate like viewport
            # To simulate viewport, perhaps print all each time, but for simplicity, print incrementally
        except KeyboardInterrupt:
            break
    console.print(Text("REPL session ended.", style=success_style))
    return True

def version_command() -> bool:
    console.print(Text(f"Hacker Lang v{VERSION}", style=info_style))
    return True

def help_command(show_banner: bool = True) -> bool:
    if show_banner:
        console.print(Panel("Hacker Lang CLI - Advanced Scripting Tool", title="Help", style=title_style))
    table = Table(title="Commands Overview", style=header_style)
    table.add_column("Command", style="bold")
    table.add_column("Description")
    table.add_column("Arguments")
    commands = [
        ("run", "Execute a .hacker script", "file [--verbose] or . for bytes project"),
        ("compile", "Compile to native executable", "file [-o output] [--verbose] [--bytes]"),
        ("check", "Validate syntax", "file [--verbose]"),
        ("init", "Generate template script", "file [--verbose]"),
        ("clean", "Remove temporary files", "[--verbose]"),
        ("repl", "Launch interactive REPL", "[--verbose]"),
        ("editor", "Launch hacker-editor", "[file]"),
        ("unpack", "Unpack and install bytes", "bytes [--verbose]"),
        ("version", "Display version", ""),
        ("help", "Show this help menu", ""),
        ("help-ui", "Show special commands list", ""),
    ]
    for cmd, desc, args in commands:
        table.add_row(cmd, desc, args)
    console.print(table)
    console.print("\nSyntax Highlight Example:", style=header_style)
    example_code = """// sudo
# obsidian
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
# logging
> sudo apt update
[
Config=Example
]"""
    console.print(Panel(example_code, title="Example", style=example_style))
    return True

def run_bytes_project(verbose: bool) -> bool:
    bytes_file = "hacker.bytes"
    try:
        with open(bytes_file, "r") as f:
            project = yaml.safe_load(f)
        console.print(Text(f"Running project {project['package']['name']} v{project['package']['version']} by {project['package']['author']}", style=success_style))
        return run_command(project["entry"], verbose)
    except Exception as e:
        console.print(Text(f"Error: {e}", style=error_style))
        return False

def compile_bytes_project(output: str, verbose: bool) -> bool:
    bytes_file = "hacker.bytes"
    try:
        with open(bytes_file, "r") as f:
            project = yaml.safe_load(f)
        if not output:
            output = project['package']['name']
        return compile_command(project["entry"], output, verbose)
    except Exception as e:
        console.print(Text(f"Error: {e}", style=error_style))
        return False

app = typer.Typer()

@app.command()
def run(file: str, verbose: bool = typer.Option(False, "--verbose")):
    ensure_hacker_dir()
    success = run_bytes_project(verbose) if file == "." else run_command(file, verbose)
    raise typer.Exit(code=0 if success else 1)

@app.command()
def compile(file: str, output: Optional[str] = typer.Option(None, "-o"), verbose: bool = typer.Option(False, "--verbose"), bytes_mode: bool = typer.Option(False, "--bytes")):
    ensure_hacker_dir()
    if not output:
        output = Path(file).stem
    success = compile_bytes_project(output, verbose) if bytes_mode else compile_command(file, output, verbose)
    raise typer.Exit(code=0 if success else 1)

@app.command()
def check(file: str, verbose: bool = typer.Option(False, "--verbose")):
    ensure_hacker_dir()
    success = check_command(file, verbose)
    raise typer.Exit(code=0 if success else 1)

@app.command()
def init(file: str, verbose: bool = typer.Option(False, "--verbose")):
    ensure_hacker_dir()
    success = init_command(file, verbose)
    raise typer.Exit(code=0 if success else 1)

@app.command()
def clean(verbose: bool = typer.Option(False, "--verbose")):
    ensure_hacker_dir()
    success = clean_command(verbose)
    raise typer.Exit(code=0 if success else 1)

@app.command()
def repl(verbose: bool = typer.Option(False, "--verbose")):
    ensure_hacker_dir()
    success = run_repl(verbose)
    raise typer.Exit(code=0 if success else 1)

@app.command()
def editor(file: Optional[str] = typer.Argument(None)):
    ensure_hacker_dir()
    success = editor_command(file)
    raise typer.Exit(code=0 if success else 1)

@app.command()
def unpack(target: str, verbose: bool = typer.Option(False, "--verbose")):
    ensure_hacker_dir()
    if target == "bytes":
        success = unpack_bytes(verbose)
    else:
        console.print(Text(f"Unknown unpack target: {target}", style=error_style))
        success = False
    raise typer.Exit(code=0 if success else 1)

@app.command()
def version():
    ensure_hacker_dir()
    success = version_command()
    raise typer.Exit(code=0 if success else 1)

@app.command()
def help_cmd():  # Renamed to avoid conflict with help
    ensure_hacker_dir()
    success = help_command(True)
    raise typer.Exit(code=0 if success else 1)

# TODO: help-ui if needed

if __name__ == "__main__":
    ensure_hacker_dir()
    if len(sys.argv) == 1:
        display_welcome()
        sys.exit(0)
    app()
