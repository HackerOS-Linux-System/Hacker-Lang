package main

import (
	"fmt"
	"strings"

	"github.com/charmbracelet/lipgloss"
)

func (m model) renderMenu() string {
	var sb strings.Builder
	sidebarW := 28
	all := allSections()
	currentCategory := ""
	for i, idx := range m.filtered {
		s := all[idx]
		if s.Category != currentCategory {
			currentCategory = s.Category
			sb.WriteString(styleMenuCategory.Width(sidebarW).Render(currentCategory) + "\n")
		}
		title := "  " + s.Title
		if len(title) > sidebarW-2 {
			title = title[:sidebarW-5] + "..."
		}
		if i == m.cursor {
			sb.WriteString(styleMenuSelected.Width(sidebarW).Render(title) + "\n")
		} else {
			sb.WriteString(styleMenuNormal.Width(sidebarW).Render(title) + "\n")
		}
	}
	return sb.String()
}

// renderHelpOverlay rysuje nakładkę ze skrótami klawiszowymi (klawisz "?").
func (m model) renderHelpOverlay() string {
	rows := [][2]string{
		{"↑↓ / j k", "nawigacja w menu / przewijanie treści"},
		{"Enter / Tab", "otwórz zaznaczoną sekcję"},
		{"Esc", "wróć do menu"},
		{"/", "szukaj (tytuł, kategoria, treść)"},
		{"n / p", "następna / poprzednia kategoria (w treści)"},
		{"g / G", "góra / dół treści"},
		{"PgUp / PgDn", "przewiń stronę"},
		{"?", "ta pomoc"},
		{"q / Ctrl+C", "wyjście"},
	}
	var sb strings.Builder
	sb.WriteString(styleH1.Render("Skróty klawiszowe") + "\n\n")
	for _, r := range rows {
		sb.WriteString(fmt.Sprintf("  %s  %s\n",
			styleHelpKey.Width(14).Render(r[0]),
			styleHelpDesc.Render(r[1])))
	}
	sb.WriteString("\n" + styleMuted().Render("naciśnij dowolny klawisz, żeby zamknąć"))
	return styleHelpBox.Render(sb.String())
}

func styleMuted() lipgloss.Style {
	return lipgloss.NewStyle().Foreground(colorMuted)
}

func (m model) View() string {
	if !m.ready {
		return "\n  Ladowanie..."
	}
	all := allSections()
	sidebarW := 30
	logoStyle := lipgloss.NewStyle().Foreground(colorMagenta).Bold(true).Width(sidebarW).Align(lipgloss.Center)
	versionStyle := lipgloss.NewStyle().Foreground(colorMuted).Width(m.width - sidebarW).Align(lipgloss.Right).PaddingRight(2)
	header := lipgloss.JoinHorizontal(lipgloss.Top,
		logoStyle.Render("◆ HACKER LANG DOCS"),
		versionStyle.Render("gen 1 / gen 2 / ROLLING  hl-docs"))
	headerBar := styleHeader.Width(m.width).Render(header)

	var contentSection string
	if m.currentView == viewContent {
		m.viewport.Width = m.width - sidebarW - 2
		breadcrumb := styleBreadcrumb.Render(fmt.Sprintf("%s › %s", all[m.sectionIndex].Category, all[m.sectionIndex].Title))
		contentSection = lipgloss.JoinVertical(lipgloss.Left, breadcrumb, m.viewport.View())
	} else {
		welcome := fmt.Sprintf("%s\n\n%s\n\n%s\n\n%s",
			styleH1.Render("Dokumentacja Hacker Lang"),
			lipgloss.NewStyle().Foreground(colorText).Render(fmt.Sprintf("Wybierz temat z menu po lewej (%d sekcji).\nObslugiwane: gen 1, gen 2, ROLLING", len(all))),
			lipgloss.NewStyle().Foreground(colorMuted).Render("↑↓ / j k — nawigacja\nEnter — otwórz\n/ — szukaj\n? — skróty klawiszowe\nq — wyjdz"),
			styleTip.Render("💡 ROLLING = najnowsze funkcje pre-gen 3"),
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
		statusLeft = lipgloss.NewStyle().Foreground(colorAccent).Render(fmt.Sprintf("  %s", all[m.sectionIndex].Title))
	} else {
		statusLeft = lipgloss.NewStyle().Foreground(colorMuted).Render(fmt.Sprintf("  %d sekcji", len(m.filtered)))
	}
	navHints := "↑↓/jk: nav  Enter: open  /: search  ?: help  q: quit"
	if m.currentView == viewContent {
		navHints = "↑↓/jk: scroll  n/p: category  Esc: menu  ?: help  q: quit"
	}
	statusRight := lipgloss.NewStyle().Foreground(colorMuted).Render(navHints + "  ")
	statusBar := lipgloss.JoinHorizontal(lipgloss.Top,
		statusLeft,
		lipgloss.NewStyle().Width(m.width-lipgloss.Width(statusLeft)-lipgloss.Width(statusRight)).Render(""),
		statusRight,
	)
	full := lipgloss.JoinVertical(lipgloss.Left, headerBar, body, styleStatusBar.Width(m.width).Render(statusBar))

	if m.showHelp {
		overlay := m.renderHelpOverlay()
		return lipgloss.Place(m.width, m.height, lipgloss.Center, lipgloss.Center, overlay,
			lipgloss.WithWhitespaceChars(" "), lipgloss.WithWhitespaceForeground(colorMuted))
	}
	return full
}
