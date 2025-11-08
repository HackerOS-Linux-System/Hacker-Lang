package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

const VERSION = "0.0.4"
const HACKER_DIR = "~/.hackeros/hacker-lang"
const BIN_DIR = HACKER_DIR + "/bin"

func expandHome(path string) string {
	if strings.HasPrefix(path, "~") {
		home, _ := os.UserHomeDir()
		return strings.Replace(path, "~", home, 1)
	}
	return path
}

func ensureHackerDir() {
	os.MkdirAll(expandHome(BIN_DIR), os.ModePerm)
	os.MkdirAll(expandHome(HACKER_DIR+"/libs"), os.ModePerm)
}

func displayWelcome() {
	fmt.Println("Hacker Lang CLI")
	fmt.Println("Advanced scripting for Debian-based Linux systems")
	fmt.Printf("Version %s\n", VERSION)
	helpCommand(false)
}

func runCommand(file string, verbose bool) bool {
	parserPath := expandHome(HACKER_DIR + "/hacker_parser.py")
	cmd := exec.Command("python3", parserPath, file)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	output, err := cmd.Output()
	if err != nil {
		fmt.Println("Error parsing file:", err)
		return false
	}
	var parsed struct {
		Deps     []string            `json:"deps"`
		Libs     []string            `json:"libs"`
		Vars     map[string]string   `json:"vars"`
		Cmds     []string            `json:"cmds"`
		Includes []string            `json:"includes"`
		Binaries []string            `json:"binaries"`
		Errors   []string            `json:"errors"`
		Config   map[string]string   `json:"config"`
	}
	if err := json.Unmarshal(output, &parsed); err != nil {
		fmt.Println("Error unmarshaling parse output:", err)
		return false
	}
	if len(parsed.Errors) > 0 {
		fmt.Println("Syntax Errors:")
		for _, e := range parsed.Errors {
			fmt.Println(e)
		}
		return false
	}
	if len(parsed.Libs) > 0 {
		fmt.Println("Warning: Missing custom libs:", parsed.Libs)
		fmt.Println("Please install them using hacker-library install <lib>")
		// Proceed anyway
	}
	tempSh, err := os.CreateTemp("", "*.sh")
	if err != nil {
		fmt.Println("Error creating temp file:", err)
		return false
	}
	defer os.Remove(tempSh.Name())
	tempSh.WriteString("#!/bin/bash\n")
	tempSh.WriteString("set -e\n")
	for k, v := range parsed.Vars {
		tempSh.WriteString(fmt.Sprintf("export %s=\"%s\"\n", k, v))
	}
	for _, dep := range parsed.Deps {
		if dep != "sudo" {
			tempSh.WriteString(fmt.Sprintf("command -v %s &> /dev/null || (sudo apt update && sudo apt install -y %s)\n", dep, dep))
		}
	}
	for _, inc := range parsed.Includes {
		libPath := expandHome(HACKER_DIR + "/libs/" + inc + "/main.hacker")
		tempSh.WriteString(fmt.Sprintf("# Included from %s\n", inc))
		libContent, err := os.ReadFile(libPath)
		if err != nil {
			fmt.Println("Error reading include:", err)
			return false
		}
		tempSh.Write(libContent)
		tempSh.WriteString("\n")
	}
	for _, cmd := range parsed.Cmds {
		tempSh.WriteString(cmd + "\n")
	}
	for _, bin := range parsed.Binaries {
		tempSh.WriteString(bin + "\n")
	}
	tempSh.Close()
	os.Chmod(tempSh.Name(), 0755)
	fmt.Printf("Executing script: %s\n", file)
	fmt.Printf("Config: %v\n", parsed.Config)
	fmt.Println("Running...")
	runCmd := exec.Command("bash", tempSh.Name())
	runCmd.Env = os.Environ()
	for k, v := range parsed.Vars {
		runCmd.Env = append(runCmd.Env, fmt.Sprintf("%s=%s", k, v))
	}
	runCmd.Stdout = os.Stdout
	runCmd.Stderr = os.Stderr
	err = runCmd.Run()
	if err != nil {
		fmt.Println("Execution failed:", err)
		return false
	}
	fmt.Println("Execution completed successfully!")
	return true
}

func compileCommand(file, output string, verbose bool) bool {
	binPath := expandHome(BIN_DIR + "/hacker-compiler")
	cmd := exec.Command(binPath, file, output)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	fmt.Printf("Compiling %s to %s\n", file, output)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	if err != nil {
		fmt.Println("Compilation failed:", err)
		return false
	}
	fmt.Println("Compilation successful!")
	return true
}

func checkCommand(file string, verbose bool) bool {
	parserPath := expandHome(HACKER_DIR + "/hacker_parser.py")
	cmd := exec.Command("python3", parserPath, file)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	output, err := cmd.Output()
	if err != nil {
		fmt.Println("Error parsing file:", err)
		return false
	}
	var parsed struct {
		Errors []string `json:"errors"`
	}
	if err := json.Unmarshal(output, &parsed); err != nil {
		fmt.Println("Error unmarshaling:", err)
		return false
	}
	if len(parsed.Errors) > 0 {
		fmt.Println("Syntax Errors:")
		for _, e := range parsed.Errors {
			fmt.Println(e)
		}
		return false
	}
	fmt.Println("Syntax validation passed!")
	return true
}

func initCommand(file string, verbose bool) bool {
	if _, err := os.Stat(file); err == nil {
		fmt.Printf("File %s already exists!\n", file)
		return false
	}
	template := `! Hacker Lang advanced template
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
`
	err := os.WriteFile(file, []byte(template), 0644)
	if err != nil {
		fmt.Println("Initialization failed:", err)
		return false
	}
	fmt.Printf("Initialized template at %s\n", file)
	if verbose {
		fmt.Println(template)
	}
	return true
}

func cleanCommand(verbose bool) bool {
	tempDir := os.TempDir()
	count := 0
	files, _ := os.ReadDir(tempDir)
	for _, f := range files {
		if strings.HasPrefix(f.Name(), "tmp") && strings.HasSuffix(f.Name(), ".sh") {
			path := filepath.Join(tempDir, f.Name())
			os.Remove(path)
			count++
			if verbose {
				fmt.Println("Removed:", path)
			}
		}
	}
	fmt.Printf("Removed %d temporary files\n", count)
	return true
}

func replCommand(verbose bool) bool {
	replPath := expandHome(HACKER_DIR + "/repl.py")
	cmd := exec.Command("python3", replPath)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	err := cmd.Run()
	if err != nil {
		fmt.Println("REPL failed:", err)
		return false
	}
	return true
}

func versionCommand() bool {
	fmt.Printf("Hacker Lang v%s\n", VERSION)
	return true
}

func helpCommand(showBanner bool) bool {
	if showBanner {
		fmt.Println("Hacker Lang CLI - Advanced Scripting Tool")
	}
	fmt.Println("Commands Overview:")
	fmt.Println("Command\tDescription\tArguments")
	commands := [][]string{
		{"run", "Execute a .hacker script", "file [--verbose]"},
		{"compile", "Compile to native executable", "file [-o output] [--verbose]"},
		{"check", "Validate syntax", "file [--verbose]"},
		{"init", "Generate template script", "file [--verbose]"},
		{"clean", "Remove temporary files", "[--verbose]"},
		{"repl", "Launch interactive REPL", "[--verbose]"},
		{"version", "Display version", ""},
		{"help", "Show this help menu", ""},
	}
	for _, cmd := range commands {
		fmt.Printf("%s\t%s\t%s\n", cmd[0], cmd[1], cmd[2])
	}
	fmt.Println("\nSyntax Highlight Example:")
	exampleCode := `// sudo
# network-utils
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
# logging
> sudo apt update
[
Config=Example
]`
	fmt.Println(exampleCode)
	return true
}

func main() {
	ensureHackerDir()
	args := os.Args[1:]
	if len(args) == 0 {
		displayWelcome()
		os.Exit(0)
	}
	command := args[0]
	args = args[1:]
	verbose := false
	var file, output, libname string
	switch command {
	case "--verbose":
		verbose = true
		if len(args) == 0 {
			displayWelcome()
			os.Exit(0)
		}
		command = args[0]
		args = args[1:]
	}
	switch command {
	case "run":
		if len(args) < 1 {
			fmt.Println("Usage: hackerc run <file> [--verbose]")
			os.Exit(1)
		}
		file = args[0]
		runCommand(file, verbose)
	case "compile":
		if len(args) < 1 {
			fmt.Println("Usage: hackerc compile <file> [-o <output>] [--verbose]")
			os.Exit(1)
		}
		file = args[0]
		output = strings.TrimSuffix(file, filepath.Ext(file))
		if len(args) > 1 && args[1] == "-o" {
			if len(args) < 3 {
				fmt.Println("Missing output after -o")
				os.Exit(1)
			}
			output = args[2]
			args = args[3:]
		} else {
			args = args[1:]
		}
		if len(args) > 0 && args[0] == "--verbose" {
			verbose = true
		}
		compileCommand(file, output, verbose)
	case "check":
		if len(args) < 1 {
			fmt.Println("Usage: hackerc check <file> [--verbose]")
			os.Exit(1)
		}
		file = args[0]
		checkCommand(file, verbose)
	case "init":
		if len(args) < 1 {
			fmt.Println("Usage: hackerc init <file> [--verbose]")
			os.Exit(1)
		}
		file = args[0]
		initCommand(file, verbose)
	case "clean":
		cleanCommand(verbose)
	case "repl":
		replCommand(verbose)
	case "version":
		versionCommand()
	case "help":
		helpCommand(true)
	case "install", "update":
		fmt.Printf("Please use hacker-library %s\n", command)
		os.Exit(0)
	default:
		fmt.Println("Unknown command:", command)
		helpCommand(false)
		os.Exit(1)
	}
}
