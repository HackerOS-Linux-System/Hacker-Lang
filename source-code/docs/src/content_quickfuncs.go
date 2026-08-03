package main

import "fmt"

// ── QUICK FUNCTIONS ──────────────────────────────────────────────────────────────────
var quickFuncSections = []DocSection{
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
}
