package main

import "core:fmt"
import "core:os"
import "core:strings"

VERSION :: "1.6"

// Funkcja wyświetlająca informacje o narzędziach CLI (domyślna)
display_cli_info :: proc() {
    fmt.println("\e[1;36mJęzyk programowania Hacker Lang dla HackerOS - Narzędzia CLI\e[0m")
    fmt.println("")

    // Sekcja: bytes - manager bibliotek/pluginów
    fmt.println("\e[1;32mbytes\e[0m \e[37m- manager bibliotek, pluginów i źródeł\e[0m")
    fmt.println("\e[37m Zarządza instalacją, usuwaniem, pluginami i źródłami z repozytorium .hk.\e[0m")
    fmt.println("\e[37m Pobiera z GitHub, klonuje repozytoria, buduje via build.hl.\e[0m")
    fmt.println("\e[37m Przykłady użycia:\e[0m")
    fmt.println("\e[37m bytes install <lib> - instaluje bibliotekę\e[0m")
    fmt.println("\e[37m bytes remove <lib> - usuwa bibliotekę\e[0m")
    fmt.println("\e[37m bytes plugin install <plugin> - instaluje plugin\e[0m")
    fmt.println("\e[37m bytes source install <source> [--no-build] - instaluje źródło (klonuje i opcjonalnie buduje)\e[0m")
    fmt.println("\e[37m Dokumentacja: Napisane w Go z Cobra CLI. Repozytorium: https://github.com/Bytes-Repository/bytes.io\e[0m")
    fmt.println("\e[37m Ścieżka instalacji: ~/.hackeros/hacker-lang/{libs,plugins,sources}\e[0m")
    fmt.println("")

    // Sekcja: hli
    fmt.println("\e[1;32mhli\e[0m \e[37m- narzędzie interaktywne\e[0m")
    fmt.println("\e[37m Zarządza budowaniem, uruchamianiem, kompilacją w trybie CLI lub interaktywnym.\e[0m")
    fmt.println("\e[37m Wsparcie dla autouzupełniania, kolorów via prompt_toolkit i rich.\e[0m")
    fmt.println("\e[37m Przykłady użycia:\e[0m")
    fmt.println("\e[37m hli run <plik> [--verbose] - uruchamia\e[0m")
    fmt.println("\e[37m hli compile <plik> [-o output] [--verbose] - kompiluje\e[0m")
    fmt.println("\e[37m hli help - pomoc\e[0m")
    fmt.println("\e[37m hli (bez arg) - tryb interaktywny\e[0m")
    fmt.println("\e[37m Dokumentacja: Python z argparse, subprocess. Wywołuje hl-runtime/compiler.\e[0m")
    fmt.println("")

    // Sekcja: hl - główne CLI
    fmt.println("\e[1;32mhl\e[0m \e[37m- główne CLI dla skryptów .hl\e[0m")
    fmt.println("\e[37m Uruchamianie, kompilacja, izolacja, pakowanie.\e[0m")
    fmt.println("\e[37m Wywołuje hl-runtime, hl-compiler, hl-containers.\e[0m")
    fmt.println("\e[37m Przykłady użycia:\e[0m")
    fmt.println("\e[37m hl run <plik> [--verbose] [--unsafe-mode]\e[0m")
    fmt.println("\e[37m hl compile <plik> [-o output] [--verbose]\e[0m")
    fmt.println("\e[37m hl isolated <plik> [--verbose] - w kontenerze\e[0m")
    fmt.println("\e[37m hl pack <plik> [-o output] [--compress] - pakuje\e[0m")
    fmt.println("\e[37m Dokumentacja: Rust z clap, nix execv.\e[0m")
    fmt.println("")
}

// Funkcja wyświetlająca dokumentację składni języka (przy -docs)
display_syntax_docs :: proc() {
    fmt.println("\e[1;36mDokumentacja składni języka programowania Hacker Lang dla HackerOS\e[0m")
    fmt.println("")
    fmt.println("\e[1;32mWstęp\e[0m")
    fmt.println("\e[37mHacker Lang: skryptowy język z fokusem na komendy shell, struktury kontrolne, bezpieczeństwo.\e[0m")
    fmt.println("\e[37mRozszerzenia: .hacker / .hl. Linie zaczynają się od operatorów.\e[0m")
    fmt.println("\e[37mKomentarze: ! na końcu lub !! dla bloków.\e[0m")
    fmt.println("\e[37mBezpieczeństwo: Analyzer blokuje dangerous cmds bez --unsafe.\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mKomendy systemowe\e[0m")
    fmt.println("\e[37m> cmd ! - Zwykła komenda shell.\e[0m")
    fmt.println("\e[37m^ > cmd ! - Z sudo (unsafe).\e[0m")
    fmt.println("\e[37m>>> cmd ! - Złożona/multi-line.\e[0m")
    fmt.println("\e[37m>> cmd ! - Z przypisaniem wyjścia.\e[0m")
    fmt.println("\e[37m& cmd ! - W tle.\e[0m")
    fmt.println("\e[37mPrzykład: > ls -la | grep file !\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mZmienne\e[0m")
    fmt.println("\e[37m@var = val - Globalna (env).\e[0m")
    fmt.println("\e[37m$var = val - Lokalna.\e[0m")
    fmt.println("\e[37mUżycie: ${var} w cmd.\e[0m")
    fmt.println("\e[37mSubstytucja w runtime VM.\e[0m")
    fmt.println("\e[37mPrzykład: $dir = /home; > cd ${dir} !\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mFunkcje\e[0m")
    fmt.println("\e[37m: func_name - Start.\e[0m")
    fmt.println("\e[37m: - Koniec.\e[0m")
    fmt.println("\e[37m. func_name - Wywołanie.\e[0m")
    fmt.println("\e[37mBrak parametrów, proste ciało.\e[0m")
    fmt.println("\e[37mPrzykład:\e[0m")
    fmt.println("\e[37m: log\e[0m")
    fmt.println("\e[37m> echo 'Log entry' !\e[0m")
    fmt.println("\e[37m:\e[0m")
    fmt.println("\e[37m. log\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mStruktury kontrolne\e[0m")
    fmt.println("\e[37m= N > cmd ! - Pętla N razy.\e[0m")
    fmt.println("\e[37m? cond > cmd ! - If (bash cond).\e[0m")
    fmt.println("\e[37mBrak else, nested via funcs.\e[0m")
    fmt.println("\e[37mPrzykład: =5> echo 'Iter' !; ? [ $x -gt 10 ] > echo 'Big' !\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mPluginy i moduły\e[0m")
    fmt.println("\e[37m\\ plugin - Ładuje plugin.\e[0m")
    fmt.println("\e[37m^ \\ plugin - Z superuser.\e[0m")
    fmt.println("\e[37m# lib - Include biblioteki (main.hacker).\e[0m")
    fmt.println("\e[37m// dep - Zależność systemowa.\e[0m")
    fmt.println("\e[37mPrzykład: # utils; \\ logger; // python3\e[0m")
    fmt.println("\e[37mLibs z ~/.hackeros/hacker-lang/libs.\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mBezpieczeństwo i analiza\e[0m")
    fmt.println("\e[37mWykrywanie: rm -rf, dd, etc.\e[0m")
    fmt.println("\e[37mSudo i dangerous wymagają --unsafe.\e[0m")
    fmt.println("\e[37mLibs dziedziczą checks.\e[0m")
    fmt.println("\e[37mJSON output via --json w hl-plsa.\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mKomentarze i format\e[0m")
    fmt.println("\e[37m! - Linia.\e[0m")
    fmt.println("\e[37m!! - Toggle blok.\e[0m")
    fmt.println("\e[37mPuste linie ignorowane.\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mPrzykładowy skrypt\e[0m")
    fmt.println("\e[37m// curl\e[0m")
    fmt.println("\e[37m# network\e[0m")
    fmt.println("\e[37m@URL = https://example.com\e[0m")
    fmt.println("\e[37m: fetch\e[0m")
    fmt.println("\e[37m> curl ${URL} !\e[0m")
    fmt.println("\e[37m:\e[0m")
    fmt.println("\e[37m? true > . fetch !\e[0m")
    fmt.println("\e[37m=2> echo 'Repeat' !\e[0m")
    fmt.println("\e[37m\\ stats\e[0m")
    fmt.println("")

    fmt.println("\e[1;32mZaawansowane cechy\e[0m")
    fmt.println("\e[37mRekurencja w funcs (ograniczona).\e[0m")
    fmt.println("\e[37mBytecode VM z cache.\e[0m")
    fmt.println("\e[37mNatywna kompilacja LLVM.\e[0m")
    fmt.println("\e[37mIzolacja kontenerami.\e[0m")
    fmt.println("\e[37mPakowanie do .tar.zst.\e[0m")
    fmt.println("\e[37mTestuj via hl run --verbose.\e[0m")
    fmt.println("")
}

main :: proc() {
    args := os.args[1:]
    show_docs := false
    for arg in args {
        switch arg {
            case "-v", "--version":
                fmt.printf("\e[33mhlh wersja %s\e[0m\n", VERSION)
                return
            case "-h", "--help":
                fmt.println("Użycie: hlh [opcje]")
                fmt.println("-v, --version: Wyświetla wersję")
                fmt.println("-h, --help: Wyświetla pomoc")
                fmt.println("-docs: Wyświetla dokumentację składni języka")
                return
            case "-docs":
                show_docs = true
        }
    }

    if show_docs {
        display_syntax_docs()
    } else {
        display_cli_info()
    }
}
