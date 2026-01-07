import argparse
import os
import shlex
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

VERSION = "1.4"
HACKER_DIR = ".hackeros/hacker-lang"
BIN_DIR = "bin"
COMPILER_PATH = "hacker-compiler"
RUNTIME_PATH = "hacker-runtime"

commands = [
    "run", "compile", "help", "version", "exit",
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
    panel = Panel("Simplified tool for running and compiling .hacker scripts\nType 'help' for available commands.", title=title, expand=False, border_style="purple")
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

def help_command(show_banner=True):
    if show_banner:
        title = Text(f"Hacker Lang Intercars (hli) - Simplified Scripting Tool v{VERSION}", style="bold purple")
        console.print(Panel("", title=title, expand=False, border_style="purple"))
    console.print("Available Commands:", style="bold blue")
    tree = Tree("Commands", style="bold blue")
    tree.add("run: Execute a .hacker script [usage: run <file> [--verbose]]", style="white")
    tree.add("compile: Compile to native executable [usage: compile <file> [-o output] [--verbose]]", style="white")
    tree.add("help: Show this help menu", style="white")
    tree.add("version: Display version", style="white")
    tree.add("exit: Exit the interactive shell", style="white")
    console.print(tree)
    console.print("\nGlobal options (in CLI mode):", style="gray")
    console.print("-v, --version: Display version", style="magenta")
    console.print("-h, --help: Display help", style="magenta")
    return True

def version_command(args):
    console.print(f"Hacker Lang Intercars (hli) v{VERSION}", style="cyan")
    return True

def create_parser():
    parser = argparse.ArgumentParser(description="Hacker Lang Intercars (hli)", add_help=False)
    parser.add_argument("-v", "--version", action="store_true", dest="global_version")
    parser.add_argument("-h", "--help", action="store_true", dest="global_help")
    subparsers = parser.add_subparsers(dest="command")
    run_parser = subparsers.add_parser("run")
    run_parser.add_argument("file", type=str)
    run_parser.add_argument("--verbose", action="store_true")
    run_parser.set_defaults(func=run_command)
    compile_parser = subparsers.add_parser("compile")
    compile_parser.add_argument("file", type=str)
    compile_parser.add_argument("-o", "--output", type=str, default=None)
    compile_parser.add_argument("--verbose", action="store_true")
    compile_parser.set_defaults(func=compile_command)
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
            if cmd == "exit":
                console.print("Exiting hli...", style="gray")
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
            console.print("\nInterrupted. Type 'exit' to quit.", style="gray")
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
