package main

import (
	"fmt"

	"github.com/charmbracelet/bubbles/key"
	"github.com/charmbracelet/bubbles/list"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type item struct {
	title, desc, details string
}

func (i item) Title() string       { return i.title }
func (i item) Description() string { return i.desc }
func (i item) FilterValue() string { return i.title }

type helpUIModel struct {
	list list.Model
}

func (m helpUIModel) Init() tea.Cmd {
	return nil
}

func (m helpUIModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
		case tea.KeyMsg:
			if msg.String() == "esc" || msg.String() == "q" {
				return m, tea.Quit
			}
			if msg.String() == "enter" {
				selected := m.list.SelectedItem().(item)
				fmt.Printf("%s\nDetails: %s\n", selected.title, selected.details)
				return m, nil
			}
		case tea.WindowSizeMsg:
			h, v := lipgloss.NewStyle().GetFrameSize()
			m.list.SetSize(msg.Width-h, msg.Height-v)
	}
	var cmd tea.Cmd
	m.list, cmd = m.list.Update(msg)
	return m, cmd
}

func (m helpUIModel) View() string {
	return m.list.View()
}

func runHelpUI() bool {
	items := []list.Item{
		item{title: "run", desc: "Execute script", details: "Usage: hackerc run <file> [--verbose] or run . for bytes project"},
		item{title: "compile", desc: "Compile to executable", details: "Usage: hackerc compile <file> [-o output] [--verbose] [--bytes]"},
		item{title: "check", desc: "Validate syntax", details: "Usage: hackerc check <file> [--verbose]"},
		item{title: "init", desc: "Generate template", details: "Usage: hackerc init <file> [--verbose]"},
		item{title: "clean", desc: "Remove temps", details: "Usage: hackerc clean [--verbose]"},
		item{title: "repl", desc: "Interactive REPL", details: "Usage: hackerc repl [--verbose]"},
		item{title: "unpack", desc: "Unpack and install bytes", details: "Usage: hackerc unpack bytes [--verbose]"},
		item{title: "version", desc: "Show version", details: "Usage: hackerc version"},
		item{title: "help", desc: "Show help", details: "Usage: hackerc help"},
		item{title: "help-ui", desc: "Interactive help UI", details: "This UI"},
	}

	m := helpUIModel{list: list.New(items, list.NewDefaultDelegate(), 0, 0)}
	m.list.Title = "Hacker Lang Commands"
	m.list.KeyMap.NextPage = key.NewBinding(key.WithKeys("pgdown", "d"))
	m.list.KeyMap.PrevPage = key.NewBinding(key.WithKeys("pgup", "u"))

	p := tea.NewProgram(m, tea.WithAltScreen())

	if _, err := p.Run(); err != nil {
		fmt.Println("Error running UI:", err)
		return false
	}
	return true
}
