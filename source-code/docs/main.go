package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

// ── Kolory i style ────────────────────────────────────────────────────────────

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

	styleTitle = lipgloss.NewStyle().
			Foreground(colorMagenta).
			Bold(true).
			Padding(0, 1)

	styleMenuNormal = lipgloss.NewStyle().
			Foreground(colorText).
			Padding(0, 2)

	styleMenuSelected = lipgloss.NewStyle().
				Foreground(colorAccent).
				Background(colorSelected).
				Bold(true).
				Padding(0, 2)

	styleMenuCategory = lipgloss.NewStyle().
				Foreground(colorYellow).
				Bold(true).
				Padding(0, 2).
				MarginTop(1)

	styleSidebar = lipgloss.NewStyle().
			Background(colorPanel).
			BorderStyle(lipgloss.NormalBorder()).
			BorderRight(true).
			BorderForeground(colorBorder).
			Padding(1, 0)

	styleContent = lipgloss.NewStyle().
			Background(colorBg).
			Padding(0, 2)

	styleStatusBar = lipgloss.NewStyle().
			Background(colorPanel).
			Foreground(colorMuted).
			Padding(0, 2)

	styleCode = lipgloss.NewStyle().
			Background(lipgloss.Color("#161b22")).
			Foreground(colorCyan).
			Padding(0, 1).
			Margin(0, 2)

	styleCodeLabel = lipgloss.NewStyle().
			Foreground(colorMuted).
			Italic(true).
			Margin(0, 2)

	styleH1 = lipgloss.NewStyle().
		Foreground(colorMagenta).
		Bold(true).
		MarginTop(1).
		MarginBottom(1)

	styleH2 = lipgloss.NewStyle().
		Foreground(colorAccent).
		Bold(true).
		MarginTop(1)

	styleH3 = lipgloss.NewStyle().
		Foreground(colorCyan).
		Bold(true)

	styleOp = lipgloss.NewStyle().
		Foreground(colorGreen).
		Bold(true)

	styleComment = lipgloss.NewStyle().
			Foreground(colorMuted).
			Italic(true)

	styleTip = lipgloss.NewStyle().
			Foreground(colorYellow).
			Background(lipgloss.Color("#1c1a00")).
			Padding(0, 1).
			Margin(0, 2)

	styleWarning = lipgloss.NewStyle().
			Foreground(colorRed).
			Background(lipgloss.Color("#1a0000")).
			Padding(0, 1).
			Margin(0, 2)

	styleHeader = lipgloss.NewStyle().
			Background(colorPanel).
			Foreground(colorText).
			Bold(true).
			Padding(0, 2)
)

// ── Struktura dokumentacji ────────────────────────────────────────────────────

type DocSection struct {
	Title    string
	Category string
	Content  string
}

// ── Tresc dokumentacji ────────────────────────────────────────────────────────

var sections = []DocSection{
	// ═══════════════════════════════════════════════════════════════════════
	// PODSTAWY
	// ═══════════════════════════════════════════════════════════════════════
	{
		Title:    "Witaj w Hacker Lang",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

Hacker Lang (HL) to jezyk skryptowy stworzony dla HackerOS.
Zaprojektowany z myśla o czytelności, bezpieczenstwie i sile.

%s
%s

%s
  - Nie ma tutaj echo - jest %s
  - Sudo to nie słowo kluczowe - to operator %s
  - Zmienne lokalne: %s  |  Export: %s
  - Funkcje maja wlasna skladnie: %s

%s

Uzyj strzalek do nawigacji. Nacisnij %s aby wyjsc.`,
			styleH1.Render("◆ Hacker Lang v0.4"),
			styleH2.Render("Pierwsze kroki"),
			styleCode.Render("hl run moj_skrypt.hl"),
			styleH2.Render("Czego sie spodziewac?"),
			styleOp.Render("~>"),
			styleOp.Render("^>"),
			styleOp.Render("%"),
			styleOp.Render("=>"),
			styleOp.Render(": nazwa def ... done"),
			styleTip.Render("💡 TIP: Kazda linia zaczyna sie od operatora. Brak operatora = blad."),
			styleOp.Render("q / Ctrl+C"),
		),
	},
	{
		Title:    "Instalacja i konfiguracja",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Hacker Lang jest preinstalowany na HackerOS.
  Binarka: %s

%s
%s

%s
%s

%s
  Pliki konfiguracyjne HL:
  %s  - inicjalizacja powloki
  %s  - historia komend
  %s  - biblioteki uzytkownika`,
			styleH1.Render("Instalacja"),
			styleH2.Render("Sprawdz wersje"),
			styleCode.Render("hl version"),
			styleH2.Render("Lokalizacja binarki"),
			styleCode.Render("/usr/bin/hl"),
			styleH2.Render("Uruchamianie skryptow"),
			styleCode.Render("hl run skrypt.hl\n# lub bezposrednio jesli skrypt ma shebang:\n./skrypt.hl"),
			styleH2.Render("Tryb interaktywny (REPL)"),
			styleCode.Render("hl repl"),
			styleH2.Render("Konfiguracja"),
			styleCode.Render("~/.hlrc"),
			styleCode.Render("~/.hl_history"),
			styleCode.Render("~/.hl/libs/"),
		),
	},
	{
		Title:    "Shebang i uruchamianie",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
  Skrypt mozna uczynic wykonywalnym:
%s

%s
%s`,
			styleH1.Render("Shebang i uruchamianie"),
			styleH2.Render("Shebang w pliku .hl"),
			styleCode.Render("#!/usr/bin/env hl\n\n~> Hello, HackerOS!\n> ls -la"),
			styleH2.Render("Uruchomienie przez hl run"),
			styleCode.Render("hl run moj_skrypt.hl arg1 arg2"),
			styleH2.Render("Uruchomienie inline"),
			styleCode.Render("hl -c \"~> Hello World\""),
			styleH2.Render("Jako wykonywalny skrypt"),
			styleCode.Render("chmod +x moj_skrypt.hl\n./moj_skrypt.hl"),
			styleH2.Render("Argumenty skryptu"),
			styleCode.Render("/// skrypt z argumentami\n\n~> Liczba argumentow: @argc\n~> Pierwszy argument: @arg0\n~> Drugi argument:     @arg1"),
		),
	},

	// ═══════════════════════════════════════════════════════════════════════
	// SKŁADNIA
	// ═══════════════════════════════════════════════════════════════════════
	{
		Title:    "Operatory - przeglad",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
  Kazda linia musi zaczynac sie od OPERATORA.

%s

  %s   ~>    Wypisz tekst (print)
  %s   >     Uruchom komende
  %s   ^>    Uruchom z sudo
  %s   ->    Uruchom w izolacji (unshare)
  %s   ^->   Uruchom z sudo + izolacja
  %s   >>    Komenda z interpolacja @zmiennych
  %s   ^>>   Jak >> ale z sudo
  %s   ->>   Jak >> ale w izolacji

  %s   %     Deklaracja zmiennej lokalnej
  %s   @     Odwolanie do zmiennej
  %s   =>    Export do srodowiska

  %s   :     Definicja funkcji
  %s   --    Wywolanie funkcji
  %s   ? ok  Warunek - jesli sukces
  %s   ? err Warunek - jesli blad

  %s   ;;    Komentarz liniowy
  %s   ///   Komentarz dokumentacyjny
  %s   //    Zaleznosc lub komentarz blokowy
  %s   #     Import biblioteki`,
			styleH1.Render("Operatory Hacker Lang"),
			styleH2.Render("Zasada podstawowa"),
			styleH2.Render("Pelna lista operatorow:"),
			styleOp.Render("PRINT  "),
			styleOp.Render("CMD    "),
			styleOp.Render("SUDO   "),
			styleOp.Render("ISO    "),
			styleOp.Render("ISO+SU "),
			styleOp.Render("VARS   "),
			styleOp.Render("VAR+SU "),
			styleOp.Render("VAR+IS "),
			styleOp.Render("VAR    "),
			styleOp.Render("REF    "),
			styleOp.Render("EXPORT "),
			styleOp.Render("FUNCDEF"),
			styleOp.Render("CALL   "),
			styleOp.Render("IFSUC  "),
			styleOp.Render("IFERR  "),
			styleOp.Render("COMM   "),
			styleOp.Render("DOC    "),
			styleOp.Render("DEP    "),
			styleOp.Render("IMPORT "),
		),
	},
	{
		Title:    "Wypisywanie tekstu (~>)",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
  Jedynym sposobem wypisywania tekstu w HL jest operator %s.
  %s jest w HL ZAKAZANE.

%s
%s

%s
%s

%s
%s

%s
  %s
  Uzywaj %s zamiast %s.`,
			styleH1.Render("Wypisywanie tekstu"),
			styleH2.Render("Operator ~>"),
			styleOp.Render("~>"),
			styleOp.Render("echo"),
			styleH2.Render("Proste uzycie"),
			styleCode.Render("~> Hello, World!\n~> Witaj na HackerOS"),
			styleH2.Render("Z interpolacja zmiennych"),
			styleCode.Render("% imie = Michal\n~> Czesc, @imie!\n~> Twoj HOME to @HOME"),
			styleH2.Render("Pusta linia"),
			styleCode.Render("::nl\n;; lub\n~> "),
			styleWarning.Render("⚠ UWAGA: echo jest zabronione!"),
			styleCode.Render(";;  ZLE:  > echo \"hello\"\n;; DOBRZE: ~> hello"),
			styleOp.Render("~>"),
			styleOp.Render("> echo"),
		),
	},
	{
		Title:    "Komendy systemowe",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
%s

%s
%s

%s
%s

%s
%s

%s
  Uzywaj %s zamiast %s w blokach komend.`,
			styleH1.Render("Komendy systemowe"),
			styleH2.Render("Zwykla komenda  >"),
			styleCode.Render("> ls -la /home\n> mkdir -p /tmp/moj_katalog\n> rm -rf /tmp/stary"),
			styleH2.Render("Komenda z sudo  ^>"),
			styleCode.Render("^> apt update\n^> systemctl restart nginx\n^> chmod 755 /usr/bin/moj_program"),
			styleH2.Render("Komenda izolowana  ->"),
			styleCode.Render("-> nmap -sV 192.168.1.1\n;; Uruchamia w osobnej przestrzeni nazw"),
			styleH2.Render("Izolacja + sudo  ^->"),
			styleCode.Render("^-> tcpdump -i eth0"),
			styleH2.Render("Z interpolacja zmiennych  >>"),
			styleCode.Render("% target = 192.168.1.1\n>> ping -c 4 @target\n>> curl -fsSL @url > @output"),
			styleH2.Render("Z interpolacja + sudo  ^>>"),
			styleCode.Render("% plik = /etc/hosts\n^>> cat @plik"),
			styleH2.Render("Przekierowania i potoki"),
			styleCode.Render("> ls -la | grep \".hl\"\n> cat /etc/passwd | cut -d: -f1 > /tmp/users.txt\n> find /home -name \"*.hl\" 2>/dev/null"),
			styleWarning.Render("⚠ Przekierowania ( > >> < | ) musza byc w komendzie, nie jako oddzielne tokeny."),
			styleOp.Render("> komenda > plik"),
			styleComment.Render(";; poprawne - cala linia to jedna komenda"),
		),
	},
	{
		Title:    "Zmienne lokalne (%)",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
%s

%s
%s

%s
  - %s  deklaruje zmienna LOKALNA w HL
  - %s  exportuje do SRODOWISKA procesu
  - Zmienne HL sa dostepne przez %s
  - Zmienne srodowiskowe przez %s lub %s`,
			styleH1.Render("Zmienne lokalne"),
			styleH2.Render("Deklaracja  %"),
			styleCode.Render("% imie    = Michal\n% wiek    = 25\n% debug   = true\n% sciezka = /usr/share/HackerOS"),
			styleH2.Render("Odwolanie  @"),
			styleCode.Render("~> Czesc @imie, masz @wiek lat\n>> ls @sciezka\n>> curl -o /tmp/plik.tar @url"),
			styleH2.Render("Interpolacja w stringach"),
			styleCode.Render("% host = 192.168.1.1\n% port = 8080\n~> Lacze sie z @host:@port\n>> curl http://@host:@port/api"),
			styleH2.Render("Zmienne specjalne"),
			styleCode.Render(";; Argumenty skryptu\n% liczba_args = @argc\n~> Arg 0: @arg0\n~> Arg 1: @arg1\n\n;; Wbudowane\n;; @HL_VERSION  - wersja HL\n;; @HL_SCRIPT   - sciezka do skryptu"),
			styleH2.Render("Odczyt zmiennych srodowiskowych"),
			styleCode.Render("::env HOME\n::env USER\n::env PATH"),
			styleH2.Render("Roznica: % vs =>"),
			styleCode.Render(""),
			styleOp.Render("%"),
			styleOp.Render("=>"),
			styleOp.Render("@nazwa"),
			styleCode.Render("::env NAZWA"),
			styleCode.Render("::get NAZWA"),
		),
	},
	{
		Title:    "Export do srodowiska (=>)",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
  Operator %s ustawia zmienna w srodowisku procesu.
  Wszystkie uruchamiane podprocesy dziedzicza te zmienne.

%s
%s

%s
%s

%s
%s

%s
  Listy sa lączone znakiem %s (konwencja PATH i podobnych).

%s
%s

%s
%s`,
			styleH1.Render("Export do srodowiska"),
			styleH2.Render("Czym jest =>"),
			styleOp.Render("=>"),
			styleH2.Render("Pojedyncza wartosc"),
			styleCode.Render("=> EDITOR = nvim\n=> BROWSER = firefox\n=> JAVA_HOME = /usr/lib/jvm/java-17\n=> GOPATH = /home/hacker/go"),
			styleH2.Render("Lista wartosci"),
			styleCode.Render("=> PATH [\n| /usr/local/bin\n| /usr/bin\n| /usr/lib/HackerOS\n| /home/hacker/.cargo/bin\n]"),
			styleH2.Render("Dlaczego lista?"),
			styleOp.Render(":"),
			styleH2.Render("Lista z interpolacja"),
			styleCode.Render("% home = /home/hacker\n=> PATH [\n| @home/.local/bin\n| @home/go/bin\n| /usr/bin\n| /usr/local/bin\n]"),
			styleH2.Render("Roznica % vs =>"),
			styleCode.Render("% LOCAL = tylko_w_HL     ;; dziecko NIE dziedziczy\n=> ENV_VAR = dla_dzieci  ;; dziecko DZIEDZICZY"),
		),
	},
	{
		Title:    "Komentarze",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
  Wyswietlane przez %s jako dokumentacja skryptu.`,
			styleH1.Render("Komentarze"),
			styleH2.Render("Komentarz liniowy  ;;"),
			styleCode.Render(";; To jest komentarz\n;; Moze byc wszedzie\n> ls -la  ;; <-- to tez dziala? NIE! ;; musi byc na poczatku linii"),
			styleH2.Render("Komentarz dokumentacyjny  ///"),
			styleCode.Render("/// Skrypt aktualizacji HackerOS\n/// Autor: HackerOS Team\n/// Wersja: 1.0.0\n\n// curl\n// git"),
			styleH2.Render("Komentarz blokowy  // tresc \\\\\\\\"),
			styleCode.Render("// To jest\nkomentarz blokowy\nktory moze byc\nwieloliniowy \\\\"),
			styleH2.Render("Dokumentacja w REPL"),
			styleCode.Render("hl ast moj_skrypt.hl"),
		),
	},

	// ═══════════════════════════════════════════════════════════════════════
	// LOGIKA
	// ═══════════════════════════════════════════════════════════════════════
	{
		Title:    "Warunki (? ok / ? err)",
		Category: "LOGIKA",
		Content: fmt.Sprintf(`%s

%s
  HL nie ma tradycyjnego %s. Warunki bazuja na
  kodzie wyjscia poprzedniej komendy.

%s
%s

%s
%s

%s
%s

%s
%s

%s
  %s  Jesli poprzednia komenda zwrocila exit 0
  %s  Jesli poprzednia komenda zwrocila exit != 0`,
			styleH1.Render("Warunki warunkowe"),
			styleH2.Render("Filozofia"),
			styleCode.Render("if/else"),
			styleH2.Render("Sprawdzenie sukcesu  ? ok"),
			styleCode.Render("> ping -c 1 google.com\n\n? ok\n    ::green Host jest dostepny\ndone"),
			styleH2.Render("Sprawdzenie bledu  ? err"),
			styleCode.Render("> ping -c 1 google.com\n\n? err\n    ::red Brak polaczenia!\n    > exit 1\ndone"),
			styleH2.Render("ok + err razem"),
			styleCode.Render("> test -f /etc/passwd\n\n? ok\n    ::green Plik istnieje\ndone\n\n? err\n    ::red Plik NIE istnieje!\ndone"),
			styleH2.Render("Zagniezdzanie"),
			styleCode.Render("> curl -fsSL https://example.com > /tmp/test\n\n? ok\n    > test -s /tmp/test\n    ? ok\n        ::green Pobrano i plik nie jest pusty\n    done\n    ? err\n        ::yellow Pobrano ale plik jest pusty\n    done\ndone"),
			styleH2.Render("Podsumowanie"),
			styleCode.Render("? ok"),
			styleCode.Render("? err"),
		),
	},
	{
		Title:    "Funkcje",
		Category: "LOGIKA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
%s

%s
  Funkcje moga byc wywolywane przed ich definicja
  o ile definicja pojawia sie w tym samym pliku.

%s
%s`,
			styleH1.Render("Funkcje"),
			styleH2.Render("Definicja funkcji  : nazwa def"),
			styleCode.Render(": przywitaj def\n    ~> Czesc z funkcji!\n    > date\ndone"),
			styleH2.Render("Wywolanie funkcji  -- nazwa"),
			styleCode.Render("-- przywitaj"),
			styleH2.Render("Funkcja z logika"),
			styleCode.Render(": sprawdz_internet def\n    ~> Sprawdzam internet...\n    > ping -c 1 -W 1 google.com\n\n    ? ok\n        ::green Internet dostepny\n    done\n\n    ? err\n        ::red Brak internetu!\n        > exit 1\n    done\ndone\n\n-- sprawdz_internet"),
			styleH2.Render("Dobre praktyki"),
			styleCode.Render(";; Definiuj funkcje na poczatku\n;; Uzywaj opisowych nazw\n;; Jedna funkcja = jedno zadanie\n\n: update_apt def\n    ::bold Aktualizacja APT...\n    ^> apt update -y\n    ^> apt upgrade -y\n    ::green APT zaktualizowany\ndone\n\n: update_snap def\n    > which snap > /dev/null 2>&1\n    ? ok\n        ^> snap refresh\n        ::green Snap zaktualizowany\n    done\ndone\n\n-- update_apt\n-- update_snap"),
			styleH2.Render("Uwaga"),
			styleCode.Render(""),
			styleWarning.Render("⚠ Funkcje nie przyjmuja argumentow. Uzywaj zmiennych globalnych % do przekazywania danych."),
		),
	},

	// ═══════════════════════════════════════════════════════════════════════
	// QUICK FUNCTIONS
	// ═══════════════════════════════════════════════════════════════════════
	{
		Title:    "Quick Functions (::) - przeglad",
		Category: "QUICK FUNCTIONS",
		Content: fmt.Sprintf(`%s

%s
  Quick functions to wbudowane narzedzia wywolywane przez %s.
  Dzialaja szybciej niz zewnetrzne komendy i sa zawsze dostepne.

%s
  %s   Kolory    - bold, red, green, yellow, cyan
  %s   Stringi   - upper, lower, len, trim, rev, replace, split...
  %s   Matematyka - abs, ceil, floor, round, max, min, rand
  %s   System    - env, date, time, pid, which
  %s   Pliki     - exists, isdir, isfile, basename, dirname, read
  %s   Zmienne   - set, get, type, unset
  %s   Wyjście   - nl, hr

%s
%s`,
			styleH1.Render("Quick Functions ::"),
			styleH2.Render("Co to sa Quick Functions?"),
			styleOp.Render("::"),
			styleH2.Render("Kategorie"),
			styleOp.Render("::bold, ::red..."),
			styleOp.Render("::upper, ::lower..."),
			styleOp.Render("::abs, ::ceil..."),
			styleOp.Render("::env, ::date..."),
			styleOp.Render("::exists, ::read..."),
			styleOp.Render("::set, ::get..."),
			styleOp.Render("::nl, ::hr"),
			styleH2.Render("Przyklad"),
			styleCode.Render("% wiadomosc = hello world\n::upper @wiadomosc\n;; OUTPUT: HELLO WORLD\n\n::len @wiadomosc\n;; OUTPUT: 11\n\n::exists /etc/passwd\n? ok\n    ::green Plik istnieje!\ndone"),
		),
	},
	{
		Title:    "Quick Functions - kolory",
		Category: "QUICK FUNCTIONS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("Quick Functions - Kolory i formatowanie"),
			styleH2.Render("Podstawowe kolory"),
			styleCode.Render("::red    Tekst w kolorze czerwonym\n::green  Tekst w kolorze zielonym\n::yellow Tekst w kolorze zoltym\n::cyan   Tekst w kolorze cyjanowym"),
			styleH2.Render("Formatowanie"),
			styleCode.Render("::bold   Tekst pogrubiony\n::nl     Pusta linia\n::hr     Pozioma linia (domyslnie 60 znakow)\n::hr 40  Pozioma linia 40 znakow"),
			styleH2.Render("Przyklad - header sekcji"),
			styleCode.Render(": sekcja def\n    ::hr 50\n    ::bold @tytul_sekcji\n    ::hr 50\ndone\n\n% tytul_sekcji = Aktualizacja systemu\n-- sekcja"),
			styleH2.Render("Przyklad - komunikaty"),
			styleCode.Render("> ping -c 1 google.com\n\n? ok\n    ::green OK: Polaczenie dziala\ndone\n\n? err\n    ::red BLAD: Brak polaczenia!\ndone"),
			styleH2.Render("Komunikaty z kontekstem"),
			styleCode.Render("% plik = /etc/passwd\n::exists @plik\n\n? ok\n    ::green Znaleziono: @plik\ndone\n\n? err\n    ::red Nie znaleziono: @plik\ndone"),
		),
	},
	{
		Title:    "Quick Functions - stringi",
		Category: "QUICK FUNCTIONS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s`,
			styleH1.Render("Quick Functions - Operacje na stringach"),
			styleH2.Render("Lista funkcji"),
			styleCode.Render("::upper <tekst>           -- zamien na WIELKIE litery\n::lower <tekst>           -- zamien na male litery\n::len <tekst>             -- dlugosc tekstu\n::trim <tekst>            -- usun biale znaki z krawedzi\n::rev <tekst>             -- odwroc tekst\n::repeat <tekst> <n>      -- powtorz n razy\n::replace <tekst> <s> <d> -- zamien s na d\n::contains <tekst> <sub>  -- czy zawiera? (exit 0/1)\n::startswith <t> <pref>   -- czy zaczyna sie od?\n::endswith <t> <suf>      -- czy konczy sie na?\n::split <tekst> <sep>     -- podziel po separatorze\n::lines <tekst>           -- wypisz linie\n::words <tekst>           -- wypisz slowa"),
			styleH2.Render("Przyklady"),
			styleCode.Render("% t = Hello World\n\n::upper @t\n;; HELLO WORLD\n\n::lower @t\n;; hello world\n\n::len @t\n;; 11\n\n::contains @t World\n? ok\n    ~> Zawiera 'World'!\ndone\n\n::replace @t World HackerOS\n;; Hello HackerOS\n\n::split @t ' '\n;; Hello\n;; World"),
		),
	},
	{
		Title:    "Quick Functions - system",
		Category: "QUICK FUNCTIONS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("Quick Functions - System"),
			styleH2.Render("Lista funkcji"),
			styleCode.Render("::env  <NAZWA>  -- pobierz zmienna srodowiskowa\n::date         -- aktualna data (RRRR-MM-DD)\n::time         -- aktualny czas (HH:MM:SS)\n::pid          -- PID biezacego procesu\n::which <prog> -- znajdz program w PATH (exit 0/1)"),
			styleH2.Render("Przyklad ::which"),
			styleCode.Render("::which curl\n\n? ok\n    ::green curl jest zainstalowany\ndone\n\n? err\n    ::red curl NIE jest zainstalowany!\n    ::yellow Instaluje curl...\n    ^> apt install -y curl\ndone"),
			styleH2.Render("Przyklad ::env + ::date"),
			styleCode.Render("::env HOME\n;; /home/hacker\n\n::date\n;; 2024-07-15\n\n::time\n;; 14:30:45\n\n::pid\n;; 12345"),
		),
	},
	{
		Title:    "Quick Functions - pliki",
		Category: "QUICK FUNCTIONS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s`,
			styleH1.Render("Quick Functions - Pliki i sciezki"),
			styleH2.Render("Lista funkcji"),
			styleCode.Render("::exists   <sciezka>  -- czy istnieje? (exit 0/1)\n::isdir    <sciezka>  -- czy to katalog?\n::isfile   <sciezka>  -- czy to plik?\n::basename <sciezka>  -- nazwa pliku bez katalogu\n::dirname  <sciezka>  -- katalog bez nazwy pliku\n::read     <plik>     -- wyswietl zawartosc pliku"),
			styleH2.Render("Przyklady"),
			styleCode.Render("% plik = /etc/os-release\n\n::exists @plik\n? ok\n    ::green Plik istnieje\n    ::read @plik\ndone\n\n::basename /home/hacker/skrypt.hl\n;; skrypt.hl\n\n::dirname /home/hacker/skrypt.hl\n;; /home/hacker\n\n::isdir /tmp\n? ok\n    ::cyan /tmp jest katalogiem\ndone"),
		),
	},

	// ═══════════════════════════════════════════════════════════════════════
	// ZALEZNOSCI
	// ═══════════════════════════════════════════════════════════════════════
	{
		Title:    "Zaleznosci (//)",
		Category: "ZALEZNOSCI",
		Content: fmt.Sprintf(`%s

%s
  Operator %s deklaruje ze skrypt wymaga danego narzedzia.
  HL automatycznie sprawdza czy narzedzie jest dostepne
  i probuje je zainstalowac jesli brakuje.

%s
%s

%s
%s

%s
%s

%s
  Umieszczaj deklaracje zaleznosci na POCZATKU pliku.`,
			styleH1.Render("Zaleznosci systemowe"),
			styleH2.Render("Czym jest //"),
			styleOp.Render("//"),
			styleH2.Render("Podstawowe uzycie"),
			styleCode.Render("// curl\n// git\n// jq\n// nmap\n\n;; ... reszta skryptu"),
			styleH2.Render("Przyklad kompletny"),
			styleCode.Render("/// Skrypt skanowania sieci\n\n// nmap\n// curl\n\n% cel = 192.168.1.0/24\n\n~> Skanowanie @cel...\n>> nmap -sn @cel"),
			styleH2.Render("Co sie dzieje gdy brakuje narzedzia?"),
			styleCode.Render(";; HL automatycznie:\n;; 1. Sprawdza czy narzedzie jest w PATH\n;; 2. Jesli nie - probuje sudo apt-get install <nazwa>\n;; 3. Jesli apt-get nie dziala - probuje lpm install\n;; 4. Jesli wszystko zawiedzie - wyswietla blad"),
			styleTip.Render("💡 TIP: Deklaracje // to dokumentacja i automatyczna instalacja w jednym."),
		),
	},
	{
		Title:    "Importy bibliotek (#)",
		Category: "ZALEZNOSCI",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("Import bibliotek HL"),
			styleH2.Render("Biblioteki standardowe"),
			styleCode.Render("# <std/net>     -- zmienne sieciowe (IP, gateway, porty)\n# <std/fs>      -- sciezki systemowe\n# <std/sys>     -- informacje o systemie\n# <std/str>     -- stale stringowe\n# <std/crypto>  -- narzedzia kryptograficzne\n# <std/proc>    -- informacje o procesach"),
			styleH2.Render("Import z detalami"),
			styleCode.Render("# <std/net> | <ports>   -- tylko zmienne portow\n# <std/net> | <dns>     -- tylko DNS\n# <std/net:1.2>         -- konkretna wersja"),
			styleH2.Render("Community (GitHub)"),
			styleCode.Render("# <community/github.com/uzytkownik/repo>\n# <community/github.com/uzytkownik/repo:v2>"),
			styleH2.Render("Biblioteki Virus (.so)"),
			styleCode.Render("# <virus/hashlib>\n# <virus/hashlib:2.0>\n\n;; Virus to natywne biblioteki .so\n;; instalowane przez: hpm install virus/hashlib"),
		),
	},

	// ═══════════════════════════════════════════════════════════════════════
	// TUTORIALE
	// ═══════════════════════════════════════════════════════════════════════
	{
		Title:    "Tutorial: Pierwszy skrypt",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

  Napiszmy razem pierwszy skrypt HL od zera.

%s
%s

%s
  Zapisz to jako %s i uruchom przez %s.

%s
%s

%s
  Gratulacje! Twoj pierwszy skrypt Hacker Lang dziala!`,
			styleH1.Render("Tutorial 1: Pierwszy skrypt"),
			styleH2.Render("Krok 1: Stwórz plik hello.hl"),
			styleCode.Render("/// Moj pierwszy skrypt Hacker Lang\n/// Wyswietla powitanie i informacje o systemie\n\n;; Wyswietl naglowek\n::hr 40\n::bold Witaj w Hacker Lang!\n::hr 40\n::nl\n\n;; Informacje o systemie\n~> System:\n::env USER\n::env HOME\n::date\n::time\n::nl\n\n;; Sprawdz czy curl jest dostepny\n::which curl\n\n? ok\n    ::green curl jest zainstalowany!\ndone\n\n? err\n    ::yellow curl nie jest zainstalowany\ndone\n\n::hr 40\n~> Skrypt zakonczony pomyslnie!"),
			styleCode.Render("hello.hl"),
			styleCode.Render("hl run hello.hl"),
			styleH2.Render("Krok 2: Co sie dzieje?"),
			styleCode.Render(";; 1. /// = komentarz dokumentacyjny\n;; 2. ::hr, ::bold = quick functions\n;; 3. ~> = wypisywanie tekstu\n;; 4. ::env = odczyt zmiennych srodowiskowych\n;; 5. ::which = sprawdzenie narzedzia\n;; 6. ? ok / ? err = warunki"),
			styleTip.Render("💡 Zapisz plik i eksperymentuj! Modyfikuj i uruchamiaj ponownie."),
		),
	},
	{
		Title:    "Tutorial: Skrypt aktualizacji",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

  Klasyczny przypadek uzycia HL: skrypt aktualizacji systemu.

%s
%s`,
			styleH1.Render("Tutorial 2: Skrypt aktualizacji"),
			styleH2.Render("Kompletny skrypt"),
			styleCode.Render("/// Aktualizacja systemu HackerOS\n/// Uruchom jako: hl run update.hl\n\n// curl\n\n;; =====================\n;; FUNKCJE\n;; =====================\n\n: sprawdz_internet def\n    > ping -c 1 -W 2 google.com > /dev/null 2>&1\n    ? err\n        ::red Brak polaczenia z internetem!\n        > exit 1\n    done\ndone\n\n: aktualizuj_apt def\n    ::hr 50\n    ::bold APT - Aktualizacja pakietow\n    ::hr 50\n    ^> apt update -y\n    ? ok\n        ^> apt upgrade -y\n        ^> apt autoremove -y\n        ::green APT zaktualizowany!\n    done\n    ? err\n        ::red Blad aktualizacji APT\n    done\ndone\n\n: aktualizuj_flatpak def\n    ::which flatpak\n    ? ok\n        ::hr 50\n        ::bold Flatpak - Aktualizacja\n        ::hr 50\n        > flatpak update -y\n        ::green Flatpak zaktualizowany!\n    done\ndone\n\n;; =====================\n;; MAIN\n;; =====================\n\n::hr 50\n::bold System Update - HackerOS\n::hr 50\n\n-- sprawdz_internet\n-- aktualizuj_apt\n-- aktualizuj_flatpak\n\n::nl\n::green Aktualizacja zakonczona!"),
		),
	},
	{
		Title:    "Tutorial: Zmienne i export",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("Tutorial 3: Zmienne i Export"),
			styleH2.Render("Zmienne lokalne vs Export"),
			styleCode.Render("/// Demonstracja roznic miedzy % i =>\n\n;; Zmienna LOKALNA - widoczna tylko w HL\n% lokalna = hello\n~> Lokalna: @lokalna\n\n;; Export - widoczna dla dzieci\n=> EKSPORTOWANA = world\n\n;; Teraz podproces widzi EKSPORTOWANA\n> bash -c \"echo Zmienna: $EKSPORTOWANA\"\n\n;; ... ale NIE widzi lokalna\n> bash -c \"echo Lokalna: $lokalna\"\n;; OUTPUT: Lokalna: (puste)"),
			styleH2.Render("Przyklad z PATH"),
			styleCode.Render("/// Konfiguracja srodowiska deweloperskiego\n\n% home = /home/hacker\n\n=> EDITOR = nvim\n=> BROWSER = firefox\n=> GOPATH = @home/go\n\n=> PATH [\n| @home/.local/bin\n| @home/go/bin\n| @home/.cargo/bin\n| /usr/local/bin\n| /usr/bin\n| /usr/lib/HackerOS\n]\n\n;; Sprawdz czy PATH sie ustawil\n::env PATH\n\n~> Srodowisko skonfigurowane!"),
			styleH2.Render("Przyklad konfiguracji hacking-env"),
			styleCode.Render("/// Konfiguracja srodowiska do pentestow\n\n// nmap\n// curl\n// git\n\n=> HACKEROS_TOOLS = /usr/share/HackerOS/tools\n=> WORDLISTS = /usr/share/wordlists\n\n=> PATH [\n| /usr/share/HackerOS/tools\n| /usr/bin\n| /usr/local/bin\n]\n\n~> Srodowisko pentestowe aktywne"),
		),
	},
	{
		Title:    "Tutorial: Sprawdzanie wersji",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

  Klasyczny pattern HL: pobierz wersje zdalna, porownaj z lokalna.

%s
%s`,
			styleH1.Render("Tutorial 4: Sprawdzanie wersji"),
			styleH2.Render("Kompletny przyklad"),
			styleCode.Render("/// Sprawdzanie i aktualizacja wersji\n\n// curl\n// git\n\n% LOCAL_JSON  = /usr/share/myapp/version.json\n% REMOTE_URL  = https://example.com/version.txt\n% LOCAL_TMP   = /tmp/local_ver.tmp\n% REMOTE_TMP  = /tmp/remote_ver.tmp\n% CHECK_TMP   = /tmp/check_ver.tmp\n\n;; Sprawdz plik lokalny\n::exists @LOCAL_JSON\n? err\n    ::red Brak pliku wersji!\n    > exit 1\ndone\n\n;; Odczytaj lokalna wersje\n> grep version @LOCAL_JSON | grep -o \"[0-9][0-9.]*\" | head -1 > @LOCAL_TMP\n\n;; Pobierz zdalna wersje\n> curl -fsSL -o @REMOTE_TMP @REMOTE_URL\n? err\n    ::red Blad pobierania wersji\n    > exit 1\ndone\n\n;; Oczyszcz z nawiasow\n> tr -d \"[]\" < @REMOTE_TMP > @CHECK_TMP\n> mv @CHECK_TMP @REMOTE_TMP\n\n;; Porownaj przez sort -V\n> cat @LOCAL_TMP @REMOTE_TMP | sort -V | tail -n1 > @CHECK_TMP\n\n> diff -q @REMOTE_TMP @CHECK_TMP > /dev/null 2>&1\n? ok\n    > diff -q @LOCAL_TMP @REMOTE_TMP > /dev/null 2>&1\n    ? ok\n        ::green Aplikacja jest aktualna!\n    done\n    ? err\n        ::yellow Dostepna nowa wersja!\n        ;; ... kod aktualizacji\n    done\ndone\n\n> rm -f @LOCAL_TMP @REMOTE_TMP @CHECK_TMP"),
		),
	},

	// ═══════════════════════════════════════════════════════════════════════
	// NARZEDZIA
	// ═══════════════════════════════════════════════════════════════════════
	{
		Title:    "hl check i diagnostyki",
		Category: "NARZEDZIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("Narzedzia HL"),
			styleH2.Render("Sprawdzanie skladni"),
			styleCode.Render("hl check moj_skrypt.hl\n;; Sprawdza skladnie i wyswietla bledy bez uruchamiania"),
			styleH2.Render("Przeglad AST"),
			styleCode.Render("hl ast moj_skrypt.hl\n;; Wyswietla AST jako JSON\n;; Przydatne do debugowania i analizy"),
			styleH2.Render("Diagnostyki - typy"),
			styleCode.Render(";; ERROR   - blad ktory zatrzymuje wykonanie\n;;           (np. echo zamiast ~>, brakujace done)\n\n;; WARNING - ostrzezenie (moze dzialac ale jest zle)\n;;           (np. > sudo zamiast ^>)\n\n;; HINT    - podpowiedz dla lepszego kodu\n;;           (np. brakujaca deklaracja //)\n\n;; NOTE    - informacja pomocnicza"),
			styleH2.Render("Kompilacja do binarki"),
			styleCode.Render("hl compile moj_skrypt.hl\n;; Tworzy statyczna binarke x86_64\n\nhl compile moj_skrypt.hl --shared\n;; Tworzy biblioteke .so (ekosystem Virus)\n\nhl compile moj_skrypt.hl -o moj_program\n;; Kompiluje z niestandardowa nazwa wyjscia"),
		),
	},
	{
		Title:    "REPL i powloka",
		Category: "NARZEDZIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("REPL i tryb powloki"),
			styleH2.Render("REPL - interaktywny tryb"),
			styleCode.Render("hl repl\n;; Interaktywny tryb z podpowiedziami, historia, kolorowaniem"),
			styleH2.Render("Komendy w REPL"),
			styleCode.Render("help   -- lista komend\nvars   -- lista zmiennych\nfuncs  -- lista funkcji\nclear  -- wyczysc ekran\ncd     -- zmien katalog\nexit   -- wyjdz"),
			styleH2.Render("Tryb powloki systemowej"),
			styleCode.Render("hl shell\n;; HL jako domyslna powloka\n;; Laduje ~/.hlrc przy starcie"),
			styleH2.Render("Przykladowy ~/.hlrc"),
			styleCode.Render("/// Konfiguracja powloki HL\n\n=> EDITOR = nvim\n=> BROWSER = firefox\n\n=> PATH [\n| /usr/local/bin\n| /usr/bin\n| /usr/lib/HackerOS\n| /home/hacker/.local/bin\n]\n\n: ll def\n    > ls -la\ndone\n\n: update def\n    > hl run /usr/share/HackerOS/Scripts/update_system.hl\ndone"),
		),
	},
}

// ── Model ─────────────────────────────────────────────────────────────────────

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
	filtered     []int // indeksy sekcji pasujace do wyszukiwania
}

func initialModel() model {
	allIdx := make([]int, len(sections))
	for i := range sections {
		allIdx[i] = i
	}
	return model{
		currentView: viewMenu,
		cursor:      0,
		filtered:    allIdx,
	}
}

func (m model) Init() tea.Cmd {
	return nil
}

// ── Wyszukiwanie ──────────────────────────────────────────────────────────────

func (m *model) applySearch() {
	if m.searchQuery == "" {
		m.filtered = make([]int, len(sections))
		for i := range sections {
			m.filtered[i] = i
		}
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

// ── Update ────────────────────────────────────────────────────────────────────

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {

	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		headerH := 3
		statusH := 2
		vpH := m.height - headerH - statusH
		if vpH < 5 {
			vpH = 5
		}
		if !m.ready {
			m.viewport = viewport.New(m.width-30, vpH)
			m.viewport.Style = styleContent
			m.ready = true
		} else {
			m.viewport.Width = m.width - 30
			m.viewport.Height = vpH
		}

	case tea.KeyMsg:
		// Tryb wyszukiwania
		if m.searchMode {
			switch msg.String() {
			case "esc", "ctrl+c":
				m.searchMode = false
				m.searchQuery = ""
				m.applySearch()
			case "enter":
				m.searchMode = false
				m.applySearch()
			case "backspace":
				if len(m.searchQuery) > 0 {
					m.searchQuery = m.searchQuery[:len(m.searchQuery)-1]
				}
			default:
				if len(msg.String()) == 1 {
					m.searchQuery += msg.String()
				}
			}
			return m, nil
		}

		switch msg.String() {
		case "q", "ctrl+c":
			return m, tea.Quit

		case "/":
			m.searchMode = true
			m.searchQuery = ""
			m.currentView = viewMenu

		case "esc":
			if m.currentView == viewContent {
				m.currentView = viewMenu
			}

		case "up", "k":
			if m.currentView == viewMenu {
				if m.cursor > 0 {
					m.cursor--
				}
			} else {
				m.viewport.LineUp(3)
			}

		case "down", "j":
			if m.currentView == viewMenu {
				if m.cursor < len(m.filtered)-1 {
					m.cursor++
				}
			} else {
				m.viewport.LineDown(3)
			}

		case "pgup":
			if m.currentView == viewContent {
				m.viewport.HalfViewUp()
			}

		case "pgdown":
			if m.currentView == viewContent {
				m.viewport.HalfViewDown()
			}

		case "g":
			if m.currentView == viewContent {
				m.viewport.GotoTop()
			}

		case "G":
			if m.currentView == viewContent {
				m.viewport.GotoBottom()
			}

		case "enter", " ":
			if m.currentView == viewMenu && len(m.filtered) > 0 {
				m.sectionIndex = m.filtered[m.cursor]
				m.currentView = viewContent
				if m.ready {
					m.viewport.SetContent(renderContent(sections[m.sectionIndex]))
					m.viewport.GotoTop()
				}
			}

		case "tab":
			// Przelacz widok
			if m.currentView == viewMenu {
				if len(m.filtered) > 0 {
					m.sectionIndex = m.filtered[m.cursor]
					m.currentView = viewContent
					if m.ready {
						m.viewport.SetContent(renderContent(sections[m.sectionIndex]))
						m.viewport.GotoTop()
					}
				}
			} else {
				m.currentView = viewMenu
			}
		}
	}

	if m.currentView == viewContent {
		var cmd tea.Cmd
		m.viewport, cmd = m.viewport.Update(msg)
		return m, cmd
	}

	return m, nil
}

// ── Render tresci ─────────────────────────────────────────────────────────────

func renderContent(s DocSection) string {
	return s.Content
}

// ── Render menu ───────────────────────────────────────────────────────────────

func (m model) renderMenu() string {
	var sb strings.Builder

	sidebarW := 28

	// Grupuj po kategorii
	currentCategory := ""

	visibleCursor := -1
	for i, idx := range m.filtered {
		if idx == m.sectionIndex {
			visibleCursor = i
			_ = visibleCursor
		}
	}

	for i, idx := range m.filtered {
		s := sections[idx]

		if s.Category != currentCategory {
			currentCategory = s.Category
			line := styleMenuCategory.Width(sidebarW).Render(s.Category)
			sb.WriteString(line + "\n")
		}

		title := "  " + s.Title
		if len(title) > sidebarW-2 {
			title = title[:sidebarW-5] + "..."
		}

		var line string
		if i == m.cursor {
			line = styleMenuSelected.Width(sidebarW).Render(title)
		} else {
			line = styleMenuNormal.Width(sidebarW).Render(title)
		}
		sb.WriteString(line + "\n")
	}

	return sb.String()
}

// ── View ──────────────────────────────────────────────────────────────────────

func (m model) View() string {
	if !m.ready {
		return "\n  Ladowanie..."
	}

	sidebarW := 30

	// ── HEADER ────────────────────────────────────────────────────────────────

	logoStyle := lipgloss.NewStyle().
		Foreground(colorMagenta).
		Bold(true).
		Width(sidebarW).
		Align(lipgloss.Center)

	versionStyle := lipgloss.NewStyle().
		Foreground(colorMuted).
		Width(m.width - sidebarW).
		Align(lipgloss.Right).
		PaddingRight(2)

	header := lipgloss.JoinHorizontal(
		lipgloss.Top,
		logoStyle.Render("◆ HACKER LANG DOCS"),
		versionStyle.Render("v0.4  hl-docs"),
	)

	headerBar := styleHeader.Width(m.width).Render(header)

	// ── CIALO ─────────────────────────────────────────────────────────────────

	menuContent := m.renderMenu()

	var contentSection string
	if m.currentView == viewContent {
		m.viewport.Width = m.width - sidebarW - 2
		contentSection = m.viewport.View()
	} else {
		// Powitanie gdy nic nie wybrano
		welcome := fmt.Sprintf("%s\n\n%s\n\n%s\n\n%s",
			styleH1.Render("Dokumentacja Hacker Lang"),
			lipgloss.NewStyle().Foreground(colorText).Render("Wybierz temat z menu po lewej stronie."),
			lipgloss.NewStyle().Foreground(colorMuted).Render("Uzywaj strzalek ↑↓ lub j/k do nawigacji.\nNacisnij Enter lub Spacje aby otworzyc."),
			styleTip.Render("💡 Nacisnij / aby wyszukac"),
		)
		contentSection = lipgloss.NewStyle().
			Width(m.width-sidebarW-2).
			Height(m.height-5).
			Padding(2, 3).
			Render(welcome)
	}

	body := lipgloss.JoinHorizontal(
		lipgloss.Top,
		styleSidebar.Width(sidebarW).Height(m.height-5).Render(menuContent),
		styleContent.Width(m.width-sidebarW-2).Render(contentSection),
	)

	// ── STATUS BAR ────────────────────────────────────────────────────────────

	var statusLeft, statusRight string

	if m.searchMode {
		statusLeft = lipgloss.NewStyle().Foreground(colorYellow).Bold(true).Render(
			fmt.Sprintf("SZUKAJ: %s_", m.searchQuery),
		)
	} else if m.currentView == viewContent {
		statusLeft = lipgloss.NewStyle().Foreground(colorAccent).Render(
			fmt.Sprintf("  %s", sections[m.sectionIndex].Title),
		)
	} else {
		if len(m.filtered) == len(sections) {
			statusLeft = lipgloss.NewStyle().Foreground(colorMuted).Render(
				fmt.Sprintf("  %d sekcji", len(sections)),
			)
		} else {
			statusLeft = lipgloss.NewStyle().Foreground(colorYellow).Render(
				fmt.Sprintf("  Wyniki: %d/%d", len(m.filtered), len(sections)),
			)
		}
	}

	navHints := "↑↓/jk: nav  Enter: open  Esc: back  /: search  q: quit"
	if m.currentView == viewContent {
		navHints = "↑↓/jk: scroll  PgUp/PgDn: page  g/G: top/bot  Esc: menu  q: quit"
	}
	statusRight = lipgloss.NewStyle().Foreground(colorMuted).Render(navHints + "  ")

	statusBar := lipgloss.JoinHorizontal(
		lipgloss.Top,
		statusLeft,
		lipgloss.NewStyle().Width(m.width-lipgloss.Width(statusLeft)-lipgloss.Width(statusRight)).Render(""),
		statusRight,
	)

	statusBarStyled := styleStatusBar.Width(m.width).Render(statusBar)

	return lipgloss.JoinVertical(lipgloss.Left, headerBar, body, statusBarStyled)
}

// ── Main ──────────────────────────────────────────────────────────────────────

func main() {
	p := tea.NewProgram(
		initialModel(),
		tea.WithAltScreen(),
		tea.WithMouseCellMotion(),
	)

	if _, err := p.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "hl-docs error: %v\n", err)
		os.Exit(1)
	}
}
