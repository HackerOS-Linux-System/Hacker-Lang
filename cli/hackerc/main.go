package main

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/charmbracelet/lipgloss"
	"github.com/pterm/pterm"
)

const (
	Version      = "1.2"
	HackerDir    = ".hackeros/hacker-lang"
	BinDir       = "bin"
	CompilerPath = "hacker-compiler"
	RuntimePath  = "hacker-runtime"
)

var (
	boldStyle     = lipgloss.NewStyle().Bold(true)
	purpleStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("#A020F0")).Bold(true)
	grayStyle     = lipgloss.NewStyle().Foreground(lipgloss.Color("#808080"))
	whiteStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("#FFFFFF"))
	cyanStyle     = lipgloss.NewStyle().Foreground(lipgloss.Color("#00FFFF"))
	greenStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("#00FF00"))
	redStyle      = lipgloss.NewStyle().Foreground(lipgloss.Color("#FF0000"))
	yellowStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("#FFFF00"))
	lightGrayStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("#D3D3D3"))
	blueStyle     = lipgloss.NewStyle().Foreground(lipgloss.Color("#0000FF")).Bold(true)
	magentaStyle  = lipgloss.NewStyle().Foreground(lipgloss.Color("#FF00FF"))
)

func ensureHackerDir() error {
	fullBinDir := filepath.Join(os.Getenv("HOME"), HackerDir, BinDir)
	return os.MkdirAll(fullBinDir, 0755)
}

func displayWelcome() {
	header := pterm.DefaultHeader.WithFullWidth().WithBackgroundStyle(pterm.NewStyle(pterm.BgMagenta)).WithTextStyle(pterm.NewStyle(pterm.FgWhite))
	header.Println("Welcome to Hacker Lang CLI v" + Version)
	pterm.DefaultSection.WithStyle(pterm.NewStyle(pterm.FgCyan)).Println(grayStyle.Render("Simplified tool for running and compiling .hacker scripts"))
	pterm.Println(whiteStyle.Render("Type 'hackerc help' for available commands."))
	helpCommand(false)
}

func runCommand(file string, verbose bool) bool {
	fullRuntimePath := filepath.Join(os.Getenv("HOME"), HackerDir, BinDir, RuntimePath)
	if _, err := os.Stat(fullRuntimePath); os.IsNotExist(err) {
		pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render("Hacker runtime not found at " + fullRuntimePath + ". Please install the Hacker Lang tools."))
		return false
	}
	args := []string{file}
	if verbose {
		args = append(args, "--verbose")
	}
	pterm.Info.WithPrefix(pterm.Prefix{Text: "INFO", Style: pterm.NewStyle(pterm.FgCyan)}).Println(cyanStyle.Render("Executing script: " + file + func() string {
		if verbose {
			return " (verbose mode)"
		}
		return ""
	}()))
	cmd := exec.Command(fullRuntimePath, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	if err != nil {
		pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render(fmt.Sprintf("Execution failed with error: %v", err)))
		return false
	}
	pterm.Success.WithPrefix(pterm.Prefix{Text: "SUCCESS", Style: pterm.NewStyle(pterm.FgGreen)}).Println(greenStyle.Render("Execution completed successfully."))
	return true
}

func compileCommand(file string, output string, verbose bool) bool {
	fullCompilerPath := filepath.Join(os.Getenv("HOME"), HackerDir, BinDir, CompilerPath)
	if _, err := os.Stat(fullCompilerPath); os.IsNotExist(err) {
		pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render("Hacker compiler not found at " + fullCompilerPath + ". Please install the Hacker Lang tools."))
		return false
	}
	args := []string{file, output}
	if verbose {
		args = append(args, "--verbose")
	}
	pterm.Info.WithPrefix(pterm.Prefix{Text: "INFO", Style: pterm.NewStyle(pterm.FgCyan)}).Println(cyanStyle.Render("Compiling script: " + file + " to " + output + func() string {
		if verbose {
			return " (verbose mode)"
		}
		return ""
	}()))
	cmd := exec.Command(fullCompilerPath, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err := cmd.Run()
	if err != nil {
		pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render(fmt.Sprintf("Compilation failed with error: %v", err)))
		return false
	}
	pterm.Success.WithPrefix(pterm.Prefix{Text: "SUCCESS", Style: pterm.NewStyle(pterm.FgGreen)}).Println(greenStyle.Render("Compilation completed successfully."))
	return true
}

func helpCommand(showBanner bool) bool {
	if showBanner {
		header := pterm.DefaultHeader.WithFullWidth().WithBackgroundStyle(pterm.NewStyle(pterm.BgMagenta)).WithTextStyle(pterm.NewStyle(pterm.FgWhite))
		header.Println("Hacker Lang CLI - Simplified Scripting Tool v" + Version)
	}
	pterm.DefaultSection.WithStyle(pterm.NewStyle(pterm.FgBlue)).Println(blueStyle.Render("Available Commands:"))
	tableData := pterm.TableData{
		{lightGrayStyle.Render("Command"), lightGrayStyle.Render("Description"), lightGrayStyle.Render("Usage")},
		{cyanStyle.Render("run"), whiteStyle.Render("Execute a .hacker script"), yellowStyle.Render("hackerc run <file> [--verbose]")},
		{cyanStyle.Render("compile"), whiteStyle.Render("Compile to native executable"), yellowStyle.Render("hackerc compile <file> [-o output] [--verbose]")},
		{cyanStyle.Render("help"), whiteStyle.Render("Show this help menu"), yellowStyle.Render("hackerc help")},
	}
	pterm.DefaultTable.WithHasHeader().WithBoxed(true).WithData(tableData).Render()
	pterm.Println()
	pterm.DefaultSection.WithStyle(pterm.NewStyle(pterm.FgGray)).Println(grayStyle.Render("Global options:"))
	pterm.DefaultBulletList.WithItems([]pterm.BulletListItem{
		{Level: 0, Text: magentaStyle.Render("-v, --version Display version")},
					  {Level: 0, Text: magentaStyle.Render("-h, --help Display help")},
	}).Render()
	return true
}

func versionCommand() bool {
	pterm.Info.WithPrefix(pterm.Prefix{Text: "INFO", Style: pterm.NewStyle(pterm.FgCyan)}).Println(cyanStyle.Render("Hacker Lang CLI v" + Version))
	return true
}

func main() {
	if err := ensureHackerDir(); err != nil {
		pterm.Fatal.WithPrefix(pterm.Prefix{Text: "FATAL", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render("Failed to create hacker directory: " + err.Error()))
	}
	if len(os.Args) == 1 {
		displayWelcome()
		os.Exit(0)
	}
	// Global flags
	globalVersion := flag.Bool("v", false, "Display version")
	globalVersionLong := flag.Bool("version", false, "Display version")
	globalHelp := flag.Bool("h", false, "Display help")
	globalHelpLong := flag.Bool("help", false, "Display help")
	flag.Parse()
	if *globalVersion || *globalVersionLong {
		versionCommand()
		os.Exit(0)
	}
	if *globalHelp || *globalHelpLong {
		helpCommand(true)
		os.Exit(0)
	}
	args := os.Args[1:]
	if len(args) == 0 {
		displayWelcome()
		os.Exit(0)
	}
	command := args[0]
	args = args[1:]
	success := true
	switch command {
		case "run":
			var file string
			var verbose bool
			runFlags := flag.NewFlagSet("run", flag.ExitOnError)
			runFlags.BoolVar(&verbose, "verbose", false, "Enable verbose output")
			runFlags.Usage = func() {
				pterm.Println(boldStyle.Render("Usage:") + " hackerc run <file> [options]\n\nExecute a .hacker script.")
				runFlags.PrintDefaults()
			}
			if err := runFlags.Parse(args); err != nil {
				pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render("Error parsing flags: " + err.Error()))
				os.Exit(1)
			}
			remaining := runFlags.Args()
			if len(remaining) != 1 {
				pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render("Error: Expected exactly one argument: <file>"))
				runFlags.Usage()
				os.Exit(1)
			}
			file = remaining[0]
			success = runCommand(file, verbose)
		case "compile":
			var file, output string
			var verbose bool
			compileFlags := flag.NewFlagSet("compile", flag.ExitOnError)
			compileFlags.StringVar(&output, "o", "", "Specify output file")
			compileFlags.StringVar(&output, "output", "", "Specify output file")
			compileFlags.BoolVar(&verbose, "verbose", false, "Enable verbose output")
			compileFlags.Usage = func() {
				pterm.Println(boldStyle.Render("Usage:") + " hackerc compile <file> [options]\n\nCompile to native executable.")
				compileFlags.PrintDefaults()
			}
			if err := compileFlags.Parse(args); err != nil {
				pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render("Error parsing flags: " + err.Error()))
				os.Exit(1)
			}
			remaining := compileFlags.Args()
			if len(remaining) != 1 {
				pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render("Error: Expected exactly one argument: <file>"))
				compileFlags.Usage()
				os.Exit(1)
			}
			file = remaining[0]
			if output == "" {
				ext := filepath.Ext(file)
				output = strings.TrimSuffix(file, ext)
			}
			success = compileCommand(file, output, verbose)
		case "help":
			success = helpCommand(true)
		default:
			pterm.Error.WithPrefix(pterm.Prefix{Text: "ERROR", Style: pterm.NewStyle(pterm.FgRed)}).Println(redStyle.Render("Unknown command: " + command))
			helpCommand(false)
			success = false
	}
	if success {
		os.Exit(0)
	} else {
		os.Exit(1)
	}
}
