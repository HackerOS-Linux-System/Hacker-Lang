# parser.py - Parser for Hacker Lang syntax.
# Handles // (deps), [ ... ] (config, ignored), > (cmds), ! (comments), @var=value (vars),
# =num > cmd (loops), ? condition > cmd (conditionals), # libname (includes).

import os
from rich.console import Console

HACKER_DIR = os.path.expanduser("~/.hacker-lang")

def generate_check_cmd(dep):
    if dep == 'sudo':
        return ''
    return f"command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})"

def parse_hacker_file(file_path, verbose=False, console=None):
    if console is None:
        console = Console()
    
    deps = set()
    vars = {}
    cmds = []
    includes = []
    errors = []
    in_config = False
    config_lines = []
    line_num = 0
    
    try:
        with open(file_path, 'r') as f:
            for line in f:
                line_num += 1
                line = line.strip()
                if not line:
                    continue
                
                if line == '[':
                    if in_config:
                        errors.append(f"Line {line_num}: Nested config section")
                    in_config = True
                    config_lines = []
                    continue
                elif line == ']':
                    if not in_config:
                        errors.append(f"Line {line_num}: Closing ] without [")
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
                            errors.append(f"Line {line_num}: Invalid variable")
                    else:
                        errors.append(f"Line {line_num}: Missing = in variable")
                elif line.startswith('='):
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
                elif line.startswith('?'):
                    parts = line[1:].split('>', 1)
                    if len(parts) == 2:
                        condition = parts[0].strip()
                        cmd = parts[1].split('!', 1)[0].strip()
                        if condition and cmd:
                            cmds.append(f"if {condition}; then {cmd}; fi")
                        else:
                            errors.append(f"Line {line_num}: Invalid conditional")
                    else:
                        errors.append(f"Line {line_num}: Invalid conditional syntax")
                elif line.startswith('#'):
                    lib = line[1:].strip()
                    if lib:
                        lib_path = os.path.join(HACKER_DIR, "libs", f"{lib}.hacker")
                        if os.path.exists(lib_path):
                            includes.append(lib)
                        else:
                            errors.append(f"Line {line_num}: Library {lib} not found")
                    else:
                        errors.append(f"Line {line_num}: Empty include")
                elif line.startswith('!'):
                    pass
                else:
                    errors.append(f"Line {line_num}: Invalid syntax")
    
        if in_config:
            errors.append("Unclosed config section")
        
        if verbose:
            console.print(f"[blue]Deps: {deps}[/blue]")
            console.print(f"[blue]Vars: {vars}[/blue]")
            console.print(f"[blue]Cmds: {cmds}[/blue]")
            console.print(f"[blue]Includes: {includes}[/blue]")
            if errors:
                console.print(f"[yellow]Errors: {errors}[/yellow]")
    
        return deps, vars, cmds, includes, errors
    
    except FileNotFoundError:
        console.print(f"[bold red]File {file_path} not found[/bold red]")
        return set(), {}, [], [], [f"File {file_path} not found"]
