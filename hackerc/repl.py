import os
import subprocess
import tempfile
from rich.console import Console
from rich.prompt import Prompt
from parser import parse_hacker_file

def run_repl(console, verbose=False):
    console.print(Panel("Hacker Lang REPL - Type 'exit' to quit", title="REPL", style="bold magenta"))
    console.print("Supports: // (system deps), # (libs/include), @ (vars), = (loops), ? (conditionals), & (background), > (cmds), [ ... ], ! (comments)")

    lines = []
    in_config = False

    while True:
        try:
            if in_config:
                prompt = "CONFIG> "
            else:
                prompt = "> "
            line = Prompt.ask(prompt, console=console).strip()

            if line.lower() == 'exit':
                break

            if line == '[':
                in_config = True
                lines.append(line)
                continue
            elif line == ']':
                if not in_config:
                    console.print("[bold red]Error: Closing ] without opening [[/bold red]")
                    continue
                in_config = False
                lines.append(line)
                continue

            lines.append(line)

            if not in_config and line and not line.startswith('!'):
                with tempfile.NamedTemporaryFile(mode='w+', suffix='.hacker', delete=False) as temp_hacker:
                    temp_hacker.write('\n'.join(lines) + '\n')
                    temp_hacker_path = temp_hacker.name

                deps, libs, vars, cmds, includes, errors = parse_hacker_file(temp_hacker_path, verbose, console)
                os.unlink(temp_hacker_path)

                if errors:
                    console.print(Panel("\n".join(errors), title="REPL Errors", style="bold red"))
                    continue

                with tempfile.NamedTemporaryFile(mode='w+', suffix='.sh', delete=False) as temp_sh:
                    temp_sh.write('#!/bin/bash\n')
                    temp_sh.write('set -e\n')

                    for var, value in vars.items():
                        temp_sh.write(f'export {var}="{value}"\n')

                    for dep in deps:
                        if dep != "sudo":
                            temp_sh.write(f"command -v {dep} &> /dev/null || (sudo apt update && sudo apt install -y {dep})\n")

                    for include in includes:
                        lib_path = os.path.join(os.path.expanduser("~/.hacker-lang"), "libs", include, "main.hacker")
                        if os.path.exists(lib_path):
                            temp_sh.write(f"# Included from {include}\n")
                            with open(lib_path, 'r') as lib_file:
                                temp_sh.write(lib_file.read() + "\n")

                    for cmd in cmds:
                        temp_sh.write(f"{cmd}\n")

                    temp_sh_path = temp_sh.name

                os.chmod(temp_sh_path, 0o755)

                try:
                    env = os.environ.copy()
                    env.update(vars)
                    output = subprocess.check_output(['bash', temp_sh_path], env=env, text=True, stderr=subprocess.STDOUT)
                    if output:
                        console.print(Panel(output.strip(), title="Output", style="bold green"))
                except subprocess.CalledProcessError as e:
                    console.print(Panel(e.output.strip(), title="Error", style="bold red"))
                finally:
                    os.unlink(temp_sh_path)

        except KeyboardInterrupt:
            console.print("\n[bold yellow]Use 'exit' to quit REPL[/bold yellow]")

    console.print("[bold green]REPL exited[/bold green]")
    return True
