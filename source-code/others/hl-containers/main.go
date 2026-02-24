package main

import (
	"archive/tar"
	"compress/gzip"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/user"
	"path/filepath"
	"syscall"
)

const (
	HackerDirSuffix = ".hackeros/hacker-lang/bin"
	RuntimeBin      = "hl-runtime"
)

func getRuntimePath() (string, error) {
	usr, err := user.Current()
	if err != nil {
		return "", err
	}
	path := filepath.Join(usr.HomeDir, HackerDirSuffix, RuntimeBin)
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return "", fmt.Errorf("runtime binary not found at %s", path)
	}
	return path, nil
}

func pack(sourceFile string, outputFile string, compress bool) {
	fmt.Printf("[*] Packing %s...\n", sourceFile)

	if outputFile == "" {
		outputFile = sourceFile + ".pkg"
		if compress {
			outputFile += ".tar.gz"
		} else {
			outputFile += ".tar"
		}
	}

	outFile, err := os.Create(outputFile)
	if err != nil {
		fmt.Printf("[x] Error creating output file: %v\n", err)
		os.Exit(1)
	}
	defer outFile.Close()

	var writer io.WriteCloser = outFile
	if compress {
		writer = gzip.NewWriter(outFile)
		defer writer.(*gzip.Writer).Close()
	}

	tarWriter := tar.NewWriter(writer)
	defer tarWriter.Close()

	file, err := os.Open(sourceFile)
	if err != nil {
		fmt.Printf("[x] Error opening source file: %v\n", err)
		os.Exit(1)
	}
	defer file.Close()

	stat, _ := file.Stat()
	header, err := tar.FileInfoHeader(stat, stat.Name())
	if err != nil {
		fmt.Printf("[x] Error creating tar header: %v\n", err)
		os.Exit(1)
	}

	if err := tarWriter.WriteHeader(header); err != nil {
		fmt.Printf("[x] Error writing header: %v\n", err)
		os.Exit(1)
	}

	if _, err := io.Copy(tarWriter, file); err != nil {
		fmt.Printf("[x] Error copying file content: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("[+] Package created: %s\n", outputFile)
}

func runIsolated(file string, verbose bool) {
	if verbose {
		fmt.Printf("[*] Preparing isolated environment for %s\n", file)
	}

	runtimePath, err := getRuntimePath()
	if err != nil {
		fmt.Printf("[x] %v\n", err)
		os.Exit(1)
	}

	// For basic isolation, we run in a separate process with a restricted environment
	// and potentially new namespaces if on Linux (CLONE_NEWPID, etc)
	// Note: This is a lightweight containerization wrapper.

	cmd := exec.Command(runtimePath, file)
	if verbose {
		cmd.Args = append(cmd.Args, "--verbose")
	}

	// Minimal environment
	cmd.Env = []string{
		"PATH=/usr/bin:/bin",
		"TERM=" + os.Getenv("TERM"),
		"HOME=/tmp",
	}

	// Linux Namespace Isolation
	cmd.SysProcAttr = &syscall.SysProcAttr{
		Cloneflags: syscall.CLONE_NEWUTS | syscall.CLONE_NEWPID | syscall.CLONE_NEWNS,
	}

	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	if verbose {
		fmt.Println("[*] Launching containerized process...")
	}

	if err := cmd.Run(); err != nil {
		// If namespace creation fails (permission denied), fallback to standard execution with warning
		if verbose {
			fmt.Printf("[!] Namespace creation failed (needs root?): %v. Falling back to restricted env only.\n", err)
		}

		cmd.SysProcAttr = nil
		if errFallback := cmd.Run(); errFallback != nil {
			fmt.Printf("[x] Execution failed: %v\n", errFallback)
			os.Exit(1)
		}
	}
}

func main() {
	if len(os.Args) < 2 {
		fmt.Println("Usage: hl-containers <command> [args]")
		os.Exit(1)
	}

	command := os.Args[1]

	switch command {
		case "run":
			runCmd := flag.NewFlagSet("run", flag.ExitOnError)
			verbose := runCmd.Bool("verbose", false, "Enable verbose logging")
			runCmd.Parse(os.Args[2:])

			if runCmd.NArg() < 1 {
				fmt.Println("Usage: hl-containers run <file>")
				os.Exit(1)
			}
			runIsolated(runCmd.Arg(0), *verbose)

		case "pack":
			packCmd := flag.NewFlagSet("pack", flag.ExitOnError)
			output := packCmd.String("output", "", "Output filename")
			compress := packCmd.Bool("compress", false, "Enable GZIP compression")
			packCmd.Parse(os.Args[2:])

			if packCmd.NArg() < 1 {
				fmt.Println("Usage: hl-containers pack <file>")
				os.Exit(1)
			}
			pack(packCmd.Arg(0), *output, *compress)

		default:
			fmt.Printf("Unknown command: %s\n", command)
			os.Exit(1)
	}
}
