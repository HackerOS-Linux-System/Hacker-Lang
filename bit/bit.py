#!/usr/bin/python3
import sys
import os
import json
import subprocess
import shutil
import hashlib
import datetime
import re

# ── ANSI ──────────────────────────────────────────────────────────────────────
RST = "\033[0m"
GRN = "\033[38;2;179;108;248m"   # #B36CF8 fioletowy
RED = "\033[31m"
YEL = "\033[38;2;249;132;44m"    # #F9842C pomaranczowy
CYN = "\033[38;2;200;200;200m"   # jasny szary
MAG = "\033[38;2;179;108;248m"   # #B36CF8 fioletowy
DIM = "\033[2m"
BLD = "\033[1m"

# ── Stale ─────────────────────────────────────────────────────────────────────
BIT_REPO_RAW  = "https://raw.githubusercontent.com/bit-io/repository/main/bit-repo/repo-list.json"
BIT_REPO_URL  = "https://github.com/bit-io/repository"

# Katalog domowy uzytkownika — bez sudo, bez /usr/lib
BIT_HOME      = os.path.expanduser("~/.hackeros/hacker-lang/libs")
BIT_CACHE_DIR = os.path.expanduser("~/.hackeros/hacker-lang/cache")
BIT_META_DIR  = os.path.expanduser("~/.hackeros/hacker-lang/meta")
BIT_REPO_FILE = os.path.join(BIT_CACHE_DIR, "repo-list.json")
BIT_LOCK_FILE = os.path.join(BIT_META_DIR, "bit.lock")

# ── Helpers ───────────────────────────────────────────────────────────────────
def pr(msg=""):
    print(msg)

def hr(n=50):
    print("─" * n)

def green(msg):  print(f"{GRN}{msg}{RST}")
def red(msg):    print(f"{RED}{msg}{RST}", file=sys.stderr)
def yellow(msg): print(f"{YEL}{msg}{RST}")

def run(cmd, *, capture=False):
    return subprocess.run(cmd, shell=True, capture_output=capture, text=True)

def run_ok(cmd) -> bool:
    return subprocess.run(cmd, shell=True, capture_output=True).returncode == 0

def run_out(cmd) -> str:
    r = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    return r.stdout.strip()

# ── Progress bar ──────────────────────────────────────────────────────────────
def pb_draw(pct: int):
    fill  = pct * 20 // 100
    empty = 20 - fill
    bar   = "[" + "-" * max(0, fill - 1) + (">" if fill > 0 else "") + "." * empty + "]"
    print(f"\r{CYN}{bar}{RST} {YEL}[{pct}%]{RST}", end="", flush=True)

def pb_done():
    print()

# ── Naglowek ──────────────────────────────────────────────────────────────────
def bit_header():
    hr(50)
    print(f"{MAG}bit{RST} — Hacker Lang Package Manager {DIM}(gen 2){RST}")
    hr(50)

# ── Inicjalizacja katalogow ────────────────────────────────────────────────────
def bit_init_dirs():
    for d in [BIT_HOME, BIT_CACHE_DIR, BIT_META_DIR]:
        os.makedirs(d, exist_ok=True)

# ── Lock file (manifest zainstalowanych) ──────────────────────────────────────
def load_lock() -> dict:
    """Wczytaj bit.lock — { pkg: { version, commit, checksum, path, installed_at } }"""
    if not os.path.exists(BIT_LOCK_FILE):
        return {}
    with open(BIT_LOCK_FILE, encoding="utf-8") as f:
        return json.load(f)

def save_lock(lock: dict):
    os.makedirs(BIT_META_DIR, exist_ok=True)
    with open(BIT_LOCK_FILE, "w", encoding="utf-8") as f:
        json.dump(lock, f, indent=2, ensure_ascii=False)

# ── Checksum katalogu ─────────────────────────────────────────────────────────
def dir_checksum(path: str) -> str:
    """SHA256 z posortowanej listy plikow i ich zawartosci."""
    h = hashlib.sha256()
    for root, dirs, files in sorted(os.walk(path)):
        dirs.sort()
        for fname in sorted(files):
            fpath = os.path.join(root, fname)
            rel   = os.path.relpath(fpath, path)
            h.update(rel.encode())
            try:
                with open(fpath, "rb") as f:
                    while chunk := f.read(65536):
                        h.update(chunk)
            except (OSError, PermissionError):
                pass
    return h.hexdigest()

# ── Wersja = commit hash ───────────────────────────────────────────────────────
def get_commit(repo_dir: str) -> str:
    r = run(f"git -C '{repo_dir}' rev-parse --short HEAD", capture=True)
    return r.stdout.strip() if r.returncode == 0 else "unknown"

def get_commit_date(repo_dir: str) -> str:
    r = run(f"git -C '{repo_dir}' log -1 --format=%ci", capture=True)
    return r.stdout.strip()[:10] if r.returncode == 0 else "unknown"

# ── Repo-list ─────────────────────────────────────────────────────────────────
def bit_fetch_repo():
    print(f"{CYN}Pobieranie listy pakietow...{RST}")
    pb_draw(0); pb_draw(40)
    bit_init_dirs()
    result = run(f'curl -fsSL -o "{BIT_REPO_FILE}" "{BIT_REPO_RAW}"')
    if result.returncode == 0:
        pb_draw(100); pb_done()
        green("Lista pakietow pobrana.")
    else:
        pb_done()
        red("Blad pobierania listy pakietow!")
        sys.exit(1)

def bit_ensure_repo():
    if not os.path.exists(BIT_REPO_FILE):
        bit_fetch_repo()

def load_repo() -> dict:
    bit_ensure_repo()
    with open(BIT_REPO_FILE, encoding="utf-8") as f:
        return json.load(f)

# ── Sciezka pakietu z wersja ──────────────────────────────────────────────────
def pkg_path(name: str, commit: str) -> str:
    """~/.hackeros/hacker-lang/libs/<name>/<commit>/"""
    return os.path.join(BIT_HOME, name, commit)

def pkg_current_link(name: str) -> str:
    """~/.hackeros/hacker-lang/libs/<name>/current  (symlink)"""
    return os.path.join(BIT_HOME, name, "current")

def resolve_current(name: str) -> str | None:
    """Zwroc sciezke aktywnej wersji lub None."""
    link = pkg_current_link(name)
    if os.path.islink(link):
        target = os.readlink(link)
        if not os.path.isabs(target):
            target = os.path.join(os.path.dirname(link), target)
        return target if os.path.isdir(target) else None
    return None

# ── bit install ───────────────────────────────────────────────────────────────
def _do_install(pkg: str, bit_url: str, bit_type: str, silent: bool = False) -> bool:
    """
    Klonuj pakiet do ~/.hackeros/hacker-lang/libs/<pkg>/<commit>/
    Zaktualizuj symlink current i bit.lock.
    Zwraca True przy sukcesie.
    """
    bit_init_dirs()

    # Klonuj do katalogu tymczasowego, zeby najpierw poznac commit
    tmp_dir = os.path.join(BIT_CACHE_DIR, f"_tmp_{pkg}")
    shutil.rmtree(tmp_dir, ignore_errors=True)

    if not silent:
        print(f"  {DIM}Klonowanie: {bit_url}{RST}")

    r = run(f"git clone --depth=1 '{bit_url}' '{tmp_dir}'", capture=silent)
    if r.returncode != 0:
        if not silent:
            red(f"Blad klonowania: {bit_url}")
        shutil.rmtree(tmp_dir, ignore_errors=True)
        return False

    commit      = get_commit(tmp_dir)
    commit_date = get_commit_date(tmp_dir)
    dest        = pkg_path(pkg, commit)

    # Juz zainstalowany w tej wersji?
    if os.path.isdir(dest):
        if not silent:
            yellow(f"  Wersja {commit} juz zainstalowana.")
        shutil.rmtree(tmp_dir, ignore_errors=True)
        _set_current(pkg, commit)
        return True

    # Przenieś z tmp na docelowy
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    shutil.move(tmp_dir, dest)

    # Checksum
    if not silent:
        print(f"  {DIM}Obliczanie checksum...{RST}", end="", flush=True)
    checksum = dir_checksum(dest)
    if not silent:
        print(f"\r  {DIM}Checksum: {checksum[:16]}...{RST}          ")

    # Build dla Rust
    if bit_type == "rust":
        src_dir = os.path.join(dest, "source-code")
        if not os.path.isdir(src_dir):
            src_dir = dest
        if not shutil.which("cargo"):
            red("cargo nie znalezione: https://rustup.rs")
            return False
        if not silent:
            print(f"  {CYN}Budowanie Rust...{RST}")
        r_build = run(f"cd '{src_dir}' && cargo build --release", capture=silent)
        if r_build.returncode != 0:
            red("Blad budowania Rust.")
            return False
        if not silent:
            green("  Zbudowano.")

    # Symlink current
    _set_current(pkg, commit)

    # Zapisz w lock
    lock = load_lock()
    lock[pkg] = {
        "version":      commit,
        "commit":       commit,
        "commit_date":  commit_date,
        "checksum":     checksum,
        "type":         bit_type,
        "url":          bit_url,
        "path":         dest,
        "installed_at": datetime.datetime.now().isoformat(timespec="seconds"),
    }
    save_lock(lock)
    return True

def _set_current(pkg: str, commit: str):
    """Ustaw symlink current na dana wersje."""
    link = pkg_current_link(pkg)
    target = commit  # wzgledny
    if os.path.islink(link) or os.path.exists(link):
        os.remove(link)
    os.symlink(target, link)

def _install_silent(pkg: str):
    repo = load_repo()
    info = repo.get(pkg, {})
    url  = info.get("url", "")
    if not url:
        yellow(f"  Pakiet '{pkg}' nie w repo — pomijam.")
        return
    lock = load_lock()
    if pkg in lock and resolve_current(pkg):
        return  # juz zainstalowany
    _do_install(pkg, url, info.get("type", "hl"), silent=True)

def bit_install(pkg: str):
    if not pkg:
        red("Podaj nazwe pakietu: bit install <nazwa>")
        sys.exit(1)

    bit_ensure_repo()
    repo = load_repo()

    print(f"\n{CYN}Instalowanie:{RST} {BLD}{pkg}{RST}")
    hr(40)

    info     = repo.get(pkg, {})
    bit_url  = info.get("url", "")
    bit_type = info.get("type", "?")

    pb_draw(10)

    if not bit_url:
        pb_done()
        red(f"Pakiet '{pkg}' nie znaleziony w repozytorium.")
        print(f"  Lista: {CYN}bit search all{RST}")
        sys.exit(1)

    pb_draw(30)

    ok = _do_install(pkg, bit_url, bit_type, silent=False)

    if not ok:
        pb_done()
        red(f"Instalacja '{pkg}' nie powiodla sie.")
        sys.exit(1)

    pb_draw(100); pb_done()

    # Pokaz wynik z locka
    lock = load_lock()
    entry = lock.get(pkg, {})
    hr(40)
    print(f"  {GRN}✓{RST} Pakiet:      {BLD}{pkg}{RST}")
    print(f"  {DIM}  Commit:      {entry.get('commit', '?')}{RST}")
    print(f"  {DIM}  Data:        {entry.get('commit_date', '?')}{RST}")
    print(f"  {DIM}  Checksum:    {entry.get('checksum', '?')[:32]}...{RST}")
    print(f"  {DIM}  Typ:         {entry.get('type', '?')}{RST}")
    print(f"  {DIM}  Lokalizacja: {entry.get('path', '?')}{RST}")
    hr(40)

# ── bit remove ────────────────────────────────────────────────────────────────
def bit_remove(pkg: str):
    if not pkg:
        red("Podaj nazwe pakietu: bit remove <nazwa>")
        sys.exit(1)

    lock = load_lock()
    pkg_dir = os.path.join(BIT_HOME, pkg)

    if pkg not in lock and not os.path.isdir(pkg_dir):
        red(f"Pakiet '{pkg}' nie jest zainstalowany.")
        sys.exit(1)

    print(f"{YEL}Usuwanie:{RST} {pkg}")
    pb_draw(30)
    shutil.rmtree(pkg_dir, ignore_errors=True)
    pb_draw(80)

    if pkg in lock:
        del lock[pkg]
        save_lock(lock)

    pb_draw(100); pb_done()
    green(f"Pakiet '{pkg}' usuniety.")

# ── bit list ──────────────────────────────────────────────────────────────────
def bit_list():
    repo = load_repo()
    lock = load_lock()
    hr(50)
    print(f"{CYN}Dostepne pakiety bit:{RST}")
    hr(50)
    pr()
    for name, info in sorted(repo.items()):
        typ     = info.get("type", "?")
        entry   = lock.get(name)
        if entry:
            commit = entry.get("commit", "?")[:7]
            print(f"  {GRN}{name:<24}{RST} {DIM}[{typ}]{RST}  {GRN}✓ {commit}{RST}")
        else:
            print(f"  {YEL}{name:<24}{RST} {DIM}[{typ}]{RST}")
    pr()
    hr(50)
    print(f"Repozytorium: {BIT_REPO_URL}")
    pr()
    installed = sum(1 for n in repo if n in lock)
    print(f"{DIM}Zainstalowanych: {installed}/{len(repo)}{RST}")

# ── bit update ────────────────────────────────────────────────────────────────
def bit_update():
    print(f"{CYN}Aktualizacja listy pakietow...{RST}")
    if os.path.exists(BIT_REPO_FILE):
        os.remove(BIT_REPO_FILE)
    bit_fetch_repo()

# ── bit upgrade ───────────────────────────────────────────────────────────────
def bit_upgrade(pkg: str):
    """Pobierz najnowszy commit i zainstaluj obok starego, przestaw current."""
    lock = load_lock()
    if pkg and pkg not in lock:
        red(f"Pakiet '{pkg}' nie jest zainstalowany.")
        sys.exit(1)

    targets = [pkg] if pkg else list(lock.keys())
    repo    = load_repo()

    for name in targets:
        info = repo.get(name, lock.get(name, {}))
        url  = info.get("url") or lock[name].get("url", "")
        typ  = info.get("type") or lock[name].get("type", "hl")
        if not url:
            yellow(f"Brak URL dla '{name}' — pomijam.")
            continue
        print(f"\n{CYN}Upgrade:{RST} {BLD}{name}{RST}")
        _do_install(name, url, typ, silent=False)

# ── bit info ──────────────────────────────────────────────────────────────────
def bit_info(pkg: str):
    if not pkg:
        red("Podaj nazwe pakietu: bit info <nazwa>")
        sys.exit(1)

    repo  = load_repo()
    lock  = load_lock()

    hr(44)
    print(f"  {CYN}Pakiet:{RST} {BLD}{pkg}{RST}")
    hr(44)

    info = repo.get(pkg)
    if info is None:
        red(f"Pakiet '{pkg}' nie znaleziony w repozytorium.")
    else:
        print(f"  {DIM}URL:  {info.get('url', '?')}{RST}")
        print(f"  {DIM}Typ:  {info.get('type', '?')}{RST}")

    entry = lock.get(pkg)
    if entry:
        pr()
        print(f"  {GRN}Zainstalowany:{RST}")
        print(f"  {DIM}  Commit:      {entry.get('commit', '?')}{RST}")
        print(f"  {DIM}  Data commit: {entry.get('commit_date', '?')}{RST}")
        print(f"  {DIM}  Zainstalowano:{entry.get('installed_at', '?')}{RST}")
        print(f"  {DIM}  Checksum:    {entry.get('checksum', '?')}{RST}")
        print(f"  {DIM}  Sciezka:     {entry.get('path', '?')}{RST}")

        # Weryfikacja checksum
        cur = resolve_current(pkg)
        if cur and os.path.isdir(cur):
            print(f"\n  {DIM}Weryfikacja checksum...{RST}", end="", flush=True)
            live = dir_checksum(cur)
            if live == entry.get("checksum", ""):
                print(f"\r  {GRN}✓ Checksum OK{RST}                    ")
            else:
                print(f"\r  {RED}✗ Checksum NIEZGODNY — plik mogl zostac zmodyfikowany!{RST}")
        # Historia wersji
        versions = _list_versions(pkg)
        if len(versions) > 1:
            pr()
            print(f"  {DIM}Historia wersji:{RST}")
            for v in versions:
                active = f" {GRN}← current{RST}" if v == entry.get("commit") else ""
                print(f"  {DIM}  {v}{RST}{active}")
    else:
        print(f"\n  {YEL}Nie zainstalowany.{RST}")

    hr(44)

# ── Historia wersji ───────────────────────────────────────────────────────────
def _list_versions(pkg: str) -> list[str]:
    pkg_dir = os.path.join(BIT_HOME, pkg)
    if not os.path.isdir(pkg_dir):
        return []
    return sorted(
        d for d in os.listdir(pkg_dir)
        if d != "current" and os.path.isdir(os.path.join(pkg_dir, d))
    )

# ── bit verify ────────────────────────────────────────────────────────────────
def bit_verify(pkg: str):
    """Weryfikuj checksum wszystkich lub jednego pakietu."""
    lock = load_lock()
    targets = [pkg] if pkg else list(lock.keys())

    if not targets:
        yellow("Brak zainstalowanych pakietow.")
        return

    hr(50)
    print(f"{CYN}Weryfikacja checksum:{RST}")
    hr(50)
    ok_count = fail_count = 0

    for name in sorted(targets):
        entry = lock.get(name)
        if not entry:
            print(f"  {YEL}{name:<24}{RST} {DIM}brak w lock{RST}")
            continue
        cur = resolve_current(name)
        if not cur or not os.path.isdir(cur):
            print(f"  {RED}{name:<24}{RST} {DIM}brak katalogu{RST}")
            fail_count += 1
            continue
        live = dir_checksum(cur)
        if live == entry.get("checksum", ""):
            print(f"  {GRN}✓{RST} {name:<24} {DIM}{entry['commit'][:7]}{RST}")
            ok_count += 1
        else:
            print(f"  {RED}✗{RST} {name:<24} {RED}NIEZGODNY{RST}")
            fail_count += 1

    hr(50)
    print(f"OK: {GRN}{ok_count}{RST}  Bledy: {RED}{fail_count}{RST}")

# ── bit clean ─────────────────────────────────────────────────────────────────
def bit_clean():
    """Usun stare wersje pakietow (zostaw current)."""
    lock = load_lock()
    cleaned = 0

    for name, entry in lock.items():
        current_commit = entry.get("commit", "")
        versions = _list_versions(name)
        for v in versions:
            if v != current_commit:
                old_path = os.path.join(BIT_HOME, name, v)
                print(f"  {DIM}Usuwam stara wersje: {name}@{v}{RST}")
                shutil.rmtree(old_path, ignore_errors=True)
                cleaned += 1

    # Cache
    cache_tmp = os.path.join(BIT_CACHE_DIR)
    for item in os.listdir(cache_tmp) if os.path.isdir(cache_tmp) else []:
        if item.startswith("_tmp_"):
            shutil.rmtree(os.path.join(cache_tmp, item), ignore_errors=True)
            cleaned += 1

    if cleaned:
        green(f"Oczyszczono {cleaned} starych elementow.")
    else:
        pr("Nic do czyszczenia.")

# ── bit workspace ─────────────────────────────────────────────────────────────
def bit_workspace():
    pr()
    print(f"{CYN}[bit workspace]{RST}")
    hr(44)

    if os.path.exists("bit.hk"):
        green("bit.hk:")
        hr(30)
        with open("bit.hk", encoding="utf-8") as f:
            print(f.read(), end="")
        hr(30)
    else:
        yellow("Brak bit.hk")
        pr()
        pr("Przyklad bit.hk:")
        pr("  [project]")
        pr("  -> name    => MojProjekt")
        pr("  -> version => 1.0.0")
        pr("  -> entry   => source-code/main.hl")
        pr("  -> type    => hl")
        pr()
        pr("  [dependencies]")
        pr("  -> tui")

    pr()
    lock = load_lock()
    if lock:
        print(f"{DIM}Zainstalowane biblioteki:{RST}")
        hr(44)
        for name, entry in sorted(lock.items()):
            commit = entry.get("commit", "?")[:7]
            date   = entry.get("commit_date", "?")
            typ    = entry.get("type", "?")
            print(f"  {GRN}{name:<20}{RST} {DIM}@ {commit}  [{typ}]  {date}{RST}")
    else:
        print(f"{DIM}Brak zainstalowanych bibliotek.{RST}")

    pr()
    print(f"{DIM}Katalog libs:  {BIT_HOME}{RST}")
    print(f"{DIM}Katalog meta:  {BIT_META_DIR}{RST}")
    print(f"{DIM}Lock file:     {BIT_LOCK_FILE}{RST}")

    if os.path.isdir(BIT_CACHE_DIR):
        size = run_out(f"du -sh '{BIT_CACHE_DIR}' 2>/dev/null | cut -f1")
        print(f"{DIM}Cache:         {size or '?'}{RST}")

# ── bit search ────────────────────────────────────────────────────────────────
def _print_pkg_list(packages: dict, title: str):
    lock = load_lock()
    hr(50)
    print(f"{CYN}{title}{RST}")
    hr(50)
    if not packages:
        yellow("Brak wynikow.")
        return
    pr()
    for name, info in sorted(packages.items()):
        typ     = info.get("type", "?")
        url     = info.get("url", "")
        desc    = info.get("description", "")
        entry   = lock.get(name)
        if entry:
            commit = entry.get("commit", "?")[:7]
            tag    = f"  {GRN}✓ zainstalowany @ {commit}{RST}"
        else:
            tag = ""
        print(f"  {GRN}{name:<24}{RST} {DIM}[{typ}]{RST}{tag}")
        if desc:
            print(f"  {DIM}   {desc}{RST}")
        if url:
            print(f"  {DIM}   {url}{RST}")
    pr()
    print(f"{DIM}Pakietow: {len(packages)}{RST}")
    hr(50)

def bit_search(query: str):
    repo = load_repo()

    if not query or query.lower() == "all":
        if not query:
            red("Podaj fraze lub uzyj: bit search all")
            sys.exit(1)
        _print_pkg_list(repo, "Wszystkie pakiety bit:")
        return

    q = query.lower()
    matches = {
        name: info
        for name, info in repo.items()
        if q in name.lower()
        or q in info.get("type", "").lower()
        or q in info.get("description", "").lower()
    }

    if not matches:
        hr(50)
        yellow(f"Brak wynikow dla '{query}'.")
        print(f"Wszystkie pakiety: {CYN}bit search all{RST}")
        hr(50)
    else:
        _print_pkg_list(matches, f"Wyniki dla: {BLD}{query}{RST}")

# ── bit run ───────────────────────────────────────────────────────────────────
def bit_run():
    pr()
    print(f"{CYN}[bit run]{RST} Uruchamianie projektu...")
    pr()

    print(f"{DIM}[1/4] Szukanie pliku wejsciowego...{RST}")
    entry_file = ""
    entry_type = "hl"

    if os.path.exists("bit.hk"):
        entry_file = run_out(
            r"awk '/^\[project\]/{f=1;next} f&&/^\[/{f=0} "
            r"f&&/-> *entry/{match($0,/=>[[:space:]]*/);v=substr($0,RSTART+RLENGTH);"
            r"gsub(/[[:space:]]/,\"\",v);print v;exit}' bit.hk"
        )

    if not entry_file:
        for loc in ["source-code/main.hl", "src/main.hl", "cmd/main.hl", "main.hl", "app/main.hl"]:
            if os.path.exists(loc):
                entry_file = loc
                break

    if not entry_file:
        for loc in ["source-code/Cargo.toml", "src/Cargo.toml", "Cargo.toml"]:
            if os.path.exists(loc):
                entry_file = loc
                entry_type = "rust"
                break

    if not entry_file:
        red("Nie znaleziono pliku wejsciowego!")
        pr()
        pr("Sprawdzone lokalizacje:")
        pr("  source-code/main.hl  src/main.hl  cmd/main.hl  main.hl")
        pr("  source-code/Cargo.toml  Cargo.toml")
        pr()
        print(f"Stwórz {CYN}bit.hk{RST} z polem entry lub jeden z powyzszych plików.")
        sys.exit(1)

    print(f"{GRN}Znaleziono:{RST} {entry_file}")
    is_hl = entry_file.endswith(".hl")

    if is_hl:
        print(f"{DIM}[2/4] Sprawdzanie poprawnosci...{RST}")
        if not run_ok(f"hl check '{entry_file}'"):
            red("Bledy w kodzie — popraw przed uruchomieniem.")
            sys.exit(1)
        green("Kod poprawny.")

    print(f"{DIM}[3/4] Sprawdzanie zaleznosci...{RST}")
    bit_init_dirs()

    if os.path.exists("bit.hk"):
        deps_raw = run_out(
            r"awk '/^\[dependencies\]/{f=1;next} f&&/^\[/{f=0} "
            r"f&&/-> /{match($0,/->[[:space:]]*/);v=substr($0,RSTART+RLENGTH);"
            r"gsub(/[[:space:]].*/,\"\",v);if(v!=\"\")print v}' bit.hk"
        )
        lock = load_lock()
        for dep in deps_raw.splitlines():
            dep = dep.strip()
            if dep and (dep not in lock or not resolve_current(dep)):
                print(f"  {DIM}instaluje zaleznosc: {dep}{RST}")
                _install_silent(dep)

    green("Zaleznosci OK.")

    print(f"{DIM}[4/4] Uruchamianie...{RST}")
    hr(40)

    if is_hl:
        os.execvp("hl", ["hl", "run", entry_file])
    else:
        cargo_dir = os.path.dirname(entry_file)
        run(f"cd '{cargo_dir}' && cargo run --release")

# ── bit installed ────────────────────────────────────────────────────────────
def bit_installed():
    lock = load_lock()
    hr(50)
    print(f"{CYN}Zainstalowane biblioteki:{RST}")
    hr(50)
    if not lock:
        yellow("Brak zainstalowanych pakietow.")
        print(f"  Zainstaluj: {GRN}bit install <nazwa>{RST}")
        hr(50)
        return
    pr()
    for name, entry in sorted(lock.items()):
        commit   = entry.get("commit", "?")[:7]
        date     = entry.get("commit_date", "?")
        typ      = entry.get("type", "?")
        cur      = resolve_current(name)
        ok_mark  = f"{GRN}✓{RST}" if cur and os.path.isdir(cur) else f"{RED}✗{RST}"
        print(f"  {ok_mark} {GRN}{name:<22}{RST} {YEL}@ {commit}{RST}  {DIM}[{typ}]  {date}{RST}")
    pr()
    hr(50)
    print(f"{DIM}Zainstalowanych: {len(lock)}{RST}")

# ── bit help ──────────────────────────────────────────────────────────────────
def bit_help():
    bit_header()
    pr()
    print(f"{BLD}Uruchamianie projektu:{RST}")
    print(f"  {GRN}bit run{RST}                    — znajdz + check + zaleznosci + uruchom")
    pr()
    print(f"{BLD}Manager pakietow:{RST}")
    print(f"  {GRN}bit install {CYN}<nazwa> ...{RST}      — zainstaluj pakiet(y)")
    print(f"  {GRN}bit remove  {CYN}<nazwa> ...{RST}      — usun pakiet(y)")
    print(f"  {GRN}bit upgrade {CYN}[nazwa]{RST}         — upgrade pakietu (lub wszystkich)")
    print(f"  {GRN}bit verify  {CYN}[nazwa]{RST}         — weryfikuj checksum")
    print(f"  {GRN}bit list{RST}                   — lista pakietow z repo")
    print(f"  {GRN}bit installed{RST}              — zainstalowane biblioteki")
    print(f"  {GRN}bit search  {CYN}<fraza>{RST}         — szukaj pakietu")
    print(f"  {GRN}bit search  {CYN}all{RST}             — pokaz wszystkie pakiety")
    print(f"  {GRN}bit update{RST}                 — aktualizuj liste repo")
    print(f"  {GRN}bit info    {CYN}<nazwa>{RST}         — info + checksum o pakiecie")
    print(f"  {GRN}bit clean{RST}                  — usun stare wersje i cache")
    pr()
    print(f"{BLD}Projekt:{RST}")
    print(f"  {GRN}bit workspace{RST}              — info o projekcie i zainstalowanych libs")
    pr()
    print(f"{BLD}Lokalizacje:{RST}")
    print(f"  {DIM}Libs:   ~/.hackeros/hacker-lang/libs/<pkg>/<commit>/{RST}")
    print(f"  {DIM}Meta:   ~/.hackeros/hacker-lang/meta/bit.lock{RST}")
    print(f"  {DIM}Cache:  ~/.hackeros/hacker-lang/cache/{RST}")
    pr()
    print(f"Repozytorium: {BIT_REPO_URL}")

# ── MAIN ──────────────────────────────────────────────────────────────────────
def main():
    args = sys.argv[1:]
    cmd  = args[0] if len(args) > 0 else ""
    pkgs = args[1:]          # lista pakietow (moze byc wiele)
    pkg  = pkgs[0] if pkgs else ""

    KNOWN_CMDS = {
        "run", "install", "remove", "upgrade", "verify",
        "list", "installed", "search", "update", "info", "clean", "workspace", "help",
    }

    if not cmd:
        bit_help()
        return

    if cmd not in KNOWN_CMDS:
        bit_header()
        pr()
        print(f"{RED}Nieznana komenda:{RST} {BLD}{cmd}{RST}")
        pr()
        print(f"Dostepne komendy: {CYN}" + "  ".join(sorted(KNOWN_CMDS)) + RST)
        print(f"Szczegoly: {GRN}bit help{RST}")
        sys.exit(1)

    # Komendy obslugujace wiele pakietow naraz
    if cmd == "install":
        if not pkgs:
            red("Podaj nazwe pakietu: bit install <nazwa> [nazwa2 ...]")
            sys.exit(1)
        bit_ensure_repo()
        for p in pkgs:
            bit_install(p)
        return

    if cmd == "remove":
        if not pkgs:
            red("Podaj nazwe pakietu: bit remove <nazwa> [nazwa2 ...]")
            sys.exit(1)
        for p in pkgs:
            bit_remove(p)
        return

    if cmd == "upgrade":
        if pkgs:
            bit_ensure_repo()
            for p in pkgs:
                bit_upgrade(p)
        else:
            bit_upgrade("")
        return

    if cmd == "verify":
        if pkgs:
            for p in pkgs:
                bit_verify(p)
        else:
            bit_verify("")
        return

    if cmd == "info":
        if not pkgs:
            red("Podaj nazwe pakietu: bit info <nazwa>")
            sys.exit(1)
        for p in pkgs:
            bit_info(p)
        return

    dispatch = {
        "run":       bit_run,
        "list":      lambda: (bit_ensure_repo(), bit_list()),
        "installed": bit_installed,
        "search":    lambda: (bit_ensure_repo(), bit_search(pkg)),
        "update":    bit_update,
        "clean":     bit_clean,
        "workspace": bit_workspace,
        "help":      bit_help,
    }

    dispatch[cmd]()

if __name__ == "__main__":
    main()
