package main

import "fmt"

// ── COREUTILS ────────────────────────────────────────────────────────────────
// Nowa kategoria: wbudowane narzędzia `/>` (hl-coreutils, source-code/coreutils/,
// jedno narzędzie = jeden plik). Zero forka/exec — działają nawet bez binarek
// systemowych, dają spójne błędy w stylu Hacker Lang.
var coreutilsSections = []DocSection{
	{
		Title:    "/> — wbudowane coreutils",
		Category: "COREUTILS",
		Content: fmt.Sprintf(`%s

%s
  Operator %s uruchamia narzędzie wbudowane w interpreter (crate
  %s) zamiast fork/exec prawdziwej binarki. Działa identycznie
  na każdym systemie, nawet bez zainstalowanych coreutils.

%s
%s

%s
  %s  — dostępne wbudowane narzędzie
  %s   — wpadamy do prawdziwego /bin/echo (fork+exec)

%s
  Każde narzędzie to osobny plik w %s.`,
			styleH1.Render("◆ /> — wbudowane coreutils"),
			styleH2.Render("Czym się różni od >"),
			styleOp.Render("/>"),
			styleOp.Render("hl-coreutils"),
			styleH2.Render("Przykład"),
			styleCode.Render("/> cat plik.txt\n/> ls /etc\n/> grep -i blad log.txt |> @wynik"),
			styleH2.Render("Porównanie"),
			styleOp.Render("/> echo hello"),
			styleOp.Render("> echo hello"),
			styleH2.Render("Uwaga"),
			styleOp.Render("source-code/coreutils/src/tools/"),
		),
	},
	{
		Title:    "/> — pliki i tekst",
		Category: "COREUTILS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s`,
			styleH1.Render("/> — pliki i tekst"),
			styleH2.Render("Odczyt / listowanie"),
			styleCode.Render("/> cat plik.txt\n/> ls -a /katalog\n/> stat plik.txt\n/> find /etc -name \"*.conf\"\n/> du -h /var/log"),
			styleH2.Render("Przetwarzanie tekstu"),
			styleCode.Render("/> grep -in blad log.txt\n/> head -n 5 plik.txt\n/> tail -n 20 plik.txt\n/> sort -r lista.txt\n/> uniq lista.txt\n/> cut -d: -f1 /etc/passwd\n/> tr 'a-z' 'A-Z' |> @wynik\n/> rev plik.txt\n/> wc -l plik.txt"),
		),
	},
	{
		Title:    "/> — pliki i uprawnienia",
		Category: "COREUTILS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s`,
			styleH1.Render("/> — zarządzanie plikami"),
			styleH2.Render("Kopiowanie / przenoszenie / usuwanie"),
			styleCode.Render("/> cp -r src dst\n/> mv stary nowy\n/> rm -rf katalog\n/> mkdir -p a/b/c\n/> touch nowy_plik.txt"),
			styleH2.Render("Uprawnienia"),
			styleCode.Render("/> chmod 755 skrypt.sh\n/> basename /usr/bin/hl\n/> dirname /usr/bin/hl"),
		),
	},
	{
		Title:    "/> — system i generatory",
		Category: "COREUTILS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s`,
			styleH1.Render("/> — system i generatory"),
			styleH2.Render("Informacje o systemie"),
			styleCode.Render("/> pwd\n/> which curl\n/> env\n/> date +%Y-%m-%d\n/> sleep 2"),
			styleH2.Render("Generowanie wyjścia"),
			styleCode.Render("/> seq 1 10\n/> printf '%s: %d\\n' test 42\n/> yes ok |> @wynik\n/> xargs -n1 echo <<< \"a b c\""),
		),
	},
}
