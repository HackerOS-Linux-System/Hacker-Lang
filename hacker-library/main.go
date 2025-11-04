package main

import (
	"fmt"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func main() {
	args := os.Args[1:]
	if len(args) == 0 {
		usage()
	}
	action := args[0]
	home := os.Getenv("HOME")
	if home == "" {
		fmt.Println("Error: HOME environment variable not set")
		os.Exit(1)
	}
	libDir := filepath.Join(home, ".hackeros", "hacker-lang", "libs")
	err := os.MkdirAll(libDir, 0755)
	if err != nil {
		fmt.Printf("Error creating library directory: %v\n", err)
		os.Exit(1)
	}
	packageListUrl := "https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/bytes.io"
	tmpList := "/tmp/bytes.io"

	downloadList := func() error {
		cmd := exec.Command("curl", "-s", "-o", tmpList, packageListUrl)
		return cmd.Run()
	}

	parsePackages := func() map[string]string {
		packages := make(map[string]string)
		data, err := os.ReadFile(tmpList)
		if err != nil {
			fmt.Printf("Error reading package list: %v\n", err)
			os.Exit(1)
		}
		lines := strings.Split(string(data), "\n")
		for _, line := range lines {
			trimmed := strings.TrimSpace(line)
			if strings.Contains(trimmed, "=>") {
				parts := strings.SplitN(trimmed, "=>", 2)
				if len(parts) == 2 {
					name := strings.TrimSpace(parts[0])
					u := strings.TrimSpace(parts[1])
					if name != "" && u != "" {
						packages[name] = u
					}
				}
			}
		}
		return packages
	}

	getPackages := func() map[string]string {
		err := downloadList()
		if err != nil {
			fmt.Printf("Error downloading package list: %v\n", err)
			os.Exit(1)
		}
		return parsePackages()
	}

	getFilename := func(urlStr string) string {
		u, err := url.Parse(urlStr)
		if err != nil {
			fmt.Printf("Error parsing URL: %v\n", err)
			os.Exit(1)
		}
		return filepath.Base(u.Path)
	}

	getInstalled := func(packages map[string]string, libDir string) []string {
		var installed []string
		for name, urlStr := range packages {
			filename := getFilename(urlStr)
			libPath := filepath.Join(libDir, filename)
			if _, err := os.Stat(libPath); err == nil {
				installed = append(installed, name)
			}
		}
		return installed
	}

	if action == "list" {
		fmt.Println("Fetching available libraries...")
		packages := getPackages()
		fmt.Println("Available libraries:")
		for name := range packages {
			fmt.Printf("- %s\n", name)
		}
		fmt.Println("\nInstalled libraries:")
		installed := getInstalled(packages, libDir)
		for _, name := range installed {
			fmt.Printf("- %s\n", name)
		}
	} else if action == "install" {
		if len(args) < 2 {
			usage()
		}
		libname := args[1]
		packages := getPackages()
		urlStr, ok := packages[libname]
		if !ok {
			fmt.Printf("Library %s not found in package list.\n", libname)
			os.Exit(1)
		}
		filename := getFilename(urlStr)
		libPath := filepath.Join(libDir, filename)
		tmpPath := filepath.Join("/tmp", filename)
		if _, err := os.Stat(libPath); err == nil {
			fmt.Printf("Removing existing %s...\n", libname)
			err := os.Remove(libPath)
			if err != nil {
				fmt.Printf("Error removing existing library: %v\n", err)
				os.Exit(1)
			}
		}
		fmt.Printf("Installing %s from %s...\n", libname, urlStr)
		cmd := exec.Command("curl", "-L", "-o", tmpPath, urlStr)
		err = cmd.Run()
		if err != nil {
			fmt.Printf("Error downloading library: %v\n", err)
			os.Exit(1)
		}
		err = os.Rename(tmpPath, libPath)
		if err != nil {
			fmt.Printf("Error moving library: %v\n", err)
			os.Exit(1)
		}
		cmd = exec.Command("chmod", "+x", libPath)
		err = cmd.Run()
		if err != nil {
			fmt.Printf("Warning: Error making library executable: %v\n", err)
		}
		fmt.Printf("Installed %s to %s\n", libname, libPath)
	} else if action == "update" {
		fmt.Println("Checking for library updates...")
		packages := getPackages()
		installed := getInstalled(packages, libDir)
		for _, lib := range installed {
			urlStr := packages[lib]
			filename := getFilename(urlStr)
			libPath := filepath.Join(libDir, filename)
			tmpPath := filepath.Join("/tmp", filename)
			fmt.Printf("Updating %s...\n", lib)
			err := os.Remove(libPath)
			if err != nil {
				fmt.Printf("Error removing old library: %v\n", err)
				continue
			}
			cmd := exec.Command("curl", "-L", "-o", tmpPath, urlStr)
			err = cmd.Run()
			if err != nil {
				fmt.Printf("Error downloading update: %v\n", err)
				continue
			}
			err = os.Rename(tmpPath, libPath)
			if err != nil {
				fmt.Printf("Error moving update: %v\n", err)
				continue
			}
			cmd = exec.Command("chmod", "+x", libPath)
			err = cmd.Run()
			if err != nil {
				fmt.Printf("Warning: Error making library executable: %v\n", err)
			}
			fmt.Printf("%s updated\n", lib)
		}
	} else {
		usage()
	}
}

func usage() {
	fmt.Println("Usage: hacker-library [list|install|update] [libname]")
	os.Exit(1)
}
