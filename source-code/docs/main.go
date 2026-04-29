package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

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

	styleH1 = lipgloss.NewStyle().Foreground(colorMagenta).Bold(true).MarginTop(1).MarginBottom(1)
	styleH2 = lipgloss.NewStyle().Foreground(colorAccent).Bold(true).MarginTop(1)
	styleOp = lipgloss.NewStyle().Foreground(colorGreen).Bold(true)

	styleCode = lipgloss.NewStyle().Background(lipgloss.Color("#161b22")).Foreground(colorCyan).Padding(0, 1).Margin(0, 2)

	styleTip = lipgloss.NewStyle().Foreground(colorYellow).Background(lipgloss.Color("#1c1a00")).Padding(0, 1).Margin(0, 2)

	styleMenuNormal   = lipgloss.NewStyle().Foreground(colorText).Padding(0, 2)
	styleMenuSelected = lipgloss.NewStyle().Foreground(colorAccent).Background(colorSelected).Bold(true).Padding(0, 2)
	styleMenuCategory = lipgloss.NewStyle().Foreground(colorYellow).Bold(true).Padding(0, 2).MarginTop(1)

	styleSidebar   = lipgloss.NewStyle().Background(colorPanel).BorderStyle(lipgloss.NormalBorder()).BorderRight(true).BorderForeground(colorBorder).Padding(1, 0)
	styleContent   = lipgloss.NewStyle().Background(colorBg).Padding(0, 2)
	styleStatusBar = lipgloss.NewStyle().Background(colorPanel).Foreground(colorMuted).Padding(0, 2)
	styleHeader    = lipgloss.NewStyle().Background(colorPanel).Foreground(colorText).Bold(true).Padding(0, 2)
)

type DocSection struct {
	Title    string
	Category string
	Content  string
}

var sections = []DocSection{
	{
		Title:    "Witaj w Hacker Lang gen 1",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

Hacker Lang (HL) to jezyk skryptowy stworzony dla HackerOS.

%s
%s

%s
  Nie ma tutaj echo - jest %s
  Sudo to nie slowo kluczowe - to operator %s
  Zmienne: %s  |  Export: %s
  Tlo: %s  |  Hsh: %s
  Petla: %s  |  Import pliku: %s
  Goroutine: %s  |  Channel: %s

%s

Uzyj strzalek. %s = wyjscie.`,
			styleH1.Render("◆ Hacker Lang gen 1"),
			styleH2.Render("Szybki start"),
			styleCode.Render("hl run skrypt.hl\nhl compile skrypt.hl  # → .bc\nhl compile skrypt.bc  # → ELF"),
			styleH2.Render("Kluczowe operatory"),
			styleOp.Render("~>"),
			styleOp.Render("^>"),
			styleOp.Render("%/@"),
			styleOp.Render("=>"),
			styleOp.Render("& (background)"),
			styleOp.Render("*> (hsh)"),
			styleOp.Render("_N"),
			styleOp.Render("<< plik.hl"),
			styleOp.Render(":*"),
			styleOp.Render(":**/*--"),
			styleTip.Render("💡 TIP: Kazda linia zaczyna sie od operatora."),
			styleOp.Render("q"),
		),
	},
	{
		Title:    "Kompilacja .hl → .bc → ELF",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

%s
  Kompilacja jest dwuetapowa w gen 1.

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("Kompilacja"),
			styleH2.Render("Pipeline"),
			styleH2.Render("Etap 1: .hl → .bc (bytecode)"),
			styleCode.Render("hl compile skrypt.hl\n# → skrypt.bc\n# Plik .bc ma shebang i jest wykonywalny"),
			styleH2.Render("Etap 2: .bc → ELF"),
			styleCode.Render("hl compile skrypt.bc\n# → skrypt (binarka ELF x86_64)"),
			styleH2.Render("Uruchamianie"),
			styleCode.Render("hl run skrypt.bc\n./skrypt.bc          # przez shebang\n./skrypt             # binarka ELF"),
		),
	},
	{
		Title:    "Manager pakietow bit",
		Category: "PODSTAWY",
		Content: fmt.Sprintf(`%s

%s
  bit to manager pakietow napisany w Hacker Lang.

%s
%s

%s
%s

%s
  Wywolaj bit bez argumentow = wyswietli pomoc.`,
			styleH1.Render("bit — Package Manager"),
			styleH2.Render("O bit"),
			styleH2.Render("Komendy"),
			styleCode.Render("bit install <nazwa>\nbit remove  <nazwa>\nbit list\nbit update\nbit info <nazwa>"),
			styleH2.Render("Uzycie w .hl"),
			styleCode.Render("# <bit/hashlib>\n# <bit/obsidian>\n# <bit/yuy>"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "Tlo & i hsh *>",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
  @_bg_pid zawiera PID ostatniego procesu w tle.`,
			styleH1.Render("Operatory & i *>"),
			styleH2.Render("& — uruchom w tle"),
			styleCode.Render("& python3 -m http.server 8080\n& redis-server\n~> PID: @_bg_pid\n\n;; Rownolegly download:\n& wget -q https://example.com/a.zip\n& wget -q https://example.com/b.zip\n& wget -q https://example.com/c.zip"),
			styleH2.Render("*> — uruchom przez hsh -c"),
			styleCode.Render("*> ls -la\n*> notify-send \"Gotowe\"\n;; *> nie interpoluje @zmiennych HL\n;; uzyj >> dla interpolacji"),
			styleH2.Render("Roznica"),
		),
	},
	{
		Title:    "Petla _N",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  Dziala z komendami, zmiennymi, quick-funkcjami i innymi operatorami.`,
			styleH1.Render("Petla _N — powtorz N razy"),
			styleH2.Render("Przyklady"),
			styleCode.Render("_10 > hacker update\n_5  ~> Powtarzam...\n_3  ::green OK\n_100 > ping -c 1 @target\n\n;; Z interpolacja:\n_10 >> curl -s http://@host/test\n\n;; Z quick-funkcja:\n_20 ::nl"),
			styleH2.Render("Uwaga"),
		),
	},
	{
		Title:    "Import pliku <<",
		Category: "SKLADNIA",
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
		Title:    "Goroutines i Channels",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
%s`,
			styleH1.Render("Goroutines i Channels"),
			styleH2.Render(":* — goroutine"),
			styleCode.Render(":*\n    > nmap -sn 192.168.1.0/24\n    > echo done\ndone"),
			styleH2.Render(":** — channel i *-- — channel op"),
			styleCode.Render(":** wyniki\n\n:*\n    > ls /tmp\n    *-- wyniki\ndone\n\n;; Odbierz:\n*-- wyniki"),
			styleH2.Render("Uwaga"),
			styleCode.Render(";; Goroutines dzialaja w trybie interpretowanym.\n;; W skompilowanych .bc → ELF sa pomijane.\n;; Dla kompilacji uzyj & (background)."),
		),
	},
	{
		Title:    "Importy gen 1",
		Category: "SKLADNIA",
		Content: fmt.Sprintf(`%s

%s
%s

%s
%s

%s
  Stara skladnia (std/, virus/, community/) jest automatycznie
  normalizowana — nadal dziala.`,
			styleH1.Render("Importy — gen 1"),
			styleH2.Render("Nowe przestrzenie nazw"),
			styleCode.Render("# <main/net>          ;; std/net\n# <main/fs>           ;; std/fs\n# <main/colors>       ;; NOWE\n# <main/cli>          ;; NOWE\n# <main/progress-bar> ;; NOWE\n# <main/json>         ;; NOWE\n# <main/hk-parser>    ;; NOWE\n# <main/hacker>       ;; NOWE\n# <bit/hashlib>       ;; virus/hashlib\n# <github/user/repo>  ;; community/user/repo"),
			styleH2.Render("Biblioteki main/ sa plikami .hl w"),
			styleCode.Render("/usr/lib/HackerOS/Hacker-Lang/main-libs/"),
			styleH2.Render("Kompatybilnosc wstecz"),
		),
	},
	{
		Title:    "Quick Functions (::)",
		Category: "QUICK FUNCTIONS",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  upper, lower, len, trim, rev, repeat, replace, contains
  startswith, endswith, split, lines, words
  abs, ceil, floor, round, max, min, rand
  env, date, time, pid, which
  exists, isdir, isfile, basename, dirname, read
  set, get, type, unset
  nl, hr, bold, red, green, yellow, cyan`,
			styleH1.Render("Quick Functions ::"),
			styleH2.Render("Przyklad"),
			styleCode.Render("::upper hello world\n;; HELLO WORLD\n\n::exists /etc/passwd\n? ok\n    ::green Plik istnieje!\ndone\n\n::hr 40\n::bold Naglowek\n::hr 40"),
			styleH2.Render("Pelna lista"),
		),
	},
	{
		Title:    "Tutorial: Pelny przyklad gen 1",
		Category: "TUTORIALE",
		Content: fmt.Sprintf(`%s

%s
%s`,
			styleH1.Render("Tutorial: gen 1 full"),
			styleH2.Render("Wszystkie nowosci gen 1 w jednym skrypcie"),
			styleCode.Render("#!/usr/bin/env hl\n/// Pelny przyklad gen 1\nusing <gen 1>\n\n// curl\n# <main/colors>\n# <main/json>\n<< helpers.hl\n\n% target = 192.168.1.1\n\n;; Tlo\n& python3 -m http.server 9000\n~> @COLOR_GREEN HTTP PID: @_bg_pid @COLOR_RESET\n\n;; Hsh\n*> notify-send \"Start\"\n\n;; Petla\n_3 > ping -c 1 @target\n\n;; Channel + goroutine\n:** wyniki\n:*\n    >> curl -s http://@target\n    *-- wyniki\ndone\n*-- wyniki\n\n~> Gotowe!"),
		),
	},
}

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
	filtered     []int
}

func initialModel() model {
	allIdx := make([]int, len(sections))
	for i := range sections { allIdx[i] = i }
	return model{currentView: viewMenu, cursor: 0, filtered: allIdx}
}

func (m model) Init() tea.Cmd { return nil }

func (m *model) applySearch() {
	if m.searchQuery == "" {
		m.filtered = make([]int, len(sections))
		for i := range sections { m.filtered[i] = i }
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

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		vpH := m.height - 5
		if vpH < 5 { vpH = 5 }
		if !m.ready {
			m.viewport = viewport.New(m.width-30, vpH)
			m.viewport.Style = styleContent
			m.ready = true
		} else {
			m.viewport.Width = m.width - 30
			m.viewport.Height = vpH
		}
	case tea.KeyMsg:
		if m.searchMode {
			switch msg.String() {
			case "esc", "ctrl+c":
				m.searchMode = false; m.searchQuery = ""; m.applySearch()
			case "enter":
				m.searchMode = false; m.applySearch()
			case "backspace":
				if len(m.searchQuery) > 0 { m.searchQuery = m.searchQuery[:len(m.searchQuery)-1] }
			default:
				if len(msg.String()) == 1 { m.searchQuery += msg.String() }
			}
			return m, nil
		}
		switch msg.String() {
		case "q", "ctrl+c": return m, tea.Quit
		case "/": m.searchMode = true; m.searchQuery = ""; m.currentView = viewMenu
		case "esc":
			if m.currentView == viewContent { m.currentView = viewMenu }
		case "up", "k":
			if m.currentView == viewMenu { if m.cursor > 0 { m.cursor-- } } else { m.viewport.LineUp(3) }
		case "down", "j":
			if m.currentView == viewMenu { if m.cursor < len(m.filtered)-1 { m.cursor++ } } else { m.viewport.LineDown(3) }
		case "enter", " ":
			if m.currentView == viewMenu && len(m.filtered) > 0 {
				m.sectionIndex = m.filtered[m.cursor]
				m.currentView = viewContent
				if m.ready { m.viewport.SetContent(sections[m.sectionIndex].Content); m.viewport.GotoTop() }
			}
		case "tab":
			if m.currentView == viewMenu {
				if len(m.filtered) > 0 {
					m.sectionIndex = m.filtered[m.cursor]; m.currentView = viewContent
					if m.ready { m.viewport.SetContent(sections[m.sectionIndex].Content); m.viewport.GotoTop() }
				}
			} else { m.currentView = viewMenu }
		case "pgup": if m.currentView == viewContent { m.viewport.HalfViewUp() }
		case "pgdown": if m.currentView == viewContent { m.viewport.HalfViewDown() }
		case "g": if m.currentView == viewContent { m.viewport.GotoTop() }
		case "G": if m.currentView == viewContent { m.viewport.GotoBottom() }
		}
	}
	if m.currentView == viewContent {
		var cmd tea.Cmd
		m.viewport, cmd = m.viewport.Update(msg)
		return m, cmd
	}
	return m, nil
}

func (m model) renderMenu() string {
	var sb strings.Builder
	sidebarW := 28
	currentCategory := ""
	for i, idx := range m.filtered {
		s := sections[idx]
		if s.Category != currentCategory {
			currentCategory = s.Category
			sb.WriteString(styleMenuCategory.Width(sidebarW).Render(s.Category) + "\n")
		}
		title := "  " + s.Title
		if len(title) > sidebarW-2 { title = title[:sidebarW-5] + "..." }
		if i == m.cursor {
			sb.WriteString(styleMenuSelected.Width(sidebarW).Render(title) + "\n")
		} else {
			sb.WriteString(styleMenuNormal.Width(sidebarW).Render(title) + "\n")
		}
	}
	return sb.String()
}

func (m model) View() string {
	if !m.ready { return "\n  Ladowanie..." }
	sidebarW := 30
	logoStyle := lipgloss.NewStyle().Foreground(colorMagenta).Bold(true).Width(sidebarW).Align(lipgloss.Center)
	versionStyle := lipgloss.NewStyle().Foreground(colorMuted).Width(m.width - sidebarW).Align(lipgloss.Right).PaddingRight(2)
	header := lipgloss.JoinHorizontal(lipgloss.Top, logoStyle.Render("◆ HACKER LANG DOCS"), versionStyle.Render("gen 1  hl-docs"))
	headerBar := styleHeader.Width(m.width).Render(header)

	var contentSection string
	if m.currentView == viewContent {
		m.viewport.Width = m.width - sidebarW - 2
		contentSection = m.viewport.View()
	} else {
		welcome := fmt.Sprintf("%s\n\n%s\n\n%s\n\n%s",
			styleH1.Render("Dokumentacja Hacker Lang gen 1"),
			lipgloss.NewStyle().Foreground(colorText).Render("Wybierz temat z menu po lewej."),
			lipgloss.NewStyle().Foreground(colorMuted).Render("Strzalki ↑↓ / j k — nawigacja\nEnter / Spacja — otwórz"),
			styleTip.Render("💡 / — szukaj"),
		)
		contentSection = lipgloss.NewStyle().Width(m.width-sidebarW-2).Height(m.height-5).Padding(2, 3).Render(welcome)
	}

	body := lipgloss.JoinHorizontal(lipgloss.Top,
		styleSidebar.Width(sidebarW).Height(m.height-5).Render(m.renderMenu()),
		styleContent.Width(m.width-sidebarW-2).Render(contentSection),
	)

	var statusLeft string
	if m.searchMode {
		statusLeft = lipgloss.NewStyle().Foreground(colorYellow).Bold(true).Render(fmt.Sprintf("SZUKAJ: %s_", m.searchQuery))
	} else if m.currentView == viewContent {
		statusLeft = lipgloss.NewStyle().Foreground(colorAccent).Render(fmt.Sprintf("  %s", sections[m.sectionIndex].Title))
	} else {
		statusLeft = lipgloss.NewStyle().Foreground(colorMuted).Render(fmt.Sprintf("  %d sekcji", len(m.filtered)))
	}
	navHints := "↑↓/jk: nav  Enter: open  /: search  q: quit"
	if m.currentView == viewContent { navHints = "↑↓/jk: scroll  PgUp/Dn: page  Esc: menu  q: quit" }
	statusRight := lipgloss.NewStyle().Foreground(colorMuted).Render(navHints + "  ")
	statusBar := lipgloss.JoinHorizontal(lipgloss.Top,
		statusLeft,
		lipgloss.NewStyle().Width(m.width-lipgloss.Width(statusLeft)-lipgloss.Width(statusRight)).Render(""),
		statusRight,
	)
	return lipgloss.JoinVertical(lipgloss.Left, headerBar, body, styleStatusBar.Width(m.width).Render(statusBar))
}

func main() {
	p := tea.NewProgram(initialModel(), tea.WithAltScreen(), tea.WithMouseCellMotion())
	if _, err := p.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "hl-docs error: %v\n", err)
		os.Exit(1)
	}
}
