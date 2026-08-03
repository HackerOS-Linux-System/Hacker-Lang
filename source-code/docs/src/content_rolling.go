package main

import "fmt"

// ── ROLLING ──────────────────────────────────────────────────────────────────
var rollingSections = []DocSection{
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
}
