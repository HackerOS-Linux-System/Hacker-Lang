import os
import sys
import subprocess
from pathlib import Path

import click
from rich.console import Console
from rich.panel import Panel
from rich.text import Text
from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn
from rich.prompt import Confirm

console = Console()

@click.command(
    name="hackerc",
    help="Compiler for hacker-lang, similar to rustc."
)
@click.argument("input", type=click.Path(exists=True, file_okay=True, dir_okay=False, readable=True))
@click.argument("output", required=False, type=click.Path(file_okay=False, dir_okay=True, writable=True))
@click.option("--verbose", is_flag=True, help="Enable verbose output.")
@click.option("--bytes", "bytes_mode", is_flag=True, help="Enable bytes mode for embedding binaries.")
@click.option("--opt", is_flag=True, help="Enable optimization.")
@click.option("--confirm", is_flag=True, help="Ask for confirmation before compiling.")
@click.version_option("1.0.0", prog_name="hackerc")
def main(input: str, output: str, verbose: bool, bytes_mode: bool, opt: bool, confirm: bool):
    """
    Compiler for hacker-lang, similar to rustc.

    Usage: hackerc <input.hacker> [output] [options]
    """
    input_path = Path(input)
    
    if output is None:
        base = input_path.stem
        if input_path.suffix == ".hacker":
            output = base
        else:
            output = base
    output_path = Path(output)

    home = os.environ.get("HOME")
    if home is None:
        console.print(Panel("HOME environment variable not set", title="Error", style="bold red"))
        sys.exit(1)

    compiler_path = Path(home) / ".hackeros" / "hacker-lang" / "bin" / "hacker-compiler"

    if not compiler_path.exists():
        console.print(Panel(f"hacker-compiler not found at {compiler_path}", title="Error", style="bold red"))
        sys.exit(1)

    cmd = [str(compiler_path), str(input_path), str(output_path)]
    if verbose:
        cmd.append("--verbose")
    if bytes_mode:
        cmd.append("--bytes")
    if opt:
        cmd.append("--opt")

    # Display compilation info
    console.print(Panel(f"Compiling {input_path.name}", title="hackerc", style="bold blue"))

    cmd_str = Text("Running: ", style="green") + Text(" ".join(cmd), style="white")
    console.print(cmd_str)

    if confirm:
        if not Confirm.ask("Proceed with compilation?"):
            console.print("Compilation cancelled.", style="yellow")
            sys.exit(0)

    # Run with progress
    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        transient=True
    ) as progress:
        task = progress.add_task("Compiling...", total=None)
        
        try:
            process = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            
            # Simulate progress (since we don't have real progress from compiler)
            import time
            while process.poll() is None:
                progress.advance(task)
                time.sleep(0.1)  # Add a small delay to avoid high CPU usage
            
            stdout, stderr = process.communicate()
            
            if process.returncode != 0:
                error_panel = Panel(
                    Text(stderr.strip() or "Unknown error", style="red"),
                    title="Compilation Failed",
                    style="bold red"
                )
                console.print(error_panel)
                if verbose:
                    console.print("STDOUT:", style="dim")
                    console.print(stdout.strip())
                sys.exit(1)
            
            success_panel = Panel(
                f"Output: {output_path}",
                title="Compilation Successful!",
                style="bold green"
            )
            console.print(success_panel)
            
            if verbose:
                console.print("Compiler Output:", style="dim")
                console.print(stdout.strip())
        
        except Exception as e:
            console.print(Panel(f"Error during compilation: {str(e)}", title="Error", style="bold red"))
            sys.exit(1)

if __name__ == "__main__":
    main()
