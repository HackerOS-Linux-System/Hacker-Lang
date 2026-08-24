module hl-docs

// hl-docs 1.2.0 — kategoria JIT (source-code/docs/src/content_jit.go, 9
// sekcji) oraz flaga -version/-v w main.go. Brak nowych zależności
// zewnętrznych.
//
// Struktura pakietów: main.go (package main, w tym katalogu) importuje
// hl-docs/src (package src, w source-code/docs/src/) jako zwykły pakiet
// Go — main.go woła wyłącznie src.InitialModel(). Wszystkie pliki wewnątrz
// src/ dzielą jeden pakiet między sobą (jak dotychczas), tylko sam
// konstruktor jest eksportowany na zewnątrz.
go 1.22.5

require (
	github.com/charmbracelet/bubbles v0.18.0
	github.com/charmbracelet/bubbletea v0.26.6
	github.com/charmbracelet/lipgloss v0.12.1
)
