package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

var (
	colorBg       = lipgloss.Color("#0d1117")
	colorPanel    = lipgloss.Color("#161b22")
	colorBorder   = lipgloss.Color("#30363d")
	colorAccent   = lipgloss.Color("#58a6ff")
	colorGreen    = lipgloss.Color("#3fb950")
	colorYellow   = lipgloss.Color("#d29922")
	colorRed      = lipgloss.Color("#f85149")
	colorMagenta  = lipgloss.Color("#bc8cff")
	colorCyan     = lipgloss.Color("#79c0ff")
	colorOrange   = lipgloss.Color("#ffa657")
	colorMuted    = lipgloss.Color("#8b949e")
	colorText     = lipgloss.Color("#e6edf3")
	colorSelected = lipgloss.Color("#1f6feb")

	styleH1 = lipgloss.NewStyle().Foreground(colorMagenta).Bold(true).MarginTop(1).MarginBottom(1)
	styleH2 = lipgloss.NewStyle().Foreground(colorAccent).Bold(true).MarginTop(1)
	styleH3 = lipgloss.NewStyle().Foreground(colorGreen).Bold(true)
	styleOp = lipgloss.NewStyle().Foreground(colorGreen).Bold(true)

	styleCode = lipgloss.NewStyle().Background(lipgloss.Color("#161b22")).Foreground(colorCyan).Padding(0, 1).Margin(0, 2)
	styleTip  = lipgloss.NewStyle().Foreground(colorYellow).Background(lipgloss.Color("#1c1a00")).Padding(0, 1).Margin(0, 2)
	styleWarn = lipgloss.NewStyle().Foreground(colorOrange).Background(lipgloss.Color("#1a1200")).Padding(0, 1).Margin(0, 2)

	styleMenuNormal   = lipgloss.NewStyle().Foreground(colorText).Padding(0, 2)
	styleMenuSelected = lipgloss.NewStyle().Foreground(colorAccent).Background(colorSelected).Bold(true).Padding(0, 2)
	styleMenuCategory = lipgloss.NewStyle().Foreground(colorYellow).Bold(true).Padding(0, 2).MarginTop(1)

	styleSidebar   = lipgloss.NewStyle().Background(colorPanel).BorderStyle(lipgloss.NormalBorder()).BorderRight(true).BorderForeground(colorBorder).Padding(1, 0)
	styleContent   = lipgloss.NewStyle().Background(colorBg).Padding(0, 2)
	styleStatusBar = lipgloss.NewStyle().Background(colorPanel).Foreground(colorMuted).Padding(0, 2)
	styleHeader    = lipgloss.NewStyle().Background(colorPanel).Foreground(colorText).Bold(true).Padding(0, 2)
)

type DocSection struct {
	Title    string
	Category string
	Content  string
}

var sections = []DocSection{
	// ── PODSTAWY ──────────────────────────────────────────────────────────────
	{
		Title:    "Witaj w Hacker Lang",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

Hacker Lang (HL) to interpretowany jezyk skryptowy dla HackerOS.

%s
%s

%s
  %s  Nie ma echo — jest %s
  %s  Sudo to operator %s
  %s  Zmienne: %s | Export: %s
  %s  Tlo: %s | Hsh: %s
  %s  Gen 2: arytmetyka, for-in, while, switch, typed vars

%s

Uzyj strzalek do nawigacji. %s = wyjscie.`,
			styleH1.Render("◆ Hacker Lang — interpreter"),
			styleH2.Render("Szybki start"),
			styleCode.Render("hl run skrypt.hl\nhl repl\nhl exec update-system"),
			styleH2.Render("Kluczowe zasady"),
			"•", styleOp.Render("~>"),
			"•", styleOp.Render("^>"),
			"•", styleOp.Render("%/@"), styleOp.Render("=>"),
			"•", styleOp.Render("&"), styleOp.Render("*>"),
			"•",
			styleTip.Render("💡 Kazda linia zaczyna sie od operatora."),
			styleOp.Render("q"),
		),
	},
	{
		Title:    "System Genow",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

%s
  Gen to wersja funkcji jezyka. Deklaracja w pliku:

%s

%s
  %s  — Podstawowa skladnia (komedy, zmienne, import, goroutines)
  %s  — Typowane zmienne, arytmetyka, for-in, while, switch, HackerOS API
  %s  — Najnowsze funkcje (pre-gen 3), nie wszystkie sa stabilne

%s
  Brak deklaracji = domyslny gen (gen 2).
  Gen 1 jest pelnie wspierany w gen 2.`,
			styleH1.Render("System Genow"),
			styleH2.Render("Deklaracja"),
			styleCode.Render("using <gen 1>      ;; gen 1\nusing <gen 2>      ;; gen 2 (domyslny)\nusing <rolling>    ;; ROLLING — najnowsze"),
			styleH2.Render("Geny"),
			styleOp.Render("gen 1"),
			styleOp.Render("gen 2"),
			styleOp.Render("ROLLING"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "ROLLING — pre-gen 3",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

%s
  ROLLING zawiera wszystkie najnowsze funkcje wprowadzone
  do Hacker Lang, ktore jeszcze nie sa czescia oficjalnego
  gen 3. Uzywaj jesli chcesz testowac nowe mozliwosci.

%s
%s

%s
  Wszystkie funkcje gen 1 i gen 2 + eksperymentalne dodatki.
  API moze sie zmienic przed wydaniem gen 3.

%s

%s
  Nie uzywaj w produkcji jesli zalezy ci na stabilnosci!`,
			styleH1.Render("◆ ROLLING — najnowsze funkcje"),
			styleH2.Render("Czym jest ROLLING?"),
			styleH2.Render("Deklaracja"),
			styleCode.Render("using <rolling>"),
			styleH2.Render("Zawiera"),
			styleTip.Render("💡 ROLLING = gen 2 + wszystko co pojawi sie w gen 3"),
			styleWarn.Render("⚠ ROLLING moze zawierac zmiany lamace kompatybilnosc!"),
		),
	},
	{
		Title:    "Manager pakietow bit",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
  %s  typ hl   — kod Hacker Lang (source-code/main.hl)
  %s  typ rust — kod Rust        (source-code/src/main.rs + Cargo.toml)

%s
  Wywolaj bit bez argumentow = projekt mode (auto-detect).`,
			styleH1.Render("bit — Package Manager"),
			styleH2.Render("Komendy"),
			styleCode.Render("bit install <nazwa>\nbit remove  <nazwa>\nbit list\nbit update\nbit info    <nazwa>\nbit help"),
			styleH2.Render("Uzycie w .hl"),
			styleCode.Render("# <bit/tui>\n# <bit/regex>\n# <bit/json-parser>"),
			styleH2.Render("Format repo-list.json"),
			styleCode.Render("{\n  \"tui\":  { \"url\": \"https://github.com/bit-io/tui.git\",  \"type\": \"hl\" },\n  \"obsidian\": { \"url\": \"...\", \"type\": \"rust\" }\n}"),
			styleH2.Render("Typy pakietow"),
			"•",
			"•",
			styleH2.Render("Uwaga"),
		),
	},

	// ── GEN 1 ─────────────────────────────────────────────────────────────────
	{
		Title:    "Operatory gen 1 — print, cmd",
		Category: "GEN 1",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  %s  — uruchom komende (brak interpolacji @var)
  %s  — uruchom z sudo
  %s  — izolacja namespace (unshare)
  %s  — sudo + izolacja
  %s  — komenda z interpolacja @zmiennych
  %s  — uruchom przez hsh -c`,
			styleH1.Render("Operatory gen 1 — podstawowe"),
			styleH2.Render("~> print"),
			styleCode.Render("~> Hello, world!\n~> Wersja: @HL_VERSION\n~> Uzytkownik: @USER"),
			styleH2.Render("Komendy"),
			styleOp.Render(">"),
			styleOp.Render("^>"),
			styleOp.Render("->"),
			styleOp.Render("^->"),
			styleOp.Render(">>"),
			styleOp.Render("*>"),
		),
	},
	{
		Title:    "Zmienne i export (gen 1)",
		Category: "GEN 1",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
  %s — odwolanie (takze w stringach)
  %s — zmienna z wynikiem komendy`,
			styleH1.Render("Zmienne i export gen 1"),
			styleH2.Render("Zmienne lokalne"),
			styleCode.Render("% target = 192.168.1.1\n% name   = HackerOS\n% count  = 0\n~> Cel: @target"),
			styleH2.Render("Export do srodowiska"),
			styleCode.Render("=> PATH = /usr/local/bin:@PATH\n=> HOME = /root\n=> EDITOR = hedit"),
			styleH2.Render("Referencje"),
			styleOp.Render("@nazwa"),
			styleOp.Render(">> hostname |> @myhost"),
		),
	},
	{
		Title:    "Tlo & i hsh *> (gen 1)",
		Category: "GEN 1",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
  @_bg_pid zawiera PID ostatniego procesu w tle.`,
			styleH1.Render("& background i *> hsh"),
			styleH2.Render("& — uruchom w tle"),
			styleCode.Render("& python3 -m http.server 8080\n& redis-server\n~> PID: @_bg_pid\n\n_3 & wget -q https://example.com/plik"),
			styleH2.Render("*> — uruchom przez hsh -c"),
			styleCode.Render("*> ls -la\n*> notify-send \"Gotowe!\"\n;; *> nie interpoluje @var — uzyj >> do interpolacji"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "Petla _N (gen 1)",
		Category: "GEN 1",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Dziala z komendami, printem, quick-funkcjami.`,
			styleH1.Render("Petla _N — powtorz N razy"),
			styleH2.Render("Przyklad"),
			styleCode.Render("_10 > hacker update\n_5  ~> Powtarzam...\n_3  ::green OK\n_20 ::nl\n\n;; Z interpolacja:\n_10 >> curl -s http://@host/test"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "Import pliku << (gen 1)",
		Category: "GEN 1",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Detail dostepny przez @_import_detail.`,
			styleH1.Render("Import pliku — <<"),
			styleH2.Render("Skladnia"),
			styleCode.Render("<< utils.hl\n<< helpers/network.hl\n<< /usr/share/HackerOS/lib/common.hl\n\n;; Z detalami:\n<< config.hl | produkcja\n<< db.hl | mysql"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "Goroutines i Channels (gen 1)",
		Category: "GEN 1",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("Goroutines i Channels"),
			styleH2.Render(":* — goroutine"),
			styleCode.Render(":*\n    > nmap -sn 192.168.1.0/24\ndone\n\n;; Z nazwa:\n:* scanner def\n    > nmap -sn 192.168.1.0/24\ndone"),
			styleH2.Render(":** / *-- — channel"),
			styleCode.Render(":** wyniki\n\n:*\n    > ls /tmp\n    *-- wyniki\ndone\n\n;; Odbierz:\n*-- wyniki"),
			styleH2.Render("Uwaga"),
			styleCode.Render(";; Goroutines to watki w interpretowanym HL.\n;; Uzyj & (background) dla prostych zadan parallelnych."),
		),
	},
	{
		Title:    "Importy bibliotek (gen 1)",
		Category: "GEN 1",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s`,
			styleH1.Render("Importy bibliotek"),
			styleH2.Render("Przestrzenie nazw"),
			styleCode.Render("# <main/net>          ;; biblioteka standardowa\n# <main/fs>\n# <main/sys>\n# <main/colors>\n# <main/cli>\n# <main/progress-bar>\n# <main/json>\n# <main/hk-parser>     ;; parser .hk\n# <main/hacker>        ;; parser .hacker\n# <bit/tui>            ;; pakiet bit\n# <github/user/repo>   ;; z GitHub"),
			styleH2.Render("Sciezka bibliotek main"),
			styleCode.Render("/usr/lib/HackerOS/Hacker-Lang/main-libs/"),
			styleH2.Render("Kompatybilnosc wstecz — stara skladnia takze dziala"),
		),
	},

	// ── GEN 2 ─────────────────────────────────────────────────────────────────
	{
		Title:    "Typowane zmienne (gen 2)",
		Category: "GEN 2",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  %s — int, float, str, bool, list, map, any (domyslne)`,
			styleH1.Render("Typowane zmienne gen 2"),
			styleH2.Render("Skladnia"),
			styleCode.Render("% count: int   = 42\n% price: float = 9.99\n% name:  str   = \"HackerOS\"\n% flag:  bool  = true\n\n;; Bez adnotacji = Any (jak gen 1):\n% version = 4.6"),
			styleH2.Render("Typy"),
			styleOp.Render("% n: typ = val"),
		),
	},
	{
		Title:    "Arytmetyka $( ) (gen 2)",
		Category: "GEN 2",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Wyrazenie obliczone przez sh lub python3 jako fallback.`,
			styleH1.Render("Arytmetyka natywna gen 2"),
			styleH2.Render("Skladnia"),
			styleCode.Render("$(2 + 2)                    ;; wypisuje 4\n$(10 * @count) -> @result  ;; przypisz do var\n\n% a: int = 10\n% b: int = 3\n$(@a + @b) -> @sum\n~> Suma: @sum"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "Pipe do zmiennej |> (gen 2)",
		Category: "GEN 2",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Przechwytuje stdout komendy do zmiennej HL.`,
			styleH1.Render("Pipe do zmiennej — |>"),
			styleH2.Render("Skladnia"),
			styleCode.Render("> hostname |> @myhost\n~> Host: @myhost\n\n>> curl -s https://ifconfig.me |> @public_ip\n~> IP: @public_ip\n\n^> id -u |> @uid\n~> UID: @uid"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "For-in loop (gen 2)",
		Category: "GEN 2",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Iteruje po slow whitespace-separated w stringu.`,
			styleH1.Render("For-in loop gen 2"),
			styleH2.Render("Skladnia"),
			styleCode.Render("@ item in /usr/bin /usr/local/bin /opt/bin\n    ~> Katalog: @item\n    ::exists @item\n    ? ok\n        ::green Istnieje!\n    done\ndone\n\n;; Z zmienna:\n% dirs = /etc /tmp /var\n@ dir in @dirs\n    ~> > @dir\ndone"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "While loop ?~ (gen 2)",
		Category: "GEN 2",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Max 100 000 iteracji (zabezpieczenie przed petla nieskonczona).`,
			styleH1.Render("While loop gen 2 — ?~"),
			styleH2.Render("Skladnia"),
			styleCode.Render("% running: bool = true\n% i: int = 0\n\n?~ @running == true\n    ~> Iteracja: @i\n    $(@i + 1) -> @i\n    > test \"@i\" = \"5\"\n    ? ok\n        % running = false\n    done\ndone"),
			styleH2.Render("Operatory warunku"),
		),
	},
	{
		Title:    "Switch/case (gen 2)",
		Category: "GEN 2",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Wildcard %s dopasowuje wszystko.`,
			styleH1.Render("Switch/case gen 2"),
			styleH2.Render("Skladnia"),
			styleCode.Render("? switch @os\n| linux\n    ~> Jestem na Linuksie!\n| windows\n    ~> Hmm, Windows?\n| *\n    ~> Nieznany system: @os\ndone\n\n;; Wartosci tekstowe:\n? switch @cmd\n| install\n    -- do_install\n| remove\n    -- do_remove\n| *\n    -- show_help\ndone"),
			styleH2.Render("Uwaga"),
			styleOp.Render("| *"),
		),
	},
	{
		Title:    "HackerOS API || (gen 2)",
		Category: "GEN 2",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Sprawdza czy narzedzie jest zainstalowane (which).`,
			styleH1.Render("HackerOS API — ||"),
			styleH2.Render("Dostepne narzedzia"),
			styleCode.Render("|| hacker update\n|| hco install gimp\n|| lpm install vlc\n|| hsh ls -la\n|| hpkg search kernel\n|| H# file.txt\n|| hedit config.conf\n|| hdev run projekt"),
			styleH2.Render("Uwaga"),
		),
	},

	// ── QUICK FUNCTIONS ───────────────────────────────────────────────────────
	{
		Title:    "Quick Functions :: — lista",
		Category: "QUICK FUNCTIONS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  %s  upper lower len trim rev repeat replace
  %s  contains startswith endswith split lines words
  %s  abs ceil floor round max min rand
  %s  env date time pid which
  %s  exists isdir isfile basename dirname read
  %s  set get type unset
  %s  nl hr bold red green yellow cyan`,
			styleH1.Render("Quick Functions ::"),
			styleH2.Render("Uzycie"),
			styleCode.Render("::upper hello world\n;; HELLO WORLD\n\n::exists /etc/passwd\n? ok\n    ::green Plik istnieje!\ndone\n\n::hr 40\n::bold Naglowek\n::hr 40"),
			styleH2.Render("Pelna lista"),
			styleH3.Render("String:"),
			styleH3.Render("String test:"),
			styleH3.Render("Math:"),
			styleH3.Render("System:"),
			styleH3.Render("Plik:"),
			styleH3.Render("Env:"),
			styleH3.Render("UI:"),
		),
	},

	// ── ROLLING ───────────────────────────────────────────────────────────────
	{
		Title:    "ROLLING — co nowego",
		Category: "ROLLING",
		Content: fmt.Sprintf(`%s

%s
  ROLLING to kanał najnowszych funkcji Hacker Lang,
  ktore wejda do gen 3. Uzywaj %s.

%s
  • Wszystkie funkcje gen 1 i gen 2
  • Eksperymentalne rozszerzenia skladni
  • Nowe quick-functions przed oficjalnym wydaniem
  • Rozszerzona integracja z HackerOS API

%s
%s

%s
  Nowe funkcje beda tu dokumentowane na biezaco.`,
			styleH1.Render("◆ ROLLING — kanał pre-gen 3"),
			styleH2.Render("Opis"),
			styleOp.Render("using <rolling>"),
			styleH2.Render("Zawiera"),
			styleH2.Render("Deklaracja"),
			styleCode.Render("#!/usr/bin/env hl\nusing <rolling>\n\n;; Masz dostep do wszystkich funkcji gen 1 + gen 2\n;; + najnowszych eksperymentalnych dodatków"),
			styleH2.Render("Status"),
		),
	},
	{
		Title:    "ROLLING — gen 3 roadmap",
		Category: "ROLLING",
		Content: fmt.Sprintf(`%s

%s
  Planowane funkcje gen 3 (nie wszystkie dostepne w ROLLING):

%s

%s
  Sledz zmiany na:
  %s`,
			styleH1.Render("Gen 3 — roadmap"),
			styleH2.Render("Planowane"),
			styleCode.Render(";; Gen 3 — planowane funkcje:\n\n;; Async/await:\n;; @~ komenda             -- asynchroniczna komenda\n\n;; Closures:\n;; :> nazwa = { ... }    -- closure\n\n;; Rozszerzone typy:\n;; % m: map = { k: v }   -- mapa\n;; % l: list = [1,2,3]  -- lista\n\n;; Natywny HTTP:\n;; >> http.get https://...  |> @response"),
			styleH2.Render("Sledz"),
			styleOp.Render("https://github.com/HackerOS-Linux-System"),
		),
	},

	// ── TUTORIALE ─────────────────────────────────────────────────────────────
	{
		Title:    "Tutorial: gen 1 — pelny przyklad",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

%s
%s`,
			styleH1.Render("Tutorial: gen 1 full"),
			styleH2.Render("Wszystkie funkcje gen 1"),
			styleCode.Render("#!/usr/bin/env hl\n/// Przyklad gen 1\nusing <gen 1>\n\n// curl\n# <main/colors>\n<< helpers.hl\n\n% target = 192.168.1.1\n\n;; Tlo\n& python3 -m http.server 9000\n~> @COLOR_GREEN PID: @_bg_pid @COLOR_RESET\n\n;; Hsh\n*> notify-send \"Start\"\n\n;; Petla\n_3 > ping -c 1 @target\n\n;; Goroutine + channel\n:** wyniki\n:*\n    >> curl -s http://@target\n    *-- wyniki\ndone\n*-- wyniki\n\n;; Funkcja\n: pokaz def\n    ~> Gotowe!\ndone\n-- pokaz"),
		),
	},
	{
		Title:    "Tutorial: gen 2 — pelny przyklad",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

%s
%s`,
			styleH1.Render("Tutorial: gen 2 full"),
			styleH2.Render("Wszystkie funkcje gen 2"),
			styleCode.Render("#!/usr/bin/env hl\n/// Przyklad gen 2\nusing <gen 2>\n\n# <main/colors>\n\n% count: int  = 0\n% limit: int  = 5\n% host:  str  = \"localhost\"\n\n;; For-in\n@ port in 80 443 8080 22 3306\n    ~> Skanuje port: @port\n    >> nc -z -w1 @host @port\n    ? ok\n        ::green Otwarty!\n    done\ndone\n\n;; While\n?~ @count < @limit\n    $(@count + 1) -> @count\n    ~> Krok @count\ndone\n\n;; Switch\n> hostname |> @myhost\n? switch @myhost\n| hackeros-dev\n    ~> Srodowisko dev\n| hackeros-prod\n    ~> Srodowisko prod\n| *\n    ~> Host: @myhost\ndone\n\n;; Arytmetyka\n$(10 * @limit + @count) -> @result\n~> Wynik: @result"),
		),
	},
	{
		Title:    "Tutorial: update-hackeros.hl",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Skrypt porownuje wersje przez sort -V (semantyczne).
  Aktualizacja przez git clone + hl run unpack.hl`,
			styleH1.Render("Tutorial: update-hackeros"),
			styleH2.Render("Jak dziala"),
			styleCode.Render(";; Skrypt uzywa:\n;; 1. ping google.com — test internetu\n;; 2. jq / grep — odczyt lokalnej wersji z JSON\n;; 3. curl — pobierz zdalna wersje\n;; 4. sort -V — porownanie semantyczne wersji\n;; 5. git clone — pobierz repo\n;; 6. hl run unpack.hl — uruchom aktualizacje\n\n;; Uruchomienie:\nhl exec update-hackeros\n;; lub:\nhl run /usr/share/HackerOS/Scripts/Bin/update-hackeros.hl"),
			styleH2.Render("Szczegoly"),
		),
	},
	{
		Title:    "Tutorial: bit — pakiety HL i Rust",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s`,
			styleH1.Render("Tutorial: bit package manager"),
			styleH2.Render("Instalacja pakietu HL"),
			styleCode.Render(";; Pakiet tui (type: hl):\nbit install tui\n;; → git clone https://github.com/bit-io/tui.git\n;; → sprawdza source-code/main.hl\n\n;; Uzycie w kodzie:\n# <bit/tui>\n-- tui_init"),
			styleH2.Render("Instalacja pakietu Rust"),
			styleCode.Render(";; Pakiet obsidian (type: rust):\nbit install obsidian\n;; → git clone ...\n;; → cd source-code && cargo build --release\n;; → kopiuje binarkę do BIT_HOME/obsidian/"),
		),
	},
}

type view int

const (
	viewMenu    view = iota
	viewContent view = iota
)

type model struct {
	currentView  view
	cursor       int
	sectionIndex int
	viewport     viewport.Model
	width        int
	height       int
	ready        bool
	searchMode   bool
	searchQuery  string
	filtered     []int
}

func initialModel() model {
	allIdx := make([]int, len(sections))
	for i := range sections { allIdx[i] = i }
	return model{currentView: viewMenu, cursor: 0, filtered: allIdx}
}

func (m model) Init() tea.Cmd { return nil }

func (m *model) applySearch() {
	if m.searchQuery == "" {
		m.filtered = make([]int, len(sections))
		for i := range sections { m.filtered[i] = i }
		return
	}
	q := strings.ToLower(m.searchQuery)
	m.filtered = nil
	for i, s := range sections {
		if strings.Contains(strings.ToLower(s.Title), q) ||
			strings.Contains(strings.ToLower(s.Category), q) ||
			strings.Contains(strings.ToLower(s.Content), q) {
			m.filtered = append(m.filtered, i)
		}
	}
	m.cursor = 0
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		vpH := m.height - 5
		if vpH < 5 { vpH = 5 }
		if !m.ready {
			m.viewport = viewport.New(m.width-30, vpH)
			m.viewport.Style = styleContent
			m.ready = true
		} else {
			m.viewport.Width = m.width - 30
			m.viewport.Height = vpH
		}
	case tea.KeyMsg:
		if m.searchMode {
			switch msg.String() {
			case "esc", "ctrl+c":
				m.searchMode = false; m.searchQuery = ""; m.applySearch()
			case "enter":
				m.searchMode = false; m.applySearch()
			case "backspace":
				if len(m.searchQuery) > 0 { m.searchQuery = m.searchQuery[:len(m.searchQuery)-1] }
			default:
				if len(msg.String()) == 1 { m.searchQuery += msg.String() }
			}
			return m, nil
		}
		switch msg.String() {
		case "q", "ctrl+c": return m, tea.Quit
		case "/": m.searchMode = true; m.searchQuery = ""; m.currentView = viewMenu
		case "esc":
			if m.currentView == viewContent { m.currentView = viewMenu }
		case "up", "k":
			if m.currentView == viewMenu { if m.cursor > 0 { m.cursor-- } } else { m.viewport.LineUp(3) }
		case "down", "j":
			if m.currentView == viewMenu { if m.cursor < len(m.filtered)-1 { m.cursor++ } } else { m.viewport.LineDown(3) }
		case "enter", " ":
			if m.currentView == viewMenu && len(m.filtered) > 0 {
				m.sectionIndex = m.filtered[m.cursor]
				m.currentView = viewContent
				if m.ready { m.viewport.SetContent(sections[m.sectionIndex].Content); m.viewport.GotoTop() }
			}
		case "tab":
			if m.currentView == viewMenu {
				if len(m.filtered) > 0 {
					m.sectionIndex = m.filtered[m.cursor]; m.currentView = viewContent
					if m.ready { m.viewport.SetContent(sections[m.sectionIndex].Content); m.viewport.GotoTop() }
				}
			} else { m.currentView = viewMenu }
		case "pgup": if m.currentView == viewContent { m.viewport.HalfViewUp() }
		case "pgdown": if m.currentView == viewContent { m.viewport.HalfViewDown() }
		case "g": if m.currentView == viewContent { m.viewport.GotoTop() }
		case "G": if m.currentView == viewContent { m.viewport.GotoBottom() }
		}
	}
	if m.currentView == viewContent {
		var cmd tea.Cmd
		m.viewport, cmd = m.viewport.Update(msg)
		return m, cmd
	}
	return m, nil
}

func (m model) renderMenu() string {
	var sb strings.Builder
	sidebarW := 28
	currentCategory := ""
	for i, idx := range m.filtered {
		s := sections[idx]
		if s.Category != currentCategory {
			currentCategory = s.Category
			sb.WriteString(styleMenuCategory.Width(sidebarW).Render(s.Category) + "\n")
		}
		title := "  " + s.Title
		if len(title) > sidebarW-2 { title = title[:sidebarW-5] + "..." }
		if i == m.cursor {
			sb.WriteString(styleMenuSelected.Width(sidebarW).Render(title) + "\n")
		} else {
			sb.WriteString(styleMenuNormal.Width(sidebarW).Render(title) + "\n")
		}
	}
	return sb.String()
}

func (m model) View() string {
	if !m.ready { return "\n  Ladowanie..." }
	sidebarW := 30
	logoStyle := lipgloss.NewStyle().Foreground(colorMagenta).Bold(true).Width(sidebarW).Align(lipgloss.Center)
	versionStyle := lipgloss.NewStyle().Foreground(colorMuted).Width(m.width - sidebarW).Align(lipgloss.Right).PaddingRight(2)
	header := lipgloss.JoinHorizontal(lipgloss.Top,
		logoStyle.Render("◆ HACKER LANG DOCS"),
		versionStyle.Render("gen 1 / gen 2 / ROLLING  hl-docs"))
	headerBar := styleHeader.Width(m.width).Render(header)

	var contentSection string
	if m.currentView == viewContent {
		m.viewport.Width = m.width - sidebarW - 2
		contentSection = m.viewport.View()
	} else {
		welcome := fmt.Sprintf("%s\n\n%s\n\n%s\n\n%s",
			styleH1.Render("Dokumentacja Hacker Lang"),
			lipgloss.NewStyle().Foreground(colorText).Render("Wybierz temat z menu po lewej.\nObslugiwane: gen 1, gen 2, ROLLING"),
			lipgloss.NewStyle().Foreground(colorMuted).Render("↑↓ / j k — nawigacja\nEnter — otwórz\n/ — szukaj\nq — wyjdz"),
			styleTip.Render("💡 ROLLING = najnowsze funkcje pre-gen 3"),
		)
		contentSection = lipgloss.NewStyle().Width(m.width-sidebarW-2).Height(m.height-5).Padding(2, 3).Render(welcome)
	}

	body := lipgloss.JoinHorizontal(lipgloss.Top,
		styleSidebar.Width(sidebarW).Height(m.height-5).Render(m.renderMenu()),
		styleContent.Width(m.width-sidebarW-2).Render(contentSection),
	)

	var statusLeft string
	if m.searchMode {
		statusLeft = lipgloss.NewStyle().Foreground(colorYellow).Bold(true).Render(fmt.Sprintf("SZUKAJ: %s_", m.searchQuery))
	} else if m.currentView == viewContent {
		statusLeft = lipgloss.NewStyle().Foreground(colorAccent).Render(fmt.Sprintf("  %s", sections[m.sectionIndex].Title))
	} else {
		statusLeft = lipgloss.NewStyle().Foreground(colorMuted).Render(fmt.Sprintf("  %d sekcji", len(m.filtered)))
	}
	navHints := "↑↓/jk: nav  Enter: open  /: search  q: quit"
	if m.currentView == viewContent { navHints = "↑↓/jk: scroll  PgUp/Dn: page  Esc: menu  q: quit" }
	statusRight := lipgloss.NewStyle().Foreground(colorMuted).Render(navHints + "  ")
	statusBar := lipgloss.JoinHorizontal(lipgloss.Top,
		statusLeft,
		lipgloss.NewStyle().Width(m.width-lipgloss.Width(statusLeft)-lipgloss.Width(statusRight)).Render(""),
		statusRight,
	)
	return lipgloss.JoinVertical(lipgloss.Left, headerBar, body, styleStatusBar.Width(m.width).Render(statusBar))
}

func main() {
	p := tea.NewProgram(initialModel(), tea.WithAltScreen(), tea.WithMouseCellMotion())
	if _, err := p.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "hl-docs error: %v\n", err)
		os.Exit(1)
	}
}
