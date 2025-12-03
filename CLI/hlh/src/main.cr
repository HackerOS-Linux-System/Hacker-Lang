module Hlh
  VERSION = "1.2"
  def self.display_info
    puts "\e[1;36mJęzyk programowania Hacker Lang dla HackerOS\e[0m"
    puts ""
    puts "\e[1;32mbytes\e[0m \e[37m- manager bibliotek/pluginów\e[0m"
    puts "\e[37m Zarządza instalacją, aktualizacją i usuwaniem bibliotek oraz pluginów dla projektów w Hacker Lang.\e[0m"
    puts "\e[37m Przykłady użycia:\e[0m"
    puts "\e[37m bytes install <pakiet> - instaluje pakiet\e[0m"
    puts "\e[37m bytes update - aktualizuje wszystkie pakiety\e[0m"
    puts ""
    puts "\e[1;32mhli\e[0m \e[37m- narzędzie dla dużych projektów w .hacker wymaga plików bytes.yaml\e[0m"
    puts "\e[37m Przeznaczone do zarządzania dużymi projektami, budowania, testowania i wdrażania.\e[0m"
    puts "\e[37m Wymaga pliku konfiguracyjnego bytes.yaml definiującego zależności i ustawienia projektu.\e[0m"
    puts "\e[37m Przykłady użycia:\e[0m"
    puts "\e[37m hli build - buduje projekt\e[0m"
    puts "\e[37m hli run - uruchom projekt\e[0m"
    puts "\e[37m hli init - tworzy przykładowy projekt projekt\e[0m"
    puts "\e[37m hli clean - czyści tymczasowe pliki\e[0m"
    puts "\e[37m hli tutorials - przyklady\e[0m"
    puts "\e[37m hli repl - używaj interaktywnego interfejsu\e[0m"
    puts ""
    puts "\e[1;32mhackerc\e[0m \e[37m- narzędzie do lekkich projektów i skryptów w .hacker nie wymaga bytes.yaml\e[0m"
    puts "\e[37m Szybkie narzędzie do kompilacji i uruchamiania prostych skryptów lub małych projektów.\e[0m"
    puts "\e[37m Nie wymaga konfiguracji, idealne do prototypowania i jednorazowych zadań.\e[0m"
    puts "\e[37m Przykłady użycia:\e[0m"
    puts "\e[37m hackerc run <plik.hacker> - uruchamia skrypt\e[0m"
    puts "\e[37m hackerc compile <plik.hacker> - kompiluje do pliku wykonywalnego\e[0m"
  end
  def self.main
    if ARGV.includes?("-v") || ARGV.includes?("--version")
      puts "\e[33mhlh wersja #{VERSION}\e[0m"
      exit
    elsif ARGV.includes?("-h") || ARGV.includes?("--help")
      puts "Użycie: hlh [opcje]"
      puts "-v, --version    Wyświetla wersję"
      puts "-h, --help       Wyświetla pomoc"
      exit
    end
    display_info
  end
end
Hlh.main
