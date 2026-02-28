package main

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

var verbose bool

func main() {
	flag.BoolVar(&verbose, "verbose", false, "włącz tryb szczegółowy")
	flag.Bool("v", false, "włącz tryb szczegółowy (skrót)")
	flag.Parse()

	args := flag.Args()
	if len(args) == 0 {
		printHelp()
		os.Exit(0)
	}

	user := os.Getenv("USER")
	if user == "" {
		user = "user" // fallback
	}
	basePath := filepath.Join("/home", user, ".hackeros", "hacker-lang", "bin")
	cachePath := filepath.Join("/home", user, ".cache", "hacker-lang")

	runtimeBin := filepath.Join(basePath, "hl-runtime")
	compilerBin := filepath.Join(basePath, "hl-compiler")
	plsaBin := filepath.Join(basePath, "hl-plsa")

	cmd := args[0]
	switch cmd {
		case "run":
			if len(args) < 2 {
				fmt.Println("Użycie: hl run <plik.hl>")
				os.Exit(1)
			}
			runCmd(runtimeBin, args[1:])

		case "compile":
			if len(args) < 2 {
				fmt.Println("Użycie: hl compile <plik.hl>")
				os.Exit(1)
			}
			runCmd(compilerBin, args[1:])

		case "clear":
			clearCache(cachePath)

		case "check":
			if len(args) < 2 {
				fmt.Println("Użycie: hl check <plik.hl>")
				os.Exit(1)
			}
			runCmd(plsaBin, args[1:])

		case "info":
			printInfo()

		case "help":
			printHelp()

		default:
			fmt.Printf("Nieznana komenda: %s\n", cmd)
			printHelp()
			os.Exit(1)
	}
}

func runCmd(bin string, args []string) {
	if verbose {
		fmt.Printf("Wywołuję: %s %s\n", bin, strings.Join(args, " "))
	}
	cmd := exec.Command(bin, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	err := cmd.Run()
	if err != nil {
		fmt.Printf("Błąd: %v\n", err)
		os.Exit(1)
	}
}

func clearCache(path string) {
	if verbose {
		fmt.Printf("Usuwam zawartość: %s\n", path)
	}
	files, err := os.ReadDir(path)
	if err != nil {
		fmt.Printf("Błąd odczytu katalogu: %v\n", err)
		os.Exit(1)
	}
	for _, file := range files {
		err := os.RemoveAll(filepath.Join(path, file.Name()))
		if err != nil {
			fmt.Printf("Błąd usuwania: %v\n", err)
		}
	}
	if verbose {
		fmt.Println("Cache wyczyszczony.")
	}
}

func printInfo() {
	fmt.Println("hl wrapper version 1.0.0")
	fmt.Println("Ścieżka binarek: ~/.hackeros/hacker-lang/bin")
}

func printHelp() {
	fmt.Println("Użycie: hl <komenda> [opcje]")
	fmt.Println("Komendy:")
	fmt.Println("  run <plik.hl>     - uruchom plik .hl za pomocą hl-runtime")
	fmt.Println("  compile <plik.hl> - skompiluj plik .hl za pomocą hl-compiler")
	fmt.Println("  clear             - usuń zawartość cache hacker-lang")
	fmt.Println("  check <plik.hl>   - sprawdź poprawność pliku za pomocą hl-plsa")
	fmt.Println("  info              - pokaż wersję i informacje")
	fmt.Println("  help              - pokaż tę pomoc")
	fmt.Println("Flagi:")
	fmt.Println("  --verbose         - tryb szczegółowy")
}
