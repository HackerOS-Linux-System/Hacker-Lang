package main

// DocSection to pojedynczy wpis dokumentacji widoczny w menu po lewej.
// Treść każdej sekcji jest renderowana od razu (nie leniwie) — koszt
// pamięciowy jest znikomy nawet przy dużej liczbie sekcji, a upraszcza to
// kod (bez cache'owania renderów).
type DocSection struct {
	Title    string
	Category string
	Content  string
}

// allSections łączy sekcje ze wszystkich plików content_*.go w jedną listę,
// w kolejności w jakiej mają się pojawiać w menu. Podział na kategorie robi
// sam model (grupuje po polu Category w kolejności występowania).
func allSections() []DocSection {
	out := make([]DocSection, 0, 64)
	out = append(out, basicsSections...)
	out = append(out, gen1Sections...)
	out = append(out, gen2Sections...)
	out = append(out, quickFuncSections...)
	out = append(out, coreutilsSections...)
	out = append(out, toolsSections...)
	out = append(out, rollingSections...)
	out = append(out, tutorialSections...)
	return out
}
