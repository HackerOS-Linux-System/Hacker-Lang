package main

import (
	"fmt"
	"strings"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/bubbles/key"
	"github.com/charmbracelet/bubbles/viewport"
	"github.com/charmbracelet/lipgloss"
)

// Supported languages
const (
	LangEnglish = "english"
	LangPolish  = "polish"
	LangSpanish = "spanish"
	LangFrench  = "french"
)

type model struct {
	viewport     viewport.Model
	content      map[string]string // language -> content
	currentLang  string
	sections     []string // List of section titles for navigation
	currentSect  int
	ready        bool
	headerStyle  lipgloss.Style
	footerStyle  lipgloss.Style
	sectionStyle lipgloss.Style
}

func (m model) Init() tea.Cmd {
	return nil
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var (
		cmd  tea.Cmd
		cmds []tea.Cmd
	)

	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch {
		case key.Matches(msg, key.NewBinding(key.WithKeys("q", "esc", "ctrl+c"))):
			return m, tea.Quit
		case key.Matches(msg, key.NewBinding(key.WithKeys("l"))):
			// Cycle languages
			langs := []string{LangEnglish, LangPolish, LangSpanish, LangFrench}
			for i, l := range langs {
				if l == m.currentLang {
					m.currentLang = langs[(i+1)%len(langs)]
					break
				}
			}
			m.updateContent()
		case key.Matches(msg, key.NewBinding(key.WithKeys("n"))):
			// Next section
			m.currentSect = (m.currentSect + 1) % len(m.sections)
			m.updateContent()
		case key.Matches(msg, key.NewBinding(key.WithKeys("p"))):
			// Previous section
			m.currentSect = (m.currentSect - 1 + len(m.sections)) % len(m.sections)
			m.updateContent()
		}
		m.viewport, cmd = m.viewport.Update(msg)
		cmds = append(cmds, cmd)
	case tea.WindowSizeMsg:
		headerHeight := lipgloss.Height(m.headerView())
		footerHeight := lipgloss.Height(m.footerView())
		verticalMarginHeight := headerHeight + footerHeight

		if !m.ready {
			// Since this program is using the full size of the viewport we
			// need to wait until we've received the window dimensions before
			// we can initialize the viewport. The initial dimensions come in
			// quickly, though asynchronously, which is why we wait for them
			// here.
			m.viewport = viewport.New(msg.Width, msg.Height-verticalMarginHeight)
			m.viewport.YPosition = headerHeight
			m.viewport.HighPerformanceRendering = false
			m.ready = true
			m.updateContent()
		} else {
			m.viewport.Width = msg.Width
			m.viewport.Height = msg.Height - verticalMarginHeight
		}
	}

	return m, tea.Batch(cmds...)
}

func (m model) View() string {
	if !m.ready {
		return "\n Initializing..."
	}
	header := m.headerView()
	footer := m.footerView()
	return fmt.Sprintf("%s\n%s\n%s", header, m.viewport.View(), footer)
}

func (m *model) headerView() string {
	title := m.headerStyle.Render(fmt.Sprintf("Hacker Lang Advanced Documentation - %s (Section: %s)", strings.Title(m.currentLang), m.sections[m.currentSect]))
	line := strings.Repeat("─", max(0, m.viewport.Width-lipgloss.Width(title)))
	return lipgloss.JoinHorizontal(lipgloss.Center, title, line)
}

func (m *model) footerView() string {
	help := "q: quit • l: change language • n: next section • p: prev section"
	return m.footerStyle.Render(help)
}

func (m *model) updateContent() {
	content := m.content[m.currentLang]
	split := strings.Split(content, "\n## ")
	fullSection := ""
	if m.currentSect == 0 {
		fullSection = split[0]
	} else {
		fullSection = "## " + split[m.currentSect]
	}
	m.viewport.SetContent(m.sectionStyle.Render(fullSection))
	m.viewport.GotoTop()
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}

func main() {
	// Styles
	headerStyle := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("205")).Padding(0, 1)
	footerStyle := lipgloss.NewStyle().Italic(true).Foreground(lipgloss.Color("240")).Padding(0, 1)
	sectionStyle := lipgloss.NewStyle().PaddingLeft(2)

	// Comprehensive documentation in multiple languages
	content := map[string]string{
		LangEnglish: `
# Hacker Lang Advanced (HLA) Documentation
Welcome to the comprehensive documentation for Hacker Lang Advanced (HLA). This guide is designed for both beginners and professionals. Beginners will find step-by-step explanations, examples, and tips to get started quickly. Professionals will appreciate in-depth discussions on advanced features, optimizations, and best practices.
## Introduction
HLA is a high-level programming language that transpiles to Rust, combining simplicity with powerful features like advanced memory management modes. It's ideal for systems programming, web development, and more.
### For Beginners:
- HLA syntax is intuitive, similar to Python but with type annotations.
- Start with basic variables and functions.
### For Professionals:
- Leverages Rust's safety and performance.
- Custom memory allocators and lifetimes for fine-grained control.
## Installation
To install HLA, use the Virus tool:
1. Download Virus from the official repository.
2. Run ` + "`" + `virus install hla` + "`" + `.
### Troubleshooting:
- Ensure Rust and Python are installed.
- For pros: Customize installation with environment variables like HLA_HOME.
## Basic Syntax
Variables:
- Immutable: def x: i32 = 5;
- Mutable: mut y: String = "hello";
Functions:
sub add(a: i32, b: i32) -> i32 [
return a + b;
]
### Beginner Examples:
def main() [
log "Hello, World!";
]
### Pro Tips:
Use generics for reusable code: sub vec<T>(v: Vec<T>) [...]
## Type System
HLA supports strong typing with inference.
Primitives: i32, u32, f64, bool, String.
Complex: Vec<T>, HashMap<K, V>, Option<T>.
### Inference:
def x = 5; // inferred as i32
### Advanced:
Custom types with classes: obj MyStruct [ def field: i32; ]
Generics with constraints: <T: Clone>
## Memory Management Modes
HLA offers multiple modes via directives:
--- auto --- : Automatic smart pointers (Arc, Rc).
--- manual --- : No std, unsafe code, custom allocator.
--- safe --- : Forbids unsafe code.
--- arena --- : Bump allocator for performance.
### Beginner Guide:
Start with --- auto --- for simplicity.
### Professional Usage:
In manual mode, manage ownership manually with unsafe blocks. Implement custom allocators for embedded systems.
Example in manual:
--- manual ---
mut ptr: *mut i32 = alloc(1);
unsafe { *ptr = 5; }
## Error Handling
Uses Rust's Result and ? operator.
Propagate errors with ! suffix: file.read()!
Custom errors with miette for pretty printing.
### For Beginners:
Wrap main in Result<(), Box<dyn Error>>.
### For Pros:
Extend HlaError enum for domain-specific errors.
## Packages and Modules
Use Virus.hk or .hcl for package management.
Import: # <rust:reqwest>
Integrates with Cargo.toml automatically.
### Beginner:
Add dependencies in Virus.hcl:
dependencies {
reqwest = "1.0"
}
### Pro:
Handle version conflicts, custom registries.
## Control Flow
If-else, loops, break/continue.
If: if x > 0 [ log "positive"; ] else [ log "non-positive"; ]
Loops: loop [ ... ] or loop i in 0..10 [ ... ]
## Classes and OOP
obj MyClass <T> [
def data: T;
sub new(d: T) -> Self [ Self { data: d } ]
]
Inheritance via composition.
## Advanced Topics
- Lifetimes: Automatically inferred, but can be explicit.
- Ownership Tracking: Transpiler tracks owners to insert .clone() or &.
- Generics and Traits: Map to Rust traits.
- Python Integration: Import Python modules seamlessly.
- Performance Optimization: Use fast mode with arena allocators.
- Security: Safe mode prevents unsafe code.
- Embedding: Use in no_std environments.
### Pro Examples:
Custom lifetime: sub borrow<'a>(r: &'a i32) -> &'a i32 [ r ]
## FAQ
Q: How to debug?
A: Use hla-errors for pretty errors.
Q: Performance vs Rust?
A: Comparable, as it transpiles directly.
## Glossary
- Transpiler: Converts HLA to Rust.
- Directive: --- mode --- for configurations.
This documentation is exhaustive; explore sections with n/p keys.
`,
		LangPolish: `
# Dokumentacja Hacker Lang Advanced (HLA)
Witaj w kompleksowej dokumentacji dla Hacker Lang Advanced (HLA). Ten przewodnik jest zaprojektowany zarówno dla początkujących, jak i profesjonalistów. Początkujący znajdą krok po kroku wyjaśnienia, przykłady i wskazówki, aby szybko zacząć. Profesjonaliści docenią dogłębne dyskusje na temat zaawansowanych funkcji, optymalizacji i najlepszych praktyk.
## Wstęp
HLA to język programowania wysokiego poziomu, który transpiluje do Rusta, łącząc prostotę z potężnymi funkcjami, takimi jak zaawansowane tryby zarządzania pamięcią. Idealny do programowania systemów, rozwoju webowego i więcej.
### Dla Początkujących:
- Składnia HLA jest intuicyjna, podobna do Pythona, ale z adnotacjami typów.
- Zacznij od podstawowych zmiennych i funkcji.
### Dla Profesjonalistów:
- Wykorzystuje bezpieczeństwo i wydajność Rusta.
- Niestandardowe alokatory pamięci i czasy życia dla precyzyjnej kontroli.
## Instalacja
Aby zainstalować HLA, użyj narzędzia Virus:
1. Pobierz Virus z oficjalnego repozytorium.
2. Uruchom ` + "`" + `virus install hla` + "`" + `.
### Rozwiązywanie Problemów:
- Upewnij się, że Rust i Python są zainstalowane.
- Dla pro: Dostosuj instalację za pomocą zmiennych środowiskowych jak HLA_HOME.
## Podstawowa Składnia
Zmienne:
- Niezmienne: def x: i32 = 5;
- Zmienne: mut y: String = "hello";
Funkcje:
sub add(a: i32, b: i32) -> i32 [
return a + b;
]
### Przykłady dla Początkujących:
def main() [
log "Witaj, Świecie!";
]
### Wskazówki dla Pro:
Używaj generyków do kodu wielokrotnego użytku: sub vec<T>(v: Vec<T>) [...]
## System Typów
HLA wspiera silne typowanie z wnioskowaniem.
Prymitywy: i32, u32, f64, bool, String.
Złożone: Vec<T>, HashMap<K, V>, Option<T>.
### Wnioskowanie:
def x = 5; // wnioskowane jako i32
### Zaawansowane:
Niestandardowe typy z klasami: obj MyStruct [ def field: i32; ]
Generyki z ograniczeniami: <T: Clone>
## Tryby Zarządzania Pamięcią
HLA oferuje wiele trybów poprzez dyrektywy:
--- auto --- : Automatyczne inteligentne wskaźniki (Arc, Rc).
--- manual --- : Bez std, unsafe kod, niestandardowy alokator.
--- safe --- : Zakazuje unsafe kodu.
--- arena --- : Alokator bump dla wydajności.
### Przewodnik dla Początkujących:
Zacznij od --- auto --- dla prostoty.
### Użycie dla Profesjonalistów:
W trybie manual, zarządzaj własnością ręcznie z blokami unsafe. Implementuj niestandardowe alokatory dla systemów wbudowanych.
Przykład w manual:
--- manual ---
mut ptr: *mut i32 = alloc(1);
unsafe { *ptr = 5; }
## Obsługa Błędów
Używa Result Rusta i operatora ?.
Propaguj błędy z sufiksem !: file.read()!
Niestandardowe błędy z miette dla ładnego drukowania.
### Dla Początkujących:
Owijaj main w Result<(), Box<dyn Error>>.
### Dla Pro:
Rozszerzaj enum HlaError dla błędów domenowych.
## Pakiety i Moduły
Używaj Virus.hk lub .hcl do zarządzania pakietami.
Import: # <rust:reqwest>
Integruje się automatycznie z Cargo.toml.
### Dla Początkujących:
Dodaj zależności w Virus.hcl:
dependencies {
reqwest = "1.0"
}
### Dla Pro:
Obsługuj konflikty wersji, niestandardowe rejestry.
## Sterowanie Przepływem
If-else, pętle, break/continue.
If: if x > 0 [ log "pozytywne"; ] else [ log "nie-pozytywne"; ]
Pętle: loop [ ... ] lub loop i in 0..10 [ ... ]
## Klasy i OOP
obj MyClass <T> [
def data: T;
sub new(d: T) -> Self [ Self { data: d } ]
]
Dziedziczenie poprzez kompozycję.
## Zaawansowane Tematy
- Czasy Życia: Automatycznie wnioskowane, ale mogą być jawne.
- Śledzenie Własności: Transpiler śledzi właścicieli, aby wstawiać .clone() lub &.
- Generyki i Traity: Mapują do traitów Rusta.
- Integracja z Pythonem: Importuj moduły Pythona bezproblemowo.
- Optymalizacja Wydajności: Używaj trybu fast z alokatorami arena.
- Bezpieczeństwo: Tryb safe zapobiega unsafe kodowi.
- Osadzanie: Używaj w środowiskach no_std.
### Przykłady dla Pro:
Niestandardowy czas życia: sub borrow<'a>(r: &'a i32) -> &'a i32 [ r ]
## FAQ
P: Jak debugować?
O: Używaj hla-errors dla ładnych błędów.
P: Wydajność vs Rust?
O: Porównywalna, ponieważ transpiluje bezpośrednio.
## Słownik
- Transpiler: Konwertuje HLA do Rusta.
- Dyrektywa: --- mode --- dla konfiguracji.
Ta dokumentacja jest wyczerpująca; eksploruj sekcje klawiszami n/p.
`,
		LangSpanish: `
# Documentación de Hacker Lang Advanced (HLA)
Bienvenido a la documentación completa de Hacker Lang Advanced (HLA). Esta guía está diseñada tanto para principiantes como para profesionales. Los principiantes encontrarán explicaciones paso a paso, ejemplos y consejos para comenzar rápidamente. Los profesionales apreciarán discusiones en profundidad sobre características avanzadas, optimizaciones y mejores prácticas.
## Introducción
HLA es un lenguaje de programación de alto nivel que se transpila a Rust, combinando simplicidad con características poderosas como modos avanzados de gestión de memoria. Ideal para programación de sistemas, desarrollo web y más.
### Para Principiantes:
- La sintaxis de HLA es intuitiva, similar a Python pero con anotaciones de tipo.
- Comienza con variables y funciones básicas.
### Para Profesionales:
- Aprovecha la seguridad y el rendimiento de Rust.
- Allocadores de memoria personalizados y lifetimes para control granular.
## Instalación
Para instalar HLA, usa la herramienta Virus:
1. Descarga Virus del repositorio oficial.
2. Ejecuta ` + "`" + `virus install hla` + "`" + `.
### Solución de Problemas:
- Asegúrate de que Rust y Python estén instalados.
- Para pros: Personaliza la instalación con variables de entorno como HLA_HOME.
## Sintaxis Básica
Variables:
- Inmutable: def x: i32 = 5;
- Mutable: mut y: String = "hola";
Funciones:
sub add(a: i32, b: i32) -> i32 [
return a + b;
]
### Ejemplos para Principiantes:
def main() [
log "¡Hola, Mundo!";
]
### Consejos para Pro:
Usa genéricos para código reutilizable: sub vec<T>(v: Vec<T>) [...]
## Sistema de Tipos
HLA soporta tipado fuerte con inferencia.
Primitivos: i32, u32, f64, bool, String.
Complejos: Vec<T>, HashMap<K, V>, Option<T>.
### Inferencia:
def x = 5; // inferido como i32
### Avanzado:
Tipos personalizados con clases: obj MyStruct [ def field: i32; ]
Genéricos con restricciones: <T: Clone>
## Modos de Gestión de Memoria
HLA ofrece múltiples modos vía directivas:
--- auto --- : Punteros inteligentes automáticos (Arc, Rc).
--- manual --- : Sin std, código unsafe, allocador personalizado.
--- safe --- : Prohíbe código unsafe.
--- arena --- : Allocador bump para rendimiento.
### Guía para Principiantes:
Comienza con --- auto --- para simplicidad.
### Uso Profesional:
En modo manual, gestiona la propiedad manualmente con bloques unsafe. Implementa allocadores personalizados para sistemas embebidos.
Ejemplo en manual:
--- manual ---
mut ptr: *mut i32 = alloc(1);
unsafe { *ptr = 5; }
## Manejo de Errores
Usa Result de Rust y operador ?.
Propaga errores con sufijo !: file.read()!
Errores personalizados con miette para impresión bonita.
### Para Principiantes:
Envuelve main en Result<(), Box<dyn Error>>.
### Para Pro:
Extiende enum HlaError para errores de dominio específicos.
## Paquetes y Módulos
Usa Virus.hk o .hcl para gestión de paquetes.
Import: # <rust:reqwest>
Se integra automáticamente con Cargo.toml.
### Para Principiantes:
Añade dependencias en Virus.hcl:
dependencies {
reqwest = "1.0"
}
### Para Pro:
Maneja conflictos de versiones, registros personalizados.
## Flujo de Control
If-else, bucles, break/continue.
If: if x > 0 [ log "positivo"; ] else [ log "no-positivo"; ]
Bucles: loop [ ... ] o loop i in 0..10 [ ... ]
## Clases y OOP
obj MyClass <T> [
def data: T;
sub new(d: T) -> Self [ Self { data: d } ]
]
Herencia vía composición.
## Temas Avanzados
- Lifetimes: Inferidos automáticamente, pero pueden ser explícitos.
- Seguimiento de Propiedad: Transpiler rastrea dueños para insertar .clone() o &.
- Genéricos y Traits: Mapean a traits de Rust.
- Integración con Python: Importa módulos de Python sin problemas.
- Optimización de Rendimiento: Usa modo fast con allocadores arena.
- Seguridad: Modo safe previene código unsafe.
- Embebido: Usa en entornos no_std.
### Ejemplos para Pro:
Lifetime personalizado: sub borrow<'a>(r: &'a i32) -> &'a i32 [ r ]
## FAQ
P: ¿Cómo depurar?
R: Usa hla-errors para errores bonitos.
P: ¿Rendimiento vs Rust?
R: Comparable, ya que transpila directamente.
## Glosario
- Transpiler: Convierte HLA a Rust.
- Directiva: --- mode --- para configuraciones.
Esta documentación es exhaustiva; explora secciones con teclas n/p.
`,
		LangFrench: `
# Documentation de Hacker Lang Advanced (HLA)
Bienvenue dans la documentation complète pour Hacker Lang Advanced (HLA). Ce guide est conçu pour les débutants et les professionnels. Les débutants trouveront des explications étape par étape, des exemples et des conseils pour démarrer rapidement. Les professionnels apprécieront des discussions approfondies sur les fonctionnalités avancées, les optimisations et les meilleures pratiques.
## Introduction
HLA est un langage de programmation de haut niveau qui se transpile en Rust, combinant simplicité avec des fonctionnalités puissantes comme des modes avancés de gestion de mémoire. Idéal pour la programmation système, le développement web et plus.
### Pour les Débutants:
- La syntaxe HLA est intuitive, similaire à Python mais avec des annotations de type.
- Commencez avec des variables et fonctions basiques.
### Pour les Professionnels:
- Tire parti de la sécurité et des performances de Rust.
- Allocateurs de mémoire personnalisés et lifetimes pour un contrôle fin.
## Installation
Pour installer HLA, utilisez l'outil Virus:
1. Téléchargez Virus depuis le dépôt officiel.
2. Exécutez ` + "`" + `virus install hla` + "`" + `.
### Dépannage:
- Assurez-vous que Rust et Python sont installés.
- Pour pros: Personnalisez l'installation avec des variables d'environnement comme HLA_HOME.
## Syntaxe Basique
Variables:
- Immuable: def x: i32 = 5;
- Mutable: mut y: String = "bonjour";
Fonctions:
sub add(a: i32, b: i32) -> i32 [
return a + b;
]
### Exemples pour Débutants:
def main() [
log "Bonjour, le Monde!";
]
### Conseils pour Pro:
Utilisez les génériques pour du code réutilisable: sub vec<T>(v: Vec<T>) [...]
## Système de Types
HLA supporte un typage fort avec inférence.
Primitives: i32, u32, f64, bool, String.
Complexes: Vec<T>, HashMap<K, V>, Option<T>.
### Inférence:
def x = 5; // inféré comme i32
### Avancé:
Types personnalisés avec classes: obj MyStruct [ def field: i32; ]
Génériques avec contraintes: <T: Clone>
## Modes de Gestion de Mémoire
HLA offre plusieurs modes via des directives:
--- auto --- : Pointeurs intelligents automatiques (Arc, Rc).
--- manual --- : Sans std, code unsafe, allocateur personnalisé.
--- safe --- : Interdit le code unsafe.
--- arena --- : Allocateur bump pour performances.
### Guide pour Débutants:
Commencez avec --- auto --- pour la simplicité.
### Utilisation Professionnelle:
En mode manual, gérez la propriété manuellement avec des blocs unsafe. Implémentez des allocateurs personnalisés pour systèmes embarqués.
Exemple en manual:
--- manual ---
mut ptr: *mut i32 = alloc(1);
unsafe { *ptr = 5; }
## Gestion des Erreurs
Utilise Result de Rust et opérateur ?.
Propagez les erreurs avec suffixe !: file.read()!
Erreurs personnalisées avec miette pour impression jolie.
### Pour Débutants:
Enveloppez main dans Result<(), Box<dyn Error>>.
### Pour Pro:
Étendez enum HlaError pour erreurs spécifiques au domaine.
## Paquets et Modules
Utilisez Virus.hk ou .hcl pour la gestion de paquets.
Import: # <rust:reqwest>
Intègre automatiquement avec Cargo.toml.
### Pour Débutants:
Ajoutez des dépendances dans Virus.hcl:
dependencies {
reqwest = "1.0"
}
### Pour Pro:
Gérez les conflits de versions, registres personnalisés.
## Flux de Contrôle
If-else, boucles, break/continue.
If: if x > 0 [ log "positif"; ] else [ log "non-positif"; ]
Boucles: loop [ ... ] ou loop i in 0..10 [ ... ]
## Classes et OOP
obj MyClass <T> [
def data: T;
sub new(d: T) -> Self [ Self { data: d } ]
]
Héritage via composition.
## Sujets Avancés
- Lifetimes: Inférés automatiquement, mais peuvent être explicites.
- Suivi de Propriété: Transpiler suit les propriétaires pour insérer .clone() ou &.
- Génériques et Traits: Mappent aux traits de Rust.
- Intégration Python: Importez des modules Python sans couture.
- Optimisation de Performances: Utilisez mode fast avec allocateurs arena.
- Sécurité: Mode safe prévient le code unsafe.
- Embarqué: Utilisez dans environnements no_std.
### Exemples pour Pro:
Lifetime personnalisé: sub borrow<'a>(r: &'a i32) -> &'a i32 [ r ]
## FAQ
Q: Comment déboguer?
R: Utilisez hla-errors pour erreurs jolies.
Q: Performances vs Rust?
R: Comparables, car transpilé directement.
## Glossaire
- Transpiler: Convertit HLA en Rust.
- Directive: --- mode --- pour configurations.
Cette documentation est exhaustive; explorez les sections avec touches n/p.
`,
	}

	// Extract sections from English for navigation (assuming same structure)
	englishContent := content[LangEnglish]
	sections := []string{"Overview"}
	for _, line := range strings.Split(englishContent, "\n") {
		if strings.HasPrefix(line, "## ") {
			sections = append(sections, strings.TrimPrefix(line, "## "))
		}
	}

	p := tea.NewProgram(model{
		content:      content,
		currentLang:  LangEnglish,
		sections:     sections,
		headerStyle:  headerStyle,
		footerStyle:  footerStyle,
		sectionStyle: sectionStyle,
	})

	if _, err := p.Run(); err != nil {
		fmt.Println("Error running program:", err)
	}
}
