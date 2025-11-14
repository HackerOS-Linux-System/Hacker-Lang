package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/bubbles/textinput"
	"github.com/charmbracelet/bubbles/viewport"
	"github.com/charmbracelet/lipgloss"
	"gopkg.in/yaml.v3"
)

const VERSION = "0.0.9" // Zaktualizowana wersja po zmianach
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
)

var (
	titleStyle   = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("201")).Underline(true).MarginBottom(1)
	headerStyle  = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("3")).MarginTop(1)
	exampleStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("6")).Border(lipgloss.RoundedBorder(), true).Padding(1)
	successStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("2")).Bold(true)
	errorStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("1")).Bold(true)
	warningStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("3"))
	infoStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("4"))
	promptStyle  = lipgloss.NewStyle().Foreground(lipgloss.Color("5")).Bold(true)
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
	fmt.Printf("%sAdvanced scripting for Debian-based Linux systems%s\n", colorCyan, colorReset)
	fmt.Printf("%sType 'hackerc help' for commands or 'hackerc repl' to start interactive mode.%s\n", colorBlue, colorReset)
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
			fmt.Println("  - " + e)
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
			fmt.Println("  - " + e)
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

type errMsg error

type replModel struct {
	textinput    textinput.Model
	viewport     viewport.Model
	lines        []string
	inConfig     bool
	verbose      bool
	output       []string
	err          errMsg
	history      []string
	historyIndex int
	width        int
	height       int
}

func newReplModel(verbose bool) *replModel {
	ti := textinput.New()
	ti.Focus()
	ti.CharLimit = 0
	ti.Width = 80
	vp := viewport.New(80, 20)
	vp.SetContent(lipgloss.JoinVertical(lipgloss.Left,
					    successStyle.Render("Hacker Lang REPL v"+VERSION+" - Enhanced Interactive Mode"),
					    "Type 'exit' to quit, 'help' for commands, 'clear' to reset",
				     "Supported: //deps, #libs, @vars, =loops, ?ifs, &bg, >cmds, [config], !comments",
	))
	return &replModel{
		textinput: ti,
		viewport:  vp,
		verbose:   verbose,
		history:   loadHistory(),
		historyIndex: -1,
		output:    []string{},
	}
}

func loadHistory() []string {
	path := expandHome(HISTORY_FILE)
	file, err := os.Open(path)
	if err != nil {
		return []string{}
	}
	defer file.Close()
	var h []string
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		h = append(h, scanner.Text())
	}
	return h
}

func saveHistory(line string) {
	path := expandHome(HISTORY_FILE)
	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return
	}
	defer f.Close()
	f.WriteString(line + "\n")
}

func (m *replModel) Init() tea.Cmd {
	return textinput.Blink
}

func (m *replModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmd tea.Cmd
	switch msg := msg.(type) {
		case tea.WindowSizeMsg:
			m.width = msg.Width
			m.height = msg.Height
			m.textinput.Width = msg.Width - 10
			m.viewport.Width = msg.Width
			m.viewport.Height = msg.Height - 3
		case tea.KeyMsg:
			switch msg.String() {
				case "ctrl+c":
					return m, tea.Quit
				case "up":
					if m.historyIndex < len(m.history)-1 {
						m.historyIndex++
						m.textinput.SetValue(m.history[len(m.history)-1-m.historyIndex])
					}
				case "down":
					if m.historyIndex > 0 {
						m.historyIndex--
						m.textinput.SetValue(m.history[len(m.history)-1-m.historyIndex])
					} else if m.historyIndex == 0 {
						m.historyIndex = -1
						m.textinput.SetValue("")
					}
				case "enter":
					line := m.textinput.Value()
					if line == "" {
						return m, nil
					}
					saveHistory(line)
					m.history = append(m.history, line)
					m.historyIndex = -1
					m.textinput.SetValue("")
					if line == "exit" {
						return m, tea.Quit
					} else if line == "help" {
						m.output = append(m.output, "REPL Commands:\n- exit: Quit REPL\n- help: This menu\n- clear: Reset session\n- verbose: Toggle verbose")
					} else if line == "clear" {
						m.lines = []string{}
						m.inConfig = false
						m.output = append(m.output, "Session cleared!")
					} else if line == "verbose" {
						m.verbose = !m.verbose
						m.output = append(m.output, fmt.Sprintf("Verbose mode: %t", m.verbose))
					} else {
						if line == "[" {
							m.inConfig = true
						} else if line == "]" {
							if !m.inConfig {
								m.output = append(m.output, "Error: Unmatched ']'")
								m.viewport.SetContent(strings.Join(m.output, "\n"))
								m.viewport.GotoBottom()
								return m, cmd
							}
							m.inConfig = false
						}
						m.lines = append(m.lines, line)
						if !m.inConfig && line != "" && !strings.HasPrefix(line, "!") {
							deps, libs, varsDict, cmds, includes, binaries, plugins, errors, _ := parseLines(m.lines, m.verbose)
							if len(errors) > 0 {
								m.output = append(m.output, "REPL Errors:\n"+strings.Join(errors, "\n"))
							} else {
								if len(libs) > 0 {
									m.output = append(m.output, "Warning: Missing custom libs: "+strings.Join(libs, ", "))
								}
								tempSh, err := os.CreateTemp("", "*.sh")
								if err != nil {
									m.output = append(m.output, fmt.Sprintf("Error: %v", err))
									m.viewport.SetContent(strings.Join(m.output, "\n"))
									m.viewport.GotoBottom()
									return m, cmd
								}
								defer os.Remove(tempSh.Name())
								tempSh.WriteString("#!/bin/bash\nset -e\n")
								for k, v := range varsDict {
									tempSh.WriteString(fmt.Sprintf("export %s=\"%s\"\n", k, v))
								}
								for _, dep := range deps {
									if dep != "sudo" {
										tempSh.WriteString(fmt.Sprintf("command -v %s || (sudo apt update && sudo apt install -y %s)\n", dep, dep))
									}
								}
								for _, inc := range includes {
									libPath := expandHome(HACKER_DIR + "/libs/" + inc + "/main.hacker")
									tempSh.WriteString(fmt.Sprintf("# include %s\n", inc))
									lf, err := os.ReadFile(libPath)
									if err != nil {
										m.output = append(m.output, fmt.Sprintf("Error: %v", err))
										m.viewport.SetContent(strings.Join(m.output, "\n"))
										m.viewport.GotoBottom()
										return m, cmd
									}
									tempSh.Write(lf)
									tempSh.WriteString("\n")
								}
								for _, cmd := range cmds {
									tempSh.WriteString(cmd + "\n")
								}
								for _, bin := range binaries {
									tempSh.WriteString(bin + "\n")
								}
								for _, plugin := range plugins {
									tempSh.WriteString(plugin + " &\n")
								}
								tempSh.Close()
								os.Chmod(tempSh.Name(), 0755)
								runCmd := exec.Command("bash", tempSh.Name())
								runCmd.Env = os.Environ()
								for k, v := range varsDict {
									runCmd.Env = append(runCmd.Env, fmt.Sprintf("%s=%s", k, v))
								}
								out, err := runCmd.CombinedOutput()
								outStr := string(out)
								if err != nil {
									m.output = append(m.output, "REPL Error:\n"+outStr)
								} else if outStr != "" {
									m.output = append(m.output, "REPL Output:\n"+outStr)
								}
							}
						}
					}
					m.viewport.SetContent(lipgloss.JoinVertical(lipgloss.Left, m.output...))
					m.viewport.GotoBottom()
					return m, cmd
			}
	}
	var tiCmd tea.Cmd
	m.textinput, tiCmd = m.textinput.Update(msg)
	var vpCmd tea.Cmd
	m.viewport, vpCmd = m.viewport.Update(msg)
	return m, tea.Batch(tiCmd, vpCmd)
}

func (m *replModel) View() string {
	prompt := "hacker> "
	if m.inConfig {
		prompt = "CONFIG> "
	}
	return lipgloss.JoinVertical(lipgloss.Left,
				     m.viewport.View(),
				     promptStyle.Render(prompt)+m.textinput.View(),
	)
}

func parseLines(lines []string, verbose bool) ([]string, []string, map[string]string, []string, []string, []string, []string, []string, map[string]string) {
	temp, err := os.CreateTemp("", "*.hacker")
	if err != nil {
		return nil, nil, nil, nil, nil, nil, nil, []string{err.Error()}, nil
	}
	defer os.Remove(temp.Name())
	_, err = temp.WriteString(strings.Join(lines, "\n") + "\n")
	if err != nil {
		return nil, nil, nil, nil, nil, nil, nil, []string{err.Error()}, nil
	}
	temp.Close()
	parserPath := expandHome(BIN_DIR + "/hacker-parser")
	cmd := exec.Command(parserPath, temp.Name())
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	output, err := cmd.Output()
	if err != nil {
		return nil, nil, nil, nil, nil, nil, nil, []string{err.Error()}, nil
	}
	var parsed struct {
		Deps     []string          `json:"deps"`
		Libs     []string          `json:"libs"`
		Vars     map[string]string `json:"vars"`
		Cmds     []string          `json:"cmds"`
		Includes []string          `json:"includes"`
		Binaries []string          `json:"binaries"`
		Plugins  []string          `json:"plugins"`
		Errors   []string          `json:"errors"`
		Config   map[string]string `json:"config"`
	}
	if err := json.Unmarshal(output, &parsed); err != nil {
		return nil, nil, nil, nil, nil, nil, nil, []string{err.Error()}, nil
	}
	return parsed.Deps, parsed.Libs, parsed.Vars, parsed.Cmds, parsed.Includes, parsed.Binaries, parsed.Plugins, parsed.Errors, parsed.Config
}

func runRepl(verbose bool) bool {
	p := tea.NewProgram(newReplModel(verbose), tea.WithAltScreen())
	if _, err := p.Run(); err != nil {
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

func helpCommand(showBanner bool) bool {
	if showBanner {
		fmt.Println(titleStyle.Render("Hacker Lang CLI - Advanced Scripting Tool"))
	}
	fmt.Println(headerStyle.Render("Commands Overview:"))
	tableStyle := lipgloss.NewStyle().Border(lipgloss.NormalBorder(), true).Padding(0, 1)
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
		{"help-ui", "Show special commands list", ""},
	}
	for _, cmd := range commands {
		rows = append(rows, fmt.Sprintf("%-15s %-40s %-40s", cmd[0], cmd[1], cmd[2]))
	}
	fmt.Println(tableStyle.Render(lipgloss.JoinVertical(lipgloss.Left, rows...)))

	fmt.Printf("\n%sSyntax Highlight Example:%s\n", headerStyle.Render(""), "")
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
		Entry   string `yaml:"entry"`
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
		Entry   string `yaml:"entry"`
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
			for i < len(args) {
				if args[i-1] == "--bytes" {
					bytes_mode = true
					continue
				}
				if args[i-1] == "-o" {
					if i >= len(args) {
						fmt.Println(errorStyle.Render("Missing output after -o"))
						os.Exit(1)
					}
					output = args[i]
					i++
					continue
				}
				if args[i-1] == "--verbose" {
					verbose = true
					continue
				}
				i++
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
