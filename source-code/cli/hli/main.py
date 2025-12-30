import argparse
import glob
import json
import os
import re
import subprocess
import sys
import tempfile
import requests
import tomllib
import yaml
import shlex
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text
from rich.tree import Tree
from prompt_toolkit import PromptSession
from prompt_toolkit.completion import WordCompleter
from prompt_toolkit.styles import Style

console = Console()
VERSION = "1.2"
HACKER_DIR = os.path.join(os.getenv("HOME"), ".hackeros", "hacker-lang")
BIN_DIR = os.path.join(HACKER_DIR, "bin")
HISTORY_FILE = os.path.join(os.getenv("HOME"), ".hackeros", "history", "hacker_repl_history")
PARSER_PATH = os.path.join(BIN_DIR, "hacker-plsa")
COMPILER_PATH = os.path.join(BIN_DIR, "hacker-compiler")
RUNTIME_PATH = os.path.join(BIN_DIR, "hacker-runtime")
REPL_PATH = os.path.join(BIN_DIR, "hacker-repl")

class Config:
    def __init__(self):
        self.name = ""
        self.version = ""
        self.author = ""
        self.description = ""
        self.entry = ""
        self.libs = {}
        self.scripts = {}
        self.meta = {}

def ensure_hacker_dir():
    os.makedirs(BIN_DIR, exist_ok=True)
    os.makedirs(os.path.join(HACKER_DIR, "libs"), exist_ok=True)
    os.makedirs(os.path.join(HACKER_DIR, "plugins"), exist_ok=True)
    os.makedirs(os.path.dirname(HISTORY_FILE), exist_ok=True)

def display_welcome():
    console.print(Panel(f"Welcome to Hacker Lang Interface (HLI) v{VERSION}", border_style="magenta", expand=False))
    console.print("Advanced scripting interface for HackerOS Linux system, inspired by Cargo", style="gray")
    console.print("Type 'hli help' for commands or 'hli repl' to start interactive mode.", style="white")
    help_command(False)

def load_project_config():
    if os.path.exists("Project.toml"):
        with open("Project.toml", "rb") as f:
            data = tomllib.load(f)
        pkg = data.get("package", {})
        config = Config()
        config.name = pkg.get("name", "")
        config.version = pkg.get("version", "")
        config.author = pkg.get("author", "")
        config.description = pkg.get("description", "")
        config.entry = data.get("entry", "")
        config.libs = data.get("libs", {})
        config.scripts = data.get("scripts", {})
        config.meta = data.get("meta", {})
        return config
    raise ValueError("no project file found (Project.toml)")

def load_project_entry():
    config = load_project_config()
    return config.entry

def run_command(file, verbose):
    if not os.path.exists(RUNTIME_PATH):
        console.print(f"Hacker runtime not found at {RUNTIME_PATH}. Please install the Hacker Lang tools.", style="red")
        return False
    args = [file]
    if verbose:
        args.append("--verbose")
    proc = subprocess.run([RUNTIME_PATH] + args)
    return proc.returncode == 0

def compile_command(file, output, verbose, bytes_mode):
    if not os.path.exists(COMPILER_PATH):
        console.print(f"Hacker compiler not found at {COMPILER_PATH}. Please install the Hacker Lang tools.", style="red")
        return False
    args = [file, output]
    if bytes_mode:
        args.append("--bytes")
    if verbose:
        args.append("--verbose")
    proc = subprocess.run([COMPILER_PATH] + args)
    return proc.returncode == 0

def check_command(file, verbose):
    if not os.path.exists(PARSER_PATH):
        console.print(f"Hacker parser not found at {PARSER_PATH}. Please install the Hacker Lang tools.", style="red")
        return False
    args = [file]
    if verbose:
        args.append("--verbose")
    proc = subprocess.run([PARSER_PATH] + args, capture_output=True, text=True)
    if proc.returncode != 0:
        console.print(f"Error parsing file: {proc.stderr}", style="red")
        return False
    try:
        parsed = json.loads(proc.stdout)
    except Exception as e:
        console.print(f"Error unmarshaling parse output: {e}", style="red")
        return False
    errors = parsed.get("errors", [])
    if not errors:
        console.print("Syntax validation passed!", style="green")
        return True
    console.print("Errors:", style="red")
    for e in errors:
        console.print(f"✖ {e}", style="red")
    return False

def init_command(file, verbose):
    target_file = file if file else "main.hacker"
    if os.path.exists(target_file):
        console.print(f"File {target_file} already exists!", style="red")
        return False
    template = '''! Hacker Lang advanced template
// sudo ! Privileged operations
// curl ! For downloads
# network-utils ! Custom library example
@APP_NAME=HackerApp ! Application name
@LOG_LEVEL=debug
=3 > echo "Iteration: $APP_NAME" ! Loop example
? [ -f /etc/os-release ] > cat /etc/os-release | grep PRETTY_NAME ! Conditional
& ping -c 1 google.com ! Background task
# logging ! Include logging library
echo "Starting update..."
sudo apt update && sudo apt upgrade -y ! System update
echo "With var: $APP_NAME"
long_running_command_with_vars
[
Author=Advanced User
Version=1.0
Description=System maintenance script
]'''
    with open(target_file, "w") as f:
        f.write(template)
    console.print(f"Initialized template at {target_file}", style="green")
    if verbose:
        console.print("Template content:", style="yellow")
        console.print(template, style="yellow")
    toml_file = "Project.toml"
    if not os.path.exists(toml_file):
        toml_template = f'''[package]
name = "my-hacker-project"
version = "0.1.0"
author = "User"
description = "My Hacker project"
entry = "{target_file}"

[libs]
python = ["library1"]
rust = ["library2"]

[scripts]
build = "hli compile {target_file}"
run = "hli run ."
release = "hli compile --bytes"

[meta]
license = "MIT"
repo = "https://github.com/user/repo"
'''
        with open(toml_file, "w") as f:
            f.write(toml_template)
        console.print("Initialized Project.toml for project", style="green")
    return True

def clean_command(verbose):
    count = 0
    for path in glob.glob("/tmp/*.sh"):
        base = os.path.basename(path)
        if base.startswith("tmp") or base.startswith("sep_"):
            if verbose:
                console.print(f"Removed: {path}", style="yellow")
            os.remove(path)
            count += 1
    console.print(f"Removed {count} temporary files", style="green")
    return True

def unpack_bytes(verbose):
    bytes_path1 = os.path.join(HACKER_DIR, "bin/bytes")
    bytes_path2 = "/usr/bin/bytes"
    if os.path.exists(bytes_path1):
        console.print(f"Bytes already installed at {bytes_path1}.", style="green")
        return True
    if os.path.exists(bytes_path2):
        console.print(f"Bytes already installed at {bytes_path2}.", style="green")
        return True
    os.makedirs(BIN_DIR, exist_ok=True)
    url = "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes"
    resp = requests.get(url)
    if resp.status_code != 200:
        console.print(f"Error: status code {resp.status_code}", style="red")
        return False
    with open(bytes_path1, "wb") as f:
        f.write(resp.content)
    os.chmod(bytes_path1, 0o755)
    if verbose:
        console.print(f"Downloaded and installed bytes from {url} to {bytes_path1}", style="green")
    console.print("Bytes installed successfully!", style="green")
    return True

def run_repl(verbose):
    if not os.path.exists(REPL_PATH):
        console.print(f"Hacker REPL not found at {REPL_PATH}. Please install the Hacker Lang tools.", style="red")
        return False
    args = []
    if verbose:
        args.append("--verbose")
    proc = subprocess.run([REPL_PATH] + args, stdin=sys.stdin)
    if proc.returncode == 0:
        console.print("REPL session ended.", style="green")
        return True
    console.print("REPL failed.", style="red")
    return False

def version_command():
    console.print(f"Hacker Lang Interface (HLI) v{VERSION}", style="cyan")
    return True

def syntax_command():
    console.print("Hacker Lang Syntax Example:", style="bold")
    example_code = '''// sudo
# obsidian
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
echo "With var: $USER"
separate_command
# logging
sudo apt update
[ Config=Example ]'''
    console.print(example_code, style="white")
    return True

def docs_command():
    console.print("Hacker Lang Documentation:", style="bold")
    console.print("Hacker Lang is an advanced scripting language for HackerOS.")
    console.print("Key features:")
    features = [
        "Privileged operations with // sudo",
        "Library includes with # lib-name",
        "Variables with @VAR=value",
        "Loops with =N > command",
        "Conditionals with ? condition > command",
        "Background tasks with & command",
        "Multi-line commands with >> and >>>",
        "Metadata blocks with [ key=value ]",
    ]
    for f in features:
        console.print(f"- {f}")
    console.print("\nFor more details, visit the official documentation or use 'hli tutorials' for examples.")
    return True

def tutorials_command():
    console.print("Hacker Lang Tutorials:", style="bold")
    console.print("Tutorial 1: Basic Script")
    console.print("Create a file main.hacker with:")
    console.print("> echo 'Hello, Hacker Lang!'")
    console.print("Run with: hli run")
    console.print("\nTutorial 2: Using Libraries")
    console.print("Add # logging to your script.")
    console.print("HLI will automatically install if missing.")
    console.print("\nTutorial 3: Projects")
    console.print("Use 'hli init' to create a project with Project.toml.")
    console.print("Then 'hli run' to execute.")
    return True

def help_command(show_banner):
    if show_banner:
        console.print(f"Hacker Lang Interface (HLI) - Advanced Scripting Tool v{VERSION}", style="bold magenta")
    console.print("Commands Overview:", style="bold")
    table = Table(box=None)
    table.add_column("Command")
    table.add_column("Description")
    table.add_column("Arguments")
    table.add_row("run", "Execute a .hacker script or project", "[file] [--verbose]")
    table.add_row("compile", "Compile to native executable or project", "[file] [-o output] [--verbose] [--bytes]")
    table.add_row("check", "Validate syntax", "[file] [--verbose]")
    table.add_row("init", "Generate template script/project", "[file] [--verbose]")
    table.add_row("clean", "Remove temporary files", "[--verbose]")
    table.add_row("repl", "Launch interactive REPL", "[--verbose]")
    table.add_row("unpack", "Unpack and install bytes", "bytes [--verbose]")
    table.add_row("docs", "Show documentation", "")
    table.add_row("tutorials", "Show tutorials", "")
    table.add_row("version", "Display version", "")
    table.add_row("help", "Show this help menu", "")
    table.add_row("syntax", "Show syntax examples", "")
    table.add_row("help-ui", "Show special commands list", "")
    console.print(table)
    return True

def run_help_ui():
    console.print("Hacker Lang Commands List", style="bold magenta")
    items = [
        "run: Execute script/project - Usage: hli run [file] [--verbose]",
        "compile: Compile to executable/project - Usage: hli compile [file] [-o output] [--verbose] [--bytes]",
        "check: Validate syntax - Usage: hli check [file] [--verbose]",
        "init: Generate template - Usage: hli init [file] [--verbose]",
        "clean: Remove temps - Usage: hli clean [--verbose]",
        "repl: Interactive REPL - Usage: hli repl [--verbose]",
        "unpack: Unpack and install bytes - Usage: hli unpack bytes [--verbose]",
        "docs: Show documentation - Usage: hli docs",
        "tutorials: Show tutorials - Usage: hli tutorials",
        "version: Show version - Usage: hli version",
        "help: Show help - Usage: hli help",
        "syntax: Show syntax examples - Usage: hli syntax",
        "help-ui: Interactive help UI - This UI",
    ]
    for item in items:
        console.print(f"- {item}", style="magenta")
    return True

def run_project(verbose):
    try:
        config = load_project_config()
    except Exception as e:
        console.print(f"{e}. Use 'hli init' to create a project.", style="red")
        return False
    console.print(f"Running project {config.name} v{config.version} by {config.author}", style="green")
    check_dependencies(config.entry, verbose)
    return run_command(config.entry, verbose)

def compile_project(output, verbose, bytes_mode):
    try:
        config = load_project_config()
    except Exception as e:
        console.print(f"{e}. Use 'hli init' to create a project.", style="red")
        return False
    if not output:
        output = config.name
    console.print(f"Compiling project {config.name} to {output} with --bytes", style="cyan")
    check_dependencies(config.entry, verbose)
    return compile_command(config.entry, output, verbose, bytes_mode)

def check_project(verbose):
    try:
        config = load_project_config()
    except Exception as e:
        console.print(f"{e}. Use 'hli init' to create a project.", style="red")
        return False
    check_dependencies(config.entry, verbose)
    return check_command(config.entry, verbose)

def check_dependencies(file, verbose):
    if not os.path.exists(file):
        console.print(f"File {file} not found for dependency check.", style="red")
        return False
    with open(file) as f:
        content = f.read()
    libs_dir = os.path.join(HACKER_DIR, "libs")
    plugins_dir = os.path.join(HACKER_DIR, "plugins")
    missing_libs = []
    missing_plugins = []
    for line in content.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        if stripped.startswith("//"):
            plugin_name = re.sub(r"[^a-zA-Z0-9_-]", "", stripped[2:].split()[0])
            if plugin_name and not glob.glob(os.path.join(plugins_dir, plugin_name + "*")) and plugin_name not in missing_plugins:
                missing_plugins.append(plugin_name)
        elif stripped.startswith("#"):
            lib_name = re.sub(r"[^a-zA-Z0-9_-]", "", stripped[1:].split()[0])
            if lib_name and not glob.glob(os.path.join(libs_dir, lib_name + "*")) and lib_name not in missing_libs:
                missing_libs.append(lib_name)
    if missing_plugins:
        if verbose:
            console.print(f"Missing plugins: {', '.join(missing_plugins)}", style="yellow")
        for p in missing_plugins:
            console.print(f"Installing plugin {p} via bytes...", style="yellow")
            proc = subprocess.run(["bytes", "plugin", "install", p])
            if proc.returncode != 0:
                return False
    if missing_libs:
        if verbose:
            console.print(f"Missing libs: {', '.join(missing_libs)}", style="yellow")
        for l in missing_libs:
            console.print(f"Installing lib {l} via bytes...", style="yellow")
            proc = subprocess.run(["bytes", "install", l])
            if proc.returncode != 0:
                return False
    return True

class TaskConfig:
    def __init__(self, vars=None, tasks=None, aliases=None):
        self.vars = vars or {}
        self.tasks = tasks or {}
        self.aliases = aliases or {}

def execute_task(task_name, config, executed=set()):
    if task_name in executed:
        raise ValueError(f"cycle detected in tasks involving {task_name}")
    executed.add(task_name)
    if task_name not in config.tasks:
        raise ValueError(f"task {task_name} not found")
    task = config.tasks[task_name]
    for req in task.get("requires", []):
        execute_task(req, config, executed)
    for cmd_str in task.get("run", []):
        for var_name, var_value in config.vars.items():
            cmd_str = cmd_str.replace("{{" + var_name + "}}", str(var_value))
        proc = subprocess.run(["sh", "-c", cmd_str])
        if proc.returncode != 0:
            raise ValueError(f"command failed: {cmd_str}")

def display_command_list():
    console.clear()
    title = Text("HLI Shell - Commands", style="bold purple")
    tree = Tree("Available Commands", style="bold blue")
    
    core = tree.add("Core commands", style="blue")
    core.add("run [file] [--verbose]")
    core.add("compile [file] [-o output] [--verbose] [--bytes]")
    core.add("check [file] [--verbose]")
    core.add("init [file] [--verbose]")
    core.add("clean [--verbose]")
    core.add("repl [--verbose]")
    
    install = tree.add("Install commands", style="blue")
    install.add("unpack bytes [--verbose]")
    
    info = tree.add("Info commands", style="blue")
    info.add("docs")
    info.add("tutorials")
    info.add("version")
    info.add("help")
    info.add("syntax")
    info.add("help-ui")
    
    tree.add("exit: Exit the shell")
    
    panel = Panel(tree, title=title, expand=False, border_style="purple")
    console.print(panel)

def handle_command(args):
    known_commands = ["run", "compile", "check", "init", "clean", "repl", "unpack", "docs", "tutorials", "version", "help", "syntax", "help-ui"]
    success = True
    if args.command in known_commands:
        if args.command == "run":
            verbose = args.verbose
            file = args.file
            if not file:
                try:
                    entry = load_project_entry()
                    check_dependencies(entry, verbose)
                    success = run_command(entry, verbose)
                except Exception as e:
                    console.print("No project found. Use 'hli init' or specify a file.", style="red")
                    success = False
            elif file == ".":
                success = run_project(verbose)
            else:
                check_dependencies(file, verbose)
                success = run_command(file, verbose)
        elif args.command == "compile":
            verbose = args.verbose
            output = args.output
            bytes_mode = args.bytes
            file = args.file
            if not file:
                try:
                    entry = load_project_entry()
                    if not output:
                        output = os.path.splitext(entry)[0]
                    check_dependencies(entry, verbose)
                    success = compile_command(entry, output, verbose, bytes_mode)
                except Exception as e:
                    console.print("No project found. Use 'hli init' or specify a file.", style="red")
                    success = False
            elif file == ".":
                success = compile_project(output, verbose, bytes_mode)
            else:
                if not output:
                    output = os.path.splitext(file)[0]
                check_dependencies(file, verbose)
                success = compile_command(file, output, verbose, bytes_mode)
        elif args.command == "check":
            verbose = args.verbose
            file = args.file
            if not file:
                try:
                    entry = load_project_entry()
                    check_dependencies(entry, verbose)
                    success = check_command(entry, verbose)
                except Exception as e:
                    console.print("No project found. Use 'hli init' or specify a file.", style="red")
                    success = False
            elif file == ".":
                success = check_project(verbose)
            else:
                check_dependencies(file, verbose)
                success = check_command(file, verbose)
        elif args.command == "init":
            success = init_command(args.file, args.verbose)
        elif args.command == "clean":
            success = clean_command(args.verbose)
        elif args.command == "repl":
            success = run_repl(args.verbose)
        elif args.command == "unpack":
            verbose = args.verbose
            if args.item != "bytes":
                console.print("Expected exactly one argument: bytes", style="red")
                success = False
            else:
                success = unpack_bytes(verbose)
        elif args.command == "docs":
            success = docs_command()
        elif args.command == "tutorials":
            success = tutorials_command()
        elif args.command == "version":
            success = version_command()
        elif args.command == "help":
            success = help_command(True)
        elif args.command == "syntax":
            success = syntax_command()
        elif args.command == "help-ui":
            success = run_help_ui()
    else:
        console.print(f"Unknown command: {args.command}", style="red")
        help_command(False)
        success = False
    return success

def interactive_main():
    commands = [
        "run", "run --verbose",
        "compile", "compile -o", "compile --verbose", "compile --bytes",
        "check", "check --verbose",
        "init", "init --verbose",
        "clean", "clean --verbose",
        "repl", "repl --verbose",
        "unpack bytes", "unpack bytes --verbose",
        "docs", "tutorials", "version", "help", "syntax", "help-ui", "exit"
    ]
    completer = WordCompleter(commands, ignore_case=True)
    style = Style.from_dict({
        'prompt': 'blue bold',
    })
    session = PromptSession(completer=completer, style=style)
    console.print("Welcome to HLI Shell! Integrated with hacker v2.1. Type 'exit' to quit.", style="blue")
    while True:
        display_command_list()
        try:
            cmd = session.prompt('hli> ')
            if cmd.strip() == "":
                continue
            if cmd == "exit":
                console.print("Exiting HLI Shell...", style="gray")
                break
            try:
                argv = shlex.split(cmd)
                args = parser.parse_args(argv)
                handle_command(args)
            except SystemExit:
                parts = shlex.split(cmd)
                command = parts[0] if parts else ""
                if command in ["install", "update", "remove"]:
                    console.print(f"Please use bytes {command}", style="yellow")
                elif os.path.exists(".hackerfile"):
                    with open(".hackerfile") as f:
                        data = yaml.safe_load(f)
                    config = TaskConfig(data.get("vars"), data.get("tasks"), data.get("aliases"))
                    aliased_task = config.aliases.get(command, command)
                    if aliased_task in config.tasks:
                        executed = set()
                        try:
                            execute_task(aliased_task, config, executed)
                        except Exception as e:
                            console.print(f"Error executing task: {e}", style="red")
                    else:
                        console.print(f"Unknown command: {cmd}", style="red")
                else:
                    pass  # argparse already handled error
            except Exception as e:
                console.print(f"Error executing command: {e}", style="red")
            console.input("Press Enter to continue...")
        except KeyboardInterrupt:
            console.print("\nInterrupted. Type 'exit' to quit.", style="gray")
            continue
        except EOFError:
            break

if __name__ == "__main__":
    ensure_hacker_dir()
    if len(sys.argv) > 1:
        command = sys.argv[1]
        if command in ["--version", "-v"]:
            version_command()
            sys.exit(0)
        elif command in ["--help", "-h"]:
            help_command(True)
            sys.exit(0)
    parser = argparse.ArgumentParser(description="Hacker Lang Interface (HLI) - Advanced Scripting Tool")
    subparsers = parser.add_subparsers(dest="command")
    run_parser = subparsers.add_parser("run", help="Execute a .hacker script or project")
    run_parser.add_argument("file", nargs="?")
    run_parser.add_argument("--verbose", action="store_true")
    compile_parser = subparsers.add_parser("compile", help="Compile to native executable or project")
    compile_parser.add_argument("file", nargs="?")
    compile_parser.add_argument("-o", "--output")
    compile_parser.add_argument("--bytes", action="store_true")
    compile_parser.add_argument("--verbose", action="store_true")
    check_parser = subparsers.add_parser("check", help="Validate syntax")
    check_parser.add_argument("file", nargs="?")
    check_parser.add_argument("--verbose", action="store_true")
    init_parser = subparsers.add_parser("init", help="Generate template script/project")
    init_parser.add_argument("file", nargs="?")
    init_parser.add_argument("--verbose", action="store_true")
    clean_parser = subparsers.add_parser("clean", help="Remove temporary files")
    clean_parser.add_argument("--verbose", action="store_true")
    repl_parser = subparsers.add_parser("repl", help="Launch interactive REPL")
    repl_parser.add_argument("--verbose", action="store_true")
    unpack_parser = subparsers.add_parser("unpack", help="Unpack and install bytes")
    unpack_parser.add_argument("item", choices=["bytes"])
    unpack_parser.add_argument("--verbose", action="store_true")
    subparsers.add_parser("docs", help="Show documentation")
    subparsers.add_parser("tutorials", help="Show tutorials")
    subparsers.add_parser("version", help="Display version")
    subparsers.add_parser("help", help="Show this help menu")
    subparsers.add_parser("syntax", help="Show syntax examples")
    subparsers.add_parser("help-ui", help="Show special commands list")
    args = parser.parse_args()
    if not args.command:
        interactive_main()
        sys.exit(0)
    else:
        if args.command not in ["run", "compile", "check", "init", "clean", "repl", "unpack", "docs", "tutorials", "version", "help", "syntax", "help-ui"]:
            if os.path.exists(".hackerfile"):
                with open(".hackerfile") as f:
                    data = yaml.safe_load(f)
                config = TaskConfig(data.get("vars"), data.get("tasks"), data.get("aliases"))
                aliased_task = config.aliases.get(args.command, args.command)
                if aliased_task in config.tasks:
                    executed = set()
                    try:
                        execute_task(aliased_task, config, executed)
                        sys.exit(0)
                    except Exception as e:
                        console.print(f"Error executing task: {e}", style="red")
                        sys.exit(1)
                else:
                    console.print(f"Unknown task: {args.command}", style="red")
                    help_command(False)
                    sys.exit(1)
            elif args.command in ["install", "update", "remove"]:
                console.print(f"Please use bytes {args.command}", style="yellow")
                sys.exit(0)
            else:
                console.print(f"Unknown command: {args.command}", style="red")
                help_command(False)
                sys.exit(1)
        success = handle_command(args)
        sys.exit(0 if success else 1)
