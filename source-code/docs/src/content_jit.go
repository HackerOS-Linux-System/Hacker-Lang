package src

import "fmt"

// ── JIT ──────────────────────────────────────────────────────────────────────
// Nowa kategoria: dokumentacja silnika JIT wbudowanego w interpreter bytecode
// (source-code/jit). Opisuje dwupoziomową architekturę (Trace JIT +
// whole-function JIT), strojenie przez zmienne środowiskowe oraz regułę
// bezpieczeństwa, która decyduje co w ogóle wolno skompilować natywnie.
var jitSections = []DocSection{
	{
		Title:    "JIT — jak to działa",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
  Interpreter bytecode wykonuje skrypt instrukcja po instrukcji, ale
  śledzi, które fragmenty są "gorące" i kompiluje je do prawdziwego
  kodu maszynowego (przez Cranelift) zamiast dalej je interpretować.
  Działa to na DWÓCH niezależnych poziomach:

%s
  %s  — gorąca PĘTLA wewnątrz funkcji (albo top-level) jest
       kompilowana sama w sobie, niezależnie od tego, co ją otacza.
  %s  — funkcja czysto obliczeniowa wołana wielokrotnie
       (nawet bez gorącej pętli w środku) jest kompilowana W CAŁOŚCI.

%s
  Oba poziomy dzielą jeden trwały silnik JIT (jeden %s
  na cały czas życia interpretera) — kompilacje po pierwszej są
  szybsze, bo ISA i księgowość Cranelift nie są budowane od zera
  za każdym razem, a wygenerowany kod nie wycieka po zakończeniu
  skryptu.

%s
  Wygenerowany kod maszynowy powstaje z %s
  — Cranelift stosuje pełny zestaw optymalizacji backendu przy
  emisji instrukcji, nie tylko dobór instrukcji do kompilacji.

%s
  JIT jest w pełni przezroczysty: wynik wykonania skryptu jest
  identyczny niezależnie od tego, czy dany fragment akurat trafił
  do JIT, czy wciąż jest interpretowany.`,
			styleH1.Render("◆ JIT — dwupoziomowa kompilacja natywna"),
			styleH2.Render("Ogólna zasada"),
			styleH2.Render("Dwa poziomy"),
			styleOp.Render("Trace JIT"),
			styleOp.Render("Whole-function JIT"),
			styleH2.Render("Wspólny silnik"),
			styleOp.Render("JITModule"),
			styleH2.Render("Jakość kodu"),
			styleOp.Render("opt_level=speed"),
			styleH2.Render("Przezroczystość"),
		),
	},
	{
		Title:    "Strojenie JIT — zmienne środowiskowe",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
%s

%s
  %s
  %s
  %s
  %s

%s
  Ciasne pętle (%s instrukcji) rozgrzewają się
  o połowę szybciej niż domyślny próg — wykonują dużo więcej
  iteracji na jednostkę "rozgrzewki" niż duże pętle, więc szybsze
  przejście na kod natywny szybciej się zwraca. Dzieje się to
  automatycznie, bez potrzeby ustawiania czegokolwiek.

%s
  Progi kompilacji nie mają wpływu na POPRAWNOŚĆ — tylko na to, jak
  szybko dany fragment przechodzi z interpretacji na kod natywny.
  Niższy próg = szybszy rozruch JIT-a, kosztem częstszych (tańszych,
  ale nie darmowych) prób kompilacji.`,
			styleH1.Render("Strojenie JIT"),
			styleH2.Render("Zmienne środowiskowe"),
			styleCode.Render("HL_NO_JIT=1                     ;; wyłącz JIT całkowicie (czysty interpreter)\nHL_JIT_TRACE_THRESHOLD=<n>       ;; próg wywołań dla tras pętli (domyślnie 50)\nHL_JIT_FUNC_THRESHOLD=<n>        ;; próg wywołań dla całych funkcji (domyślnie 50)\nHL_JIT_STATS=1                    ;; wypisz statystyki JIT na stderr po wykonaniu"),
			styleH2.Render("Przykład"),
			styleOp.Render("hl run skrypt.hl"),
			styleCode.Render("HL_JIT_STATS=1 hl run benchmark.hl\n;; [hl jit stats] trasy pętli: 3 | funkcje: 2 | ..."),
			styleOp.Render("HL_NO_JIT=1 hl run skrypt.hl"),
			styleCode.Render("HL_JIT_TRACE_THRESHOLD=10 hl run skrypt.hl  ;; kompiluj wcześniej"),
			styleH2.Render("Adaptacyjny próg dla ciasnych pętli"),
			styleOp.Render("<= 8"),
			styleTip.Render("💡 Progi to strojenie WYDAJNOŚCI, nie poprawności."),
		),
	},
	{
		Title:    "Bezpieczeństwo kompilacji JIT",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
  JIT NIGDY nie kompiluje fragmentu, który mógłby wykonać efekt
  uboczny albo operować na stringach — kwalifikują się WYŁĄCZNIE:
  ładowanie stałych liczbowych/bool, GetVar/SetVar, arytmetyka,
  porównania, kontrola przepływu (Jump/JumpIfFalse/JumpIfTrue/Return).

%s
  %s  ExecCmd / ExecCapture — komendy systemowe
  %s  LoadStr / Concat / ToString / CallQuick — operacje na stringach
  %s  CallFunc — wywołania innych funkcji (whole-function JIT
       nigdy więc nie rekursuje z powrotem do interpretera)
  %s  ForInStart / ForInNext / HackerOsCall

%s
  Zanim jakikolwiek fragment zostanie skompilowany, interpreter
  sprawdza KAŻDĄ używaną w nim zmienną: musi już mieć przydzielony
  slot (być choć raz ustawiona) I jej aktualna wartość musi być
  czystą liczbą. Jeśli którykolwiek warunek zawiedzie — JIT po
  prostu NIE kompiluje tego fragmentu i skrypt dalej działa
  poprawnie przez interpretację, bez żadnego ryzyka.

%s
  Dzięki %s w optymalizatorze (source-code/compiler),
  warunki oparte na znanych stałych (np. %s) są
  składane w %s na etapie kompilacji do bytecode —
  co poszerza zbiór pętli/funkcji kwalifikujących się do JIT,
  bo %s sam w sobie NIE jest jit-eligible (ma
  specjalną ścieżkę dla stringów-warunków w interpreterze).`,
			styleH1.Render("Bezpieczeństwo JIT"),
			styleH2.Render("Zasada"),
			styleH2.Render("Nigdy nie kwalifikuje się"),
			"•", "•", "•", "•",
			styleH2.Render("Weryfikacja zmiennych"),
			styleH2.Render("Synergia z optymalizatorem"),
			styleOp.Render("constant foldingu"),
			styleOp.Render("while true"),
			styleOp.Render("LoadBool"),
			styleOp.Render("Truthy"),
		),
	},
	{
		Title:    "Dispatch JIT — O(1) i ponawianie prób",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
  Sprawdzenie "czy ten fragment jest już skompilowany" wykonuje się
  na KAŻDEJ iteracji KAŻDEJ pętli wstecznej i na KAŻDE wywołanie
  KAŻDEJ funkcji w skrypcie — niezależnie od tego, czy dany fragment
  faktycznie trafił do JIT. To jest najgorętsza możliwa ścieżka w
  całym interpreterze.

%s
  Zamiast mapy haszującej, interpreter trzyma te informacje w
  zwykłych tablicach (%s) indeksowanych
  BEZPOŚREDNIO przez offset instrukcji / indeks nazwy — zero
  hashowania, zero kolizji, dostęp w stałym czasie niezależnie od
  tego, ile fragmentów już skompilowano.

%s
  Pierwsza próba kompilacji może się nie powieść z przyczyny
  DYNAMICZNEJ — np. zmienna użyta w pętli nie jest jeszcze
  stabilnie liczbowa przy pierwszym przejściu przez próg. Taka
  przyczyna może zniknąć sama w kolejnych iteracjach/wywołaniach.

%s
  Do %s prób ponownych na fragment, zanim
  JIT trwale się podda. Przyczyny STRUKTURALNE (np. instrukcja
  %s w ciele) nigdy się same nie naprawiają —
  te blokują się trwale od razu, bez marnowania prób.`,
			styleH1.Render("Dispatch JIT — wydajność sprawdzeń"),
			styleH2.Render("Dlaczego to ważne"),
			styleH2.Render("Tablice zamiast map"),
			styleOp.Render("Vec<Option<_>>"),
			styleH2.Render("Ponawianie prób (retry-with-backoff)"),
			styleH2.Render("Limit prób"),
			styleOp.Render("MAX_JIT_RETRIES = 4"),
			styleOp.Render("ExecCmd"),
		),
	},
	{
		Title:    "Kompilator → JIT: wątkowanie skoków",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
  Zanim bytecode w ogóle trafi do interpretera/JIT-a, optymalizator
  (source-code/compiler) przechodzi przez moduł i ZWIJA łańcuchy
  "doskoków": %s wskazujący na
  inny bezwarunkowy skok jest przekierowywany PROSTO na ostateczny
  cel, pomijając pośrednie przeskoki.

%s
  Ten sam mechanizm łapie też skoki powstałe z %s
  — gdy warunek pętli/gałęzi jest znany na etapie kompilacji
  (np. %s), warunkowy skok zwija się w
  bezwarunkowy, a NOWO powstały skok jest wątkowany dokładnie tak
  samo jak te napisane wprost w źródle.

%s
  Trace JIT liczy rozmiar trasy jako odległość między skokiem
  wstecznym a jego celem — im mniej pośrednich doskoków po drodze,
  tym krótsza (i częściej mieszcząca się pod limitem) trasa, i tym
  mniej bloków musi zbudować Cranelift przy kompilacji.

%s
  Ta transformacja NIGDY nie przesuwa ani nie usuwa żadnej
  instrukcji — zmienia WYŁĄCZNIE pole docelowe samego skoku.
  Żadne inne miejsce w module nie wymaga korekty. Limit liczby
  "skoków po skoku" (64) gwarantuje, że pass zawsze się kończy,
  nawet dla spreparowanych cyklicznych łańcuchów.`,
			styleH1.Render("Wątkowanie skoków (jump threading)"),
			styleH2.Render("Co to robi"),
			styleOp.Render("Jump → Jump → Jump"),
			styleH2.Render("Współpraca ze zwijaniem gałęzi"),
			styleOp.Render("dead branch elimination"),
			styleOp.Render("while true"),
			styleH2.Render("Dlaczego to pomaga JIT-owi"),
			styleH2.Render("Bezpieczeństwo transformacji"),
		),
	},
	{
		Title:    "Wydajność samej kompilacji",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
  Odkąd JIT ponawia nieudane próby kompilacji (patrz strojenie
  wyżej), pojedyncza trasa lub funkcja może być kompilowana WIĘCEJ
  niż raz w czasie życia jednego skryptu. Sam koszt kompilacji też
  się liczy, nie tylko szybkość wygenerowanego kodu.

%s
  Silnik JIT trzyma jeden %s
  Cranelift przez cały czas życia interpretera i tylko go %s
  między kolejnymi kompilacjami, zamiast alokować od zera za każdym
  razem — dotyczy to zarówno kontekstu kompilacji, jak i kontekstu
  budowniczego funkcji.

%s
  To jest udokumentowany, zalecany przez Cranelift wzorzec dla
  JIT-ów kompilujących wiele fragmentów w czasie życia jednego
  procesu — nie prowizorka.`,
			styleH1.Render("Wydajność kompilacji, nie tylko kodu wynikowego"),
			styleH2.Render("Dlaczego to teraz ważniejsze"),
			styleH2.Render("Ponowne użycie kontekstu Cranelift"),
			styleOp.Render("Context"),
			styleOp.Render("czyści"),
			styleH2.Render("To nie prowizorka"),
		),
	},
	{
		Title:    "Trace linking — natywne łańcuchowanie tras",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
  Gdy skompilowana trasa pętli KOŃCZY działanie (wychodzi z pętli),
  normalnie wraca do Rust, który sprawdza, dokąd dalej. Trace
  linking pozwala jej, w PEWNYCH przypadkach, doskoczyć PROSTO do
  INNEJ już skompilowanej trasy — w kodzie natywnym, bez powrotu
  do pętli dispatchu.

%s
  Każdy skompilowany fragment dostaje wskaźnik do współdzielonej
  %s: %s oznacza "pod offsetem %s
  jest już skompilowana trasa". Przy KAŻDYM wyjściu wygenerowany kod
  SPRAWDZA tę tablicę i, jeśli cel jest tam wpisany, woła go
  bezpośrednio.

%s
  Sprawdzenie dzieje się w RUNTIME, nie przy kompilacji — trasa A
  skompilowana PRZED trasą B automatycznie zacznie się do niej
  doskakiwać przy swoim NASTĘPNYM wykonaniu, gdy tylko B zostanie
  wpisana do tablicy. Nie trzeba niczego łatać w już wyemitowanym
  kodzie maszynowym A.

%s
  Dotyczy WYŁĄCZNIE tras pętli, nigdy całych funkcji — offset
  zwracany przez whole-function JIT to zawsze koniec ciała funkcji,
  którego wołający i tak nie traktuje jako celu skoku (to relacja
  wywołanie/powrót, nie "kontynuuj od tego miejsca").`,
			styleH1.Render("Trace linking"),
			styleH2.Render("Problem"),
			styleH2.Render("Mechanizm"),
			styleOp.Render("tablicy linków"),
			styleOp.Render("link_table[i] != 0"),
			styleOp.Render("i"),
			styleH2.Render("Retroaktywność"),
			styleH2.Render("Zakres"),
		),
	},
	{
		Title:    "Pomijanie zbędnych odczytów z pamięci",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
  Na starcie skompilowanego fragmentu KAŻDY użyty rejestr jest
  domyślnie ładowany z pamięci — nawet jeśli jego pierwsze użycie
  w kodzie i tak go NADPISUJE, zanim ktokolwiek go przeczyta. Taki
  odczyt jest zmarnowany.

%s
  Jeśli rejestr jest zapisywany PRZED jakimkolwiek odczytem — w
  obrębie bloku wejściowego, czyli JEDYNEGO możliwego punktu
  wejścia do skompilowanej funkcji (dla trasy pętli to dokładnie
  cel jej własnego skoku wstecznego, więc wykonuje się na KAŻDEJ
  iteracji) — początkowa wartość z pamięci jest z definicji
  niezauważalna, więc ładowanie jej w ogóle się pomija.

%s
  Analiza jest CELOWO ograniczona do prostego przebiegu liniowego
  jednego bloku — żadnego ogólnego dataflow z punktem stałym po
  grafie z cyklami. To wystarcza na najczęstszy, najbardziej
  wartościowy przypadek (rejestry pomocnicze przeliczane na nowo
  na starcie każdej iteracji), a jednocześnie jest na tyle proste,
  że da się to zweryfikować przez zwykłą inspekcję kodu.

%s
  Nawet gdyby powyższy dowód miał defekt, pominięty odczyt zastępuje
  tania stała 0.0 — NIGDY nieokreślona wartość SSA. Skutkiem
  ewentualnego błędu byłby co najwyżej zły wynik, nigdy odczyt
  niezainicjalizowanej pamięci ani cofnięcie się do niezdefiniowanego
  zachowania. Dotyczy WYŁĄCZNIE rejestrów — zmienne HL
  (GetVar/SetVar) zawsze są ładowane, bez wyjątków.`,
			styleH1.Render("Pomijanie zbędnych load'ów"),
			styleH2.Render("Problem"),
			styleH2.Render("Rozwiązanie"),
			styleH2.Render("Celowo ograniczony zakres"),
			styleH2.Render("Siatka bezpieczeństwa"),
		),
	},
	{
		Title:    "Inlining pojedynczego wywołania funkcji",
		Category: "JIT",
		Content: fmt.Sprintf(`%s

%s
  %s był świadomie wykluczony z whole-function JIT — żeby
  skompilowany natywnie kod nigdy nie musiał wracać do interpretera.
  Zdjęcie tego ograniczenia bez ograniczeń otwierałoby drogę do
  rekursji i cykli w kodzie natywnym.

%s
  Funkcja kwalifikuje się do wklejenia wywołania, gdy ma DOKŁADNIE
  JEDNO %s, którego cel:
  %s  istnieje i daje się rozwiązać w tym samym module,
  %s  NIE jest samą sobą (bez samo-wywołań),
  %s  sam jest w pełni jit-eligible — a więc z DEFINICJI nie
       zawiera ŻADNEGO CallFunc.

%s
  Skoro wklejana funkcja sama nigdy nie zawiera CallFunc, inlining
  jest z góry ograniczony do JEDNEGO POZIOMU głębokości — nie ma
  możliwości łańcucha ani cyklu, więc nie trzeba wykrywać cykli
  osobnym mechanizmem: sama reguła to gwarantuje statycznie.

%s
  Ciało wywoływanej funkcji jest wklejane bezpośrednio w miejscu
  wywołania (jej rejestry NIGDY nie kolidują z rejestrami wołającego
  — numeracja rejestrów jest globalnie unikalna w całym module, więc
  nie trzeba niczego przenumerowywać), a jej końcowy %s
  zamienia się w zwykły skok do miejsca zaraz po wywołaniu —
  zamiast wyjścia z całej skompilowanej funkcji.

%s
  Ograniczone do JEDNEGO miejsca wywołania na funkcję: najczęstszy
  i najbardziej wartościowy wzorzec — wywołanie pomocniczej funkcji
  WEWNĄTRZ gorącej pętli — to i tak JEDNA instrukcja CallFunc w
  bytecode, niezależnie od tego, ile razy pętla wykona się w
  praktyce.`,
			styleH1.Render("Inlining wywołań funkcji"),
			styleH2.Render("Problem"),
			styleOp.Render("CallFunc"),
			styleH2.Render("Reguła kwalifikacji"),
			styleOp.Render("CallFunc"),
			"•", "•", "•",
			styleH2.Render("Dlaczego to bezpieczne"),
			styleH2.Render("Jak to działa"),
			styleOp.Render("Return"),
			styleH2.Render("Ograniczenie do jednego miejsca"),
		),
	},
}
