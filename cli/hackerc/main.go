package main

import (
	"fmt"
	"io"
	"io/ioutil"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/charmbracelet/lipgloss"
	"github.com/gosuri/uiprogress"
	"github.com/pterm/pterm"
	"github.com/urfave/cli/v2"
)

const VERSION = "1.3.0"

var HACKER_DIR = filepath.Join(os.Getenv("HOME"), ".hackeros/hacker-lang")
var FRONTEND_BIN_DIR = filepath.Join(HACKER_DIR, "bin/frontend")
var MIDDLE_END_BIN_DIR = filepath.Join(HACKER_DIR, "bin/middle-end")
var BIN_DIR = filepath.Join(HACKER_DIR, "bin")
var LEXER_PATH = filepath.Join(FRONTEND_BIN_DIR, "hacker-lexer")
var PARSER_PATH = filepath.Join(FRONTEND_BIN_DIR, "hacker-parser")
var SA_PATH = filepath.Join(MIDDLE_END_BIN_DIR, "hacker-sa")
var AST_PATH = filepath.Join(MIDDLE_END_BIN_DIR, "hacker-ast")
var COMPILER_PATH = filepath.Join(BIN_DIR, "hacker-compiler")
var RUNTIME_PATH = filepath.Join(BIN_DIR, "hacker-runtime")

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
	dirs := []string{FRONTEND_BIN_DIR, MIDDLE_END_BIN_DIR, BIN_DIR}
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

func chainPipeline(file string, stages []string, finalOutput *string, verbose bool, mode string) bool {
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
		bar.Incr()
		var err error
		switch stage {
			case "lexer":
				currentJson, err = getJsonOutput(LEXER_PATH, &file, []string{}, verbose)
			case "parser":
				parserArgs := []string{"--mode", mode}
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
				outFile := "temp_exec"
				if finalOutput != nil {
					outFile = *finalOutput
				}
				compilerArgs = append(compilerArgs, outFile)
				if verbose {
					compilerArgs = append(compilerArgs, "--verbose")
				}
				if finalOutput != nil {
					compilerArgs = append(compilerArgs, "--bytes")
				}
				input := tempFiles[len(tempFiles)-1]
				_, err = getJsonOutput(COMPILER_PATH, &input, compilerArgs, verbose)
				if _, statErr := os.Stat(outFile); statErr == nil {
					err = nil
				}
		}
		if err != nil {
			fmt.Println(errorStyle.Render("Pipeline failed at " + stage))
			return false
		}
		if stage != "compiler" {
			tempFile, tempErr := ioutil.TempFile("", "*.json")
			if tempErr != nil {
				return false
			}
			tempFile.WriteString(currentJson)
			tempFile.Close()
			tempFiles = append(tempFiles, tempFile.Name())
		}
	}
	return true
}

func displayWelcome() {
	fmt.Println(purpleStyle.Render("Welcome to Hacker Lang CLI v" + VERSION))
	fmt.Println(grayStyle.Render("Simplified tool for running and compiling .hacker scripts"))
	fmt.Println(whiteStyle.Render("Type 'hackerc help' for available commands.\n"))
	helpCommand(true)
}

func runCommand(file string, verbose bool) bool {
	stages := []string{"lexer", "parser", "sa", "ast", "compiler"}
	tempExec := "temp_hackerc_exec"
	success := chainPipeline(file, stages, &tempExec, verbose, "hackerc")
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

func compileCommand(file string, output string, verbose bool) bool {
	stages := []string{"lexer", "parser", "sa", "ast", "compiler"}
	return chainPipeline(file, stages, &output, verbose, "hackerc")
}

func helpCommand(showBanner bool) bool {
	if showBanner {
		fmt.Println(purpleStyle.Render("Hacker Lang CLI - Simplified Scripting Tool v" + VERSION + "\n"))
	}
	fmt.Println(purpleStyle.Render("Available Commands:"))
	table := pterm.TableData{{"Command", "Description", "Usage"}}
	table = append(table, []string{"run", "Execute a .hacker script", "hackerc run <file> [--verbose]"})
	table = append(table, []string{"compile", "Compile to native executable", "hackerc compile <file> [-o output] [--verbose]"})
	table = append(table, []string{"help", "Show this help menu", "hackerc help"})
	pterm.DefaultTable.WithHasHeader().WithData(table).Render()
	fmt.Println(grayStyle.Render("\nGlobal options:"))
	fmt.Println("-v, --version Display version")
	fmt.Println("-h, --help Display help")
	return true
}

func versionCommand() bool {
	fmt.Println(cyanStyle.Render("Hacker Lang CLI v" + VERSION))
	return true
}

func main() {
	if err := ensureHackerDir(); err != nil {
		fmt.Println(errorStyle.Render("Failed to create directories: " + err.Error()))
		os.Exit(1)
	}
	app := &cli.App{
		Name:  "hackerc",
		Usage: "Simplified Hacker Lang CLI",
		Commands: []*cli.Command{
			{
				Name:  "run",
				Usage: "Execute a .hacker script",
				Flags: []cli.Flag{
					&cli.BoolFlag{Name: "verbose"},
				},
				Action: func(c *cli.Context) error {
					file := c.Args().First()
					if file == "" {
						displayWelcome()
						return cli.Exit("", 0)
					}
					verbose := c.Bool("verbose")
					success := runCommand(file, verbose)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:  "compile",
				Usage: "Compile to native executable",
				Flags: []cli.Flag{
					&cli.StringFlag{Name: "output", Aliases: []string{"o"}},
					&cli.BoolFlag{Name: "verbose"},
				},
				Action: func(c *cli.Context) error {
					file := c.Args().First()
					if file == "" {
						displayWelcome()
						return cli.Exit("", 0)
					}
					output := c.String("output")
					if output == "" {
						output = strings.TrimSuffix(filepath.Base(file), filepath.Ext(file))
					}
					verbose := c.Bool("verbose")
					success := compileCommand(file, output, verbose)
					if success {
						return nil
					}
					return cli.Exit("", 1)
				},
			},
			{
				Name:  "help",
				Usage: "Show this help menu",
				Action: func(c *cli.Context) error {
					helpCommand(true)
					return nil
				},
			},
		},
		Version: VERSION,
	}
	app.Before = func(c *cli.Context) error {
		if c.Args().Len() == 0 {
			displayWelcome()
			os.Exit(0)
		}
		return nil
	}
	app.Run(os.Args)
}
