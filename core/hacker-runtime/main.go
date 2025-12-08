package main

import (
	"bufio"
	"bytes"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

const (
	reset  = "\x1b[0m"
	red    = "\x1b[31m"
	green  = "\x1b[32m"
	yellow = "\x1b[33m"
	blue   = "\x1b[34m"
	purple = "\x1b[35m"
	cyan   = "\x1b[36m"
	white  = "\x1b[37m"
	bold   = "\x1b[1m"
	gray   = "\x1b[90m"
	VERSION = "1.2"
	HACKER_DIR = "~/.hackeros/hacker-lang"
	BIN_DIR    = HACKER_DIR + "/bin"
)

type Plugin struct {
	Path    string
	IsSuper bool
}

type ParseResult struct {
	Deps         []string
	Libs         []string
	Vars         map[string]string
	LocalVars    map[string]string
	Cmds         []string
	CmdsWithVars []string
	CmdsSeparate []string
	Includes     []string
	Binaries     []string
	Plugins      []Plugin
	Errors       []string
	Config       map[string]string
}

func expandHome(path string) (string, error) {
	if !strings.HasPrefix(path, "~") {
		return path, nil
	}
	home := os.Getenv("HOME")
	if home == "" {
		return "", fmt.Errorf("home not found")
	}
	return home + path[1:], nil
}

func ensureHackerDir() error {
	hackerDir, err := expandHome(HACKER_DIR)
	if err != nil {
		return err
	}
	binDir := filepath.Join(hackerDir, "bin")
	if err := os.MkdirAll(binDir, 0755); err != nil {
		return err
	}
	libsDir := filepath.Join(hackerDir, "libs")
	if err := os.MkdirAll(libsDir, 0755); err != nil {
		return err
	}
	return nil
}

func printColored(color, format string, args ...interface{}) {
	fmt.Printf(color+format+reset, args...)
}

func runParser(file string, verbose bool) (*ParseResult, error) {
	binDir, err := expandHome(BIN_DIR)
	if err != nil {
		return nil, err
	}
	parserPath := filepath.Join(binDir, "hacker-parser")
	args := []string{parserPath, file}
	if verbose {
		args = append(args, "--verbose")
	}
	cmd := exec.Command(args[0], args[1:]...)
	cmd.Stdin = os.Stdin
	cmd.Stderr = os.Stderr
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	output, err := io.ReadAll(stdout)
	if err != nil {
		return nil, err
	}
	if err := cmd.Wait(); err != nil {
		printColored(red, "Parser failed or exited with error\n")
		return nil, fmt.Errorf("parser failed")
	}
	var root map[string]interface{}
	if err := json.Unmarshal(output, &root); err != nil {
		return nil, err
	}
	result := &ParseResult{
		Deps:         []string{},
		Libs:         []string{},
		Vars:         make(map[string]string),
		LocalVars:    make(map[string]string),
		Cmds:         []string{},
		CmdsWithVars: []string{},
		CmdsSeparate: []string{},
		Includes:     []string{},
		Binaries:     []string{},
		Plugins:      []Plugin{},
		Errors:       []string{},
		Config:       make(map[string]string),
	}
	// Errors
	if errs, ok := root["errors"].([]interface{}); ok {
		for _, e := range errs {
			if s, ok := e.(string); ok {
				result.Errors = append(result.Errors, s)
			}
		}
	}
	if len(result.Errors) > 0 {
		printColored(red, "\nErrors:\n")
		for _, e := range result.Errors {
			printColored(red, " ✖ %s\n", e)
		}
		return nil, fmt.Errorf("errors found")
	}
	// Simple arrays
	arrayKeys := []string{"deps", "libs", "cmds", "cmds_with_vars", "cmds_separate", "includes", "binaries"}
	arrayPtrs := []*[]string{&result.Deps, &result.Libs, &result.Cmds, &result.CmdsWithVars, &result.CmdsSeparate, &result.Includes, &result.Binaries}
	for i, k := range arrayKeys {
		if arr, ok := root[k].([]interface{}); ok {
			for _, v := range arr {
				if s, ok := v.(string); ok {
					*arrayPtrs[i] = append(*arrayPtrs[i], s)
				}
			}
		}
	}
	// HashMaps
	hashKeys := []string{"vars", "local_vars", "config"}
	hashPtrs := []map[string]string{result.Vars, result.LocalVars, result.Config}
	for i, k := range hashKeys {
		if obj, ok := root[k].(map[string]interface{}); ok {
			for key, val := range obj {
				if s, ok := val.(string); ok {
					hashPtrs[i][key] = s
				}
			}
		}
	}
	// Plugins
	if plugs, ok := root["plugins"].([]interface{}); ok {
		for _, p := range plugs {
			if po, ok := p.(map[string]interface{}); ok {
				if path, ok := po["path"].(string); ok {
					isSuper := false
					if s, ok := po["is_super"].(bool); ok {
						isSuper = s
					}
					result.Plugins = append(result.Plugins, Plugin{Path: path, IsSuper: isSuper})
				}
			}
		}
	}
	// Fallback to .hacker-config if config empty
	if len(result.Config) == 0 {
		configPath := ".hacker-config"
		f, err := os.Open(configPath)
		if err == nil {
			defer f.Close()
			scanner := bufio.NewScanner(f)
			for scanner.Scan() {
				line := strings.TrimSpace(scanner.Text())
				if len(line) == 0 || line[0] == '#' || line[0] == '!' {
					continue
				}
				parts := strings.SplitN(line, "=", 2)
				if len(parts) == 2 {
					key := strings.TrimSpace(parts[0])
					val := strings.TrimSpace(parts[1])
					if key != "" && val != "" {
						result.Config[key] = val
					}
				}
			}
		}
	}
	// Warning for libs
	if len(result.Libs) > 0 {
		printColored(yellow, "Warning: Missing custom libs: ")
		for _, lib := range result.Libs {
			printColored(yellow, "%s ", lib)
		}
		printColored(yellow, "\nPlease install them using `bytes install <lib>`\n")
	}
	return result, nil
}

func createTempScript(content string) (string, error) {
	var b [12]byte
	_, err := rand.Read(b[:])
	if err != nil {
		return "", err
	}
	urlEncoder := base64.RawURLEncoding
	namePart := urlEncoder.EncodeToString(b[:])
	path := "/tmp/hacker_" + namePart + ".sh"
	file, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0755)
	if err != nil {
		return "", err
	}
	defer file.Close()
	if _, err := file.WriteString("#!/bin/bash\nset -e\n"); err != nil {
		return "", err
	}
	if _, err := file.WriteString(content); err != nil {
		return "", err
	}
	return path, nil
}

func appendEnv(content *bytes.Buffer, parsed *ParseResult) {
	for k, v := range parsed.Vars {
		content.WriteString(fmt.Sprintf("export %s=\"%s\"\n", k, v))
	}
	for k, v := range parsed.LocalVars {
		content.WriteString(fmt.Sprintf("%s=\"%s\"\n", k, v))
	}
}

func getEnvMap() map[string]string {
	m := make(map[string]string)
	for _, e := range os.Environ() {
		if i := strings.Index(e, "="); i >= 0 {
			m[e[:i]] = e[i+1:]
		}
	}
	return m
}

func runCommand(file string, verbose bool) bool {
	parsed, err := runParser(file, verbose)
	if err != nil {
		return false
	}
	var mainContent bytes.Buffer
	mainContent.WriteString("#!/bin/bash\nset -e\n")
	appendEnv(&mainContent, parsed)
	for _, dep := range parsed.Deps {
		if dep != "sudo" {
			line := fmt.Sprintf("command -v %s >/dev/null 2>&1 || (sudo apt update && sudo apt install -y %s)\n", dep, dep)
			mainContent.WriteString(line)
		}
	}
	for _, inc := range parsed.Includes {
		line := fmt.Sprintf("# Included from %s\n", inc)
		mainContent.WriteString(line)
	}
	for _, cmd := range parsed.Cmds {
		mainContent.WriteString(cmd + "\n")
	}
	for _, cmd := range parsed.CmdsWithVars {
		mainContent.WriteString(cmd + "\n")
	}
	for _, bin := range parsed.Binaries {
		mainContent.WriteString(bin + "\n")
	}
	for _, p := range parsed.Plugins {
		if p.IsSuper {
			mainContent.WriteString("sudo ")
		}
		mainContent.WriteString(p.Path + " &\n")
	}
	mainScriptPath, err := createTempScript(mainContent.String())
	if err != nil {
		printColored(red, "Failed to create main temp script\n")
		return false
	}
	defer os.Remove(mainScriptPath)
	separatePaths := []string{}
	for _, cmdStr := range parsed.CmdsSeparate {
		var sepContent bytes.Buffer
		sepContent.WriteString("#!/bin/bash\nset -e\n")
		appendEnv(&sepContent, parsed)
		sepContent.WriteString(cmdStr + "\n")
		path, err := createTempScript(sepContent.String())
		if err != nil {
			printColored(red, "Failed to create separate temp script\n")
			return false
		}
		separatePaths = append(separatePaths, path)
		defer os.Remove(path)
	}
	var envList []string
	if len(parsed.Vars) > 0 {
		envMap := getEnvMap()
		for k, v := range parsed.Vars {
			envMap[k] = v
		}
		for k, v := range envMap {
			envList = append(envList, k+"="+v)
		}
	}
	printColored(cyan, "Executing script: %s\n", file)
	printColored(cyan, "Config: ")
	for k, v := range parsed.Config {
		printColored(cyan, "%s=%s ", k, v)
	}
	printColored(cyan, "\n")
	printColored(green, "Running...\n")
	for _, path := range separatePaths {
		cmd := exec.Command("bash", path)
		cmd.Env = envList
		cmd.Stdin = os.Stdin
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		_ = cmd.Run() // Ignore exit code to match Zig behavior
	}
	mainCmd := exec.Command("bash", mainScriptPath)
	mainCmd.Env = envList
	mainCmd.Stdin = os.Stdin
	mainCmd.Stdout = os.Stdout
	mainCmd.Stderr = os.Stderr
	if err := mainCmd.Run(); err != nil {
		printColored(red, "Execution failed\n")
		return false
	}
	printColored(green, "Execution completed successfully!\n")
	return true
}

func main() {
	if err := ensureHackerDir(); err != nil {
		printColored(red, "Failed to ensure hacker dir: %v\n", err)
		os.Exit(1)
	}
	args := os.Args[1:]
	if len(args) == 0 {
		printColored(red, "Usage: hacker-runtime <file.hacker> [--verbose]\n")
		os.Exit(1)
	}
	file := args[0]
	verbose := false
	if len(args) > 1 {
		if args[1] == "--verbose" {
			verbose = true
		} else {
			printColored(red, "Unknown argument: %s\n", args[1])
			os.Exit(1)
		}
	}
	success := runCommand(file, verbose)
	if success {
		os.Exit(0)
	} else {
		os.Exit(1)
	}
}
