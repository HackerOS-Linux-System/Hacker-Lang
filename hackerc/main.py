import argparse
import os
import subprocess
import sys
import tempfile
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.syntax import Syntax
from rich.text import Text
from parser import parse_hacker_file
from repl import run_repl

console = Console()
VERSION = "0.0.1"
HACKER_DIR = os.path.expanduser("~/.hacker-lang")
BIN_DIR = os.path.join(HACKER_DIR, "bin")

def ensure_hacker_dir():
    os.makedirs(BIN_DIR, exist_ok=True)
    os.makedirs(os.path.join(HACKER_DIR, "libs"), exist_ok=True)

def display_welcome():
    banner = Text("Hacker Lang", style="bold magenta")
    banner.append("\nSimple scripting for Debian-based Linux", style="italic cyan")
    banner.append(f"\nVersion {VERSION}", style="bold blue")
    console.print(Panel(banner, expand=False))
    help_command(show_banner=False)

def run_command(file_path, verbose=False):
    deps, libs, vars, cmds, includes, errors = parse_hacker_file(file_path, verbose)
    if errors:
        console.print(Panel("\n".join(errors), title="Syntax Errors", style="bold red"))
        return False

    with tempfile.NamedTemporaryFile(mode='w+', suffix='.sh', delete=False) as temp_sh:
        temp_sh.write('#!/bin/bash\n')
        temp_sh.write('set -e\n')

        for var, value in vars.items():
            temp_sh.write(f'export {var}="{value}"\n')

        for dep in deps:
            check_cmd = f"command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})"
            if check_cmd and dep != "sudo":
                temp_sh.write(f"{check_cmd}\n")

        for include in includes:
            lib_path = os.path.join(HACKER_DIR, "libs", include, "main.hacker")
            if os.path.exists(lib_path):
                temp_sh.write(f"# Included from {include}\n")
                with open(lib_path, 'r') as lib_file:
                    temp_sh.write(lib_file.read() + "\n")

        for cmd in cmds:
            temp_sh.write(f"{cmd}\n")

        temp_sh_path = temp_sh.name

    os.chmod(temp_sh_path, 0o755)

    console.print(Panel(f"Running script from {file_path}", title="Hacker Lang Run", style="bold green"))
    try:
        env = os.environ.copy()
        env.update(vars)
        subprocess.check_call(['bash', temp_sh_path], env=env)
        console.print("[bold green]Execution successful![/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Execution failed with code {e.returncode}[/bold red]")
        return False
    finally:
        os.unlink(temp_sh_path)

def compile_command(file_path, output, verbose=False):
    deps, libs, vars, cmds, includes, errors = parse_hacker_file(file_path, verbose)
    if errors:
        console.print(Panel("\n".join(errors), title="Syntax Errors", style="bold red"))
        return False

    if not output:
        output = os.path.splitext(file_path)[0]

    bin_path = os.path.join(BIN_DIR, "hacker-compiler")
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-compiler not found in ~/.hacker-lang/bin/.[/bold red]")
        return False

    console.print(Panel(f"Compiling {file_path} to {output}", title="Hacker Lang Compile", style="bold blue"))
    cmd = [bin_path, file_path, output]
    if verbose:
        cmd.append('--verbose')
    try:
        subprocess.check_call(cmd)
        console.print("[bold green]Compilation successful![/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Compilation failed with code {e.returncode}[/bold red]")
        return False

def check_command(file_path, verbose=False):
    console.print(Panel(f"Checking syntax of {file_path}", title="Hacker Lang Check", style="bold cyan"))
    deps, libs, vars, cmds, includes, errors = parse_hacker_file(file_path, verbose)
    if errors:
        console.print(Panel("\n".join(errors), title="Syntax Errors", style="bold red"))
        return False
    console.print("[bold green]Syntax check passed![/bold green]")
    return True

def init_command(file_path, verbose=False):
    if os.path.exists(file_path):
        console.print(f"[bold red]File {file_path} already exists![/bold red]")
        return False

    template = """! Hacker Lang template script
// sudo ! System dependency for privileged commands
# bit-jump ! Custom library to be installed
@USER=admin ! Set environment variable
@LOG_DIR=/var/log/hacker
=2 > echo "Hello from $USER" ! Loop twice
? [ -d /tmp ] > echo "/tmp exists" ! Conditional check
& sleep 10 ! Run command in background
# util ! Include util library
> sudo apt update ! Update system packages
[
Configuration
Author: Hacker Lang User
Purpose: System update automation
]
"""
    try:
        with open(file_path, 'w') as f:
            f.write(template)
        console.print(Panel(f"Created template file {file_path}", title="Hacker Lang Init", style="bold green"))
        if verbose:
            console.print(Syntax(template, "hacker", theme="monokai", line_numbers=True))
        return True
    except Exception as e:
        console.print(f"[bold red]Failed to create {file_path}: {e}[/bold red]")
        return False

def clean_command(verbose=False):
    console.print(Panel("Cleaning temporary files", title="Hacker Lang Clean", style="bold yellow"))
    temp_dir = tempfile.gettempdir()
    count = 0
    for f in os.listdir(temp_dir):
        if f.endswith('.sh') and f.startswith('tmp'):
            try:
                os.unlink(os.path.join(temp_dir, f))
                count += 1
                if verbose:
                    console.print(f"[yellow]Removed {f}[/yellow]")
            except:
                pass
    console.print(f"[bold green]Cleaned {count} temporary files[/bold green]")
    return True

def install_command(libname, verbose=False):
    bin_path = os.path.join(BIN_DIR, "hacker-library")
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-library not found in ~/.hacker-lang/bin/.[/bold red]")
        return False

    cmd = ['node', bin_path, 'install', libname]
    if verbose:
        console.print(f"[blue]Running: {' '.join(cmd)}[/blue]")
    try:
        subprocess.check_call(cmd)
        console.print(f"[bold green]Installed {libname}[/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Install failed with code {e.returncode}[/bold red]")
        return False

def update_command(verbose=False):
    bin_path = os.path.join(BIN_DIR, "hacker-library")
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-library not found in ~/.hacker-lang/bin/.[/bold red]")
        return False

    cmd = ['node', bin_path, 'update']
    if verbose:
        console.print(f"[blue]Running: {' '.join(cmd)}[/blue]")
    try:
        subprocess.check_call(cmd)
        console.print("[bold green]Update check completed![/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Update failed with code {e.returncode}[/bold red]")
        return False

def version_command():
    console.print(Panel(f"Hacker Lang CLI version {VERSION}", title="Version", style="bold blue"))

def help_command(show_banner=True):
    if show_banner:
        console.print(Panel("Hacker Lang CLI - Simple scripting for Debian-based Linux", title="Welcome", style="bold magenta"))

    table = Table(title="Available Commands", style="bold cyan")
    table.add_column("Command", style="cyan")
    table.add_column("Description", style="green")
    table.add_column("Arguments", style="yellow")

    commands = [
        ("run", "Run a .hacker file via .sh", "file"),
        ("compile", "Compile to native binary", "file [-o output]"),
        ("check", "Check syntax", "file"),
        ("init", "Create template .hacker file", "file"),
        ("clean", "Remove temp .sh files", ""),
        ("install", "Install a custom library", "libname"),
        ("update", "Check for library updates", ""),
        ("repl", "Start interactive REPL", ""),
        ("version", "Show CLI version", ""),
        ("help", "Show this help", "")
    ]

    for cmd, desc, args in commands:
        table.add_row(cmd, desc, args)

    console.print(table)
    console.print("\nSyntax Example:")
    console.print(Syntax(
        """// sudo
# bit-jump
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
# util
> sudo apt update
[
Config
]""",
        "hacker",
        theme="monokai",
        line_numbers=True
    ))

def main():
    ensure_hacker_dir()
    parser = argparse.ArgumentParser(
        description="Hacker Lang CLI - Enhanced scripting for Debian-based Linux",
        formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument('--verbose', action='store_true', help='Enable verbose output')
    subparsers = parser.add_subparsers(dest='command')

    run_parser = subparsers.add_parser('run', help='Run .hacker file')
    run_parser.add_argument('file', help='Path to .hacker file')
    run_parser.set_defaults(func=run_command)

    compile_parser = subparsers.add_parser('compile', help='Compile to binary')
    compile_parser.add_argument('file', help='Path to .hacker file')
    compile_parser.add_argument('-o', '--output', help='Output binary path')
    compile_parser.set_defaults(func=compile_command)

    check_parser = subparsers.add_parser('check', help='Check syntax')
    check_parser.add_argument('file', help='Path to .hacker file')
    check_parser.set_defaults(func=check_command)

    init_parser = subparsers.add_parser('init', help='Create template')
    init_parser.add_argument('file', help='Path to .hacker file')
    init_parser.set_defaults(func=init_command)

    clean_parser = subparsers.add_parser('clean', help='Clean temp files')
    clean_parser.set_defaults(func=clean_command)

    install_parser = subparsers.add_parser('install', help='Install library')
    install_parser.add_argument('libname', help='Library name')
    install_parser.set_defaults(func=install_command)

    update_parser = subparsers.add_parser('update', help='Update libraries')
    update_parser.set_defaults(func=update_command)

    repl_parser = subparsers.add_parser('repl', help='Start REPL')
    repl_parser.set_defaults(func=lambda v: run_repl(console, verbose=v))

    version_parser = subparsers.add_parser('version', help='Show version')
    version_parser.set_defaults(func=version_command)

    help_parser = subparsers.add_parser('help', help='Show help')
    help_parser.set_defaults(func=help_command)

    args = parser.parse_args()
    if not args.command:
        display_welcome()
        sys.exit(0)

    success = True
    if args.command in ['run', 'compile', 'check', 'init']:
        success = args.func(args.file, args.output if 'output' in args else None, args.verbose)
    elif args.command == 'install':
        success = args.func(args.libname, args.verbose)
    elif args.command in ['clean', 'update', 'version', 'help', 'repl']:
        success = args.func(args.verbose)

    sys.exit(0 if success else 1)

if __name__ == '__main__':
    main()
