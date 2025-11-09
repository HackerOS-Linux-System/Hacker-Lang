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

const VERSION = "0.0.8"
const HACKER_DIR = "~/.hackeros/hacker-lang"
const BIN_DIR = HACKER_DIR + "/bin"
const HISTORY_FILE = "~./hackeros/history/hacker_repl_history" // Zmiana, aby nie było ukryte

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
	titleStyle   = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("201")).Underline(true)
	headerStyle  = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("3"))
	exampleStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("6"))
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
}

func displayWelcome() {
	fmt.Printf("%s%sHacker Lang CLI%s\n", colorBold, colorPurple, colorReset)
	fmt.Printf("%sAdvanced scripting for Debian-based Linux systems%s\n", colorCyan, colorReset)
	fmt.Printf("%sVersion %s%s\n", colorBlue, VERSION, colorReset)
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
		fmt.Printf("%sError parsing file: %v%s\n", colorRed, err, colorReset)
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
		fmt.Printf("%sError unmarshaling parse output: %v%s\n", colorRed, err, colorReset)
		return false
	}
	if len(parsed.Errors) > 0 {
		fmt.Printf("%s%sSyntax Errors:%s\n", colorBold, colorRed, colorReset)
		for _, e := range parsed.Errors {
			fmt.Println(e)
		}
		return false
	}
	if len(parsed.Libs) > 0 {
		fmt.Printf("%sWarning: Missing custom libs: %v%s\n", colorYellow, parsed.Libs, colorReset)
		fmt.Printf("%sPlease install them using bytes install <lib>%s\n", colorYellow, colorReset)
	}
	tempSh, err := os.CreateTemp("", "*.sh")
	if err != nil {
		fmt.Printf("%sError creating temp file: %v%s\n", colorRed, err, colorReset)
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
			fmt.Printf("%sError reading include: %v%s\n", colorRed, err, colorReset)
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
	fmt.Printf("%sExecuting script: %s%s\n", colorGreen, file, colorReset)
	fmt.Printf("%sConfig: %v%s\n", colorGreen, parsed.Config, colorReset)
	fmt.Printf("%sRunning...%s\n", colorGreen, colorReset)
	runCmd := exec.Command("bash", tempSh.Name())
	runCmd.Env = os.Environ()
	for k, v := range parsed.Vars {
		runCmd.Env = append(runCmd.Env, fmt.Sprintf("%s=%s", k, v))
	}
	runCmd.Stdout = os.Stdout
	runCmd.Stderr = os.Stderr
	err = runCmd.Run()
	if err != nil {
		fmt.Printf("%sExecution failed: %v%s\n", colorRed, err, colorReset)
		return false
	}
	fmt.Printf("%sExecution completed successfully!%s\n", colorGreen, colorReset)
	return true
}

func compileCommand(file, output string, verbose bool) bool {
	binPath := expandHome(BIN_DIR + "/hacker-compiler")
	cmd := exec.Command(binPath, file, output)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	fmt.Printf("%sCompiling %s to %s%s\n", colorBlue, file, output, colorReset)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	if err != nil {
		fmt.Printf("%sCompilation failed: %v%s\n", colorRed, err, colorReset)
		return false
	}
	fmt.Printf("%sCompilation successful!%s\n", colorGreen, colorReset)
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
		fmt.Printf("%sError parsing file: %v%s\n", colorRed, err, colorReset)
		return false
	}
	var parsed struct {
		Errors []string `json:"errors"`
	}
	if err := json.Unmarshal(output, &parsed); err != nil {
		fmt.Printf("%sError unmarshaling: %v%s\n", colorRed, err, colorReset)
		return false
	}
	if len(parsed.Errors) > 0 {
		fmt.Printf("%s%sSyntax Errors:%s\n", colorBold, colorRed, colorReset)
		for _, e := range parsed.Errors {
			fmt.Println(e)
		}
		return false
	}
	fmt.Printf("%sSyntax validation passed!%s\n", colorGreen, colorReset)
	return true
}

func initCommand(file string, verbose bool) bool {
	if _, err := os.Stat(file); err == nil {
		fmt.Printf("%sFile %s already exists!%s\n", colorRed, file, colorReset)
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
		fmt.Printf("%sInitialization failed: %v%s\n", colorRed, err, colorReset)
		return false
	}
	fmt.Printf("%sInitialized template at %s%s\n", colorGreen, file, colorReset)
	if verbose {
		fmt.Printf("%s%s%s\n", colorYellow, template, colorReset)
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
				fmt.Printf("%sRemoved: %s%s\n", colorYellow, path, colorReset)
			}
		}
	}
	fmt.Printf("%sRemoved %d temporary files%s\n", colorGreen, count, colorReset)
	return true
}

func unpackBytes(verbose bool) bool {
	bytesPath1 := expandHome("~/hackeros/hacker-lang/bin/bytes")
	bytesPath2 := "/usr/bin/bytes"
	if _, err := os.Stat(bytesPath1); err == nil {
		fmt.Printf("%sBytes already installed at %s.%s\n", colorGreen, bytesPath1, colorReset)
		return true
	}
	if _, err := os.Stat(bytesPath2); err == nil {
		fmt.Printf("%sBytes already installed at %s.%s\n", colorGreen, bytesPath2, colorReset)
		return true
	}
	// Pobierz do bytesPath1
	dir := filepath.Dir(bytesPath1)
	err := os.MkdirAll(dir, os.ModePerm)
	if err != nil {
		fmt.Printf("%sError creating directory: %v%s\n", colorRed, err, colorReset)
		return false
	}
	url := "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes"
	resp, err := http.Get(url)
	if err != nil {
		fmt.Printf("%sError downloading bytes: %v%s\n", colorRed, err, colorReset)
		return false
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		fmt.Printf("%sError: status code %d%s\n", colorRed, resp.StatusCode, colorReset)
		return false
	}
	f, err := os.Create(bytesPath1)
	if err != nil {
		fmt.Printf("%sError creating file: %v%s\n", colorRed, err, colorReset)
		return false
	}
	defer f.Close()
	_, err = io.Copy(f, resp.Body)
	if err != nil {
		fmt.Printf("%sError writing file: %v%s\n", colorRed, err, colorReset)
		return false
	}
	err = os.Chmod(bytesPath1, 0755)
	if err != nil {
		fmt.Printf("%sError setting permissions: %v%s\n", colorRed, err, colorReset)
		return false
	}
	if verbose {
		fmt.Printf("%sDownloaded and installed bytes from %s to %s%s\n", colorGreen, url, bytesPath1, colorReset)
	}
	fmt.Printf("%sBytes installed successfully!%s\n", colorGreen, colorReset)
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
	vp.SetContent("Hacker Lang REPL v0.8 - Enhanced Interactive Mode\nType 'exit' to quit, 'help' for commands, 'clear' to reset\nSupported: //deps, #libs, @vars, =loops, ?ifs, &bg, >cmds, [config], !comments")
	return &replModel{
		textinput:    ti,
		viewport:     vp,
		verbose:      verbose,
		history:      loadHistory(),
		historyIndex: -1,
		output:       []string{},
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
			m.viewport.Height = msg.Height - 3 // for prompt and input
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
					m.viewport.SetContent(strings.Join(m.output, "\n"))
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
	promptStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("5"))
	return m.viewport.View() + "\n" + promptStyle.Render(prompt) + m.textinput.View() + "\n"
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
	p := tea.NewProgram(newReplModel(verbose))
	if _, err := p.Run(); err != nil {
		fmt.Printf("%sREPL failed: %v%s\n", colorRed, err, colorReset)
		return false
	}
	fmt.Printf("%sREPL session ended.%s\n", colorGreen, colorReset)
	return true
}

func versionCommand() bool {
	fmt.Printf("%sHacker Lang v%s%s\n", colorBlue, VERSION, colorReset)
	return true
}

func helpCommand(showBanner bool) bool {
	if showBanner {
		fmt.Printf("%s%sHacker Lang CLI - Advanced Scripting Tool%s\n", colorBold, colorPurple, colorReset)
	}
	fmt.Println(titleStyle.Render("Commands Overview:"))
	fmt.Println(headerStyle.Render(fmt.Sprintf("%-10s %-30s %-30s", "Command", "Description", "Arguments")))
	commands := [][]string{
		{"run", "Execute a .hacker script", "file [--verbose]"},
		{"compile", "Compile to native executable", "file [-o output] [--verbose]"},
		{"check", "Validate syntax", "file [--verbose]"},
		{"init", "Generate template script", "file [--verbose]"},
		{"clean", "Remove temporary files", "[--verbose]"},
		{"repl", "Launch interactive REPL", "[--verbose]"},
		{"unpack", "Unpack and install bytes", "bytes [--verbose]"},
		{"version", "Display version", ""},
		{"help", "Show this help menu", ""},
		{"help-ui", "Show special commands list", ""},
		{"unpack bytes", "Checks if the bytes utility is installed, if not installs it.", ""},
	}
	for _, cmd := range commands {
		fmt.Printf("%-10s %-30s %-30s\n", cmd[0], cmd[1], cmd[2])
	}
	fmt.Printf("\n%sSyntax Highlight Example:%s\n", headerStyle.Render(""), colorReset)
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
		fmt.Printf("%sError reading %s: %v%s\n", colorRed, bytesFile, err, colorReset)
		return false
	}
	var project struct {
		Package struct {
			Name        string `yaml:"name"`
			Version     string `yaml:"version"`
			Author      string `yaml:"author"`
			Description string `yaml:"description"`
		} `yaml:"package"`
		Entry  string   `yaml:"entry"`
		Libs   []string `yaml:"libs"`
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
		fmt.Printf("%sError parsing YAML: %v%s\n", colorRed, err, colorReset)
		return false
	}
	fmt.Printf("%sRunning project %s v%s by %s%s\n", colorGreen, project.Package.Name, project.Package.Version, project.Package.Author, colorReset)
	return runCommand(project.Entry, verbose)
}

func compileBytesProject(output string, verbose bool) bool {
	bytesFile := "hacker.bytes"
	data, err := os.ReadFile(bytesFile)
	if err != nil {
		fmt.Printf("%sError reading %s: %v%s\n", colorRed, bytesFile, err, colorReset)
		return false
	}
	var project struct {
		Package struct {
			Name        string `yaml:"name"`
			Version     string `yaml:"version"`
			Author      string `yaml:"author"`
			Description string `yaml:"description"`
		} `yaml:"package"`
		Entry  string   `yaml:"entry"`
		Libs   []string `yaml:"libs"`
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
		fmt.Printf("%sError parsing YAML: %v%s\n", colorRed, err, colorReset)
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
	fmt.Printf("%sCompiling project %s to %s with --bytes%s\n", colorBlue, project.Package.Name, output, colorReset)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err = cmd.Run()
	if err != nil {
		fmt.Printf("%sCompilation failed: %v%s\n", colorRed, err, colorReset)
		return false
	}
	fmt.Printf("%sCompilation successful!%s\n", colorGreen, colorReset)
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
				fmt.Printf("%sUsage: hackerc run <file> [--verbose]%s\n", colorRed, colorReset)
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
				fmt.Printf("%sUsage: hackerc compile <file> [-o <output>] [--verbose] [--bytes]%s\n", colorRed, colorReset)
				os.Exit(1)
			}
			file = args[0]
			output = strings.TrimSuffix(file, filepath.Ext(file))
			bytes_mode := false
			i := 0
			for i < len(args) {
				if args[i] == "--bytes" {
					bytes_mode = true
					i++
					continue
				}
				if args[i] == "-o" {
					i++
					if i >= len(args) {
						fmt.Printf("%sMissing output after -o%s\n", colorRed, colorReset)
						os.Exit(1)
					}
					output = args[i]
					i++
					continue
				}
				if args[i] == "--verbose" {
					verbose = true
					i++
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
				fmt.Printf("%sUsage: hackerc check <file> [--verbose]%s\n", colorRed, colorReset)
				os.Exit(1)
			}
			file = args[0]
			if len(args) > 1 && args[1] == "--verbose" {
				verbose = true
			}
			success = checkCommand(file, verbose)
		case "init":
			if len(args) < 1 {
				fmt.Printf("%sUsage: hackerc init <file> [--verbose]%s\n", colorRed, colorReset)
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
		case "unpack":
			if len(args) < 1 {
				fmt.Printf("%sUsage: hackerc unpack bytes [--verbose]%s\n", colorRed, colorReset)
				success = false
			} else if args[0] == "bytes" {
				if len(args) > 1 && args[1] == "--verbose" {
					verbose = true
				}
				success = unpackBytes(verbose)
			} else {
				fmt.Printf("%sUnknown unpack target: %s%s\n", colorRed, args[0], colorReset)
				success = false
			}
		case "version":
			success = versionCommand()
		case "help":
			success = helpCommand(true)
		case "help-ui":
			success = runHelpUI()
		case "install", "update", "remove":
			fmt.Printf("%sPlease use bytes %s%s\n", colorYellow, command, colorReset)
			success = true
		default:
			fmt.Printf("%sUnknown command: %s%s\n", colorRed, command, colorReset)
			helpCommand(false)
			success = false
	}
	if success {
		os.Exit(0)
	} else {
		os.Exit(1)
	}
}
