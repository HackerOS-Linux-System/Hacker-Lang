import argparse
import os
import shlex
import shutil
import subprocess
import sys
from prompt_toolkit import PromptSession
from prompt_toolkit.completion import WordCompleter
from prompt_toolkit.styles import Style
from rich.console import Console
from rich.panel import Panel
from rich.text import Text
from rich.tree import Tree
console = Console()
VERSION = "1.7.5"
HACKER_DIR = ".hackeros/hacker-lang"
BIN_DIR = "bin"
COMPILER_PATH = "hl-compiler"
RUNTIME_PATH = "hl-runtime"
PLSA_PATH = "hl-plsa"
REPL_PATH = "hl-repl"
BYTES_PATH = "bytes"
VIRUS_PATH = "/usr/bin/virus"
DOCS_PATH = "/usr/bin/hlh"
commands = [
    "run", "compile", "clear", "check", "info", "help", "version", "exit",
    "install", "remove", "plugin", "repl", "docs",
    "set", "bb", "tt", "ss", "ii", "rr", "cc", "virus-docs", "clean",
    "--verbose", "-o", "--output"
]
completer = WordCompleter(commands, ignore_case=True)
style = Style.from_dict({
    'prompt': 'blue bold',
})
def ensure_hacker_dir():
    full_bin_dir = os.path.join(os.environ.get("HOME"), HACKER_DIR, BIN_DIR)
    os.makedirs(full_bin_dir, exist_ok=True)
def display_welcome():
    title = Text(f"Welcome to Hacker Lang Intercars (hli) v{VERSION}", style="bold purple")
    panel = Panel("Enhanced interactive tool for managing Hacker Lang scripts.\nType 'help' for available commands.", title=title, expand=False, border_style="purple")
    console.print(panel)
    help_command(show_banner=False)
def run_command(args):
    full_runtime_path = os.path.join(os.environ.get("HOME"), HACKER_DIR, BIN_DIR, RUNTIME_PATH)
    if not os.path.exists(full_runtime_path):
        console.print(f"Hacker runtime not found at {full_runtime_path}. Please install the Hacker Lang tools.", style="red")
        return False
    cmd_args = [args.file]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Executing script: {args.file} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([full_runtime_path] + cmd_args)
        console.print("Execution completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Execution failed with error: {e}", style="red")
        return False
def compile_command(args):
    full_compiler_path = os.path.join(os.environ.get("HOME"), HACKER_DIR, BIN_DIR, COMPILER_PATH)
    if not os.path.exists(full_compiler_path):
        console.print(f"Hacker compiler not found at {full_compiler_path}. Please install the Hacker Lang tools.", style="red")
        return False
    if args.output is None:
        args.output = os.path.splitext(args.file)[0]
    cmd_args = [args.file, args.output]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Compiling script: {args.file} to {args.output} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([full_compiler_path] + cmd_args)
        console.print("Compilation completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Compilation failed with error: {e}", style="red")
        return False
def clear_command(args):
    cache_path = os.path.join(os.environ.get("HOME"), ".cache", "hacker-lang")
    if not os.path.exists(cache_path):
        console.print("Cache directory does not exist.", style="yellow")
        return True
    if args.verbose:
        console.print(f"Clearing cache at: {cache_path}", style="cyan")
    try:
        for filename in os.listdir(cache_path):
            file_path = os.path.join(cache_path, filename)
            if os.path.isfile(file_path) or os.path.islink(file_path):
                os.unlink(file_path)
            elif os.path.isdir(file_path):
                shutil.rmtree(file_path)
        console.print("Cache cleared successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Failed to clear cache: {e}", style="red")
        return False
def check_command(args):
    full_plsa_path = os.path.join(os.environ.get("HOME"), HACKER_DIR, BIN_DIR, PLSA_PATH)
    if not os.path.exists(full_plsa_path):
        console.print(f"Hacker PLSA not found at {full_plsa_path}. Please install the Hacker Lang tools (hacker unpack hl-utils).", style="red")
        return False
    cmd_args = [args.file]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Checking script: {args.file} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([full_plsa_path] + cmd_args)
        console.print("Check completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Check failed with error: {e}", style="red")
        return False
def install_command(args):
    full_bytes_path = os.path.join(os.environ.get("HOME"), HACKER_DIR, BIN_DIR, BYTES_PATH)
    if not os.path.exists(full_bytes_path):
        console.print(f"Bytes tool not found at {full_bytes_path}. Please install the Hacker Lang tools.", style="red")
        return False
    cmd_args = ["install", args.library]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Installing library: {args.library} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([full_bytes_path] + cmd_args)
        console.print("Installation completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Installation failed with error: {e}", style="red")
        return False
def remove_command(args):
    full_bytes_path = os.path.join(os.environ.get("HOME"), HACKER_DIR, BIN_DIR, BYTES_PATH)
    if not os.path.exists(full_bytes_path):
        console.print(f"Bytes tool not found at {full_bytes_path}. Please install the Hacker Lang tools.", style="red")
        return False
    cmd_args = ["remove", args.library]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Removing library: {args.library} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([full_bytes_path] + cmd_args)
        console.print("Removal completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Removal failed with error: {e}", style="red")
        return False
def plugin_install_command(args):
    full_bytes_path = os.path.join(os.environ.get("HOME"), HACKER_DIR, BIN_DIR, BYTES_PATH)
    if not os.path.exists(full_bytes_path):
        console.print(f"Bytes tool not found at {full_bytes_path}. Please install the Hacker Lang tools (hacker unpack hl-utils).", style="red")
        return False
    cmd_args = ["plugin", "install", args.plugin_name]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Installing plugin: {args.plugin_name} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([full_bytes_path] + cmd_args)
        console.print("Plugin installation completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Plugin installation failed with error: {e}", style="red")
        return False
def plugin_remove_command(args):
    full_bytes_path = os.path.join(os.environ.get("HOME"), HACKER_DIR, BIN_DIR, BYTES_PATH)
    if not os.path.exists(full_bytes_path):
        console.print(f"Bytes tool not found at {full_bytes_path}. Please install the Hacker Lang tools.", style="red")
        return False
    cmd_args = ["plugin", "remove", args.plugin_name]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Removing plugin: {args.plugin_name} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([full_bytes_path] + cmd_args)
        console.print("Plugin removal completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Plugin removal failed with error: {e}", style="red")
        return False
def repl_command(args):
    home = os.environ.get("HOME")
    venv_activate = os.path.join(home, HACKER_DIR, "venv", "bin", "activate")
    full_repl_path = os.path.join(home, HACKER_DIR, BIN_DIR, REPL_PATH)
    if not os.path.exists(full_repl_path):
        console.print(f"Hacker REPL not found at {full_repl_path}. Please install the Hacker Lang tools.", style="red")
        return False
    if not os.path.exists(venv_activate):
        console.print(f"Venv activate script not found at {venv_activate}.", style="red")
        return False
    console.print("Starting REPL...", style="cyan")
    try:
        cmd = f"source {venv_activate} && {full_repl_path}"
        if args.verbose:
            cmd += " --verbose"
        subprocess.run(["bash", "-c", cmd])
        console.print("REPL session completed.", style="green")
        return True
    except Exception as e:
        console.print(f"REPL failed with error: {e}", style="red")
        return False
def docs_command(args):
    if not os.path.exists(DOCS_PATH):
        console.print(f"HLH tool not found at {DOCS_PATH}. Please install the tools.", style="red")
        return False
    cmd_args = []
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Opening docs {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([DOCS_PATH] + cmd_args)
        console.print("Docs opened successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Docs failed with error: {e}", style="red")
        return False
def set_command(args):
    if not os.path.exists(VIRUS_PATH):
        console.print(f"Virus tool not found at {VIRUS_PATH}.", style="red")
        return False
    cmd_args = ["set"]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Running virus set {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([VIRUS_PATH] + cmd_args)
        console.print("Command completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Command failed with error: {e}", style="red")
        return False
def bb_command(args):
    if not os.path.exists(VIRUS_PATH):
        console.print(f"Virus tool not found at {VIRUS_PATH}.", style="red")
        return False
    cmd_args = ["bb"]
    if hasattr(args, "bb_arg") and args.bb_arg:
        if args.bb_arg in ["==", "=", ",,", ","]:
            cmd_args.append(args.bb_arg)
        else:
            console.print(f"Invalid argument for bb: {args.bb_arg}", style="red")
            return False
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Running virus bb {args.bb_arg if hasattr(args, 'bb_arg') and args.bb_arg else ''} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([VIRUS_PATH] + cmd_args)
        console.print("Command completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Command failed with error: {e}", style="red")
        return False
def tt_command(args):
    if not os.path.exists(VIRUS_PATH):
        console.print(f"Virus tool not found at {VIRUS_PATH}.", style="red")
        return False
    cmd_args = ["tt"]
    if hasattr(args, "tt_arg") and args.tt_arg:
        cmd_args.append(args.tt_arg)
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Running virus tt {args.tt_arg if hasattr(args, 'tt_arg') and args.tt_arg else ''} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([VIRUS_PATH] + cmd_args)
        console.print("Command completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Command failed with error: {e}", style="red")
        return False
def ss_command(args):
    if not os.path.exists(VIRUS_PATH):
        console.print(f"Virus tool not found at {VIRUS_PATH}.", style="red")
        return False
    cmd_args = ["ss"]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Running virus ss {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([VIRUS_PATH] + cmd_args)
        console.print("Command completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Command failed with error: {e}", style="red")
        return False
def ii_command(args):
    if not os.path.exists(VIRUS_PATH):
        console.print(f"Virus tool not found at {VIRUS_PATH}.", style="red")
        return False
    cmd_args = ["ii", args.lib]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Running virus ii {args.lib} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([VIRUS_PATH] + cmd_args)
        console.print("Command completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Command failed with error: {e}", style="red")
        return False
def rr_command(args):
    if not os.path.exists(VIRUS_PATH):
        console.print(f"Virus tool not found at {VIRUS_PATH}.", style="red")
        return False
    cmd_args = ["rr", args.lib]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Running virus rr {args.lib} {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([VIRUS_PATH] + cmd_args)
        console.print("Command completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Command failed with error: {e}", style="red")
        return False
def cc_command(args):
    if not os.path.exists(VIRUS_PATH):
        console.print(f"Virus tool not found at {VIRUS_PATH}.", style="red")
        return False
    cmd_args = ["cc"]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Running virus cc {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([VIRUS_PATH] + cmd_args)
        console.print("Command completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Command failed with error: {e}", style="red")
        return False
def virus_docs_command(args):
    if not os.path.exists(VIRUS_PATH):
        console.print(f"Virus tool not found at {VIRUS_PATH}.", style="red")
        return False
    cmd_args = ["docs"]
    if args.verbose:
        cmd_args.append("--verbose")
    console.print(f"Running virus docs {'(verbose mode)' if args.verbose else ''}", style="cyan")
    try:
        subprocess.run([VIRUS_PATH] + cmd_args)
        console.print("Command completed successfully.", style="green")
        return True
    except Exception as e:
        console.print(f"Command failed with error: {e}", style="red")
        return False
def clean_command(args):
    console.clear()
    return True
def info_command(args):
    console.print(f"Hacker Lang Intercars (hli) v{VERSION}", style="cyan")
    console.print(f"Binaries path: ~/{HACKER_DIR}/{BIN_DIR}", style="cyan")
    return True
def help_command(show_banner=True):
    if show_banner:
        title = Text(f"Hacker Lang Intercars (hli) - Enhanced Scripting Tool v{VERSION}", style="bold purple")
        console.print(Panel("", title=title, expand=False, border_style="purple"))
    console.print("Available Commands:", style="bold blue")
    tree = Tree("Commands", style="bold blue")
    tree.add("run: Execute a .hacker script [usage: run <file> [--verbose]]", style="white")
    tree.add("compile: Compile to native executable [usage: compile <file> [-o output] [--verbose]]", style="white")
    tree.add("clear: Clear the hacker-lang cache [usage: clear [--verbose]]", style="white")
    tree.add("check: Check a .hacker script [usage: check <file> [--verbose]]", style="white")
    tree.add("install: Install a library [usage: install <library> [--verbose]]", style="white")
    tree.add("remove: Remove a library [usage: remove <library> [--verbose]]", style="white")
    tree.add("plugin install: Install a plugin [usage: plugin install <plugin> [--verbose]]", style="white")
    tree.add("plugin remove: Remove a plugin [usage: plugin remove <plugin> [--verbose]]", style="white")
    tree.add("repl: Start the REPL [usage: repl [--verbose]]", style="white")
    tree.add("docs: Open documentation [usage: docs [--verbose]]", style="white")
    tree.add("set: Run virus set [usage: set [--verbose]]", style="white")
    tree.add("bb: Run virus bb (with optional ==, =, ,,, ,) [usage: bb [==|=|,,|,] [--verbose]]", style="white")
    tree.add("tt: Run virus tt (optional rust) [usage: tt [rust] [--verbose]]", style="white")
    tree.add("ss: Run virus ss [usage: ss [--verbose]]", style="white")
    tree.add("ii: Run virus ii <lib> [usage: ii <lib> [--verbose]]", style="white")
    tree.add("rr: Run virus rr <lib> [usage: rr <lib> [--verbose]]", style="white")
    tree.add("cc: Run virus cc [usage: cc [--verbose]]", style="white")
    tree.add("virus-docs: Run virus docs [usage: virus-docs [--verbose]]", style="white")
    tree.add("clean: Clear the screen [usage: clean]", style="white")
    tree.add("info: Display tool info", style="white")
    tree.add("help: Show this help menu", style="white")
    tree.add("version: Display version", style="white")
    tree.add("exit: Exit the interactive shell", style="white")
    console.print(tree)
    console.print("\nGlobal options (in CLI mode):", style="bright_black")
    console.print("-v, --version: Display version", style="magenta")
    console.print("-h, --help: Display help", style="magenta")
    console.print("--verbose: Enable verbose mode", style="magenta")
    return True
def version_command(args):
    console.print(f"Hacker Lang Intercars (hli) v{VERSION}", style="cyan")
    return True
def create_parser():
    parser = argparse.ArgumentParser(description="Hacker Lang Intercars (hli)", add_help=False)
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("-v", "--version", action="store_true", dest="global_version")
    parser.add_argument("-h", "--help", action="store_true", dest="global_help")
    subparsers = parser.add_subparsers(dest="command")
    run_parser = subparsers.add_parser("run")
    run_parser.add_argument("file", type=str)
    run_parser.set_defaults(func=run_command)
    compile_parser = subparsers.add_parser("compile")
    compile_parser.add_argument("file", type=str)
    compile_parser.add_argument("-o", "--output", type=str, default=None)
    compile_parser.set_defaults(func=compile_command)
    clear_parser = subparsers.add_parser("clear")
    clear_parser.set_defaults(func=clear_command)
    check_parser = subparsers.add_parser("check")
    check_parser.add_argument("file", type=str)
    check_parser.set_defaults(func=check_command)
    install_parser = subparsers.add_parser("install")
    install_parser.add_argument("library", type=str)
    install_parser.set_defaults(func=install_command)
    remove_parser = subparsers.add_parser("remove")
    remove_parser.add_argument("library", type=str)
    remove_parser.set_defaults(func=remove_command)
    plugin_parser = subparsers.add_parser("plugin")
    plugin_subparsers = plugin_parser.add_subparsers(dest="plugin_cmd")
    plugin_install_parser = plugin_subparsers.add_parser("install")
    plugin_install_parser.add_argument("plugin_name", type=str)
    plugin_install_parser.set_defaults(func=plugin_install_command)
    plugin_remove_parser = plugin_subparsers.add_parser("remove")
    plugin_remove_parser.add_argument("plugin_name", type=str)
    plugin_remove_parser.set_defaults(func=plugin_remove_command)
    repl_parser = subparsers.add_parser("repl")
    repl_parser.set_defaults(func=repl_command)
    docs_parser = subparsers.add_parser("docs")
    docs_parser.set_defaults(func=docs_command)
    set_parser = subparsers.add_parser("set")
    set_parser.set_defaults(func=set_command)
    bb_parser = subparsers.add_parser("bb")
    bb_parser.add_argument("bb_arg", nargs="?", default=None)
    bb_parser.set_defaults(func=bb_command)
    tt_parser = subparsers.add_parser("tt")
    tt_parser.add_argument("tt_arg", nargs="?", default=None)
    tt_parser.set_defaults(func=tt_command)
    ss_parser = subparsers.add_parser("ss")
    ss_parser.set_defaults(func=ss_command)
    ii_parser = subparsers.add_parser("ii")
    ii_parser.add_argument("lib", type=str)
    ii_parser.set_defaults(func=ii_command)
    rr_parser = subparsers.add_parser("rr")
    rr_parser.add_argument("lib", type=str)
    rr_parser.set_defaults(func=rr_command)
    cc_parser = subparsers.add_parser("cc")
    cc_parser.set_defaults(func=cc_command)
    virus_docs_parser = subparsers.add_parser("virus-docs")
    virus_docs_parser.set_defaults(func=virus_docs_command)
    clean_parser = subparsers.add_parser("clean")
    clean_parser.set_defaults(func=clean_command)
    info_parser = subparsers.add_parser("info")
    info_parser.set_defaults(func=info_command)
    help_parser = subparsers.add_parser("help")
    help_parser.set_defaults(func=lambda args: help_command(True))
    version_parser = subparsers.add_parser("version")
    version_parser.set_defaults(func=version_command)
    return parser
def interactive_mode():
    ensure_hacker_dir()
    display_welcome()
    session = PromptSession(completer=completer, style=style)
    parser = create_parser()
    while True:
        console.clear()
        help_command(show_banner=False)
        try:
            cmd = session.prompt('hli> ')
            if cmd.strip() == "":
                continue
            if cmd.strip() == "exit":
                console.print("Exiting hli...", style="bright_black")
                sys.exit(0)
            args_list = shlex.split(cmd)
            namespace = parser.parse_args(args_list)
            success = True
            if namespace.global_version:
                success = version_command(None)
            elif namespace.global_help:
                success = help_command(True)
            elif hasattr(namespace, "func"):
                if namespace.command == "compile" and namespace.output is None:
                    namespace.output = os.path.splitext(namespace.file)[0]
                success = namespace.func(namespace)
            else:
                console.print("Unknown command", style="red")
                help_command(False)
                success = False
            console.input("Press [enter] to continue...")
        except argparse.ArgumentError as e:
            console.print(f"Error: {e}", style="red")
            console.input("Press [enter] to continue...")
        except SystemExit:
            # Argparse calls sys.exit on error, catch it
            console.input("Press [enter] to continue...")
        except KeyboardInterrupt:
            console.print("\nInterrupted. Type 'exit' to quit.", style="bright_black")
            console.input("Press [enter] to continue...")
        except EOFError:
            break
        except Exception as e:
            console.print(f"Error: {e}", style="red")
            console.input("Press [enter] to continue...")
def cli_mode():
    ensure_hacker_dir()
    parser = create_parser()
    if len(sys.argv) == 1:
        display_welcome()
        sys.exit(0)
    try:
        namespace = parser.parse_args()
        success = True
        if namespace.global_version:
            success = version_command(None)
        elif namespace.global_help:
            success = help_command(True)
        elif hasattr(namespace, "func"):
            if namespace.command == "compile" and namespace.output is None:
                namespace.output = os.path.splitext(namespace.file)[0]
            success = namespace.func(namespace)
        else:
            success = False
    except:
        success = False
    sys.exit(0 if success else 1)
if __name__ == "__main__":
    if len(sys.argv) > 1:
        cli_mode()
    else:
        interactive_mode()
