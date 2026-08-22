package src

import "fmt"

// ── TUTORIALE ──────────────────────────────────────────────────────────────────
var tutorialSections = []DocSection{
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
