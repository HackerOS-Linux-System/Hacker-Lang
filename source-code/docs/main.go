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
// whole-function JIT. 1.2.1: tryb myszy domyślnie WYŁĄCZONY (patrz niżej) —
// naprawia klawisze strzałek nie reagujące w ogóle w wielu terminalach
// (kontenery, SSH bez negocjacji trybu myszy).
const docsVersion = "1.2.1"

func main() {
	showVersion := flag.Bool("version", false, "wypisz wersję hl-docs i wyjdź")
	flag.BoolVar(showVersion, "v", false, "skrót dla -version")
	flag.Parse()

	if *showVersion {
		fmt.Printf("hl-docs %s\n", docsVersion)
		return
	}

	// Mysz domyślnie WYŁĄCZONA (odwrócone względem wcześniejszej wersji).
	// tea.WithMouseCellMotion() potrafi rozregulować CAŁY strumień wejścia
	// (nie tylko obsługę myszy) w terminalach, które nie negocjują trybu
	// raportowania myszy poprawnie — typowe dla kontenerów Docker, wielu
	// konfiguracji SSH/tmux i konsoli bez pełnej emulacji xterm. Objaw:
	// aplikacja w ogóle nie reaguje na klawisze strzałek (ani nic innego),
	// bo bubbletea czeka na dane wejściowe w formacie, którego terminal
	// nigdy nie wyśle. hl-docs jest z założenia nawigowany klawiaturą
	// (strzałki/j-k/enter/tab) — mysz to czysty dodatek, więc bezpieczny
	// domyślny wybór to "wyłączona", z jawnym opt-in dla terminali, które
	// obsługują ją poprawnie.
	opts := []tea.ProgramOption{tea.WithAltScreen()}
	if os.Getenv("HL_DOCS_MOUSE") != "" {
		opts = append(opts, tea.WithMouseCellMotion())
	}

	p := tea.NewProgram(src.InitialModel(), opts...)
	if _, err := p.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "hl-docs error: %v\n", err)
		os.Exit(1)
	}
}
