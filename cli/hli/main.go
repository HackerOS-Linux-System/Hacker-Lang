package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/pterm/pterm"
	"github.com/spf13/cobra"
	"gopkg.in/yaml.v3"
)

const Version = "1.2"

var (
	HackerDir    = filepath.Join(os.Getenv("HOME"), ".hackeros", "hacker-lang")
	BinDir       = filepath.Join(HackerDir, "bin")
	HistoryFile  = filepath.Join(os.Getenv("HOME"), ".hackeros", "history", "hacker_repl_history")
	ParserPath   = filepath.Join(BinDir, "hacker-plsa")
	CompilerPath = filepath.Join(BinDir, "hacker-compiler")
	RuntimePath  = filepath.Join(BinDir, "hacker-runtime")
	ReplPath     = filepath.Join(BinDir, "hacker-repl")
)

type Config struct {
	Name        string
	Version     string
	Author      string
	Description string
	Entry       string
	Libs        map[string][]string
	Scripts     map[string]string
	Meta        map[string]string
}

func ensureHackerDir() error {
	if err := os.MkdirAll(BinDir, 0755); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(HackerDir, "libs"), 0755); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(HackerDir, "plugins"), 0755); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(HistoryFile), 0755); err != nil {
		return err
	}
	return nil
}

func displayWelcome() {
	header := pterm.DefaultHeader.WithFullWidth().WithBackgroundStyle(pterm.NewStyle(pterm.BgLightMagenta)).WithTextStyle(pterm.NewStyle(pterm.FgBlack, pterm.Bold))
	header.Println("Welcome to Hacker Lang Interface (HLI) v" + Version)
	pterm.Println(pterm.Gray("Advanced scripting interface for HackerOS Linux system, inspired by Cargo"))
	pterm.Println(pterm.White("Type 'hli help' for commands or 'hli repl' to start interactive mode."))
	helpCommand(false)
}

func loadProjectConfig() (*Config, error) {
	if _, err := os.Stat("bytes.yaml"); err == nil {
		data, err := os.ReadFile("bytes.yaml")
		if err != nil {
			return nil, err
		}
		var yamlConfig struct {
			Package struct {
				Name    string `yaml:"name"`
				Version string `yaml:"version"`
				Author  string `yaml:"author"`
			} `yaml:"package"`
			Entry string `yaml:"entry"`
		}
		if err := yaml.Unmarshal(data, &yamlConfig); err != nil {
			return nil, err
		}
		return &Config{
			Name:    yamlConfig.Package.Name,
			Version: yamlConfig.Package.Version,
			Author:  yamlConfig.Package.Author,
			Entry:   yamlConfig.Entry,
		}, nil
	} else if _, err := os.Stat("package.hfx"); err == nil {
		data, err := os.ReadFile("package.hfx")
		if err != nil {
			return nil, err
		}
		return parseHFX(string(data))
	}
	return nil, fmt.Errorf("no project file found (bytes.yaml or package.hfx)")
}

func parseHFX(content string) (*Config, error) {
	config := &Config{
		Libs:    make(map[string][]string),
		Scripts: make(map[string]string),
		Meta:    make(map[string]string),
	}
	var currentSection string
	var currentLang string
	scanner := bufio.NewScanner(strings.NewReader(content))
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "//") { // assume comments start with //
			continue
		}
		if strings.HasSuffix(line, "{") || strings.HasSuffix(line, "[") {
			key := strings.TrimSpace(strings.TrimSuffix(strings.TrimSuffix(line, "{"), "["))
			switch key {
			case "package":
				currentSection = "package"
			case "-> libs":
				currentSection = "libs"
			case "-> scripts":
				currentSection = "scripts"
			case "-> meta":
				currentSection = "meta"
			}
			continue
		}
		if line == "}" || line == "]" {
			currentSection = ""
			currentLang = ""
			continue
		}
		if currentSection == "libs" {
			if strings.HasPrefix(line, "-> ") && strings.HasSuffix(line, ":") {
				currentLang = strings.TrimSuffix(strings.TrimPrefix(line, "-> "), ":")
				config.Libs[currentLang] = []string{}
				continue
			} else if strings.HasPrefix(line, "-> ") {
				lib := strings.TrimPrefix(line, "-> ")
				if currentLang != "" {
					config.Libs[currentLang] = append(config.Libs[currentLang], lib)
				}
				continue
			}
		}
		if (currentSection == "scripts" || currentSection == "meta") && strings.HasPrefix(line, "-> ") {
			subline := strings.TrimPrefix(line, "-> ")
			parts := strings.SplitN(subline, ":", 2)
			if len(parts) == 2 {
				key := strings.TrimSpace(parts[0])
				value := strings.TrimSpace(parts[1])
				value = strings.TrimSuffix(value, ",")
				value = strings.Trim(value, "\"")
				if currentSection == "scripts" {
					config.Scripts[key] = value
				} else if currentSection == "meta" {
					config.Meta[key] = value
				}
			}
			continue
		}
		parts := strings.SplitN(line, ":", 2)
		if len(parts) != 2 {
			continue
		}
		key := strings.TrimSpace(parts[0])
		value := strings.TrimSpace(parts[1])
		value = strings.TrimSuffix(value, ",")
		value = strings.Trim(value, "\"")
		switch currentSection {
		case "package":
			switch key {
			case "name":
				config.Name = value
			case "version":
				config.Version = value
			case "author":
				config.Author = value
			case "description":
				config.Description = value
			}
		default:
			if key == "entry" {
				config.Entry = value
			}
		}
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	if config.Entry == "" {
		return nil, fmt.Errorf("missing entry in package.hfx")
	}
	return config, nil
}

func loadProjectEntry() (string, error) {
	config, err := loadProjectConfig()
	if err != nil {
		return "", err
	}
	return config.Entry, nil
}

func runCommand(file string, verbose bool) bool {
	if _, err := os.Stat(RuntimePath); os.IsNotExist(err) {
		pterm.Error.Println("Hacker runtime not found at " + RuntimePath + ". Please install the Hacker Lang tools.")
		return false
	}
	args := []string{file}
	if verbose {
		args = append(args, "--verbose")
	}
	cmd := exec.Command(RuntimePath, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	return err == nil
}

func compileCommand(file string, output string, verbose bool, bytesMode bool) bool {
	if _, err := os.Stat(CompilerPath); os.IsNotExist(err) {
		pterm.Error.Println("Hacker compiler not found at " + CompilerPath + ". Please install the Hacker Lang tools.")
		return false
	}
	args := []string{file, output}
	if bytesMode {
		args = append(args, "--bytes")
	}
	if verbose {
		args = append(args, "--verbose")
	}
	cmd := exec.Command(CompilerPath, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	return err == nil
}

func checkCommand(file string, verbose bool) bool {
	if _, err := os.Stat(ParserPath); os.IsNotExist(err) {
		pterm.Error.Println("Hacker parser not found at " + ParserPath + ". Please install the Hacker Lang tools.")
		return false
	}
	args := []string{file}
	if verbose {
		args = append(args, "--verbose")
	}
	cmd := exec.Command(ParserPath, args...)
	var out bytes.Buffer
	var errOut bytes.Buffer
	cmd.Stdout = &out
	cmd.Stderr = &errOut
	err := cmd.Run()
	if err != nil {
		pterm.Error.Println("Error parsing file: " + errOut.String())
		return false
	}
	var parsed struct {
		Errors []string `json:"errors"`
	}
	if err := json.Unmarshal(out.Bytes(), &parsed); err != nil {
		pterm.Error.Println("Error unmarshaling parse output: " + err.Error())
		return false
	}
	if len(parsed.Errors) == 0 {
		pterm.Success.Println("Syntax validation passed!")
		return true
	}
	pterm.Error.Println("Errors:")
	for _, e := range parsed.Errors {
		pterm.Println(pterm.Red("✖ ") + e)
	}
	return false
}

func initCommand(file string, verbose bool) bool {
	targetFile := file
	if targetFile == "" {
		targetFile = "main.hacker"
	}
	if _, err := os.Stat(targetFile); err == nil {
		pterm.Error.Println("File " + targetFile + " already exists!")
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

echo "Starting update..." 
sudo apt update && sudo apt upgrade -y ! System update

echo "With var: $APP_NAME"

long_running_command_with_vars 
[ 
Author=Advanced User 
Version=1.0 
Description=System maintenance script 
]`
	if err := os.WriteFile(targetFile, []byte(template), 0644); err != nil {
		pterm.Error.Println("Failed to write template: " + err.Error())
		return false
	}
	pterm.Success.Println("Initialized template at " + targetFile)
	if verbose {
		pterm.Warning.Println("Template content:")
		pterm.Println(pterm.Yellow(template))
	}
	// Create project file if not exists
	bytesFile := "bytes.yaml"
	hfxFile := "package.hfx"
	_, errBytes := os.Stat(bytesFile)
	_, errHfx := os.Stat(hfxFile)
	if os.IsNotExist(errBytes) && os.IsNotExist(errHfx) {
		// Prefer creating package.hfx as per new instruction
		hfxTemplate := fmt.Sprintf(`package {
name: "my-hacker-project",
version: "0.1.0",
author: "User",
description: "My Hacker project"
}
entry: "%s"
-> libs [
-> python:
-> library1
-> rust:
-> library2
]
-> scripts {
-> build: "hackerc compile %s"
-> run: "hacker run ."
-> release: "hacker compile --bytes"
}
-> meta {
-> license: "MIT"
-> repo: "https://github.com/user/repo"
}`, targetFile, targetFile)
		if err := os.WriteFile(hfxFile, []byte(hfxTemplate), 0644); err != nil {
			pterm.Error.Println("Failed to write package.hfx: " + err.Error())
			return false
		}
		pterm.Success.Println("Initialized package.hfx for project")
	} else if os.IsNotExist(errBytes) {
		// If hfx exists, no need
	} else {
		// Update entry if needed, but skip for simplicity
	}
	return true
}

func cleanCommand(verbose bool) bool {
	count := 0
	files, _ := filepath.Glob("/tmp/*.sh")
	for _, path := range files {
		base := filepath.Base(path)
		if strings.HasPrefix(base, "tmp") || strings.HasPrefix(base, "sep_") {
			if verbose {
				pterm.Warning.Println("Removed: " + path)
			}
			os.Remove(path)
			count++
		}
	}
	pterm.Success.Println(fmt.Sprintf("Removed %d temporary files", count))
	return true
}

func unpackBytes(verbose bool) bool {
	bytesPath1 := filepath.Join(HackerDir, "bin/bytes")
	bytesPath2 := "/usr/bin/bytes"
	if _, err := os.Stat(bytesPath1); err == nil {
		pterm.Success.Println("Bytes already installed at " + bytesPath1 + ".")
		return true
	}
	if _, err := os.Stat(bytesPath2); err == nil {
		pterm.Success.Println("Bytes already installed at " + bytesPath2 + ".")
		return true
	}
	if err := os.MkdirAll(BinDir, 0755); err != nil {
		pterm.Error.Println("Failed to create bin dir: " + err.Error())
		return false
	}
	url := "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes"
	resp, err := http.Get(url)
	if err != nil {
		pterm.Error.Println("Failed to download: " + err.Error())
		return false
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		pterm.Error.Println(fmt.Sprintf("Error: status code %d", resp.StatusCode))
		return false
	}
	f, err := os.Create(bytesPath1)
	if err != nil {
		pterm.Error.Println("Failed to create file: " + err.Error())
		return false
	}
	defer f.Close()
	_, err = io.Copy(f, resp.Body)
	if err != nil {
		pterm.Error.Println("Failed to copy: " + err.Error())
		return false
	}
	if err := os.Chmod(bytesPath1, 0755); err != nil {
		pterm.Error.Println("Failed to chmod: " + err.Error())
		return false
	}
	if verbose {
		pterm.Success.Println("Downloaded and installed bytes from " + url + " to " + bytesPath1)
	}
	pterm.Success.Println("Bytes installed successfully!")
	return true
}

func runRepl(verbose bool) bool {
	if _, err := os.Stat(ReplPath); os.IsNotExist(err) {
		pterm.Error.Println("Hacker REPL not found at " + ReplPath + ". Please install the Hacker Lang tools.")
		return false
	}
	args := []string{}
	if verbose {
		args = append(args, "--verbose")
	}
	cmd := exec.Command(ReplPath, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	err := cmd.Run()
	if err == nil {
		pterm.Success.Println("REPL session ended.")
		return true
	}
	pterm.Error.Println("REPL failed.")
	return false
}

func versionCommand() bool {
	pterm.Println(pterm.Cyan("Hacker Lang Interface (HLI) v" + Version))
	return true
}

func syntaxCommand() bool {
	pterm.DefaultHeader.Println("Hacker Lang Syntax Example:")
	exampleCode := `// sudo

# obsidian
@USER=admin 
=2 > echo $USER 
? [ -d /tmp ] > echo OK 
& sleep 10

echo "With var: $USER"

separate_command

# logging

sudo apt update 
[ Config=Example ]`
	pterm.Println(pterm.White(exampleCode))
	return true
}

func docsCommand() bool {
	pterm.DefaultHeader.Println("Hacker Lang Documentation:")
	pterm.Println("Hacker Lang is an advanced scripting language for HackerOS.")
	pterm.Println("Key features:")
	bulletList := pterm.BulletListPrinter{}
	bulletList.Items = []pterm.BulletListItem{
		{Level: 0, Text: "Privileged operations with // sudo"},
		{Level: 0, Text: "Library includes with # lib-name"},
		{Level: 0, Text: "Variables with @VAR=value"},
		{Level: 0, Text: "Loops with =N > command"},
		{Level: 0, Text: "Conditionals with ? condition > command"},
		{Level: 0, Text: "Background tasks with & command"},
		{Level: 0, Text: "Multi-line commands with >> and >>>"},
		{Level: 0, Text: "Metadata blocks with [ key=value ]"},
	}
	bulletList.Render()
	pterm.Println("\nFor more details, visit the official documentation or use 'hli tutorials' for examples.")
	return true
}

func tutorialsCommand() bool {
	pterm.DefaultHeader.Println("Hacker Lang Tutorials:")
	pterm.Println("Tutorial 1: Basic Script")
	pterm.Println("Create a file main.hacker with:")
	pterm.Println("> echo 'Hello, Hacker Lang!'")
	pterm.Println("Run with: hli run")
	pterm.Println("\nTutorial 2: Using Libraries")
	pterm.Println("Add # logging to your script.")
	pterm.Println("HLI will automatically install if missing.")
	pterm.Println("\nTutorial 3: Projects")
	pterm.Println("Use 'hli init' to create a project with bytes.yaml.")
	pterm.Println("Then 'hli run' to execute.")
	return true
}

func helpCommand(showBanner bool) bool {
	if showBanner {
		header := pterm.DefaultHeader.WithFullWidth().WithBackgroundStyle(pterm.NewStyle(pterm.BgLightMagenta)).WithTextStyle(pterm.NewStyle(pterm.FgBlack, pterm.Bold))
		header.Println("Hacker Lang Interface (HLI) - Advanced Scripting Tool v" + Version)
	}
	pterm.DefaultSection.Println("Commands Overview:")
	tableData := [][]string{
		{"Command", "Description", "Arguments"},
		{"run", "Execute a .hacker script or project", "[file] [--verbose]"},
		{"compile", "Compile to native executable or project", "[file] [-o output] [--verbose] [--bytes]"},
		{"check", "Validate syntax", "[file] [--verbose]"},
		{"init", "Generate template script/project", "[file] [--verbose]"},
		{"clean", "Remove temporary files", "[--verbose]"},
		{"repl", "Launch interactive REPL", "[--verbose]"},
		{"unpack", "Unpack and install bytes", "bytes [--verbose]"},
		{"docs", "Show documentation", ""},
		{"tutorials", "Show tutorials", ""},
		{"version", "Display version", ""},
		{"help", "Show this help menu", ""},
		{"syntax", "Show syntax examples", ""},
		{"help-ui", "Show special commands list", ""},
	}
	pterm.DefaultTable.WithHasHeader().WithData(tableData).Render()
	return true
}

func runHelpUi() bool {
	pterm.DefaultHeader.WithBackgroundStyle(pterm.NewStyle(pterm.BgLightMagenta)).Println("Hacker Lang Commands List")
	bulletList := pterm.BulletListPrinter{}
	bulletList.Items = []pterm.BulletListItem{
		{Level: 0, Text: "run: Execute script/project - Usage: hli run [file] [--verbose]"},
		{Level: 0, Text: "compile: Compile to executable/project - Usage: hli compile [file] [-o output] [--verbose] [--bytes]"},
		{Level: 0, Text: "check: Validate syntax - Usage: hli check [file] [--verbose]"},
		{Level: 0, Text: "init: Generate template - Usage: hli init [file] [--verbose]"},
		{Level: 0, Text: "clean: Remove temps - Usage: hli clean [--verbose]"},
		{Level: 0, Text: "repl: Interactive REPL - Usage: hli repl [--verbose]"},
		{Level: 0, Text: "unpack: Unpack and install bytes - Usage: hli unpack bytes [--verbose]"},
		{Level: 0, Text: "docs: Show documentation - Usage: hli docs"},
		{Level: 0, Text: "tutorials: Show tutorials - Usage: hli tutorials"},
		{Level: 0, Text: "version: Show version - Usage: hli version"},
		{Level: 0, Text: "help: Show help - Usage: hli help"},
		{Level: 0, Text: "syntax: Show syntax examples - Usage: hli syntax"},
		{Level: 0, Text: "help-ui: Interactive help UI - This UI"},
	}
	bulletList.Render()
	return true
}

func runProject(verbose bool) bool {
	config, err := loadProjectConfig()
	if err != nil {
		pterm.Error.Println(err.Error() + ". Use 'hli init' to create a project.")
		return false
	}
	pterm.Success.Println(fmt.Sprintf("Running project %s v%s by %s", config.Name, config.Version, config.Author))
	checkDependencies(config.Entry, verbose)
	return runCommand(config.Entry, verbose)
}

func compileProject(output string, verbose bool, bytesMode bool) bool {
	config, err := loadProjectConfig()
	if err != nil {
		pterm.Error.Println(err.Error() + ". Use 'hli init' to create a project.")
		return false
	}
	if output == "" {
		output = config.Name
	}
	pterm.Println(pterm.Cyan(fmt.Sprintf("Compiling project %s to %s with --bytes", config.Name, output)))
	checkDependencies(config.Entry, verbose)
	return compileCommand(config.Entry, output, verbose, bytesMode)
}

func checkProject(verbose bool) bool {
	config, err := loadProjectConfig()
	if err != nil {
		pterm.Error.Println(err.Error() + ". Use 'hli init' to create a project.")
		return false
	}
	checkDependencies(config.Entry, verbose)
	return checkCommand(config.Entry, verbose)
}

func checkDependencies(file string, verbose bool) bool {
	if _, err := os.Stat(file); os.IsNotExist(err) {
		pterm.Error.Println("File " + file + " not found for dependency check.")
		return false
	}
	content, err := os.ReadFile(file)
	if err != nil {
		pterm.Error.Println("Failed to read file: " + err.Error())
		return false
	}
	libsDir := filepath.Join(HackerDir, "libs")
	pluginsDir := filepath.Join(HackerDir, "plugins")
	missingLibs := []string{}
	missingPlugins := []string{}
	lines := strings.Split(string(content), "\n")
	for _, line := range lines {
		stripped := strings.TrimSpace(line)
		if stripped == "" {
			continue
		}
		if strings.HasPrefix(stripped, "//") {
			pluginName := strings.TrimSpace(stripped[2:])
			pluginName = regexp.MustCompile(`[^a-zA-Z0-9_-]`).ReplaceAllString(strings.Split(pluginName, " ")[0], "")
			if pluginName != "" {
				matches, _ := filepath.Glob(filepath.Join(pluginsDir, pluginName+"*"))
				if len(matches) == 0 && !contains(missingPlugins, pluginName) {
					missingPlugins = append(missingPlugins, pluginName)
				}
			}
		} else if strings.HasPrefix(stripped, "#") {
			libName := strings.TrimSpace(stripped[1:])
			libName = regexp.MustCompile(`[^a-zA-Z0-9_-]`).ReplaceAllString(strings.Split(libName, " ")[0], "")
			if libName != "" {
				matches, _ := filepath.Glob(filepath.Join(libsDir, libName+"*"))
				if len(matches) == 0 && !contains(missingLibs, libName) {
					missingLibs = append(missingLibs, libName)
				}
			}
		}
	}
	if len(missingPlugins) > 0 {
		if verbose {
			pterm.Warning.Println("Missing plugins: " + strings.Join(missingPlugins, ", "))
		}
		for _, p := range missingPlugins {
			pterm.Warning.Println("Installing plugin " + p + " via bytes...")
			cmd := exec.Command("bytes", "plugin", "install", p)
			cmd.Stdout = os.Stdout
			cmd.Stderr = os.Stderr
			if err := cmd.Run(); err != nil {
				return false
			}
		}
	}
	if len(missingLibs) > 0 {
		if verbose {
			pterm.Warning.Println("Missing libs: " + strings.Join(missingLibs, ", "))
		}
		for _, l := range missingLibs {
			pterm.Warning.Println("Installing lib " + l + " via bytes...")
			cmd := exec.Command("bytes", "install", l)
			cmd.Stdout = os.Stdout
			cmd.Stderr = os.Stderr
			if err := cmd.Run(); err != nil {
				return false
			}
		}
	}
	return true
}

func contains(slice []string, item string) bool {
	for _, s := range slice {
		if s == item {
			return true
		}
	}
	return false
}

type TaskConfig struct {
	Vars map[string]interface{} `yaml:"vars"`
	Tasks map[string]struct {
		Requires []string `yaml:"requires"`
		Run []string `yaml:"run"`
	} `yaml:"tasks"`
	Aliases map[string]string `yaml:"aliases"`
}

func executeTask(taskName string, config *TaskConfig, executed map[string]struct{}) error {
	if _, ok := executed[taskName]; ok {
		return fmt.Errorf("cycle detected in tasks involving %s", taskName)
	}
	executed[taskName] = struct{}{}
	task, ok := config.Tasks[taskName]
	if !ok {
		return fmt.Errorf("task %s not found", taskName)
	}
	for _, req := range task.Requires {
		if err := executeTask(req, config, executed); err != nil {
			return err
		}
	}
	for _, cmdStr := range task.Run {
		// Substitute vars
		for varName, varValue := range config.Vars {
			cmdStr = strings.ReplaceAll(cmdStr, "{{"+varName+"}}", fmt.Sprint(varValue))
		}
		cmd := exec.Command("sh", "-c", cmdStr)
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		if err := cmd.Run(); err != nil {
			return fmt.Errorf("command failed: %s", cmdStr)
		}
	}
	return nil
}

var rootCmd = &cobra.Command{
	Use:   "hli",
	Short: "Hacker Lang Interface (HLI) - Advanced Scripting Tool",
	Run: func(cmd *cobra.Command, args []string) {
		displayWelcome()
	},
}

func init() {
	if err := ensureHackerDir(); err != nil {
		pterm.Fatal.Println("Failed to ensure hacker dir: " + err.Error())
	}
	rootCmd.AddCommand(runCmd)
	rootCmd.AddCommand(compileCmd)
	rootCmd.AddCommand(checkCmd)
	rootCmd.AddCommand(initCmd)
	rootCmd.AddCommand(cleanCmd)
	rootCmd.AddCommand(replCmd)
	rootCmd.AddCommand(unpackCmd)
	rootCmd.AddCommand(docsCmd)
	rootCmd.AddCommand(tutorialsCmd)
	rootCmd.AddCommand(versionCmd)
	rootCmd.AddCommand(helpCmd)
	rootCmd.AddCommand(syntaxCmd)
	rootCmd.AddCommand(helpUiCmd)
}

var runCmd = &cobra.Command{
	Use:   "run [file]",
	Short: "Execute a .hacker script or project",
	Run: func(cmd *cobra.Command, args []string) {
		verbose, _ := cmd.Flags().GetBool("verbose")
		var file string
		if len(args) > 0 {
			file = args[0]
		}
		var success bool
		if file == "" {
			entry, err := loadProjectEntry()
			if err != nil {
				pterm.Error.Println("No project found. Use 'hli init' or specify a file.")
				success = false
			} else {
				checkDependencies(entry, verbose)
				success = runCommand(entry, verbose)
			}
		} else if file == "." {
			success = runProject(verbose)
		} else {
			checkDependencies(file, verbose)
			success = runCommand(file, verbose)
		}
		if !success {
			os.Exit(1)
		}
	},
}

func init() {
	runCmd.Flags().BoolP("verbose", "", false, "Enable verbose output")
}

var compileCmd = &cobra.Command{
	Use:   "compile [file]",
	Short: "Compile to native executable or project",
	Run: func(cmd *cobra.Command, args []string) {
		verbose, _ := cmd.Flags().GetBool("verbose")
		output, _ := cmd.Flags().GetString("output")
		bytesMode, _ := cmd.Flags().GetBool("bytes")
		var file string
		if len(args) > 0 {
			file = args[0]
		}
		var success bool
		if file == "" {
			entry, err := loadProjectEntry()
			if err != nil {
				pterm.Error.Println("No project found. Use 'hli init' or specify a file.")
				success = false
			} else {
				if output == "" {
					output = strings.TrimSuffix(entry, filepath.Ext(entry))
				}
				checkDependencies(entry, verbose)
				success = compileCommand(entry, output, verbose, bytesMode)
			}
		} else if file == "." {
			success = compileProject(output, verbose, bytesMode)
		} else {
			if output == "" {
				output = strings.TrimSuffix(file, filepath.Ext(file))
			}
			checkDependencies(file, verbose)
			success = compileCommand(file, output, verbose, bytesMode)
		}
		if !success {
			os.Exit(1)
		}
	},
}

func init() {
	compileCmd.Flags().StringP("output", "o", "", "Specify output file")
	compileCmd.Flags().Bool("bytes", false, "Enable bytes mode")
	compileCmd.Flags().BoolP("verbose", "", false, "Enable verbose output")
}

var checkCmd = &cobra.Command{
	Use:   "check [file]",
	Short: "Validate syntax",
	Run: func(cmd *cobra.Command, args []string) {
		verbose, _ := cmd.Flags().GetBool("verbose")
		var file string
		if len(args) > 0 {
			file = args[0]
		}
		var success bool
		if file == "" {
			entry, err := loadProjectEntry()
			if err != nil {
				pterm.Error.Println("No project found. Use 'hli init' or specify a file.")
				success = false
			} else {
				checkDependencies(entry, verbose)
				success = checkCommand(entry, verbose)
			}
		} else if file == "." {
			success = checkProject(verbose)
		} else {
			checkDependencies(file, verbose)
			success = checkCommand(file, verbose)
		}
		if !success {
			os.Exit(1)
		}
	},
}

func init() {
	checkCmd.Flags().BoolP("verbose", "", false, "Enable verbose output")
}

var initCmd = &cobra.Command{
	Use:   "init [file]",
	Short: "Generate template script/project",
	Run: func(cmd *cobra.Command, args []string) {
		verbose, _ := cmd.Flags().GetBool("verbose")
		var file string
		if len(args) > 0 {
			file = args[0]
		}
		success := initCommand(file, verbose)
		if !success {
			os.Exit(1)
		}
	},
}

func init() {
	initCmd.Flags().BoolP("verbose", "", false, "Enable verbose output (show template content)")
}

var cleanCmd = &cobra.Command{
	Use:   "clean",
	Short: "Remove temporary files",
	Run: func(cmd *cobra.Command, args []string) {
		verbose, _ := cmd.Flags().GetBool("verbose")
		success := cleanCommand(verbose)
		if !success {
			os.Exit(1)
		}
	},
}

func init() {
	cleanCmd.Flags().BoolP("verbose", "", false, "Show removed files")
}

var replCmd = &cobra.Command{
	Use:   "repl",
	Short: "Launch interactive REPL",
	Run: func(cmd *cobra.Command, args []string) {
		verbose, _ := cmd.Flags().GetBool("verbose")
		success := runRepl(verbose)
		if !success {
			os.Exit(1)
		}
	},
}

func init() {
	replCmd.Flags().BoolP("verbose", "", false, "Enable verbose output")
}

var unpackCmd = &cobra.Command{
	Use:   "unpack bytes",
	Short: "Unpack and install bytes",
	Run: func(cmd *cobra.Command, args []string) {
		verbose, _ := cmd.Flags().GetBool("verbose")
		if len(args) != 1 || args[0] != "bytes" {
			pterm.Error.Println("Expected exactly one argument: bytes")
			cmd.Help()
			os.Exit(1)
		}
		success := unpackBytes(verbose)
		if !success {
			os.Exit(1)
		}
	},
}

func init() {
	unpackCmd.Flags().BoolP("verbose", "", false, "Enable verbose output")
}

var docsCmd = &cobra.Command{
	Use:   "docs",
	Short: "Show documentation",
	Run: func(cmd *cobra.Command, args []string) {
		success := docsCommand()
		if !success {
			os.Exit(1)
		}
	},
}

var tutorialsCmd = &cobra.Command{
	Use:   "tutorials",
	Short: "Show tutorials",
	Run: func(cmd *cobra.Command, args []string) {
		success := tutorialsCommand()
		if !success {
			os.Exit(1)
		}
	},
}

var versionCmd = &cobra.Command{
	Use:   "version",
	Short: "Display version",
	Run: func(cmd *cobra.Command, args []string) {
		success := versionCommand()
		if !success {
			os.Exit(1)
		}
	},
}

var helpCmd = &cobra.Command{
	Use:   "help",
	Short: "Show this help menu",
	Run: func(cmd *cobra.Command, args []string) {
		success := helpCommand(true)
		if !success {
			os.Exit(1)
		}
	},
}

var syntaxCmd = &cobra.Command{
	Use:   "syntax",
	Short: "Show syntax examples",
	Run: func(cmd *cobra.Command, args []string) {
		success := syntaxCommand()
		if !success {
			os.Exit(1)
		}
	},
}

var helpUiCmd = &cobra.Command{
	Use:   "help-ui",
	Short: "Show special commands list",
	Run: func(cmd *cobra.Command, args []string) {
		success := runHelpUi()
		if !success {
			os.Exit(1)
		}
	},
}

func main() {
	if len(os.Args) > 1 {
		command := os.Args[1]
		if command == "--version" || command == "-v" {
			versionCommand()
			os.Exit(0)
		} else if command == "--help" || command == "-h" {
			helpCommand(true)
			os.Exit(0)
		}
		knownCommands := []string{"run", "compile", "check", "init", "clean", "repl", "unpack", "docs", "tutorials", "version", "help", "syntax", "help-ui"}
		isKnown := false
		for _, kc := range knownCommands {
			if command == kc {
				isKnown = true
				break
			}
		}
		if !isKnown {
			if _, err := os.Stat(".hackerfile"); err == nil {
				data, err := os.ReadFile(".hackerfile")
				if err != nil {
					pterm.Error.Println("Error reading .hackerfile: " + err.Error())
					os.Exit(1)
				}
				var config TaskConfig
				if err := yaml.Unmarshal(data, &config); err != nil {
					pterm.Error.Println("Error parsing .hackerfile: " + err.Error())
					os.Exit(1)
				}
				aliasedTask := command
				if alias, ok := config.Aliases[command]; ok {
					aliasedTask = alias
				}
				if _, ok := config.Tasks[aliasedTask]; ok {
					executed := make(map[string]struct{})
					if err := executeTask(aliasedTask, &config, executed); err != nil {
						pterm.Error.Println("Error executing task: " + err.Error())
						os.Exit(1)
					}
					os.Exit(0)
				} else {
					pterm.Error.Println("Unknown task: " + command)
					helpCommand(false)
					os.Exit(1)
				}
			} else if command == "install" || command == "update" || command == "remove" {
				pterm.Warning.Println("Please use bytes " + command)
				os.Exit(0)
			} else {
				pterm.Error.Println("Unknown command: " + command)
				helpCommand(false)
				os.Exit(1)
			}
		}
	}
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
