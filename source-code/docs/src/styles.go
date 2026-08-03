package main

import "github.com/charmbracelet/lipgloss"

// ── Paleta kolorów ──────────────────────────────────────────────────────────
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
)

// ── Style tekstu ─────────────────────────────────────────────────────────────
var (
	styleH1 = lipgloss.NewStyle().Foreground(colorMagenta).Bold(true).MarginTop(1).MarginBottom(1)
	styleH2 = lipgloss.NewStyle().Foreground(colorAccent).Bold(true).MarginTop(1)
	styleH3 = lipgloss.NewStyle().Foreground(colorGreen).Bold(true)
	styleOp = lipgloss.NewStyle().Foreground(colorGreen).Bold(true)

	styleCode = lipgloss.NewStyle().Background(lipgloss.Color("#161b22")).Foreground(colorCyan).Padding(0, 1).Margin(0, 2)
	styleTip  = lipgloss.NewStyle().Foreground(colorYellow).Background(lipgloss.Color("#1c1a00")).Padding(0, 1).Margin(0, 2)
	styleWarn = lipgloss.NewStyle().Foreground(colorOrange).Background(lipgloss.Color("#1a1200")).Padding(0, 1).Margin(0, 2)
)

// ── Style layoutu ────────────────────────────────────────────────────────────
var (
	styleMenuNormal   = lipgloss.NewStyle().Foreground(colorText).Padding(0, 2)
	styleMenuSelected = lipgloss.NewStyle().Foreground(colorAccent).Background(colorSelected).Bold(true).Padding(0, 2)
	styleMenuCategory = lipgloss.NewStyle().Foreground(colorYellow).Bold(true).Padding(0, 2).MarginTop(1)

	styleSidebar   = lipgloss.NewStyle().Background(colorPanel).BorderStyle(lipgloss.NormalBorder()).BorderRight(true).BorderForeground(colorBorder).Padding(1, 0)
	styleContent   = lipgloss.NewStyle().Background(colorBg).Padding(0, 2)
	styleStatusBar = lipgloss.NewStyle().Background(colorPanel).Foreground(colorMuted).Padding(0, 2)
	styleHeader    = lipgloss.NewStyle().Background(colorPanel).Foreground(colorText).Bold(true).Padding(0, 2)

	styleBreadcrumb = lipgloss.NewStyle().Foreground(colorMuted).Padding(0, 2)
	styleHelpBox    = lipgloss.NewStyle().Background(colorPanel).Foreground(colorText).
			BorderStyle(lipgloss.RoundedBorder()).BorderForeground(colorAccent).Padding(1, 3)
	styleHelpKey  = lipgloss.NewStyle().Foreground(colorGreen).Bold(true)
	styleHelpDesc = lipgloss.NewStyle().Foreground(colorText)
)
