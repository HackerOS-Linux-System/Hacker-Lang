package main

import (
	"fmt"
	"os"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/bubbles/list"
	"github.com/charmbracelet/lipgloss"
)

var (
	titleStyle = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("201")).Underline(true).MarginBottom(1)
	descStyle  = lipgloss.NewStyle().Margin(1, 0)
	exampleStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("6")).Border(lipgloss.RoundedBorder(), true).Padding(1)
	instructionsStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("8")).Italic(true).MarginTop(1)
)

type state int

const (
	listState state = iota
	detailState
)

type command struct {
	name string
	desc string
	args string
}

func (c command) Title() string       { return c.name }
func (c command) Description() string { return c.desc }
func (c command) FilterValue() string { return c.name + " " + c.desc + " " + c.args }

type model struct {
	list      list.Model
	viewState state
	selected  *command
	width     int
	height    int
}

func initialModel() model {
	commands := []list.Item{
		command{"run", "Execute a .hacker script", "file [--verbose] or . for bytes project"},
		command{"compile", "Compile to native executable", "file [-o output] [--verbose] [--bytes]"},
		command{"check", "Validate syntax", "file [--verbose]"},
		command{"init", "Generate template script", "file [--verbose]"},
		command{"clean", "Remove temporary files", "[--verbose]"},
		command{"repl", "Launch interactive REPL", "[--verbose]"},
		command{"editor", "Launch hacker-editor", "[file]"},
		command{"unpack", "Unpack and install bytes", "bytes [--verbose]"},
		command{"version", "Display version", ""},
		command{"help", "Show this help menu", ""},
		command{"help-ui", "Show special commands list", ""},
	}

	delegate := list.NewDefaultDelegate()
	delegate.Styles.SelectedTitle = delegate.Styles.SelectedTitle.Foreground(lipgloss.Color("5")).Bold(true)

	l := list.New(commands, delegate, 0, 0)
	l.Title = "Hacker Lang CLI Commands"
	l.Styles.Title = titleStyle
	l.SetShowStatusBar(false)
	l.SetFilteringEnabled(true)

	return model{
		list:      l,
		viewState: listState,
	}
}

func (m model) Init() tea.Cmd {
	return nil
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmd tea.Cmd

	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		m.list.SetWidth(msg.Width)
		m.list.SetHeight(msg.Height - 4) // Leave space for instructions
		return m, nil

	case tea.KeyMsg:
		switch msg.String() {
		case "q", "ctrl+c":
			return m, tea.Quit
		case "esc":
			if m.viewState == detailState {
				m.viewState = listState
				m.selected = nil
			}
			return m, nil
		case "enter":
			if m.viewState == listState {
				if i, ok := m.list.SelectedItem().(command); ok {
					m.selected = &i
					m.viewState = detailState
				}
			}
			return m, nil
		}
	}

	if m.viewState == listState {
		m.list, cmd = m.list.Update(msg)
		return m, cmd
	}

	return m, nil
}

func (m model) View() string {
	content := ""
	if m.viewState == listState {
		content = m.list.View()
	} else if m.selected != nil {
		title := titleStyle.Render(m.selected.name)
		desc := descStyle.Render("Description: " + m.selected.desc)
		args := descStyle.Render("Arguments: " + m.selected.args)
		exampleCode := `// sudo
# obsidian
@USER=admin
=2 > echo $USER
? [ -d /tmp ] > echo OK
& sleep 10
# logging
> sudo apt update
[
Config=Example
]`
		example := exampleStyle.Render("Syntax Example:\n\n" + exampleCode)
		content = lipgloss.JoinVertical(lipgloss.Left, title, desc, args, example)
	}

	instructions := instructionsStyle.Render("q: quit, esc: back, enter: select")
	return lipgloss.JoinVertical(lipgloss.Left, content, instructions)
}

func main() {
	p := tea.NewProgram(initialModel(), tea.WithAltScreen())
	if _, err := p.Run(); err != nil {
		fmt.Printf("Error: %v", err)
		os.Exit(1)
	}
}
