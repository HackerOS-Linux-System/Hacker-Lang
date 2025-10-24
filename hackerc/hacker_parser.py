import os
from rich.console import Console

HACKER_DIR = os.path.expanduser("~/.hackeros/hacker-lang")  # Updated path

def parse_hacker_file(file_path, verbose=False, console=None):
    if console is None:
        console = Console()

    deps = set()
    libs = set()
    vars_dict = {}
    cmds = []
    includes = []
    errors = []
    in_config = False
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
                    continue
                elif line == ']':
                    if not in_config:
                        errors.append(f"Line {line_num}: Closing ] without [")
                    in_config = False
                    continue

                if in_config:
                    continue

                if line.startswith('//'):
                    dep = line[2:].strip()
                    if dep:
                        deps.add(dep)
                    else:
                        errors.append(f"Line {line_num}: Empty system dependency")
                elif line.startswith('#'):
                    lib = line[1:].strip()
                    if lib:
                        lib_path = os.path.join(HACKER_DIR, "libs", lib, "main.hacker")
                        if os.path.exists(lib_path):
                            includes.append(lib)
                            sub_deps, sub_libs, sub_vars, sub_cmds, sub_includes, sub_errors = parse_hacker_file(lib_path, verbose, console)
                            deps.update(sub_deps)
                            libs.update(sub_libs)
                            vars_dict.update(sub_vars)
                            cmds.extend(sub_cmds)
                            includes.extend(sub_includes)
                            for err in sub_errors:
                                errors.append(f"In {lib}: {err}")
                        else:
                            libs.add(lib)
                    else:
                        errors.append(f"Line {line_num}: Empty library/include")
                elif line.startswith('>'):
                    cmd = line[1:].split('!', 1)[0].strip()
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
                            vars_dict[var] = value
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
                                cmds.extend([cmd] * num)
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
                elif line.startswith('&'):
                    cmd = line[1:].split('!', 1)[0].strip()
                    if cmd:
                        cmds.append(f"{cmd} &")
                    else:
                        errors.append(f"Line {line_num}: Empty background command")
                elif line.startswith('!'):
                    pass
                else:
                    errors.append(f"Line {line_num}: Invalid syntax")

        if in_config:
            errors.append("Unclosed config section")

        if verbose:
            console.print(f"[blue]System Deps: {deps}[/blue]")
            console.print(f"[blue]Custom Libs: {libs}[/blue]")
            console.print(f"[blue]Vars: {vars_dict}[/blue]")
            console.print(f"[blue]Cmds: {cmds}[/blue]")
            console.print(f"[blue]Includes: {includes}[/blue]")
            if errors:
                console.print(f"[yellow]Errors: {errors}[/yellow]")

        return deps, libs, vars_dict, cmds, includes, errors

    except FileNotFoundError:
        console.print(f"[bold red]File {file_path} not found[/bold red]")
        return set(), set(), {}, [], [], [f"File {file_path} not found"]
