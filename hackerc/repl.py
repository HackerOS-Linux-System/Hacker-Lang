import os
import subprocess
import tempfile
from rich.console import Console
from rich.prompt import Prompt
from rich.panel import Panel
from hacker_parser import parse_hacker_file  # Changed from parser to hacker_parser

def run_repl(console, verbose=False):
    console.print(Panel("Hacker Lang REPL - Type 'exit' to quit", title="REPL", style="bold magenta"))
    console.print("Supports: // (deps), # (libs), @ (vars), = (loop), ? (if), & (bg), > (cmd), [ ] (config), ! (comment)")

    lines = []
    in_config = False

    while True:
        try:
            prompt = "CONFIG> " if in_config else "> "
            line = Prompt.ask(prompt, console=console).strip()

            if line.lower() == 'exit':
                break

            if line == '[':
                in_config = True
                lines.append(line)
                continue
            if line == ']':
                if not in_config:
                    console.print("[bold red]Error: Closing ] without [[/bold red]")
                    continue
                in_config = False
                lines.append(line)
                continue

            lines.append(line)

            if not in_config and line and not line.startswith('!'):
                with tempfile.NamedTemporaryFile(mode='w+', suffix='.hacker', delete=False) as f:
                    f.write('\n'.join(lines) + '\n')
                    temp_path = f.name

                deps, libs, vars_dict, cmds, includes, errors = parse_hacker_file(temp_path, verbose, console)
                os.unlink(temp_path)

                if errors:
                    console.print(Panel("\n".join(errors), title="REPL Errors", style="bold red"))
                    continue

                with tempfile.NamedTemporaryFile(mode='w+', suffix='.sh', delete=False) as f:
                    f.write('#!/bin/bash\nset -e\n')
                    for k, v in vars_dict.items():
                        f.write(f'export {k}="{v}"\n')
                    for dep in deps:
                        if dep != "sudo":
                            f.write(f"command -v {dep} || (sudo apt update && sudo apt install -y {dep})\n")
                    for inc in includes:
                        lib_path = os.path.join(os.path.expanduser("~/.hackeros/hacker-lang"), "libs", inc, "main.hacker")  # Updated path
                        if os.path.exists(lib_path):
                            f.write(f"# include {inc}\n")
                            with open(lib_path) as lf:
                                f.write(lf.read() + "\n")
                    for cmd in cmds:
                        f.write(cmd + "\n")
                    sh_path = f.name

                os.chmod(sh_path, 0o755)
                try:
                    env = os.environ.copy()
                    env.update(vars_dict)
                    output = subprocess.check_output(['bash', sh_path], env=env, text=True, stderr=subprocess.STDOUT)
                    if output.strip():
                        console.print(Panel(output.strip(), title="Output", style="bold green"))
                except subprocess.CalledProcessError as e:
                    console.print(Panel(e.output.strip(), title="Error", style="bold red"))
                finally:
                    os.unlink(sh_path)

        except KeyboardInterrupt:
            console.print("\n[bold yellow]Use 'exit' to quit[/bold yellow]")

    console.print("[bold green]REPL exited[/bold green]")
    return True
