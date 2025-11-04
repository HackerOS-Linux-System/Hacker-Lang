import os
import subprocess
import tempfile
import readline  # For history
from rich.console import Console
from rich.prompt import Prompt
from rich.panel import Panel
from hacker_parser import parse_hacker_file

HISTORY_FILE = os.path.expanduser("~/.hacker_repl_history")

def load_history():
    if os.path.exists(HISTORY_FILE):
        with open(HISTORY_FILE, 'r') as f:
            for line in f:
                readline.add_history(line.strip())

def save_history(line):
    with open(HISTORY_FILE, 'a') as f:
        f.write(line + '\n')

def run_repl(verbose=False):
    console = Console()
    console.print(Panel("Hacker Lang REPL v0.1 - Enhanced Interactive Mode\nType 'exit' to quit, 'help' for commands, 'clear' to reset", title="REPL Welcome", border_style="bold magenta"))
    console.print("Supported: //deps, #libs, @vars, =loops, ?ifs, &bg, >cmds, [config], !comments")
    load_history()
    lines = []
    in_config = False
    while True:
        try:
            prompt = "CONFIG> " if in_config else "hacker> "
            line = Prompt.ask(prompt, console=console).strip()
            if not line:
                continue
            save_history(line)
            if line.lower() == 'exit':
                break
            elif line.lower() == 'help':
                console.print(Panel("REPL Commands:\n- exit: Quit REPL\n- help: This menu\n- clear: Reset session\n- verbose: Toggle verbose", border_style="bold cyan"))
                continue
            elif line.lower() == 'clear':
                lines = []
                in_config = False
                console.print("[bold yellow]Session cleared![/bold yellow]")
                continue
            elif line.lower() == 'verbose':
                verbose = not verbose
                console.print(f"[bold blue]Verbose mode: {'ON' if verbose else 'OFF'}[/bold blue]")
                continue
            if line == '[':
                in_config = True
                lines.append(line)
                continue
            if line == ']':
                if not in_config:
                    console.print("[bold red]Error: Unmatched ']'[/bold red]")
                    continue
                in_config = False
                lines.append(line)
                continue
            lines.append(line)
            if not in_config and line and not line.startswith('!'):
                with tempfile.NamedTemporaryFile(mode='w+', suffix='.hacker', delete=False) as f:
                    f.write('\n'.join(lines) + '\n')
                    temp_path = f.name
                deps, libs, vars_dict, cmds, includes, binaries, errors, config = parse_hacker_file(temp_path, verbose, console)
                os.unlink(temp_path)
                if errors:
                    console.print(Panel("\n".join(errors), title="REPL Errors", border_style="bold red"))
                    continue
                with tempfile.NamedTemporaryFile(mode='w+', suffix='.sh', delete=False) as f:
                    f.write('#!/bin/bash\nset -e\n')
                    for k, v in vars_dict.items():
                        f.write(f'export {k}="{v}"\n')
                    for dep in deps:
                        if dep != "sudo":
                            f.write(f"command -v {dep} || (sudo apt update && sudo apt install -y {dep})\n")
                    for inc in includes:
                        lib_path = os.path.join(HACKER_DIR, "libs", inc, "main.hacker")
                        if os.path.exists(lib_path):
                            f.write(f"# include {inc}\n")
                            with open(lib_path) as lf:
                                f.write(lf.read() + "\n")
                    for cmd in cmds:
                        f.write(cmd + "\n")
                    for bin_path in binaries:
                        f.write(f"{bin_path}\n")
                    sh_path = f.name
                os.chmod(sh_path, 0o755)
                try:
                    env = os.environ.copy()
                    env.update(vars_dict)
                    output = subprocess.check_output(['bash', sh_path], env=env, text=True, stderr=subprocess.STDOUT)
                    if output.strip():
                        console.print(Panel(output.strip(), title="REPL Output", border_style="bold green"))
                except subprocess.CalledProcessError as e:
                    console.print(Panel(e.output.strip(), title="REPL Error", border_style="bold red"))
                finally:
                    os.unlink(sh_path)
        except KeyboardInterrupt:
            console.print("\n[bold yellow]Interrupt detected. Use 'exit' to quit.[/bold yellow]")
    console.print("[bold green]REPL session ended.[/bold green]")
    return True
