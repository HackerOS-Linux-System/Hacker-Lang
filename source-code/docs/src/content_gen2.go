package main

import "fmt"

// ── GEN 2 ──────────────────────────────────────────────────────────────────
var gen2Sections = []DocSection{
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
}
