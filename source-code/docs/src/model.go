package src

import (
	"strings"

	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
)

type view int

const (
	viewMenu view = iota
	viewContent
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
	showHelp     bool // "?" — nakładka z listą skrótów klawiszowych
}

// InitialModel zwraca początkowy stan aplikacji hl-docs — jedyny
// eksportowany punkt wejścia pakietu `src`, wołany przez main.go
// (`tea.NewProgram(src.InitialModel(), ...)`). Sam typ `model` pozostaje
// nieeksportowany — wołający nigdy nie musi nazywać go wprost, bo
// zwracana wartość jest przekazywana do bubbletea wyłącznie jako
// implementacja interfejsu `tea.Model`.
func InitialModel() model {
	all := allSections()
	allIdx := make([]int, len(all))
	for i := range all {
		allIdx[i] = i
	}
	return model{currentView: viewMenu, cursor: 0, filtered: allIdx}
}

func (m model) Init() tea.Cmd { return nil }

func (m *model) applySearch() {
	all := allSections()
	if m.searchQuery == "" {
		m.filtered = make([]int, len(all))
		for i := range all {
			m.filtered[i] = i
		}
		return
	}
	q := strings.ToLower(m.searchQuery)
	m.filtered = nil
	for i, s := range all {
		if strings.Contains(strings.ToLower(s.Title), q) ||
			strings.Contains(strings.ToLower(s.Category), q) ||
			strings.Contains(strings.ToLower(s.Content), q) {
			m.filtered = append(m.filtered, i)
		}
	}
	m.cursor = 0
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	all := allSections()
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		vpH := m.height - 5
		if vpH < 5 {
			vpH = 5
		}
		if !m.ready {
			m.viewport = viewport.New(m.width-30, vpH)
			m.viewport.Style = styleContent
			m.ready = true
		} else {
			m.viewport.Width = m.width - 30
			m.viewport.Height = vpH
		}
	case tea.KeyMsg:
		if m.showHelp {
			// Nakładka pomocy pochłania wszystkie klawisze poza tym, co ją zamyka.
			switch msg.String() {
			case "?", "esc", "q", "enter", " ":
				m.showHelp = false
			}
			return m, nil
		}
		if m.searchMode {
			switch msg.String() {
			case "esc", "ctrl+c":
				m.searchMode = false
				m.searchQuery = ""
				m.applySearch()
			case "enter":
				m.searchMode = false
				m.applySearch()
			case "backspace":
				if len(m.searchQuery) > 0 {
					m.searchQuery = m.searchQuery[:len(m.searchQuery)-1]
				}
			default:
				if len(msg.String()) == 1 {
					m.searchQuery += msg.String()
				}
			}
			return m, nil
		}
		switch msg.String() {
		case "q", "ctrl+c":
			return m, tea.Quit
		case "?":
			m.showHelp = true
		case "/":
			m.searchMode = true
			m.searchQuery = ""
			m.currentView = viewMenu
		case "esc":
			if m.currentView == viewContent {
				m.currentView = viewMenu
			}
		case "up", "k":
			if m.currentView == viewMenu {
				if m.cursor > 0 {
					m.cursor--
				}
			} else {
				m.viewport.LineUp(3)
			}
		case "down", "j":
			if m.currentView == viewMenu {
				if m.cursor < len(m.filtered)-1 {
					m.cursor++
				}
			} else {
				m.viewport.LineDown(3)
			}
		case "enter", " ":
			if m.currentView == viewMenu && len(m.filtered) > 0 {
				m.sectionIndex = m.filtered[m.cursor]
				m.currentView = viewContent
				if m.ready {
					m.viewport.SetContent(all[m.sectionIndex].Content)
					m.viewport.GotoTop()
				}
			}
		case "tab":
			if m.currentView == viewMenu {
				if len(m.filtered) > 0 {
					m.sectionIndex = m.filtered[m.cursor]
					m.currentView = viewContent
					if m.ready {
						m.viewport.SetContent(all[m.sectionIndex].Content)
						m.viewport.GotoTop()
					}
				}
			} else {
				m.currentView = viewMenu
			}
		case "n":
			if m.currentView == viewContent {
				m.jumpCategory(1)
			}
		case "p":
			if m.currentView == viewContent {
				m.jumpCategory(-1)
			}
		case "pgup":
			if m.currentView == viewContent {
				m.viewport.HalfViewUp()
			}
		case "pgdown":
			if m.currentView == viewContent {
				m.viewport.HalfViewDown()
			}
		case "g":
			if m.currentView == viewContent {
				m.viewport.GotoTop()
			}
		case "G":
			if m.currentView == viewContent {
				m.viewport.GotoBottom()
			}
		}
	}
	if m.currentView == viewContent {
		var cmd tea.Cmd
		m.viewport, cmd = m.viewport.Update(msg)
		return m, cmd
	}
	return m, nil
}

// jumpCategory przeskakuje do pierwszej sekcji następnej/poprzedniej kategorii
// względem aktualnie otwartej (klawisze "n" / "p" w widoku treści).
func (m *model) jumpCategory(dir int) {
	all := allSections()
	cur := all[m.sectionIndex].Category
	// znajdź granice kategorii w pełnej (nieprzefiltrowanej) liście
	firstOfCat := map[string]int{}
	order := []string{}
	for i, s := range all {
		if _, ok := firstOfCat[s.Category]; !ok {
			firstOfCat[s.Category] = i
			order = append(order, s.Category)
		}
	}
	curIdx := 0
	for i, c := range order {
		if c == cur {
			curIdx = i
			break
		}
	}
	next := curIdx + dir
	if next < 0 || next >= len(order) {
		return
	}
	m.sectionIndex = firstOfCat[order[next]]
	if m.ready {
		m.viewport.SetContent(all[m.sectionIndex].Content)
		m.viewport.GotoTop()
	}
}
