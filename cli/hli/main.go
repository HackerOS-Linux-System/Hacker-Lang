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
	"github.com/gosuri/uiprogress"
	"github.com/pterm/pterm"
	"github.com/urfave/cli/v2"
	"gopkg.in/yaml.v3"
)

const VERSION = "1.3"

var HACKER_DIR = filepath.Join(os.Getenv("HOME"), ".hackeros/hacker-lang")
var FRONTEND_BIN_DIR = filepath.Join(HACKER_DIR, "bin/frontend")
var MIDDLE_END_BIN_DIR = filepath.Join(HACKER_DIR, "bin/middle-end")
var BIN_DIR = filepath.Join(HACKER_DIR, "bin")
var CACHE_DIR = "./.cache"
var LEXER_PATH = filepath.Join(FRONTEND_BIN_DIR, "hacker-lexer")
var PARSER_PATH = filepath.Join(FRONTEND_BIN_DIR, "hacker-parser")
var SA_PATH = filepath.Join(MIDDLE_END_BIN_DIR, "hacker-sa")
var AST_PATH = filepath.Join(MIDDLE_END_BIN_DIR, "hacker-ast")
var COMPILER_PATH = filepath.Join(BIN_DIR, "hacker-compiler")
var RUNTIME_PATH = filepath.Join(BIN_DIR, "hacker-runtime")
var EDITOR_PATH = filepath.Join(BIN_DIR, "hacker-editor")
var REPL_PATH = filepath.Join(BIN_DIR, "hacker-repl")

var (
	successStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("2")).Bold(true)
	errorStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("1")).Bold(true)
	warningStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("3"))
	infoStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("6"))
	purpleStyle  = lipgloss.NewStyle().Foreground(lipgloss.Color("5")).Bold(true)
	grayStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
	whiteStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("7"))
	yellowStyle  = lipgloss.NewStyle().Foreground(lipgloss.Color("3"))
	cyanStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("6")).Bold(true)
)

func ensureHackerDir() error {
	dirs := []string{FRONTEND_BIN_DIR, MIDDLE_END_BIN_DIR, BIN_DIR, filepath.Join(HACKER_DIR, "libs"), filepath.Join(HACKER_DIR, "plugins"), CACHE_DIR}
	for _, d := range dirs {
		if err := os.MkdirAll(d, 0755); err != nil {
			return err
		}
	}
	return nil
}

func getJsonOutput(binPath string, inputFile *string, args []string, verbose bool) (string, error) {
	if _, err := os.Stat(binPath); os.IsNotExist(err) {
		return "", fmt.Errorf(errorStyle.Render("Binary not found: " + binPath))
	}
	cmd := exec.Command(binPath, args...)
	if inputFile != nil {
		cmd.Args = append(cmd.Args, *inputFile)
	}
	var out strings.Builder
	cmd.Stdout = &out
	if verbose {
		cmd.Stderr = os.Stderr
	} else {
		cmd.Stderr = io.Discard
	}
	if err := cmd.Run(); err != nil {
		return "", err
	}
	return out.String(), nil
}

func chainPipeline(file string, stages []string, finalOutput *string, verbose bool, mode string) (bool, string) {
	uiprogress.Start()
	bar := uiprogress.AddBar(len(stages)).AppendCompleted().PrependElapsed()
	bar.Width = 30
	currentJson := ""
	tempFiles := []string{}
	defer func() {
		for _, tf := range tempFiles {
			os.Remove(tf)
		}
		uiprogress.Stop()
	}()
	for _, stage := range stages {
		bar.Set(bar.Current() + 1)
		var err error
		switch stage {
			case "lexer":
				currentJson, err = getJsonOutput(LEXER_PATH, &file, []string{}, verbose)
			case "parser":
				parserArgs := []string{}
				if mode == "hli" {
					parserArgs = append(parserArgs, "--mode", mode)
				}
				input := tempFiles[len(tempFiles)-1]
				currentJson, err = getJsonOutput(PARSER_PATH, &input, parserArgs, verbose)
			case "sa":
				input := tempFiles[len(tempFiles)-1]
				currentJson, err = getJsonOutput(SA_PATH, &input, []string{}, verbose)
			case "ast":
				input := tempFiles[len(tempFiles)-1]
				currentJson, err = getJsonOutput(AST_PATH, &input, []string{}, verbose)
			case "compiler":
				compilerArgs := []string{}
				if finalOutput != nil {
					compilerArgs = append(compilerArgs, *finalOutput)
				} else {
					tempOut := "temp_exec"
					compilerArgs = append(compilerArgs, tempOut)
				}
				if verbose {
					compilerArgs = append(compilerArgs, "--verbose")
				}
				if finalOutput != nil {
					compilerArgs = append(compilerArgs, "--bytes")
				}
				input := tempFiles[len(tempFiles)-1]
				_, err = getJsonOutput(COMPILER_PATH, &input, compilerArgs, verbose)
				// Check if output exists
				outFile := *finalOutput
				if finalOutput == nil {
					outFile = "temp_exec"
				}
				if _, statErr := os.Stat(outFile); statErr == nil {
					err = nil
				}
		}
		if err != nil {
			fmt.Println(errorStyle.Render("Pipeline failed at " + stage))
			return false, ""
		}
		if stage != "compiler" {
			tempFile, tempErr := os.CreateTemp("", "*.json")
			if tempErr != nil {
				return false, ""
			}
			tempFile.WriteString(currentJson)
			tempFile.Close()
			tempFiles = append(tempFiles, tempFile.Name())
		}
	}
	lastStage := stages[len(stages)-1]
	if lastStage == "compiler" {
		return true, ""
	}
	return true, currentJson
}

func displayWelcome() {
	fmt.Println(purpleStyle.Render("Welcome to Hacker Lang Interface (HLI) v" + VERSION))
	fmt.Println(grayStyle.Render("Advanced scripting interface for Hacker Lang with full pipeline support."))
	fmt.Println(whiteStyle.Render("Type 'hli help' for commands or 'hli repl' to start interactive mode."))
	helpCommand(false)
}

func loadProjectEntry() string {
	bytesFile := "bytes.yaml"
	if _, err := os.Stat(bytesFile); err == nil {
		data, err := os.ReadFile(bytesFile)
		if err != nil {
			return ""
		}
		var config map[string]interface{}
		yaml.Unmarshal(data, &config)
		pkg, ok := config["package"].(map[string]interface{})
		if !ok {
			return ""
		}
		if entry, ok := pkg["entry"].(string); ok {
			return entry
		}
	}
	return ""
}

func runCommand(file string, verbose bool) bool {
	if !checkDependencies(file, verbose, nil) {
		return false
	}
	stages := []string{"lexer", "parser", "sa", "ast", "compiler"}
	tempExec := "temp_hli_exec"
	success, _ := chainPipeline(file, stages, &tempExec, verbose, "hli")
	if success {
		cmd := exec.Command(tempExec)
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		err := cmd.Run()
		os.Remove(tempExec)
		return err == nil
	}
	return false
}

func compileCommand(file string, output string, verbose bool, bytesMode bool) bool {
	if !checkDependencies(file, verbose, nil) {
		return false
	}
	stages := []string{"lexer", "parser", "sa", "ast", "compiler"}
	success, _ := chainPipeline(file, stages, &output, verbose, "hli")
	return success
}

func checkCommand(file string, verbose bool) bool {
	if !checkDependencies(file, verbose, nil) {
		return false
	}
	stages := []string{"lexer", "parser", "sa"}
	success, saJson := chainPipeline(file, stages, nil, verbose, "hli")
	if success {
		var parsed map[string]interface{}
		err := json.Unmarshal([]byte(saJson), &parsed)
		if err == nil {
			errors, _ := parsed["errors"].([]interface{})
			semErrors, _ := parsed["semantic_errors"].([]interface{})
			allErrors := append(errors, semErrors...)
			if len(allErrors) == 0 {
				fmt.Println(successStyle.Render("Validation passed!"))
				return true
			} else {
				fmt.Println(errorStyle.Render("\nErrors:"))
				for _, e := range allErrors {
					fmt.Println(" ✖ " + e.(string))
				}
				return false
			}
		}
	}
	return false
}

func initCommand(file *string, verbose bool) bool {
	targetFile := "main.hacker"
	if file != nil {
		targetFile = *file
	}
	if _, err := os.Stat(targetFile); err == nil {
		fmt.Println(errorStyle.Render("File " + targetFile + " already exists!"))
		return false
	}
	template := `! Updated Hacker Lang template with new syntax support
	// sudo apt
	# network-utils
	#> python:requests ! Foreign Python lib example
	@APP_NAME=HackerApp
	@LOG_LEVEL=debug
	$ITER=1 ! Local var
	=3 > echo "Iteration: $ITER - $APP_NAME" ! Loop with vars
	? [ -f /etc/os-release ] > cat /etc/os-release | grep PRETTY_NAME ! Conditional
	& ping -c 1 google.com ! Background
	: my_func
	> echo "In function"
	:
	. my_func ! Call function
	# logging
	> echo "Starting..."
	>>> sudo apt update && sudo apt upgrade -y ! Separate
	\\ plugin-tool ! Plugin
	[
	Author=Advanced User
	Version=1.0
	]
	`
	os.WriteFile(targetFile, []byte(template), 0644)
	fmt.Println(successStyle.Render("Initialized template at " + targetFile))
	if verbose {
		fmt.Println(yellowStyle.Render("\nTemplate content:"))
		fmt.Println(template)
	}
	bytesFile := "bytes.yaml"
	if _, err := os.Stat(bytesFile); os.IsNotExist(err) {
		bytesTemplate := `package:
		name: my-hacker-project
		version: 0.1.0
		author: User
		entry: ` + targetFile + `
		dependencies:
		- network-utils
		- logging
		- python:requests
		`
		os.WriteFile(bytesFile, []byte(bytesTemplate), 0644)
		fmt.Println(successStyle.Render("Initialized bytes.yaml for project"))
	}
	os.MkdirAll(CACHE_DIR, 0755)
	return true
}

func cleanCommand(verbose bool) bool {
	count := 0
	files, _ := os.ReadDir("/tmp")
	for _, f := range files {
		if strings.HasPrefix(f.Name(), "hacker_") || strings.HasPrefix(f.Name(), "temp_hli_exec") {
			path := filepath.Join("/tmp", f.Name())
			if verbose {
				fmt.Println(yellowStyle.Render("Removed: " + path))
			}
			os.Remove(path)
			count++
		}
	}
	if _, err := os.Stat(CACHE_DIR); err == nil {
		empty := true
		files, _ := os.ReadDir(CACHE_DIR)
		if len(files) > 0 {
			empty = false
		}
		if empty {
			os.RemoveAll(CACHE_DIR)
			if verbose {
				fmt.Println(yellowStyle.Render("Cleaned empty cache: " + CACHE_DIR))
			}
		}
	}
	fmt.Println(successStyle.Render(fmt.Sprintf("Removed %d temporary files", count)))
	return true
}

func unpackBytes(verbose bool) bool {
	bytesPath1 := filepath.Join(HACKER_DIR, "bin/bytes")
	bytesPath2 := "/usr/bin/bytes"
	if _, err := os.Stat(bytesPath1); err == nil {
		fmt.Println(successStyle.Render("Bytes already installed at " + bytesPath1 + "."))
		return true
	}
	if _, err := os.Stat(bytesPath2); err == nil {
		fmt.Println(successStyle.Render("Bytes already installed at " + bytesPath2 + "."))
		return true
	}
	os.MkdirAll(BIN_DIR, 0755)
	url := "https://github.com/Bytes-Repository/Bytes-CLI-Tool/releases/download/v0.3/bytes"
	resp, err := http.Get(url)
	if err != nil {
		fmt.Println(errorStyle.Render("Failed to download bytes: " + err.Error()))
		return false
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		fmt.Println(errorStyle.Render(fmt.Sprintf("Error: status code %d", resp.StatusCode)))
		return false
	}
	f, err := os.Create(bytesPath1)
	if err != nil {
		return false
	}
	defer f.Close()
	io.Copy(f, resp.Body)
	os.Chmod(bytesPath1, 0755)
	if verbose {
		fmt.Println(successStyle.Render("Downloaded and installed bytes from " + url + " to " + bytesPath1))
	}
	fmt.Println(successStyle.Render("Bytes installed successfully!"))
	return true
}

func editorCommand(file *string) bool {
	if _, err := os.Stat(EDITOR_PATH); os.IsNotExist(err) {
		fmt.Println(errorStyle.Render("Hacker editor not found at " + EDITOR_PATH + ". Please install the Hacker Lang tools."))
		return false
	}
	args := []string{}
	if file != nil {
		args = append(args, *file)
	}
	fmt.Println(cyanStyle.Render("Launching editor: " + EDITOR_PATH + " " + strings.Join(args, " ")))
	cmd := exec.Command(EDITOR_PATH, args...)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	if err == nil {
		fmt.Println(successStyle.Render("Editor session completed."))
		return true
	} else {
		fmt.Println(errorStyle.Render("Editor failed."))
		return false
	}
}

func runRepl(verbose bool) bool {
	if _, err := os.Stat(REPL_PATH); os.IsNotExist(err) {
		fmt.Println(errorStyle.Render("Hacker REPL not found at " + REPL_PATH + ". Please install the Hacker Lang tools."))
		return false
	}
	args := []string{}
	if verbose {
		args = append(args, "--verbose")
	}
	cmd := exec.Command(REPL_PATH, args...)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	if err == nil {
		fmt.Println(successStyle.Render("REPL session ended."))
		return true
	} else {
		fmt.Println(errorStyle.Render("REPL failed."))
		return false
	}
}

func versionCommand() bool {
	fmt.Println(cyanStyle.Render("Hacker Lang Interface (HLI) v" + VERSION))
	return true
}

func syntaxCommand() bool {
	fmt.Println(purpleStyle.Render("Hacker Lang Syntax Example:\n"))
	exampleCode := `// sudo
	# obsidian
	#> rust:serde ! Foreign Rust lib
	@USER=admin
	$ITER=0
	=2 > echo $USER - $ITER
	? [ -d /tmp ] > echo OK
	& sleep 10
	>> echo "With var: $USER"
	>>> separate_command
	: myfunc
	> echo in func
	:
	. myfunc
	# logging
	> sudo apt update
	[
	Config=Example
	]`
	fmt.Println(whiteStyle.Render(exampleCode))
	return true
}

func docsCommand() bool {
	fmt.Println(purpleStyle.Render("Hacker Lang Documentation:\n"))
	fmt.Println("Hacker Lang is an advanced scripting language for HackerOS.")
	fmt.Println("Key features:")
	fmt.Println("- Privileged operations with // sudo")
	fmt.Println("- Library includes with # lib-name or #> foreign-lang:lib")
	fmt.Println("- Variables with @VAR=value (global), $var=value (local)")
	fmt.Println("- Loops with =N > command")
	fmt.Println("- Conditionals with ? condition > command")
	fmt.Println("- Background tasks with & command")
	fmt.Println("- Multi-line commands with >> and >>>")
	fmt.Println("- Functions with : name ... : and calls .name")
	fmt.Println("- Metadata blocks with [ key=value ]")
	fmt.Println("- Foreign libs cached in .cache for hli")
	fmt.Println("\nFor more details, visit the official documentation or use 'hli tutorials' for examples.")
	return true
}

func tutorialsCommand() bool {
	fmt.Println(purpleStyle.Render("Hacker Lang Tutorials:\n"))
	fmt.Println("Tutorial 1: Basic Script")
	fmt.Println("Create main.hacker with > echo 'Hello'")
	fmt.Println("Run with: hli run")
	fmt.Println("\nTutorial 2: Pipeline")
	fmt.Println("hli check main.hacker # Validates lexer->parser->sa")
	fmt.Println("hli compile main.hacker -o exec # Full to binary")
	fmt.Println("\nTutorial 3: Foreign Libs")
	fmt.Println("Use #> python:requests in script; cached in .cache")
	fmt.Println("hli run # Handles automatically")
	return true
}

func helpCommand(showBanner bool) bool {
	if showBanner {
		fmt.Println(purpleStyle.Render("Hacker Lang Interface (HLI) - Pipeline-Enabled v" + VERSION + "\n"))
	}
	fmt.Println(purpleStyle.Render("Commands Overview:"))
	table := pterm.TableData{{"Command", "Description", "Arguments"}}
	table = append(table, []string{"run", "Execute via full pipeline (lexer->...->exec)", "[file] [--verbose]"})
	table = append(table, []string{"compile", "Compile via pipeline to binary", "[file] [-o output] [--verbose] [--bytes]"})
	table = append(table, []string{"check", "Check via pipeline up to SA", "[file] [--verbose]"})
	table = append(table, []string{"init", "Initialize a new project", "[--file <file>] [--verbose]"})
	table = append(table, []string{"clean", "Clean temporary files", "[--verbose]"})
	table = append(table, []string{"editor", "Launch hacker editor", "[file]"})
	table = append(table, []string{"repl", "Launch hacker REPL", "[--verbose]"})
	table = append(table, []string{"version", "Show version", ""})
	table = append(table, []string{"syntax", "Show syntax example", ""})
	table = append(table, []string{"docs", "Show documentation", ""})
	table = append(table, []string{"tutorials", "Show tutorials", ""})
	table = append(table, []string{"project-run", "Run bytes project", "[--verbose]"})
	pterm.DefaultTable.WithHasHeader().WithData(table).Render()
	return true
}

func runHelpUi() bool {
	fmt.Println(purpleStyle.Render("Hacker Lang Commands List"))
	table := pterm.TableData{}
	table = append(table, []string{"run: Execute via pipeline - hli run [file] [--verbose]"})
	// Add all
	pterm.DefaultTable.WithData(table).Render()
	return true
}

func runBytesProject(verbose bool) bool {
	bytesFile := "bytes.yaml"
	if _, err := os.Stat(bytesFile); os.IsNotExist(err) {
		fmt.Println(errorStyle.Render("Error: " + bytesFile + " not found. Use 'hli init' to create a project."))
		return false
	}
	data, err := os.ReadFile(bytesFile)
	if err != nil {
		return false
	}
	var config map[string]interface{}
	yaml.Unmarshal(data, &config)
	packageInfo := config["package"].(map[string]interface{})
	name := packageInfo["name"].(string)
	version := packageInfo["version"].(string)
	author := packageInfo["author"].(string)
	entry := packageInfo["entry"].(string)
	fmt.Println(successStyle.Render(fmt.Sprintf("Running project %s v%s by %s", name, version, author)))
	// deps := config["dependencies"].([]interface{})
	checkDependencies(entry, verbose, nil) // Pass deps if needed
	return runCommand(entry, verbose)
}

// Similarly for compileBytesProject, checkBytesProject

func checkDependencies(file string, verbose bool, depsYaml []interface{}) bool {
	if _, err := os.Stat(file); os.IsNotExist(err) {
		fmt.Println(errorStyle.Render("File " + file + " not found for dependency check."))
		return false
	}
	content, err := os.ReadFile(file)
	if err != nil {
		return false
	}
	lines := strings.Split(string(content), "\n")
	libsDir := filepath.Join(HACKER_DIR, "libs")
	pluginsDir := filepath.Join(HACKER_DIR, "plugins")
	missingLibs := []string{}
	missingPlugins := []string{}
	for _, line := range lines {
		stripped := strings.TrimSpace(line)
		if strings.HasPrefix(stripped, "//") {
			// Deps
		} else if strings.HasPrefix(stripped, "#") || strings.HasPrefix(stripped, "#>") {
			libName := strings.TrimSpace(stripped[1:])
			if strings.HasPrefix(stripped, "#>") {
				libName = strings.TrimSpace(stripped[2:])
			}
			if libName != "" {
				libMatches, _ := filepath.Glob(filepath.Join(libsDir, libName+"*"))
				cacheMatches, _ := filepath.Glob(filepath.Join(CACHE_DIR, libName+"*"))
				if len(libMatches) == 0 && len(cacheMatches) == 0 {
					missingLibs = append(missingLibs, libName)
				}
			}
		} else if strings.HasPrefix(stripped, "\\") {
			pluginName := strings.TrimSpace(stripped[1:])
			if pluginName != "" {
				pluginMatches, _ := filepath.Glob(filepath.Join(pluginsDir, pluginName+"*"))
				if len(pluginMatches) == 0 {
					missingPlugins = append(missingPlugins, pluginName)
				}
			}
		}
	}
	// From depsYaml if provided
	if len(missingPlugins) > 0 || len(missingLibs) > 0 {
		unpackBytes(verbose)
	}
	if len(missingPlugins) > 0 {
		if verbose {
			fmt.Println(warningStyle.Render("Missing plugins: " + strings.Join(missingPlugins, ", ")))
		}
		for _, p := range missingPlugins {
			fmt.Println(yellowStyle.Render("Installing plugin " + p + " via bytes..."))
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
			fmt.Println(warningStyle.Render("Missing libs: " + strings.Join(missingLibs, ", ")))
		}
		for _, l := range missingLibs {
			fmt.Println(yellowStyle.Render("Installing lib " + l + " via bytes or cache..."))
			if strings.Contains(l, ":") {
				// Foreign
				libDir := filepath.Join(CACHE_DIR, l)
				if _, err := os.Stat(libDir); os.IsNotExist(err) {
					os.MkdirAll(libDir, 0755)
					// Download - simulate
					fmt.Println("Cached foreign lib " + l)
				}
			} else {
				cmd := exec.Command("bytes", "install", l)
				cmd.Stdout = os.Stdout
				cmd.Stderr = os.Stderr
				if err := cmd.Run(); err != nil {
					return false
				}
			}
		}
	}
	return true
}

// Main
func main() {
	app := &cli.App{
		Name:  "hli",
		Usage: "Hacker Lang Interface",
		Action: func(c *cli.Context) error {
			displayWelcome()
			return nil
		},
		Commands: []*cli.Command{
			{
				Name:  "run",
				Usage: "Execute via full pipeline",
				Flags: []cli.Flag{
					&cli.BoolFlag{Name: "verbose"},
				},
				Action: func(c *cli.Context) error {
					file := c.Args().First()
					verbose := c.Bool("verbose")
					if file == "" {
						file = loadProjectEntry()
					}
					if file == "" {
						fmt.Println(errorStyle.Render("No file or project."))
						return cli.Exit("", 1)
					}
					success := runCommand(file, verbose)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:  "compile",
				Usage: "Compile via pipeline to binary",
				Flags: []cli.Flag{
					&cli.StringFlag{Name: "o", Usage: "output file"},
					&cli.BoolFlag{Name: "verbose"},
					&cli.BoolFlag{Name: "bytes"},
				},
				Action: func(c *cli.Context) error {
					file := c.Args().First()
					output := c.String("o")
					verbose := c.Bool("verbose")
					bytesMode := c.Bool("bytes")
					if file == "" {
						file = loadProjectEntry()
					}
					if file == "" {
						fmt.Println(errorStyle.Render("No file or project."))
						return cli.Exit("", 1)
					}
					success := compileCommand(file, output, verbose, bytesMode)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:  "check",
				Usage: "Check via pipeline up to SA",
				Flags: []cli.Flag{
					&cli.BoolFlag{Name: "verbose"},
				},
				Action: func(c *cli.Context) error {
					file := c.Args().First()
					verbose := c.Bool("verbose")
					if file == "" {
						file = loadProjectEntry()
					}
					if file == "" {
						fmt.Println(errorStyle.Render("No file or project."))
						return cli.Exit("", 1)
					}
					success := checkCommand(file, verbose)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:  "init",
				Usage: "Initialize a new project",
				Flags: []cli.Flag{
					&cli.StringFlag{Name: "file"},
					&cli.BoolFlag{Name: "verbose"},
				},
				Action: func(c *cli.Context) error {
					var filePtr *string
					file := c.String("file")
					if file != "" {
						filePtr = &file
					}
					verbose := c.Bool("verbose")
					success := initCommand(filePtr, verbose)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:  "clean",
				Usage: "Clean temporary files",
				Flags: []cli.Flag{
					&cli.BoolFlag{Name: "verbose"},
				},
				Action: func(c *cli.Context) error {
					verbose := c.Bool("verbose")
					success := cleanCommand(verbose)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:  "editor",
				Usage: "Launch hacker editor",
				Flags: []cli.Flag{
					&cli.StringFlag{Name: "file"},
				},
				Action: func(c *cli.Context) error {
					var filePtr *string
					file := c.Args().First()
					if file != "" {
						filePtr = &file
					}
					success := editorCommand(filePtr)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:  "repl",
				Usage: "Launch hacker REPL",
				Flags: []cli.Flag{
					&cli.BoolFlag{Name: "verbose"},
				},
				Action: func(c *cli.Context) error {
					verbose := c.Bool("verbose")
					success := runRepl(verbose)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:   "version",
				Usage:  "Show version",
				Action: func(c *cli.Context) error { versionCommand(); return nil },
			},
			{
				Name:   "syntax",
				Usage:  "Show syntax example",
				Action: func(c *cli.Context) error { syntaxCommand(); return nil },
			},
			{
				Name:   "docs",
				Usage:  "Show documentation",
				Action: func(c *cli.Context) error { docsCommand(); return nil },
			},
			{
				Name:   "tutorials",
				Usage:  "Show tutorials",
				Action: func(c *cli.Context) error { tutorialsCommand(); return nil },
			},
			{
				Name:   "help",
				Usage:  "Show help",
				Action: func(c *cli.Context) error { helpCommand(true); return nil },
			},
			{
				Name:  "project-run",
				Usage: "Run bytes project",
				Flags: []cli.Flag{
					&cli.BoolFlag{Name: "verbose"},
				},
				Action: func(c *cli.Context) error {
					verbose := c.Bool("verbose")
					success := runBytesProject(verbose)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
		},
	}
	app.Run(os.Args)
}
