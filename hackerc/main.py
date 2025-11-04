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
from rich.progress import Progress, SpinnerColumn, TextColumn
from hacker_parser import parse_hacker_file
from repl import run_repl

console = Console()
VERSION = "0.0.4"  # Updated version
HACKER_DIR = os.path.expanduser("~/.hackeros/hacker-lang")
BIN_DIR = os.path.join(HACKER_DIR, "bin")

def ensure_hacker_dir():
    os.makedirs(BIN_DIR, exist_ok=True)
    os.makedirs(os.path.join(HACKER_DIR, "libs"), exist_ok=True)

def display_welcome():
    banner = Text("Hacker Lang CLI", style="bold magenta underline")
    banner.append("\nAdvanced scripting for Debian-based Linux systems", style="italic cyan")
    banner.append(f"\nVersion {VERSION}", style="bold blue")
    console.print(Panel(banner, expand=False, border_style="bold green"))
    help_command(show_banner=False)

def run_command(file_path, verbose=False):
    deps, libs, vars_dict, cmds, includes, binaries, errors, config = parse_hacker_file(file_path, verbose)
    if errors:
        console.print(Panel("\n".join(errors), title="Syntax Errors", border_style="bold red"))
        return False
    # Install missing libs
    for lib in libs:
        install_command(lib, verbose)
    # Temp shell script
    with tempfile.NamedTemporaryFile(mode='w+', suffix='.sh', delete=False) as temp_sh:
        temp_sh.write('#!/bin/bash\n')
        temp_sh.write('set -e\n')
        for var, value in vars_dict.items():
            temp_sh.write(f'export {var}="{value}"\n')
        for dep in deps:
            if dep != "sudo":
                temp_sh.write(f"command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})\n")
        for include in includes:
            lib_path = os.path.join(HACKER_DIR, "libs", include, "main.hacker")
            if os.path.exists(lib_path):
                temp_sh.write(f"# Included from {include}\n")
                with open(lib_path, 'r') as lib_file:
                    temp_sh.write(lib_file.read() + "\n")
        for cmd in cmds:
            temp_sh.write(f"{cmd}\n")
        for bin_path in binaries:
            temp_sh.write(f"{bin_path}\n")  # Call binary libs
        temp_sh_path = temp_sh.name
    os.chmod(temp_sh_path, 0o755)
    console.print(Panel(f"Executing script: {file_path}\nConfig: {config}", title="Run Mode", border_style="bold green"))
    try:
        env = os.environ.copy()
        env.update(vars_dict)
        with Progress(SpinnerColumn(), TextColumn("[progress.description]{task.description}"), transient=True) as progress:
            task = progress.add_task(description="Running...", total=None)
            subprocess.check_call(['bash', temp_sh_path], env=env)
        console.print("[bold green]Execution completed successfully![/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Execution failed (code: {e.returncode})[/bold red]")
        return False
    finally:
        os.unlink(temp_sh_path)

def compile_command(file_path, output=None, verbose=False):
    if not output:
        output = os.path.splitext(file_path)[0]
    bin_path = os.path.join(BIN_DIR, "hacker-compiler")
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-compiler not found in ~/.hackeros/hacker-lang/bin/.[/bold red]")
        return False
    console.print(Panel(f"Compiling {file_path} to {output}", title="Compile Mode", border_style="bold blue"))
    cmd = [bin_path, file_path, output]
    if verbose:
        cmd.append('--verbose')
    try:
        with Progress(SpinnerColumn(), TextColumn("[progress.description]{task.description}"), transient=True) as progress:
            task = progress.add_task(description="Compiling...", total=None)
            subprocess.check_call(cmd)
        console.print("[bold green]Compilation successful![/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Compilation failed (code: {e.returncode})[/bold red]")
        return False

def check_command(file_path, verbose=False):
    console.print(Panel(f"Validating syntax of {file_path}", title="Check Mode", border_style="bold cyan"))
    _, _, _, _, _, _, errors, _ = parse_hacker_file(file_path, verbose)
    if errors:
        console.print(Panel("\n".join(errors), title="Syntax Errors", border_style="bold red"))
        return False
    console.print("[bold green]Syntax validation passed![/bold green]")
    return True

def init_command(file_path, verbose=False):
    if os.path.exists(file_path):
        console.print(f"[bold red]File {file_path} already exists![/bold red]")
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
        with open(file_path, 'w') as f:
            f.write(template)
        console.print(Panel(f"Initialized template at {file_path}", title="Init Mode", border_style="bold green"))
        if verbose:
            console.print(Syntax(template, "hacker", theme="dracula", line_numbers=True))
        return True
    except Exception as e:
        console.print(f"[bold red]Initialization failed: {e}[/bold red]")
        return False

def clean_command(verbose=False):
    console.print(Panel("Cleaning up temporary files", title="Clean Mode", border_style="bold yellow"))
    temp_dir = tempfile.gettempdir()
    count = 0
    with Progress(SpinnerColumn(), TextColumn("[progress.description]{task.description}"), transient=True) as progress:
        task = progress.add_task(description="Scanning temps...", total=None)
        for f in os.listdir(temp_dir):
            if f.endswith('.sh') and f.startswith('tmp'):
                try:
                    os.unlink(os.path.join(temp_dir, f))
                    count += 1
                    if verbose:
                        console.print(f"[yellow]Removed: {f}[/yellow]")
                except:
                    pass
    console.print(f"[bold green]Removed {count} temporary files[/bold green]")
    return True

def install_command(libname, verbose=False):
    bin_path = os.path.join(BIN_DIR, "hacker-library")
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-library not found.[/bold red]")
        return False
    cmd = ['node', bin_path, 'install', libname]
    console.print(Panel(f"Installing library: {libname}", title="Install Mode", border_style="bold magenta"))
    if verbose:
        console.print(f"[blue]Executing: {' '.join(cmd)}[/blue]")
    try:
        with Progress(SpinnerColumn(), TextColumn("[progress.description]{task.description}"), transient=True) as progress:
            task = progress.add_task(description="Installing...", total=None)
            subprocess.check_call(cmd)
        console.print(f"[bold green]Library {libname} installed![/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Installation failed (code: {e.returncode})[/bold red]")
        return False

def update_command(verbose=False):
    bin_path = os.path.join(BIN_DIR, "hacker-library")
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-library not found.[/bold red]")
        return False
    cmd = ['node', bin_path, 'update']
    console.print(Panel("Updating libraries", title="Update Mode", border_style="bold blue"))
    if verbose:
        console.print(f"[blue]Executing: {' '.join(cmd)}[/blue]")
    try:
        with Progress(SpinnerColumn(), TextColumn("[progress.description]{task.description}"), transient=True) as progress:
            task = progress.add_task(description="Updating...", total=None)
            subprocess.check_call(cmd)
        console.print("[bold green]Libraries updated![/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Update failed (code: {e.returncode})[/bold red]")
        return False

def version_command():
    console.print(Panel(f"Hacker Lang v{VERSION}", title="Version Info", border_style="bold blue"))

def help_command(show_banner=True):
    if show_banner:
        console.print(Panel("Hacker Lang CLI - Advanced Scripting Tool", title="Welcome", border_style="bold magenta"))
    table = Table(title="Commands Overview", border_style="bold cyan", show_header=True, header_style="bold yellow")
    table.add_column("Command", style="cyan bold")
    table.add_column("Description", style="green")
    table.add_column("Arguments", style="yellow")
    commands = [
        ("run", "Execute a .hacker script", "file [--verbose]"),
        ("compile", "Compile to native executable", "file [-o output] [--verbose]"),
        ("check", "Validate syntax", "file [--verbose]"),
        ("init", "Generate template script", "file [--verbose]"),
        ("clean", "Remove temporary files", "[--verbose]"),
        ("install", "Install custom library", "libname [--verbose]"),
        ("update", "Update installed libraries", "[--verbose]"),
        ("repl", "Launch interactive REPL", "[--verbose]"),
        ("version", "Display version", ""),
        ("help", "Show this help menu", "")
    ]
    for cmd, desc, args in commands:
        table.add_row(cmd, desc, args)
    console.print(table)
    console.print("\nSyntax Highlight Example:")
    console.print(Syntax(
        """// sudo
# network-utils
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
# logging
> sudo apt update
[
Config=Example
]""",
        "hacker",
        theme="dracula",
        line_numbers=True,
        background_color="default"
    ))

def main():
    ensure_hacker_dir()
    parser = argparse.ArgumentParser(
        description="Hacker Lang CLI - Enhanced scripting for Debian-based Linux",
        formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument('--verbose', action='store_true', help='Enable verbose output')
    subparsers = parser.add_subparsers(dest='command')
    # run
    run_parser = subparsers.add_parser('run', help='Run .hacker file')
    run_parser.add_argument('file', help='Path to .hacker file')
    run_parser.set_defaults(func=run_command)
    # compile
    compile_parser = subparsers.add_parser('compile', help='Compile to binary')
    compile_parser.add_argument('file', help='Path to .hacker file')
    compile_parser.add_argument('-o', '--output', help='Output binary path')
    compile_parser.set_defaults(func=compile_command)
    # check
    check_parser = subparsers.add_parser('check', help='Check syntax')
    check_parser.add_argument('file', help='Path to .hacker file')
    check_parser.set_defaults(func=check_command)
    # init
    init_parser = subparsers.add_parser('init', help='Create template')
    init_parser.add_argument('file', help='Path to .hacker file')
    init_parser.set_defaults(func=init_command)
    # clean
    clean_parser = subparsers.add_parser('clean', help='Clean temp files')
    clean_parser.set_defaults(func=clean_command)
    # install
    install_parser = subparsers.add_parser('install', help='Install library')
    install_parser.add_argument('libname', help='Library name')
    install_parser.set_defaults(func=install_command)
    # update
    update_parser = subparsers.add_parser('update', help='Update libraries')
    update_parser.set_defaults(func=update_command)
    # repl
    repl_parser = subparsers.add_parser('repl', help='Start REPL')
    repl_parser.set_defaults(func=run_repl)
    # version
    version_parser = subparsers.add_parser('version', help='Show version')
    version_parser.set_defaults(func=version_command)
    # help
    help_parser = subparsers.add_parser('help', help='Show help')
    help_parser.set_defaults(func=help_command)
    args = parser.parse_args()
    if not args.command:
        display_welcome()
        sys.exit(0)
    verbose = args.verbose
    try:
        if args.command == 'compile':
            success = args.func(args.file, getattr(args, 'output', None), verbose)
        elif args.command in ['run', 'check', 'init']:
            success = args.func(args.file, verbose)
        elif args.command == 'install':
            success = args.func(args.libname, verbose)
        elif args.command in ['clean', 'update', 'repl']:
            success = args.func(verbose)
        else:
            success = args.func()
    except Exception as e:
        console.print(f"[bold red]Critical Error: {e}[/bold red]")
        success = False
    sys.exit(0 if success else 1)

if __name__ == '__main__':
    main()
