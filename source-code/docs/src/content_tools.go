package src

import "fmt"

// ── NARZĘDZIA ────────────────────────────────────────────────────────────────
// Nowa kategoria: narzędzia dookoła samego języka — manager pakietów bit,
// generator dokumentacji/tłumacz składni hlh, oraz zmienna środowiskowa do
// testowania hl poza prawdziwym HackerOS.
var toolsSections = []DocSection{
	{
		Title:    "bit — Package Manager",
		Category: "NARZĘDZIA",
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
  %s  typ hl   — kod Hacker Lang (source-code/main.hl)
  %s  typ rust — kod Rust        (source-code/src/main.rs + Cargo.toml)

%s
  Wywołaj bit bez argumentów = projekt mode (auto-detect).`,
			styleH1.Render("bit — Package Manager"),
			styleH2.Render("Komendy"),
			styleCode.Render("bit install <nazwa>\nbit remove  <nazwa>\nbit upgrade [nazwa]\nbit verify  [nazwa]\nbit list\nbit installed\nbit search  <fraza>\nbit update\nbit info    <nazwa>\nbit clean\nbit run     <nazwa>\nbit workspace\nbit io\nbit help"),
			styleH2.Render("bit io — otwórz stronę bit-io"),
			styleCode.Render("bit io\n;; próbuje po kolei: xdg-open, gio open, open, wslview\n;; otwiera https://bit-io.github.io/website/"),
			styleH2.Render("Użycie w .hl"),
			styleCode.Render("# <bit/tui>\n# <bit/regex>\n# <bit/json-parser>"),
			styleH2.Render("Format repo-list.json"),
			styleCode.Render("{\n  \"tui\":  { \"url\": \"https://github.com/bit-io/tui.git\",  \"type\": \"hl\" },\n  \"obsidian\": { \"url\": \"...\", \"type\": \"rust\" }\n}"),
			styleH2.Render("Typy pakietów"),
			"•",
			"•",
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "hlh — dokumentacja i tłumacz składni",
		Category: "NARZĘDZIA",
		Content: fmt.Sprintf(`%s

%s
  %s to narzędzie (source-code/hlh.hl) do generowania dokumentacji
  Markdown ze skryptów .hl oraz "tłumaczenia" terse-składni na czytelne,
  komentowane objaśnienia — linia po linii. Przydatne przy nauce języka
  lub przeglądaniu cudzego kodu.

%s
%s

%s
  Bez %s wynik trafia na stdout zamiast do pliku.`,
			styleH1.Render("◆ hlh — Hacker Lang Helper"),
			styleH2.Render("Czym jest"),
			styleOp.Render("hlh"),
			styleH2.Render("Komendy"),
			styleCode.Render("hlh doc       <plik.hl>              ;; dokumentacja Markdown\nhlh doc       <plik.hl> -o out.md    ;; zapisz do pliku\nhlh translate <plik.hl>              ;; objaśnij każdą linię\nhlh translate <plik.hl> -o out.txt   ;; zapisz do pliku\nhlh help"),
			styleH2.Render("Uwaga"),
			styleOp.Render("-o <ścieżka>"),
		),
	},
	{
		Title:    "Testowanie hl poza HackerOS",
		Category: "NARZĘDZIA",
		Content: fmt.Sprintf(`%s

%s
  %s odmawia działania, jeśli nie wykryje prawdziwego
  HackerOS (katalogi /usr/share/HackerOS, /usr/lib/HackerOS,
  /usr/bin/hacker oraz NAME="HackerOS" w /etc/os-release).

%s
%s

%s
%s

%s
  Domyślne zachowanie (bez tej zmiennej) jest identyczne jak wcześniej —
  guard jest zawsze aktywny w normalnej instalacji HackerOS.`,
			styleH1.Render("Testowanie hl bez HackerOS"),
			styleH2.Render("Guard HackerOS"),
			styleOp.Render("hl"),
			styleH2.Render("Opcja 1 — zmienna środowiskowa"),
			styleCode.Render("HL_NOT_ON_HACKEROS=YES hl run skrypt.hl"),
			styleH2.Render("Opcja 2 — prawdziwe katalogi testowe"),
			styleCode.Render("sudo ./scripts/setup-hackeros-env.sh\n;; tworzy katalogi + wpis w /etc/os-release"),
			styleH2.Render("Uwaga"),
		),
	},
}
