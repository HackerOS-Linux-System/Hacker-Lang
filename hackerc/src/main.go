package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/charmbracelet/lipgloss"
	"gopkg.in/yaml.v3"
)

const VERSION = "1.1" // Zaktualizowana wersja po zmianach

const HACKER_DIR = "~/.hackeros/hacker-lang"
const BIN_DIR = HACKER_DIR + "/bin"
const HISTORY_FILE = "~/.hackeros/history/hacker_repl_history"

const (
	colorReset  = "\033[0m"
	colorRed    = "\033[31m"
	colorGreen  = "\033[32m"
	colorYellow = "\033[33m"
	colorBlue   = "\033[34m"
	colorPurple = "\033[35m"
	colorCyan   = "\033[36m"
	colorWhite  = "\033[37m"
	colorBold   = "\033[1m"
	colorGray   = "\033[90m"
)

var (
	titleStyle   = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("201")).Underline(true).MarginBottom(1)
	headerStyle  = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("245")).MarginTop(1)
	exampleStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("15")).Padding(1)
	successStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("15")).Bold(true)
	errorStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("1")).Bold(true)
	warningStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("245"))
	infoStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("15"))
	promptStyle  = lipgloss.NewStyle().Foreground(lipgloss.Color("201")).Bold(true)
)

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
	os.MkdirAll(filepath.Dir(expandHome(HISTORY_FILE)), os.ModePerm)
}

func displayWelcome() {
	fmt.Printf("%s%sWelcome to Hacker Lang CLI v%s%s\n", colorBold, colorPurple, VERSION, colorReset)
	fmt.Printf("%sAdvanced scripting for HackerOS Linux system%s\n", colorGray, colorReset)
	fmt.Printf("%sType 'hackerc help' for commands or 'hackerc repl' to start interactive mode.%s\n", colorWhite, colorReset)
	helpCommand(false)
}

func runCommand(file string, verbose bool) bool {
	parserPath := expandHome(BIN_DIR + "/hacker-parser")
	cmd := exec.Command(parserPath, file)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	output, err := cmd.Output()
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error parsing file: %v", err)))
		return false
	}
	var parsed struct {
		Deps     []string          `json:"deps"`
		Libs     []string          `json:"libs"`
		Vars     map[string]string `json:"vars"`
		Cmds     []string          `json:"cmds"`
		Includes []string          `json:"includes"`
		Binaries []string          `json:"binaries"`
		Errors   []string          `json:"errors"`
		Config   map[string]string `json:"config"`
		Plugins  []string          `json:"plugins"`
	}
	if err := json.Unmarshal(output, &parsed); err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error unmarshaling parse output: %v", err)))
		return false
	}
	if len(parsed.Errors) > 0 {
		fmt.Println(errorStyle.Render("Syntax Errors:"))
		for _, e := range parsed.Errors {
			fmt.Println(" - " + e)
		}
		return false
	}
	if len(parsed.Libs) > 0 {
		fmt.Println(warningStyle.Render(fmt.Sprintf("Warning: Missing custom libs: %v", parsed.Libs)))
		fmt.Println(warningStyle.Render("Please install them using bytes install <lib>"))
	}
	tempSh, err := os.CreateTemp("", "*.sh")
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error creating temp file: %v", err)))
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
			fmt.Println(errorStyle.Render(fmt.Sprintf("Error reading include: %v", err)))
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
	for _, plugin := range parsed.Plugins {
		tempSh.WriteString(plugin + " &\n")
	}
	tempSh.Close()
	os.Chmod(tempSh.Name(), 0755)
	fmt.Println(infoStyle.Render(fmt.Sprintf("Executing script: %s", file)))
	fmt.Println(infoStyle.Render(fmt.Sprintf("Config: %v", parsed.Config)))
	fmt.Println(successStyle.Render("Running..."))
	runCmd := exec.Command("bash", tempSh.Name())
	runCmd.Env = os.Environ()
	for k, v := range parsed.Vars {
		runCmd.Env = append(runCmd.Env, fmt.Sprintf("%s=%s", k, v))
	}
	runCmd.Stdout = os.Stdout
	runCmd.Stderr = os.Stderr
	err = runCmd.Run()
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Execution failed: %v", err)))
		return false
	}
	fmt.Println(successStyle.Render("Execution completed successfully!"))
	return true
}

func compileCommand(file, output string, verbose bool) bool {
	binPath := expandHome(BIN_DIR + "/hacker-compiler")
	cmd := exec.Command(binPath, file, output)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	fmt.Println(infoStyle.Render(fmt.Sprintf("Compiling %s to %s", file, output)))
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Compilation failed: %v", err)))
		return false
	}
	fmt.Println(successStyle.Render("Compilation successful!"))
	return true
}

func checkCommand(file string, verbose bool) bool {
	parserPath := expandHome(BIN_DIR + "/hacker-parser")
	cmd := exec.Command(parserPath, file)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	output, err := cmd.Output()
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error parsing file: %v", err)))
		return false
	}
	var parsed struct {
		Errors []string `json:"errors"`
	}
	if err := json.Unmarshal(output, &parsed); err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error unmarshaling: %v", err)))
		return false
	}
	if len(parsed.Errors) > 0 {
		fmt.Println(errorStyle.Render("Syntax Errors:"))
		for _, e := range parsed.Errors {
			fmt.Println(" - " + e)
		}
		return false
	}
	fmt.Println(successStyle.Render("Syntax validation passed!"))
	return true
}

func initCommand(file string, verbose bool) bool {
	if _, err := os.Stat(file); err == nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("File %s already exists!", file)))
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
		fmt.Println(errorStyle.Render(fmt.Sprintf("Initialization failed: %v", err)))
		return false
	}
	fmt.Println(successStyle.Render(fmt.Sprintf("Initialized template at %s", file)))
	if verbose {
		fmt.Println(warningStyle.Render(template))
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
				fmt.Println(warningStyle.Render(fmt.Sprintf("Removed: %s", path)))
			}
		}
	}
	fmt.Println(successStyle.Render(fmt.Sprintf("Removed %d temporary files", count)))
	return true
}

func unpackBytes(verbose bool) bool {
	bytesPath1 := expandHome("~/hackeros/hacker-lang/bin/bytes")
	bytesPath2 := "/usr/bin/bytes"
	if _, err := os.Stat(bytesPath1); err == nil {
		fmt.Println(successStyle.Render(fmt.Sprintf("Bytes already installed at %s.", bytesPath1)))
		return true
	}
	if _, err := os.Stat(bytesPath2); err == nil {
		fmt.Println(successStyle.Render(fmt.Sprintf("Bytes already installed at %s.", bytesPath2)))
		return true
	}
	dir := filepath.Dir(bytesPath1)
	err := os.MkdirAll(dir, os.ModePerm)
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error creating directory: %v", err)))
		return false
	}
	url := "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes"
	resp, err := http.Get(url)
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error downloading bytes: %v", err)))
		return false
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error: status code %d", resp.StatusCode)))
		return false
	}
	f, err := os.Create(bytesPath1)
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error creating file: %v", err)))
		return false
	}
	defer f.Close()
	_, err = io.Copy(f, resp.Body)
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error writing file: %v", err)))
		return false
	}
	err = os.Chmod(bytesPath1, 0755)
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error setting permissions: %v", err)))
		return false
	}
	if verbose {
		fmt.Println(successStyle.Render(fmt.Sprintf("Downloaded and installed bytes from %s to %s", url, bytesPath1)))
	}
	fmt.Println(successStyle.Render("Bytes installed successfully!"))
	return true
}

func editorCommand(file string) bool {
	editorPath := expandHome(BIN_DIR + "/hacker-editor")
	args := []string{}
	if file != "" {
		args = append(args, file)
	}
	cmd := exec.Command(editorPath, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	fmt.Println(infoStyle.Render(fmt.Sprintf("Launching editor: %s %s", editorPath, file)))
	err := cmd.Run()
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Editor failed: %v", err)))
		return false
	}
	fmt.Println(successStyle.Render("Editor session completed."))
	return true
}

func runRepl(verbose bool) bool {
	replPath := expandHome(BIN_DIR + "/hacker-repl")
	cmd := exec.Command(replPath)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	err := cmd.Run()
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("REPL failed: %v", err)))
		return false
	}
	fmt.Println(successStyle.Render("REPL session ended."))
	return true
}

func versionCommand() bool {
	fmt.Println(infoStyle.Render(fmt.Sprintf("Hacker Lang v%s", VERSION)))
	return true
}

func syntaxCommand() bool {
	fmt.Println(headerStyle.Render("Hacker Lang Syntax Example:"))
	exampleCode := `// sudo
	# obsidian
	@USER=admin
	=2 > echo $USER
	? [ -d /tmp ] > echo OK
	& sleep 10
	# logging
	> sudo apt update
	[
	Config=Example
	]`
	fmt.Println(exampleStyle.Render(exampleCode))
	return true
}

func helpCommand(showBanner bool) bool {
	if showBanner {
		fmt.Println(titleStyle.Render("Hacker Lang CLI - Advanced Scripting Tool"))
	}
	fmt.Println(headerStyle.Render("Commands Overview:"))
	tableStyle := lipgloss.NewStyle().Padding(0, 1)
	rows := []string{
		lipgloss.NewStyle().Bold(true).Render(fmt.Sprintf("%-15s %-40s %-40s", "Command", "Description", "Arguments")),
	}
	commands := [][]string{
		{"run", "Execute a .hacker script", "file [--verbose] or . for bytes project"},
		{"compile", "Compile to native executable", "file [-o output] [--verbose] [--bytes]"},
		{"check", "Validate syntax", "file [--verbose]"},
		{"init", "Generate template script", "file [--verbose]"},
		{"clean", "Remove temporary files", "[--verbose]"},
		{"repl", "Launch interactive REPL", "[--verbose]"},
		{"editor", "Launch hacker-editor", "[file]"},
		{"unpack", "Unpack and install bytes", "bytes [--verbose]"},
		{"version", "Display version", ""},
		{"help", "Show this help menu", ""},
		{"syntax", "Show syntax examples", ""},
		{"help-ui", "Show special commands list", ""},
	}
	for _, cmd := range commands {
		rows = append(rows, fmt.Sprintf("%-15s %-40s %-40s", cmd[0], cmd[1], cmd[2]))
	}
	fmt.Println(tableStyle.Render(lipgloss.JoinVertical(lipgloss.Left, rows...)))
	return true
}

func runBytesProject(verbose bool) bool {
	bytesFile := "hacker.bytes"
	data, err := os.ReadFile(bytesFile)
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error reading %s: %v", bytesFile, err)))
		return false
	}
	var project struct {
		Package struct {
			Name        string `yaml:"name"`
			Version     string `yaml:"version"`
			Author      string `yaml:"author"`
			Description string `yaml:"description"`
		} `yaml:"package"`
		Entry   string   `yaml:"entry"`
		Libs    []string `yaml:"libs"`
		Scripts struct {
			Build   string `yaml:"build"`
			Run     string `yaml:"run"`
			Release string `yaml:"release"`
		} `yaml:"scripts"`
		Meta struct {
			License string `yaml:"license"`
			Repo    string `yaml:"repo"`
		} `yaml:"meta"`
	}
	if err := yaml.Unmarshal(data, &project); err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error parsing YAML: %v", err)))
		return false
	}
	fmt.Println(successStyle.Render(fmt.Sprintf("Running project %s v%s by %s", project.Package.Name, project.Package.Version, project.Package.Author)))
	return runCommand(project.Entry, verbose)
}

func compileBytesProject(output string, verbose bool) bool {
	bytesFile := "hacker.bytes"
	data, err := os.ReadFile(bytesFile)
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error reading %s: %v", bytesFile, err)))
		return false
	}
	var project struct {
		Package struct {
			Name        string `yaml:"name"`
			Version     string `yaml:"version"`
			Author      string `yaml:"author"`
			Description string `yaml:"description"`
		} `yaml:"package"`
		Entry   string   `yaml:"entry"`
		Libs    []string `yaml:"libs"`
		Scripts struct {
			Build   string `yaml:"build"`
			Run     string `yaml:"run"`
			Release string `yaml:"release"`
		} `yaml:"scripts"`
		Meta struct {
			License string `yaml:"license"`
			Repo    string `yaml:"repo"`
		} `yaml:"meta"`
	}
	if err := yaml.Unmarshal(data, &project); err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error parsing YAML: %v", err)))
		return false
	}
	if output == "" {
		output = project.Package.Name
	}
	binPath := expandHome(BIN_DIR + "/hacker-compiler")
	cmd := exec.Command(binPath, project.Entry, output, "--bytes")
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	fmt.Println(infoStyle.Render(fmt.Sprintf("Compiling project %s to %s with --bytes", project.Package.Name, output)))
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err = cmd.Run()
	if err != nil {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Compilation failed: %v", err)))
		return false
	}
	fmt.Println(successStyle.Render("Compilation successful!"))
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
	var file, output string
	if command == "--verbose" {
		verbose = true
		if len(args) == 0 {
			displayWelcome()
			os.Exit(0)
		}
		command = args[0]
		args = args[1:]
	}
	success := true
	switch command {
		case "run":
			if len(args) < 1 {
				fmt.Println(errorStyle.Render("Usage: hackerc run <file> [--verbose]"))
				os.Exit(1)
			}
			file = args[0]
			if file == "." {
				success = runBytesProject(verbose)
			} else {
				if len(args) > 1 && args[1] == "--verbose" {
					verbose = true
				}
				success = runCommand(file, verbose)
			}
		case "compile":
			if len(args) < 1 {
				fmt.Println(errorStyle.Render("Usage: hackerc compile <file> [-o <output>] [--verbose] [--bytes]"))
				os.Exit(1)
			}
			file = args[0]
			output = strings.TrimSuffix(file, filepath.Ext(file))
			bytes_mode := false
			i := 1
			for ; i < len(args); i++ {
				if args[i] == "--bytes" {
					bytes_mode = true
				} else if args[i] == "-o" {
					if i+1 >= len(args) {
						fmt.Println(errorStyle.Render("Missing output after -o"))
						os.Exit(1)
					}
					output = args[i+1]
					i++
				} else if args[i] == "--verbose" {
					verbose = true
				}
			}
			if bytes_mode {
				success = compileBytesProject(output, verbose)
			} else {
				success = compileCommand(file, output, verbose)
			}
		case "check":
			if len(args) < 1 {
				fmt.Println(errorStyle.Render("Usage: hackerc check <file> [--verbose]"))
				os.Exit(1)
			}
			file = args[0]
			if len(args) > 1 && args[1] == "--verbose" {
				verbose = true
			}
			success = checkCommand(file, verbose)
		case "init":
			if len(args) < 1 {
				fmt.Println(errorStyle.Render("Usage: hackerc init <file> [--verbose]"))
				os.Exit(1)
			}
			file = args[0]
			if len(args) > 1 && args[1] == "--verbose" {
				verbose = true
			}
			success = initCommand(file, verbose)
		case "clean":
			if len(args) > 0 && args[0] == "--verbose" {
				verbose = true
			}
			success = cleanCommand(verbose)
		case "repl":
			if len(args) > 0 && args[0] == "--verbose" {
				verbose = true
			}
			success = runRepl(verbose)
		case "editor":
			file = ""
			if len(args) > 0 {
				file = args[0]
			}
			success = editorCommand(file)
		case "unpack":
			if len(args) < 1 {
				fmt.Println(errorStyle.Render("Usage: hackerc unpack bytes [--verbose]"))
				success = false
			} else if args[0] == "bytes" {
				if len(args) > 1 && args[1] == "--verbose" {
					verbose = true
				}
				success = unpackBytes(verbose)
			} else {
				fmt.Println(errorStyle.Render(fmt.Sprintf("Unknown unpack target: %s", args[0])))
				success = false
			}
		case "version":
			success = versionCommand()
		case "help":
			success = helpCommand(true)
		case "syntax":
			success = syntaxCommand()
		case "help-ui":
			success = runHelpUI()
		case "install", "update", "remove":
			fmt.Println(warningStyle.Render(fmt.Sprintf("Please use bytes %s", command)))
			success = true
		default:
			fmt.Println(errorStyle.Render(fmt.Sprintf("Unknown command: %s", command)))
			helpCommand(false)
			success = false
	}
	if success {
		os.Exit(0)
	} else {
		os.Exit(1)
	}
}
