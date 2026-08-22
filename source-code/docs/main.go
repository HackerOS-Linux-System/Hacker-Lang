package main

import (
	"flag"
	"fmt"
	"os"

	tea "github.com/charmbracelet/bubbletea"

	"hl-docs/src"
)

// docsVersion śledzi zawartość hl-docs niezależnie od wersji samego Hacker
// Lang (HL_VERSION/HL_GEN w interpreterze) — bumpowany przy każdym większym
// rozszerzeniu treści. 1.1.0: kategoria JIT. 1.1.1: dispatch O(1) +
// ponawianie prób. 1.1.2: wątkowanie skoków (compiler) + ponowne użycie
// kontekstu Cranelift (jit). 1.2.0: trace linking, pomijanie zbędnych
// load'ów (dowiedzione bezpieczne dla pojedynczego bloku wejściowego) i
// ograniczony (jeden poziom, jedno miejsce wywołania) inlining CallFunc w
// whole-function JIT.
const docsVersion = "1.2.0"

func main() {
	showVersion := flag.Bool("version", false, "wypisz wersję hl-docs i wyjdź")
	flag.BoolVar(showVersion, "v", false, "skrót dla -version")
	flag.Parse()

	if *showVersion {
		fmt.Printf("hl-docs %s\n", docsVersion)
		return
	}

	opts := []tea.ProgramOption{tea.WithAltScreen(), tea.WithMouseCellMotion()}
	if os.Getenv("HL_DOCS_NO_MOUSE") != "" {
		// Niektóre terminale (zwłaszcza przez SSH/tmux bez odpowiedniej
		// konfiguracji raportowania myszy) źle sobie radzą z trybem mouse
		// cell motion — pozwól to wyłączyć bez przebudowy binarki, tak jak
		// HL_NO_JIT/HL_NOT_ON_HACKEROS pozwalają dostroić resztę Hacker Lang.
		opts = []tea.ProgramOption{tea.WithAltScreen()}
	}

	p := tea.NewProgram(src.InitialModel(), opts...)
	if _, err := p.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "hl-docs error: %v\n", err)
		os.Exit(1)
	}
}
