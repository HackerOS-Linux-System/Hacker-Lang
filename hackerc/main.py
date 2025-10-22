# hackerc.py - Enhanced CLI for Hacker Lang with rich, expanded commands, and prettier interface.
# Syntax: Dependencies with //, Config with [ ... ], Commands with >, Comments with !, Variables with @var=value, Loops with *num > cmd.
# New CLI commands: compile, run, help, check (validates syntax), init (creates template .hacker file), clean (removes temp files).
# Enhanced UI with rich: tables for help, syntax highlighting, better progress bars, and error handling.
# Place in ~/.hackeros/hacker-lang/bin/hackerc and make executable.
# Ensure hacker-compiler and hacker-library.js are in the same directory.

import argparse
import os
import subprocess
import tempfile
import re
from rich.console import Console
from rich.panel import Panel
from rich.progress import Progress, SpinnerColumn, BarColumn, TextColumn
from rich.table import Table
from rich.syntax import Syntax
from rich.text import Text

console = Console()
VERSION = "0.3.0"

def parse_hacker_file(file_path, verbose=False):
    deps = set()
    vars = {}
    cmds = []
    in_config = False
    config_lines = []
    errors = []
    line_num = 0
    with open(file_path, 'r') as f:
        for line in f:
            line_num += 1
            line = line.strip()
            if not line:
                continue
            
            if line == '[':
                if in_config:
                    errors.append(f"Line {line_num}: Nested config section detected")
                in_config = True
                config_lines = []
                continue
            elif line == ']':
                if not in_config:
                    errors.append(f"Line {line_num}: Closing ] without opening [")
                in_config = False
                continue
            
            if in_config:
                config_lines.append(line)
                continue
            
            if line.startswith('//'):
                dep = line[2:].strip()
                if dep:
                    deps.add(dep)
                else:
                    errors.append(f"Line {line_num}: Empty dependency")
            elif line.startswith('>'):
                parts = line[1:].split('!', 1)
                cmd = parts[0].strip()
                if cmd:
                    cmds.append(cmd)
                else:
                    errors.append(f"Line {line_num}: Empty command")
            elif line.startswith('@'):
                if '=' in line:
                    var, value = line[1:].split('=', 1)
                    var = var.strip()
                    value = value.strip()
                    if var and value:
                        vars[var] = value
                    else:
                        errors.append(f"Line {line_num}: Invalid variable assignment")
                else:
                    errors.append(f"Line {line_num}: Missing = in variable")
            elif line.startswith('*'):
                parts = line[1:].split('>', 1)
                if len(parts) == 2:
                    try:
                        num = int(parts[0].strip())
                        if num < 0:
                            errors.append(f"Line {line_num}: Negative loop count")
                            continue
                        cmd = parts[1].split('!', 1)[0].strip()
                        if cmd:
                            for _ in range(num):
                                cmds.append(cmd)
                        else:
                            errors.append(f"Line {line_num}: Empty loop command")
                    except ValueError:
                        errors.append(f"Line {line_num}: Invalid loop count")
                else:
                    errors.append(f"Line {line_num}: Invalid loop syntax")
            elif line.startswith('!'):
                pass
            else:
                errors.append(f"Line {line_num}: Invalid syntax")
    
    if in_config:
        errors.append("File ended with unclosed config section")
    
    if verbose:
        console.print(f"[blue]Deps: {deps}[/blue]")
        console.print(f"[blue]Vars: {vars}[/blue]")
        console.print(f"[blue]Cmds: {cmds}[/blue]")
        if errors:
            console.print(f"[yellow]Warnings: {errors}[/yellow]")
    
    return deps, vars, cmds, errors

def generate_check_cmd(dep):
    if dep == 'sudo':
        return ''
    return f"command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})"

def run_command(file_path, verbose=False):
    deps, vars, cmds, errors = parse_hacker_file(file_path, verbose)
    if errors:
        console.print(Panel("\n".join(errors), title="Syntax Errors", style="bold red"))
        return False
    
    with Progress(SpinnerColumn(), "[progress.description]{task.description}", BarColumn(), transient=True) as progress:
        total = len(deps) + len(vars) + len(cmds)
        task = progress.add_task("[green]Preparing script...", total=total)
        
        with tempfile.NamedTemporaryFile(mode='w+', suffix='.sh', delete=False) as temp_sh:
            temp_sh.write('#!/bin/bash\n')
            temp_sh.write('set -e\n')
            
            for var, value in vars.items():
                temp_sh.write(f'export {var}="{value}"\n')
                progress.update(task, advance=1)
            
            for dep in deps:
                check_cmd = generate_check_cmd(dep)
                if check_cmd:
                    temp_sh.write(f"{check_cmd}\n")
                progress.update(task, advance=1)
            
            for cmd in cmds:
                temp_sh.write(f"{cmd}\n")
                progress.update(task, advance=1)
            
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
    deps, vars, cmds, errors = parse_hacker_file(file_path, verbose)
    if errors:
        console.print(Panel("\n".join(errors), title="Syntax Errors", style="bold red"))
        return False
    
    if not output:
        output = os.path.splitext(file_path)[0]
    
    bin_path = os.path.join(os.path.dirname(__file__), 'hacker-compiler')
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-compiler not found.[/bold red]")
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
    deps, vars, cmds, errors = parse_hacker_file(file_path, verbose)
    if errors:
        console.print(Panel("\n".join(errors), title="Syntax Errors", style="bold red"))
        return False
    console.print("[bold green]Syntax check passed![/bold green]")
    return True

def init_command(file_path, verbose=False):
    if os.path.exists(file_path):
        console.print(f"[bold red]File {file_path} already exists![/bold red]")
        return False
    
    template = """// sudo
// apt
@MY_VAR=example
*2 > echo $MY_VAR ! Print variable twice
> sudo apt update ! Run update
[
Configuration section
Multiline support
]
! This is a comment
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
    with Progress(SpinnerColumn(), "[progress.description]{task.description}", transient=True) as progress:
        task = progress.add_task("[yellow]Scanning for temp files...", total=None)
        for f in os.listdir(temp_dir):
            if f.endswith('.sh') and f.startswith('tmp'):
                try:
                    os.unlink(os.path.join(temp_dir, f))
                    count += 1
                    if verbose:
                        console.print(f"[yellow]Removed {f}[/yellow]")
                except:
                    pass
        progress.update(task, completed=True)
    console.print(f"[bold green]Cleaned {count} temporary files[/bold green]")
    return True

def lib_command(args, verbose=False):
    bin_path = os.path.join(os.path.dirname(__file__), 'hacker-library.js')
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-library.js not found.[/bold red]")
        return False
    
    cmd = ['node', bin_path] + args
    if verbose:
        console.print(f"[blue]Running: {' '.join(cmd)}[/blue]")
    try:
        subprocess.check_call(cmd)
        console.print("[bold green]Library command successful![/bold green]")
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Library command failed with code {e.returncode}[/bold red]")
        return False

def help_command():
    table = Table(title="Hacker Lang CLI Commands", style="bold magenta")
    table.add_column("Command", style="cyan")
    table.add_column("Description", style="green")
    table.add_column("Arguments", style="yellow")
    
    commands = [
        ("run", "Run a .hacker file by translating to .sh", "file"),
        ("compile", "Compile a .hacker file to native binary", "file [-o output]"),
        ("check", "Check syntax of a .hacker file", "file"),
        ("init", "Create a template .hacker file", "file"),
        ("clean", "Remove temporary .sh files", ""),
        ("lib", "Manage libraries (list, install)", "action [libname]"),
        ("version", "Show CLI version", ""),
        ("help", "Show this help message", "")
    ]
    
    for cmd, desc, args in commands:
        table.add_row(cmd, desc, args)
    
    console.print(Panel(table, title="Hacker Lang Help", style="bold magenta"))
    console.print("\nSyntax Example:")
    console.print(Syntax(
        """// sudo
// apt
@MY_VAR=example
*2 > echo $MY_VAR ! Print twice
> sudo apt update ! Run update
[
Configuration
]
! Comment""",
        "hacker",
        theme="monokai",
        line_numbers=True
    ))

def version_command():
    console.print(Panel(f"Hacker Lang CLI version {VERSION}", title="Version", style="bold blue"))

def main():
    parser = argparse.ArgumentParser(
        description="Hacker Lang CLI - Enhanced scripting for Debian-based Linux",
        formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument('--verbose', action='store_true', help='Enable verbose output')
    subparsers = parser.add_subparsers(dest='command', required=True)
    
    run_parser = subparsers.add_parser('run', help='Run .hacker file via .sh')
    run_parser.add_argument('file', help='Path to .hacker file')
    run_parser.set_defaults(func=run_command)
    
    compile_parser = subparsers.add_parser('compile', help='Compile .hacker to native binary')
    compile_parser.add_argument('file', help='Path to .hacker file')
    compile_parser.add_argument('-o', '--output', help='Output binary path')
    compile_parser.set_defaults(func=compile_command)
    
    check_parser = subparsers.add_parser('check', help='Check syntax of .hacker file')
    check_parser.add_argument('file', help='Path to .hacker file')
    check_parser.set_defaults(func=check_command)
    
    init_parser = subparsers.add_parser('init', help='Create template .hacker file')
    init_parser.add_argument('file', help='Path to .hacker file')
    init_parser.set_defaults(func=init_command)
    
    clean_parser = subparsers.add_parser('clean', help='Remove temporary .sh files')
    clean_parser.set_defaults(func=clean_command)
    
    lib_parser = subparsers.add_parser('lib', help='Manage libraries')
    lib_parser.add_argument('action', choices=['list', 'install'], help='Action: list or install')
    lib_parser.add_argument('libname', nargs='?', help='Library name for install')
    lib_parser.set_defaults(func=lib_command)
    
    version_parser = subparsers.add_parser('version', help='Show version')
    version_parser.set_defaults(func=version_command)
    
    help_parser = subparsers.add_parser('help', help='Show help message')
    help_parser.set_defaults(func=help_command)
    
    args = parser.parse_args()
    success = True
    if args.command in ['run', 'compile', 'check', 'init']:
        success = args.func(args.file, args.output if 'output' in args else None, args.verbose)
    elif args.command == 'lib':
        lib_args = [args.action]
        if args.libname:
            lib_args.append(args.libname)
        success = args.func(lib_args, args.verbose)
    elif args.command in ['clean', 'version', 'help']:
        success = args.func()
    
    exit(0 if success else 1)

if __name__ == '__main__':
    main()
