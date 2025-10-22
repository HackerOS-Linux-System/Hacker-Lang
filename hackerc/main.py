# hackerc.py - Updated Main CLI tool for Hacker Lang, written in Python with rich for pretty output.
# Updated syntax: Dependencies with //, Config sections with [ ... ] (ignored, treated as multi-line comments).
# Commands with >, Comments with ! (full line or inline after !).
# Added more optional features: 
# - CLI now has 'lib' subcommand to manage libraries via hacker-library.js.
# - Added '--verbose' flag for more detailed output.
# - Added 'version' subcommand.
# - In parser, added support for optional variables: @var=value, which are set as env vars in the shell script.
# - Added simple loops: *num > cmd (repeat cmd num times).
# Place this in ~/.hackeros/hacker-lang/bin/hackerc and make it executable.
# Ensure hacker-compiler and hacker-library.js are in the same directory.

import argparse
import os
import subprocess
import tempfile
from rich.console import Console
from rich.panel import Panel
from rich.progress import Progress

console = Console()
VERSION = "0.2.0"

def parse_hacker_file(file_path, verbose=False):
    deps = set()
    cmds = []
    vars = {}  # For @var=value
    in_config = False
    config_lines = []  # Collect config but ignore
    with open(file_path, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            
            if line == '[':
                in_config = True
                config_lines = []
                continue
            elif line == ']':
                in_config = False
                # Ignore config_lines
                continue
            
            if in_config:
                config_lines.append(line)
                continue
            
            if line.startswith('//'):
                dep = line[2:].strip()
                if dep:
                    deps.add(dep)
            elif line.startswith('>'):
                parts = line[1:].split('!', 1)
                cmd = parts[0].strip()
                if cmd:
                    cmds.append(cmd)
            elif line.startswith('@'):
                # Variable assignment: @var=value
                if '=' in line:
                    var, value = line[1:].split('=', 1)
                    vars[var.strip()] = value.strip()
            elif line.startswith('*'):
                # Loop: *num > cmd
                parts = line[1:].split('>', 1)
                if len(parts) == 2:
                    try:
                        num = int(parts[0].strip())
                        cmd = parts[1].split('!', 1)[0].strip()
                        for _ in range(num):
                            cmds.append(cmd)
                    except ValueError:
                        if verbose:
                            console.print(f"[yellow]Invalid loop count in: {line}[/yellow]")
            elif line.startswith('!'):
                # Full-line comment, ignore
                pass
    if verbose:
        console.print(f"[blue]Parsed deps: {deps}[/blue]")
        console.print(f"[blue]Parsed vars: {vars}[/blue]")
        console.print(f"[blue]Parsed cmds: {cmds}[/blue]")
    return deps, vars, cmds

def generate_check_cmd(dep):
    if dep == 'sudo':
        return '' 
    return f"command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})"

def run_command(file_path, verbose=False):
    deps, vars, cmds = parse_hacker_file(file_path, verbose)
    
    with Progress() as progress:
        total = len(deps) + len(vars) + len(cmds)
        task = progress.add_task("[green]Preparing script...", total=total)
        
        with tempfile.NamedTemporaryFile(mode='w+', suffix='.sh', delete=False) as temp_sh:
            temp_sh.write('#!/bin/bash\n')
            temp_sh.write('set -e\n')  # Exit on error
            
            # Set variables as export
            for var, value in vars.items():
                temp_sh.write(f'export {var}="{value}"\n')
                progress.update(task, advance=1)
            
            # Dependency checks
            for dep in deps:
                check_cmd = generate_check_cmd(dep)
                if check_cmd:
                    temp_sh.write(f"{check_cmd}\n")
                progress.update(task, advance=1)
            
            # Commands
            for cmd in cmds:
                temp_sh.write(f"{cmd}\n")
                progress.update(task, advance=1)
            
            temp_sh_path = temp_sh.name
    
    os.chmod(temp_sh_path, 0o755)
    
    console.print(Panel(f"Running script from {file_path}", title="Hacker Lang Run", style="bold green"))
    try:
        env = os.environ.copy()
        env.update(vars)  # Also set in current env, but mainly in script
        subprocess.check_call(['bash', temp_sh_path], env=env)
        console.print("[bold green]Execution successful![/bold green]")
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Execution failed with code {e.returncode}[/bold red]")
    finally:
        os.unlink(temp_sh_path)

def compile_command(file_path, output, verbose=False):
    if not output:
        output = os.path.splitext(file_path)[0]
    
    bin_path = os.path.join(os.path.dirname(__file__), 'hacker-compiler')
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-compiler not found.[/bold red]")
        return
    
    console.print(Panel(f"Compiling {file_path} to {output}", title="Hacker Lang Compile", style="bold blue"))
    cmd = [bin_path, file_path, output]
    if verbose:
        cmd.append('--verbose')
    try:
        subprocess.check_call(cmd)
        console.print("[bold green]Compilation successful![/bold green]")
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Compilation failed with code {e.returncode}[/bold red]")

def lib_command(args, verbose=False):
    bin_path = os.path.join(os.path.dirname(__file__), 'hacker-library.js')
    if not os.path.exists(bin_path):
        console.print("[bold red]hacker-library.js not found.[/bold red]")
        return
    
    cmd = ['node', bin_path] + args
    if verbose:
        console.print(f"[blue]Running: {' '.join(cmd)}[/blue]")
    try:
        subprocess.check_call(cmd)
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Library command failed with code {e.returncode}[/bold red]")

def version_command():
    console.print(f"Hacker Lang CLI version {VERSION}")

def main():
    parser = argparse.ArgumentParser(description="Hacker Lang CLI - Simple scripting for Debian-based Linux.")
    parser.add_argument('--verbose', action='store_true', help='Enable verbose output')
    subparsers = parser.add_subparsers(dest='command', required=True)
    
    run_parser = subparsers.add_parser('run', help='Run .hacker file via .sh')
    run_parser.add_argument('file', help='Path to .hacker file')
    run_parser.set_defaults(func=run_command)
    
    compile_parser = subparsers.add_parser('compile', help='Compile .hacker to native binary')
    compile_parser.add_argument('file', help='Path to .hacker file')
    compile_parser.add_argument('-o', '--output', help='Output binary path')
    compile_parser.set_defaults(func=compile_command)
    
    lib_parser = subparsers.add_parser('lib', help='Manage libraries via hacker-library')
    lib_parser.add_argument('action', choices=['list', 'install'], help='Action: list or install')
    lib_parser.add_argument('libname', nargs='?', help='Library name for install')
    lib_parser.set_defaults(func=lib_command)
    
    version_parser = subparsers.add_parser('version', help='Show version')
    version_parser.set_defaults(func=version_command)
    
    args = parser.parse_args()
    if args.command in ['run', 'compile']:
        args.func(args.file, args.output if 'output' in args else None, args.verbose)
    elif args.command == 'lib':
        lib_args = [args.action]
        if args.libname:
            lib_args.append(args.libname)
        args.func(lib_args, args.verbose)
    elif args.command == 'version':
        args.func()

if __name__ == '__main__':
    main()
