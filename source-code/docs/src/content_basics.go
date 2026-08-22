package src

import "fmt"

// ── PODSTAWY ──────────────────────────────────────────────────────────────────
var basicsSections = []DocSection{
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
}
