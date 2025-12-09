package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"
)

const VERSION = "1.1" // Zaktualizowana wersja po zmianach

const HACKER_DIR = "~/.hackeros/hacker-lang"

const BIN_DIR = HACKER_DIR + "/bin"

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

func runCommand(file string, verbose bool) bool {
	parserPath := expandHome(BIN_DIR + "/hacker-plsa")
	cmd := exec.Command(parserPath, file)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}
	output, err := cmd.Output()
	if err != nil {
		fmt.Printf("Error parsing file: %v\n", err)
		return false
	}
	var parsed struct {
		Deps         []string                  `json:"deps"`
		Libs         []string                  `json:"libs"`
		Vars         map[string]string         `json:"vars"`
		LocalVars    map[string]string         `json:"local_vars"`
		Cmds         []string                  `json:"cmds"`
		CmdsWithVars []string                  `json:"cmds_with_vars"`
		CmdsSeparate []string                  `json:"cmds_separate"`
		Includes     []string                  `json:"includes"`
		Binaries     []string                  `json:"binaries"`
		Errors       []string                  `json:"errors"`
		Config       map[string]string         `json:"config"`
		Plugins      []map[string]interface{}  `json:"plugins"` // Adjusted for JSON structure
	}
	if err := json.Unmarshal(output, &parsed); err != nil {
		fmt.Printf("Error unmarshaling parse output: %v\n", err)
		return false
	}
	if len(parsed.Config) == 0 {
		configFile := ".hacker-config"
		if _, err := os.Stat(configFile); err == nil {
			content, err := os.ReadFile(configFile)
			if err == nil {
				lines := strings.Split(string(content), "\n")
				for _, line := range lines {
					line = strings.TrimSpace(line)
					if line == "" || strings.HasPrefix(line, "!") {
						continue
					}
					parts := strings.SplitN(line, "=", 2)
					if len(parts) == 2 {
						key := strings.TrimSpace(parts[0])
						value := strings.TrimSpace(parts[1])
						parsed.Config[key] = value
					}
				}
			}
		}
	}
	if len(parsed.Errors) > 0 {
		fmt.Println("\nErrors:")
		for _, e := range parsed.Errors {
			fmt.Println(" " + colorRed + "✖ " + colorReset + e)
		}
		fmt.Println()
		return false
	}
	// Removed lib warning

	// Main temp script
	tempSh, err := os.CreateTemp("", "*.sh")
	if err != nil {
		fmt.Printf("Error creating temp file: %v\n", err)
		return false
	}
	defer os.Remove(tempSh.Name())
	tempSh.WriteString("#!/bin/bash\n")
	tempSh.WriteString("set -e\n")
	for k, v := range parsed.Vars {
		tempSh.WriteString(fmt.Sprintf("export %s=\"%s\"\n", k, v))
	}
	for k, v := range parsed.LocalVars {
		tempSh.WriteString(fmt.Sprintf("%s=\"%s\"\n", k, v))
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
			fmt.Printf("Error reading include: %v\n", err)
			return false
		}
		tempSh.Write(libContent)
		tempSh.WriteString("\n")
	}
	for _, cmd := range parsed.Cmds {
		tempSh.WriteString(cmd + "\n")
	}
	for _, cmd := range parsed.CmdsWithVars {
		tempSh.WriteString(cmd + "\n")
	}
	for _, bin := range parsed.Binaries {
		tempSh.WriteString(bin + "\n")
	}
	for _, plugin := range parsed.Plugins {
		path := plugin["path"].(string)
		isSuper := plugin["super"].(bool)
		cmdStr := path + " &"
		if isSuper {
			cmdStr = "sudo " + cmdStr
		}
		tempSh.WriteString(cmdStr + "\n")
	}
	tempSh.Close()
	os.Chmod(tempSh.Name(), 0755)
	// Separate scripts for cmds_separate
	var separateTemps []string
	for i, sepCmd := range parsed.CmdsSeparate {
		sepTemp, err := os.CreateTemp("", fmt.Sprintf("sep_%d_*.sh", i))
		if err != nil {
			fmt.Printf("Error creating separate temp file: %v\n", err)
			return false
		}
		defer os.Remove(sepTemp.Name())
		sepTemp.WriteString("#!/bin/bash\n")
		sepTemp.WriteString("set -e\n")
		for k, v := range parsed.Vars {
			sepTemp.WriteString(fmt.Sprintf("export %s=\"%s\"\n", k, v))
		}
		for k, v := range parsed.LocalVars {
			sepTemp.WriteString(fmt.Sprintf("%s=\"%s\"\n", k, v))
		}
		sepTemp.WriteString(sepCmd + "\n")
		sepTemp.Close()
		os.Chmod(sepTemp.Name(), 0755)
		separateTemps = append(separateTemps, sepTemp.Name())
	}
	fmt.Printf("Executing script: %s\n", file)
	fmt.Printf("Config: %v\n", parsed.Config)
	fmt.Println("Running...")
	// Removed progress bar

	// Run separate scripts
	for _, sepPath := range separateTemps {
		runSep := exec.Command("bash", sepPath)
		runSep.Env = os.Environ()
		for k, v := range parsed.Vars {
			runSep.Env = append(runSep.Env, fmt.Sprintf("%s=%s", k, v))
		}
		runSep.Stdout = os.Stdout
		runSep.Stderr = os.Stderr
		err = runSep.Run()
		if err != nil {
			fmt.Printf("Separate command execution failed: %v\n", err)
			return false
		}
	}
	// Run main script
	runCmd := exec.Command("bash", tempSh.Name())
	runCmd.Env = os.Environ()
	for k, v := range parsed.Vars {
		runCmd.Env = append(runCmd.Env, fmt.Sprintf("%s=%s", k, v))
	}
	runCmd.Stdout = os.Stdout
	runCmd.Stderr = os.Stderr
	err = runCmd.Run()
	if err != nil {
		fmt.Printf("Execution failed: %v\n", err)
		return false
	}
	fmt.Println("Execution completed successfully!")
	return true
}

func main() {
	ensureHackerDir()
	args := os.Args[1:]
	if len(args) == 0 {
		fmt.Println("Usage: hacker-runtime <file> [--verbose]")
		os.Exit(1)
	}
	verbose := false
	file := args[0]
	if len(args) > 1 && args[1] == "--verbose" {
		verbose = true
	}
	success := runCommand(file, verbose)
	if success {
		os.Exit(0)
	} else {
		os.Exit(1)
	}
}
