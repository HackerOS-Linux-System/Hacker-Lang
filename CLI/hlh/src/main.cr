# src/main.cr

require "option_parser"
require "colorize"

module Hlh
  VERSION = "1.2"

  def self.display_info
    puts "Język programowania Hacker Lang dla HackerOS".colorize(:cyan).bold
    puts ""
    puts "bytes".colorize(:green).bold + " - manager bibliotek/pluginów".colorize(:white)
    puts "  Zarządza instalacją, aktualizacją i usuwaniem bibliotek oraz pluginów dla projektów w Hacker Lang."
    puts "  Przykłady użycia:"
    puts "    bytes install <pakiet>  - instaluje pakiet"
    puts "    bytes update            - aktualizuje wszystkie pakiety"
    puts ""
    puts "hli".colorize(:green).bold + " - narzędzie dla dużych projektów w .hacker wymaga plików bytes.yaml".colorize(:white)
    puts "  Przeznaczone do zarządzania dużymi projektami, budowania, testowania i wdrażania."
    puts "  Wymaga pliku konfiguracyjnego bytes.yaml definiującego zależności i ustawienia projektu."
    puts "  Przykłady użycia:"
    puts "    hli build               - buduje projekt"
    puts "    hli run                 - uruchom projekt"
    puts "    hli init                - tworzy przykładowy projekt projekt"
    puts ""
    puts "hackerc".colorize(:green).bold + " - narzędzie do lekkich projektów i skryptów w .hacker nie wymaga bytes.yaml".colorize(:white)
    puts "  Szybkie narzędzie do kompilacji i uruchamiania prostych skryptów lub małych projektów."
    puts "  Nie wymaga konfiguracji, idealne do prototypowania i jednorazowych zadań."
    puts "  Przykłady użycia:"
    puts "    hackerc run <plik.hacker> - uruchamia skrypt"
    puts "    hackerc compile <plik.hacker> - kompiluje do pliku wykonywalnego"
  end

  def self.main
    OptionParser.parse do |parser|
      parser.banner = "Użycie: hlh [opcje]"

      parser.on("-v", "--version", "Wyświetla wersję") do
        puts "hlh wersja #{VERSION}".colorize(:yellow)
        exit
      end

      parser.on("-h", "--help", "Wyświetla pomoc") do
        puts parser
        exit
      end
    end

    display_info
  end
end

Hlh.main
