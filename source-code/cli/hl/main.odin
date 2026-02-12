package main

import "core:fmt"
import "core:os"
import "core:strings"
import "core:path/filepath"
import "core:sys/linux"
import "base:runtime"

// ANSI color codes for lots of colors as requested!
Color_Reset :: "\e[0m"
Color_Bold :: "\e[1m"
Color_Red :: "\e[31m"
Color_Green :: "\e[32m"
Color_Yellow :: "\e[33m"
Color_Blue :: "\e[34m"
Color_Magenta :: "\e[35m"
Color_Cyan :: "\e[36m"
Color_White :: "\e[37m"
Color_Gray :: "\e[90m"
Color_LightGray :: "\e[37m" // Actually white, but for light gray feel
Color_Purple :: "\e[95m" // Bright magenta as purple-ish
Color_Orange :: "\e[38;5;208m" // Custom orange
Color_Pink :: "\e[38;5;205m" // Custom pink
Color_Lime :: "\e[38;5;10m" // Lime green
Color_Teal :: "\e[38;5;14m" // Teal
Color_Indigo :: "\e[38;5;54m" // Indigo
Color_Gold :: "\e[38;5;220m" // Gold

// Background colors for headers
Bg_Magenta :: "\e[45m"
Bg_Cyan :: "\e[46m"
Bg_Red :: "\e[41m"
Bg_Green :: "\e[42m"

// Constants
Version :: "1.6.2"
Hacker_Dir :: ".hackeros/hacker-lang"
Bin_Dir :: "bin"
Compiler_Path :: "hl-compiler"
Runtime_Path :: "hl-runtime"

execute_external :: proc(exec_path: string, args: []string) -> (exit_code: int, ok: bool) {
	argv_buf: [dynamic]cstring
	defer delete(argv_buf)
	append(&argv_buf, cstring(raw_data(exec_path)))
	for a in args {
		append(&argv_buf, cstring(raw_data(a)))
	}
	append(&argv_buf, cstring(nil))
	pid, ferr := linux.fork()
	if ferr != .NONE {
		return -1, false
	}
	if pid == 0 {
		eerr := linux.execve(cstring(raw_data(exec_path)), raw_data(argv_buf[:]), nil)
		if eerr != .NONE {
			fmt.eprintln("execve failed:", eerr)
		}
		os.exit(127)
	}
	status: u32
	_, werr := linux.waitpid(pid, &status, {}, nil)
	if werr != .NONE {
		return -1, false
	}
	if linux.WIFEXITED(status) {
		return int(linux.WEXITSTATUS(status)), true
	}
	return -1, false
}

ensure_hacker_dir :: proc() -> bool {
	home_dir := os.get_env("HOME")
	if home_dir == "" {
		fmt.eprintln(Color_Red, "Failed to get HOME environment variable.", Color_Reset)
		return false
	}
	full_bin_dir, _ := filepath.join([]string{home_dir, Hacker_Dir, Bin_Dir})
	err := os.make_directory(full_bin_dir, 0o755)
	if err != 0 && !os.is_dir(full_bin_dir) {
		fmt.eprintln(Color_Red, "Failed to create hacker directory: ", err, Color_Reset)
		return false
	}
	return true
}

display_welcome :: proc() {
	// Fancy header with background and colors, plus simple ASCII art border
	fmt.println(Bg_Magenta, Color_White, Color_Bold, "┌──────────────────────────────────────────────┐", Color_Reset)
	fmt.println(Bg_Magenta, Color_White, Color_Bold, "│     Welcome to Hacker Lang CLI v", Version, "     │", Color_Reset)
	fmt.println(Bg_Magenta, Color_White, Color_Bold, "└──────────────────────────────────────────────┘", Color_Reset)
	fmt.println(Color_Cyan, Color_Bold, "Simplified tool for running and compiling .hacker scripts", Color_Reset)
	fmt.println(Color_White, "Type 'hl help' for available commands.", Color_Reset)
	fmt.println()
	help_command(false)
}

run_command :: proc(file: string, verbose: bool) -> bool {
	home_dir := os.get_env("HOME")
	full_runtime_path, _ := filepath.join([]string{home_dir, Hacker_Dir, Bin_Dir, Runtime_Path})
	if !os.exists(full_runtime_path) {
		fmt.eprintln(Color_Red, Color_Bold, "ERROR: Hacker runtime not found at ", full_runtime_path, ". Please install the Hacker Lang tools.", Color_Reset)
		return false
	}
	args: [dynamic]string
	defer delete(args)
	append(&args, file)
	if verbose {
		append(&args, "--verbose")
	}
	verbose_str := verbose ? " (verbose mode)" : ""
	fmt.println(Color_Cyan, Color_Bold, "INFO: Executing script: ", file, verbose_str, Color_Reset)
	exit_code, ok := execute_external(full_runtime_path, args[:])
	if !ok {
		fmt.eprintln(Color_Red, Color_Bold, "ERROR: Failed to execute process.", Color_Reset)
		return false
	}
	if exit_code != 0 {
		fmt.eprintln(Color_Red, Color_Bold, "ERROR: Execution failed with exit code: ", exit_code, Color_Reset)
		return false
	}
	fmt.println(Color_Green, Color_Bold, "SUCCESS: Execution completed successfully.", Color_Reset)
	return true
}

compile_command :: proc(file: string, output: string, verbose: bool) -> bool {
	home_dir := os.get_env("HOME")
	full_compiler_path, _ := filepath.join([]string{home_dir, Hacker_Dir, Bin_Dir, Compiler_Path})
	if !os.exists(full_compiler_path) {
		fmt.eprintln(Color_Red, Color_Bold, "ERROR: Hacker compiler not found at ", full_compiler_path, ". Please install the Hacker Lang tools.", Color_Reset)
		return false
	}
	args: [dynamic]string
	defer delete(args)
	append(&args, file)
	append(&args, output)
	if verbose {
		append(&args, "--verbose")
	}
	verbose_str := verbose ? " (verbose mode)" : ""
	fmt.println(Color_Cyan, Color_Bold, "INFO: Compiling script: ", file, " to ", output, verbose_str, Color_Reset)
	exit_code, ok := execute_external(full_compiler_path, args[:])
	if !ok {
		fmt.eprintln(Color_Red, Color_Bold, "ERROR: Failed to execute process.", Color_Reset)
		return false
	}
	if exit_code != 0 {
		fmt.eprintln(Color_Red, Color_Bold, "ERROR: Compilation failed with exit code: ", exit_code, Color_Reset)
		return false
	}
	fmt.println(Color_Green, Color_Bold, "SUCCESS: Compilation completed successfully.", Color_Reset)
	return true
}

help_command :: proc(show_banner: bool) -> bool {
	if show_banner {
		fmt.println(Bg_Magenta, Color_White, Color_Bold, "┌──────────────────────────────────────────────┐", Color_Reset)
		fmt.println(Bg_Magenta, Color_White, Color_Bold, "│  Hacker Lang CLI - Simplified Scripting Tool │", Color_Reset)
		fmt.println(Bg_Magenta, Color_White, Color_Bold, "│                 v", Version, "                  │", Color_Reset)
		fmt.println(Bg_Magenta, Color_White, Color_Bold, "└──────────────────────────────────────────────┘", Color_Reset)
		fmt.println()
	}
	fmt.println(Color_Blue, Color_Bold, "Available Commands:", Color_Reset)
	fmt.println(Color_Gray, "─────────────────────────────────────────────────────────────", Color_Reset)
	// Aligned table display with colors and fixed widths
	fmt.printf("%s%-10s %-30s %-40s%s\n", Color_LightGray, "Command", "Description", "Usage", Color_Reset)
	fmt.printf("%s%-10s%s %s%-30s%s %s%-40s%s\n", Color_Cyan, "run", Color_Reset, Color_White, "Execute a .hacker script", Color_Reset, Color_Yellow, "hl run <file> [--verbose]", Color_Reset)
	fmt.printf("%s%-10s%s %s%-30s%s %s%-40s%s\n", Color_Cyan, "compile", Color_Reset, Color_White, "Compile to native executable", Color_Reset, Color_Yellow, "hl compile <file> [-o output] [--verbose]", Color_Reset)
	fmt.printf("%s%-10s%s %s%-30s%s %s%-40s%s\n", Color_Cyan, "help", Color_Reset, Color_White, "Show this help menu", Color_Reset, Color_Yellow, "hl help", Color_Reset)
	fmt.println(Color_Gray, "─────────────────────────────────────────────────────────────", Color_Reset)
	fmt.println()
	fmt.println(Color_Gray, Color_Bold, "Global options:", Color_Reset)
	fmt.println(Color_Magenta, "-v, --version    Display version", Color_Reset)
	fmt.println(Color_Magenta, "-h, --help       Display help", Color_Reset)
	return true
}

version_command :: proc() -> bool {
	fmt.println(Color_Cyan, Color_Bold, "INFO: Hacker Lang CLI v", Version, Color_Reset)
	return true
}

_main :: proc() -> int {
	if !ensure_hacker_dir() {
		return 1
	}
	args := os.args[1:]
	if len(args) == 0 {
		display_welcome()
		return 0
	}
	// Parse global flags first
	global_version := false
	global_help := false
	filtered_args: [dynamic]string
	defer delete(filtered_args)
	for arg in args {
		switch arg {
		case "-v", "--version":
			global_version = true
		case "-h", "--help":
			global_help = true
		case:
			append(&filtered_args, arg)
		}
	}
	if global_version {
		version_command()
		return 0
	}
	if global_help {
		help_command(true)
		return 0
	}
	if len(filtered_args) == 0 {
		display_welcome()
		return 0
	}
	command := filtered_args[0]
	sub_args := filtered_args[1:]
	success := true
	switch command {
	case "run":
		file := ""
		verbose := false
		i := 0
		for i < len(sub_args) {
			arg := sub_args[i]
			if arg == "--verbose" {
				verbose = true
			} else if file == "" {
				file = arg
			} else {
				fmt.eprintln(Color_Red, Color_Bold, "ERROR: Unexpected argument: ", arg, Color_Reset)
				success = false
				break
			}
			i += 1
		}
		if file == "" {
			fmt.eprintln(Color_Red, Color_Bold, "ERROR: Expected exactly one argument: <file>", Color_Reset)
			fmt.println(Color_Bold, "Usage:", Color_Reset, " hl run <file> [options]\n\nExecute a .hacker script.")
			fmt.println(" --verbose    Enable verbose output")
			success = false
		}
		if success {
			success = run_command(file, verbose)
		}
	case "compile":
		file := ""
		output := ""
		verbose := false
		i := 0
		for i < len(sub_args) {
			arg := sub_args[i]
			switch arg {
			case "-o", "--output":
				i += 1
				if i >= len(sub_args) {
					fmt.eprintln(Color_Red, Color_Bold, "ERROR: Missing value for -o/--output", Color_Reset)
					success = false
					break
				}
				output = sub_args[i]
			case "--verbose":
				verbose = true
			case:
				if file == "" {
					file = arg
				} else {
					fmt.eprintln(Color_Red, Color_Bold, "ERROR: Unexpected argument: ", arg, Color_Reset)
					success = false
					break
				}
			}
			i += 1
		}
		if file == "" {
			fmt.eprintln(Color_Red, Color_Bold, "ERROR: Expected exactly one argument: <file>", Color_Reset)
			fmt.println(Color_Bold, "Usage:", Color_Reset, " hl compile <file> [options]\n\nCompile to native executable.")
			fmt.println(" -o, --output string    Specify output file")
			fmt.println(" --verbose              Enable verbose output")
			success = false
		}
		if output == "" {
			ext := filepath.ext(file)
			output = strings.trim_suffix(file, ext)
		}
		if success {
			success = compile_command(file, output, verbose)
		}
	case "help":
		success = help_command(true)
	case:
		fmt.eprintln(Color_Red, Color_Bold, "ERROR: Unknown command: ", command, Color_Reset)
		help_command(false)
		success = false
	}
	return success ? 0 : 1
}

main :: proc() {
	os.exit(_main())
}
