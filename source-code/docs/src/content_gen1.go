package main

import "fmt"

// ── GEN 1 ──────────────────────────────────────────────────────────────────
var gen1Sections = []DocSection{
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
}
