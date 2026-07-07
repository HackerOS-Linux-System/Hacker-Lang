#!/usr/bin/env lua
--- bit — Hacker Lang Package Manager
--- Port 1:1 z bit.py + obsługa config.hk
--- Wymaga: curl, git, jq

-- ── Pure-Lua JSON (bez zewnętrznych zależności) ───────────────────────────────
local json = (function()
    local M = {}

    local function skip(s, i)
        while i <= #s and s:sub(i,i):match("%s") do i=i+1 end
        return i
    end

    local function parse_str(s, i)
        i = i + 1  -- skip opening "
        local r = {}
        while i <= #s do
            local c = s:sub(i,i)
            if c == '"' then return table.concat(r), i+1 end
            if c == '\\' then
                i = i+1; c = s:sub(i,i)
                local esc = {['"']='"',['\\']='\\',['/']=  '/',['n']='\n',
                             ['r']='\r',['t']='\t',['b']='\b',['f']='\f'}
                if esc[c] then r[#r+1]=esc[c]
                elseif c=='u' then
                    local h = tonumber(s:sub(i+1,i+4),16) or 0
                    if h < 0x80 then r[#r+1]=string.char(h)
                    elseif h < 0x800 then
                        r[#r+1]=string.char(0xC0+math.floor(h/64), 0x80+(h%64))
                    else
                        r[#r+1]=string.char(0xE0+math.floor(h/4096),
                                            0x80+math.floor((h%4096)/64), 0x80+(h%64))
                    end
                    i=i+4
                else r[#r+1]=c end
            else r[#r+1]=c end
            i=i+1
        end
        error("unterminated string")
    end

    local parse_val  -- forward decl

    local function parse_obj(s, i)
        i = skip(s, i+1)
        local o = {}
        if s:sub(i,i)=='}' then return o, i+1 end
        while true do
            i = skip(s,i)
            local k; k,i = parse_str(s,i)
            i = skip(s,i); assert(s:sub(i,i)==':'); i=skip(s,i+1)
            local v; v,i = parse_val(s,i)
            o[k]=v
            i=skip(s,i)
            local c=s:sub(i,i)
            if c=='}' then return o, i+1
            elseif c==',' then i=i+1
            else error("bad object at "..i) end
        end
    end

    local function parse_arr(s, i)
        i = skip(s, i+1)
        local a = {}
        if s:sub(i,i)==']' then return a, i+1 end
        while true do
            i=skip(s,i)
            local v; v,i=parse_val(s,i)
            a[#a+1]=v
            i=skip(s,i)
            local c=s:sub(i,i)
            if c==']' then return a, i+1
            elseif c==',' then i=i+1
            else error("bad array at "..i) end
        end
    end

    local function parse_num(s, i)
        local j=i
        if s:sub(j,j)=='-' then j=j+1 end
        while j<=#s and s:sub(j,j):match("[0-9]") do j=j+1 end
        if j<=#s and s:sub(j,j)=='.' then j=j+1
            while j<=#s and s:sub(j,j):match("[0-9]") do j=j+1 end end
        if j<=#s and s:sub(j,j):match("[eE]") then j=j+1
            if j<=#s and s:sub(j,j):match("[+-]") then j=j+1 end
            while j<=#s and s:sub(j,j):match("[0-9]") do j=j+1 end end
        return tonumber(s:sub(i,j-1)), j
    end

    parse_val = function(s, i)
        i=skip(s,i)
        local c=s:sub(i,i)
        if     c=='"' then return parse_str(s,i)
        elseif c=='{' then return parse_obj(s,i)
        elseif c=='[' then return parse_arr(s,i)
        elseif c=='t' then assert(s:sub(i,i+3)=="true");  return true,  i+4
        elseif c=='f' then assert(s:sub(i,i+4)=="false"); return false, i+5
        elseif c=='n' then assert(s:sub(i,i+3)=="null");  return nil,   i+4
        elseif c:match("[%-0-9]") then return parse_num(s,i)
        else error("unexpected '"..c.."' at "..i) end
    end

    function M.decode(s)
        local ok, v = pcall(function() local r,_ = parse_val(s, skip(s,1)); return r end)
        return ok and v or nil
    end

    local function enc(v, ind, lv)
        local t=type(v)
        if v==nil then return "null" end
        if t=="boolean" then return v and "true" or "false" end
        if t=="number"  then
            return v==math.floor(v) and string.format("%d",v) or string.format("%g",v) end
        if t=="string"  then
            local s=v:gsub('\\','\\\\'):gsub('"','\\"'):gsub('\n','\\n'):gsub('\r','\\r'):gsub('\t','\\t')
            return '"'..s..'"' end
        if t=="table" then
            local p0=ind and string.rep("  ",lv)   or ""
            local p1=ind and string.rep("  ",lv+1) or ""
            local nl=ind and "\n" or ""
            local cs=ind and ",\n" or ","
            if #v>0 then
                local a={}; for _,x in ipairs(v) do a[#a+1]=p1..enc(x,ind,lv+1) end
                return "["..nl..table.concat(a,cs)..nl..p0.."]"
            else
                local ks={}; for k in pairs(v) do ks[#ks+1]=k end; table.sort(ks)
                local a={}; for _,k in ipairs(ks) do a[#a+1]=p1..'"'..k..'": '..enc(v[k],ind,lv+1) end
                return "{"..nl..table.concat(a,cs)..nl..p0.."}"
            end
        end
        return "null"
    end

    function M.encode(v)        return enc(v, false, 0) end
    function M.encode_pretty(v) return enc(v, true,  0) end

    return M
end)()

-- ── ANSI ──────────────────────────────────────────────────────────────────────
local RST = "\027[0m"
local GRN = "\027[38;2;179;108;248m"
local RED = "\027[31m"
local YEL = "\027[38;2;249;132;44m"
local CYN = "\027[38;2;200;200;200m"
local MAG = "\027[38;2;179;108;248m"
local DIM = "\027[2m"
local BLD = "\027[1m"

local BIT_REPO_RAW = "https://raw.githubusercontent.com/bit-io/repository/main/bit-repo/repo-list.json"
local BIT_REPO_URL = "https://github.com/bit-io/repository"

-- ── Resolve paths z env + config.hk ──────────────────────────────────────────
local HOME = os.getenv("HOME") or "/home/user"
local BIT_HOME      = os.getenv("BIT_HOME")      or (HOME.."/.hackeros/hacker-lang/libs")
local BIT_CACHE_DIR = os.getenv("BIT_CACHE_DIR") or (HOME.."/.hackeros/hacker-lang/cache")
local BIT_META_DIR  = os.getenv("BIT_META_DIR")  or (HOME.."/.hackeros/hacker-lang/meta")
local BIT_LOCK_FILE = os.getenv("BIT_LOCK_FILE") or (HOME.."/.hackeros/hacker-lang/meta/bit.lock")
local BIT_REPO_FILE = BIT_CACHE_DIR.."/repo-list.json"

local function resolve_paths()
    local cfg = HOME.."/.config/hackeros/hacker-lang/config.hk"
    local f = io.open(cfg, "r")
    if not f then return end
    for line in f:lines() do
        local p = line:match("^%->%s*active_path%s*=>%s*(.-)%s*$")
        if p and p ~= "" then
            BIT_HOME      = p.."/libs"
            BIT_CACHE_DIR = p.."/cache"
            BIT_META_DIR  = p.."/meta"
            BIT_LOCK_FILE = p.."/meta/bit.lock"
            BIT_REPO_FILE = p.."/cache/repo-list.json"
            break
        end
    end
    f:close()
end

-- ── Helpers ───────────────────────────────────────────────────────────────────
local function pr(m)     print(m or "") end
local function hr(n)     print(("─"):rep(n or 50)) end
local function green(m)  print(GRN..m..RST) end
local function red(m)    io.stderr:write(RED..m..RST.."\n") end
local function yellow(m) print(YEL..m..RST) end
local function bold(m)   print(BLD..m..RST) end

local function run(cmd)     return os.execute(cmd) end
local function run_ok(cmd)
    local r = os.execute(cmd.." >/dev/null 2>&1")
    return r == 0 or r == true
end
local function run_out(cmd)
    local p = io.popen(cmd.." 2>/dev/null")
    if not p then return "" end
    local s = p:read("*a") or ""; p:close()
    return s:match("^%s*(.-)%s*$")
end

-- ── Progress bar ──────────────────────────────────────────────────────────────
local function pb_draw(pct)
    local fill  = math.floor(pct*20/100)
    local empty = 20-fill
    local bar   = "[" .. ("-"):rep(math.max(0,fill-1))
                      .. (fill>0 and ">" or "") .. ("."):rep(empty) .. "]"
    io.write(string.format("\r%s%s%s %s[%d%%]%s", CYN, bar, RST, YEL, pct, RST))
    io.flush()
end
local function pb_done() print("") end

-- ── Nagłówek ──────────────────────────────────────────────────────────────────
local function bit_header()
    hr(50)
    print(MAG.."bit"..RST.." — Hacker Lang Package Manager "..DIM.."(gen 2)"..RST)
    hr(50)
end

-- ── Init ──────────────────────────────────────────────────────────────────────
local function bit_init_dirs()
    os.execute("mkdir -p '"..BIT_HOME.."'")
    os.execute("mkdir -p '"..BIT_CACHE_DIR.."'")
    os.execute("mkdir -p '"..BIT_META_DIR.."'")
end

-- ── JSON helpers ──────────────────────────────────────────────────────────────
local function read_json(path)
    local f = io.open(path, "r")
    if not f then return nil end
    local s = f:read("*a"); f:close()
    return json.decode(s)
end

local function write_json(path, t)
    -- Użyj jq do bezpiecznego zapisu (atomowy via temp file)
    local s = json.encode(t)
    local tmp = run_out("mktemp")
    if tmp == "" then tmp = path..".tmp" end
    local f = io.open(tmp, "w")
    if f then f:write(s); f:close()
        os.execute("mv '"..tmp.."' '"..path.."'")
    end
end

-- ── Lock file ─────────────────────────────────────────────────────────────────
local function load_lock()
    return read_json(BIT_LOCK_FILE) or {}
end

local function lock_has(pkg)
    local lock = load_lock()
    return lock[pkg] ~= nil
end

local function lock_set(pkg, ver, commit, extra)
    bit_init_dirs()
    local lock = load_lock()
    local date = run_out("date -Iseconds")
    lock[pkg] = {
        version      = ver,
        commit       = commit,
        commit_date  = (extra and extra.commit_date) or "?",
        checksum     = (extra and extra.checksum)    or "",
        url          = (extra and extra.url)         or "",
        type         = (extra and extra.typ)         or "hl",
        installed_at = date,
        path         = (extra and extra.path)        or (BIT_HOME.."/"..pkg.."/"..commit),
    }
    write_json(BIT_LOCK_FILE, lock)
end

local function lock_del(pkg)
    local lock = load_lock()
    if not lock[pkg] then return end
    lock[pkg] = nil
    write_json(BIT_LOCK_FILE, lock)
end

-- ── Checksum ──────────────────────────────────────────────────────────────────
local function dir_checksum(path)
    return run_out("find '"..path.."' -type f | sort | xargs sha256sum 2>/dev/null | sha256sum | cut -c1-64")
end

-- ── Git ───────────────────────────────────────────────────────────────────────
local function get_commit(d)
    local c = run_out("git -C '"..d.."' rev-parse --short HEAD")
    return c ~= "" and c or "unknown"
end
local function get_commit_date(d)
    return run_out("git -C '"..d.."' log -1 --format=%ci"):sub(1,10)
end

-- ── Resolve current ───────────────────────────────────────────────────────────
local function resolve_current(pkg)
    local link = BIT_HOME.."/"..pkg.."/current"
    local t = run_out("readlink -f '"..link.."'")
    if t ~= "" and run_ok("test -d '"..t.."'") then return t end
    return nil
end

local function list_versions(pkg)
    local d   = BIT_HOME.."/"..pkg
    local raw = run_out("find '"..d.."' -maxdepth 1 -mindepth 1 -type d ! -name current 2>/dev/null")
    local vs  = {}
    for line in (raw.."\n"):gmatch("([^\n]+)\n") do
        local n = line:match("[^/]+$")
        if n then vs[#vs+1]=n end
    end
    table.sort(vs)
    return vs
end

-- ── Repo ──────────────────────────────────────────────────────────────────────
local function bit_fetch_repo()
    print(CYN.."Pobieranie listy pakietów..."..RST)
    pb_draw(0); pb_draw(40)
    bit_init_dirs()
    local ok2 = run("curl -fsSL -o '"..BIT_REPO_FILE.."' '"..BIT_REPO_RAW.."'")
    if ok2==0 or ok2==true then
        pb_draw(100); pb_done()
        green("Lista pakietów pobrana.")
    else
        pb_done(); red("Błąd pobierania listy pakietów!"); os.exit(1)
    end
end

local function bit_ensure_repo()
    local f = io.open(BIT_REPO_FILE, "r")
    if f then f:close() else bit_fetch_repo() end
end

local function load_repo()
    bit_ensure_repo()
    return read_json(BIT_REPO_FILE) or {}
end

-- ── Set current symlink ───────────────────────────────────────────────────────
local function set_current(pkg, commit)
    local link = BIT_HOME.."/"..pkg.."/current"
    os.execute("rm -f '"..link.."'")
    os.execute("cd '"..BIT_HOME.."/"..pkg.."' && ln -sf '"..commit.."' current")
end

-- ── Install ───────────────────────────────────────────────────────────────────
local function _do_install(pkg, url, silent)
    bit_init_dirs()
    local tmp = BIT_CACHE_DIR.."/_tmp_"..pkg
    os.execute("rm -rf '"..tmp.."'")
    if not silent then print("  "..DIM.."Klonowanie: "..url..RST) end
    local ok2 = run("git clone --depth=1 '"..url.."' '"..tmp.."'"
                    ..(silent and " >/dev/null 2>&1" or ""))
    if ok2 ~= 0 and ok2 ~= true then
        if not silent then red("Błąd klonowania: "..url) end
        os.execute("rm -rf '"..tmp.."'"); return false
    end
    local commit      = get_commit(tmp)
    local commit_date = get_commit_date(tmp)
    local dest        = BIT_HOME.."/"..pkg.."/"..commit
    if run_ok("test -d '"..dest.."'") then
        if not silent then yellow("  Wersja "..commit.." już zainstalowana.") end
        os.execute("rm -rf '"..tmp.."'")
        set_current(pkg, commit); return true
    end
    os.execute("mkdir -p '"..dest.."'")
    os.execute("cp -r '"..tmp.."/.' '"..dest.."/'")
    os.execute("rm -rf '"..tmp.."'")
    if not silent then
        io.write("  "..DIM.."Obliczanie checksum..."..RST); io.flush()
    end
    local checksum = dir_checksum(dest)
    if not silent then
        print("\r  "..DIM.."Checksum: "..checksum:sub(1,16).."..."..RST.."          ")
    end
    set_current(pkg, commit)
    lock_set(pkg, commit, commit, {
        commit_date = commit_date,
        checksum    = checksum,
        url         = url,
        typ         = "hl",
        path        = dest,
    })
    return true
end

local function _install_silent(pkg)
    local repo = load_repo()
    local info = repo[pkg] or {}
    local url  = info.url or ""
    if url=="" then yellow("  Pakiet '"..pkg.."' nie w repo — pomijam."); return end
    if lock_has(pkg) and resolve_current(pkg) then return end
    _do_install(pkg, url, true)
end

local function bit_install(pkg)
    if not pkg or pkg=="" then red("Podaj nazwę pakietu: bit install <nazwa>"); os.exit(1) end
    bit_ensure_repo()
    local repo = load_repo()
    print("\n"..CYN.."Instalowanie:"..RST.." "..BLD..pkg..RST)
    hr(40)
    local info = repo[pkg] or {}
    local url  = info.url or ""
    pb_draw(10)
    if url=="" then
        pb_done(); red("Pakiet '"..pkg.."' nie znaleziony w repozytorium.")
        print("  Lista: "..CYN.."bit search all"..RST); os.exit(1)
    end
    pb_draw(30)
    if not _do_install(pkg, url, false) then
        pb_done(); red("Instalacja '"..pkg.."' nie powiodła się."); os.exit(1)
    end
    pb_draw(100); pb_done()
    local lock  = load_lock()
    local entry = lock[pkg] or {}
    hr(40)
    print("  "..GRN.."✓"..RST.." Pakiet:      "..BLD..pkg..RST)
    print("  "..DIM.."  Commit:      "..(entry.commit or "?")..RST)
    print("  "..DIM.."  Data:        "..(entry.commit_date or "?")..RST)
    local cs = entry.checksum or "?"
    print("  "..DIM.."  Checksum:    "..cs:sub(1,32).."..."..RST)
    print("  "..DIM.."  Typ:         "..(entry.type or "?")..RST)
    print("  "..DIM.."  Lokalizacja: "..(entry.path or BIT_HOME.."/"..pkg)..RST)
    hr(40)
end

-- ── Remove ────────────────────────────────────────────────────────────────────
local function bit_remove(pkg)
    if not pkg or pkg=="" then red("Podaj nazwę pakietu: bit remove <nazwa>"); os.exit(1) end
    local pkg_dir = BIT_HOME.."/"..pkg
    if not lock_has(pkg) and not run_ok("test -d '"..pkg_dir.."'") then
        red("Pakiet '"..pkg.."' nie jest zainstalowany."); os.exit(1)
    end
    print(YEL.."Usuwanie:"..RST.." "..pkg)
    pb_draw(30); os.execute("rm -rf '"..pkg_dir.."'"); pb_draw(80)
    lock_del(pkg)
    pb_draw(100); pb_done(); green("Pakiet '"..pkg.."' usunięty.")
end

-- ── List ──────────────────────────────────────────────────────────────────────
local function bit_list()
    local repo = load_repo()
    local lock = load_lock()
    hr(50); print(CYN.."Dostępne pakiety bit:"..RST); hr(50); pr()
    local names = {}
    for k in pairs(repo) do names[#names+1]=k end
    table.sort(names)
    for _, name in ipairs(names) do
        local info  = repo[name]
        local typ   = info.type or "?"
        local entry = lock[name]
        if entry then
            local commit = (entry.commit or "?"):sub(1,7)
            print(string.format("  %s%-24s%s %s[%s]%s  %s✓ %s%s",
                GRN,name,RST, DIM,typ,RST, GRN,commit,RST))
        else
            print(string.format("  %s%-24s%s %s[%s]%s", YEL,name,RST, DIM,typ,RST))
        end
    end
    pr(); hr(50)
    print("Repozytorium: "..BIT_REPO_URL); pr()
    local inst = 0
    for k in pairs(repo) do if lock[k] then inst=inst+1 end end
    print(DIM.."Zainstalowanych: "..inst.."/"..#names..RST)
end

-- ── Update ────────────────────────────────────────────────────────────────────
local function bit_update()
    print(CYN.."Aktualizacja listy pakietów..."..RST)
    os.execute("rm -f '"..BIT_REPO_FILE.."'")
    bit_fetch_repo()
end

-- ── Upgrade ───────────────────────────────────────────────────────────────────
local function bit_upgrade(pkg)
    local lock = load_lock()
    if pkg and pkg~="" and not lock[pkg] then
        red("Pakiet '"..pkg.."' nie jest zainstalowany."); os.exit(1)
    end
    local repo    = load_repo()
    local targets = {}
    if pkg and pkg~="" then
        targets = {pkg}
    else
        for k in pairs(lock) do targets[#targets+1]=k end; table.sort(targets)
    end
    for _, name in ipairs(targets) do
        local info = repo[name] or lock[name] or {}
        local url  = info.url or (lock[name] and lock[name].url) or ""
        if url=="" then yellow("Brak URL dla '"..name.."' — pomijam.")
        else
            print("\n"..CYN.."Upgrade:"..RST.." "..BLD..name..RST)
            _do_install(name, url, false)
        end
    end
end

-- ── Info ──────────────────────────────────────────────────────────────────────
local function bit_info(pkg)
    if not pkg or pkg=="" then red("Podaj nazwę pakietu: bit info <nazwa>"); os.exit(1) end
    local repo  = load_repo()
    local lock  = load_lock()
    hr(44); print("  "..CYN.."Pakiet:"..RST.." "..BLD..pkg..RST); hr(44)
    local info = repo[pkg]
    if not info then red("Pakiet '"..pkg.."' nie znaleziony w repozytorium.")
    else
        print("  "..DIM.."URL:  "..(info.url or "?")..RST)
        print("  "..DIM.."Typ:  "..(info.type or "?")..RST)
    end
    local entry = lock[pkg]
    if entry then
        pr(); print("  "..GRN.."Zainstalowany:"..RST)
        print("  "..DIM.."  Commit:       "..(entry.commit or "?")..RST)
        print("  "..DIM.."  Data commit:  "..(entry.commit_date or "?")..RST)
        print("  "..DIM.."  Zainstalowano:"..(entry.installed_at or "?")..RST)
        print("  "..DIM.."  Checksum:     "..(entry.checksum or "?")..RST)
        print("  "..DIM.."  Ścieżka:      "..(entry.path or BIT_HOME.."/"..pkg)..RST)
        local cur = resolve_current(pkg)
        if cur then
            io.write("\n  "..DIM.."Weryfikacja checksum..."..RST); io.flush()
            local live = dir_checksum(cur)
            if live == (entry.checksum or "") then
                print("\r  "..GRN.."✓ Checksum OK"..RST.."                    ")
            else
                print("\r  "..RED.."✗ Checksum NIEZGODNY!"..RST)
            end
        end
        local versions = list_versions(pkg)
        if #versions > 1 then
            pr(); print("  "..DIM.."Historia wersji:"..RST)
            for _, v in ipairs(versions) do
                local active = v==(entry.commit or "") and (" "..GRN.."← current"..RST) or ""
                print("  "..DIM.."  "..v..RST..active)
            end
        end
    else print("\n  "..YEL.."Nie zainstalowany."..RST) end
    hr(44)
end

-- ── Verify ────────────────────────────────────────────────────────────────────
local function bit_verify(pkg)
    local lock = load_lock()
    local targets = {}
    if pkg and pkg~="" then targets={pkg}
    else for k in pairs(lock) do targets[#targets+1]=k end; table.sort(targets) end
    if #targets==0 then yellow("Brak zainstalowanych pakietów."); return end
    hr(50); print(CYN.."Weryfikacja checksum:"..RST); hr(50)
    local ok_c, fail_c = 0, 0
    for _, name in ipairs(targets) do
        local entry = lock[name]
        if not entry then
            print(string.format("  %s%-24s%s %sbrak w lock%s", YEL,name,RST,DIM,RST))
        else
            local cur = resolve_current(name)
            if not cur then
                print(string.format("  %s%-24s%s %sbrak katalogu%s", RED,name,RST,DIM,RST))
                fail_c=fail_c+1
            else
                local live = dir_checksum(cur)
                if live==(entry.checksum or "") then
                    print(string.format("  %s✓%s %-24s %s%s%s",
                        GRN,RST, name, DIM,(entry.commit or "?"):sub(1,7),RST))
                    ok_c=ok_c+1
                else
                    print(string.format("  %s✗%s %-24s %sNIEZGODNY%s",
                        RED,RST, name, RED,RST))
                    fail_c=fail_c+1
                end
            end
        end
    end
    hr(50); print("OK: "..GRN..ok_c..RST.."  Błędy: "..RED..fail_c..RST)
end

-- ── Clean ─────────────────────────────────────────────────────────────────────
local function bit_clean()
    local lock = load_lock(); local cleaned = 0
    for name, entry in pairs(lock) do
        local cur = entry.commit or ""
        for _, v in ipairs(list_versions(name)) do
            if v~=cur then
                local old = BIT_HOME.."/"..name.."/"..v
                print("  "..DIM.."Usuwam: "..name.."@"..v..RST)
                os.execute("rm -rf '"..old.."'"); cleaned=cleaned+1
            end
        end
    end
    local tmps = run_out("find '"..BIT_CACHE_DIR.."' -maxdepth 1 -name '_tmp_*' -type d 2>/dev/null")
    for line in (tmps.."\n"):gmatch("([^\n]+)\n") do
        if line~="" then os.execute("rm -rf '"..line.."'"); cleaned=cleaned+1 end
    end
    if cleaned>0 then green("Oczyszczono "..cleaned.." starych elementów.")
    else pr("Nic do czyszczenia.") end
end

-- ── Workspace ─────────────────────────────────────────────────────────────────
local function bit_workspace()
    pr(); print(CYN.."[bit workspace]"..RST); hr(44)
    local f = io.open("bit.hk", "r")
    if f then
        green("bit.hk:"); hr(30); print(f:read("*a")); f:close(); hr(30)
    else
        yellow("Brak bit.hk"); pr()
        pr("Przykład bit.hk:")
        pr("  [project]");  pr("  -> name    => MojProjekt")
        pr("  -> version => 1.0.0"); pr("  -> entry   => source-code/main.hl")
        pr("  -> type    => hl");   pr(); pr("  [dependencies]"); pr("  -> tui")
    end
    pr()
    local lock = load_lock()
    local names = {}
    for k in pairs(lock) do names[#names+1]=k end; table.sort(names)
    if #names>0 then
        print(DIM.."Zainstalowane biblioteki:"..RST); hr(44)
        for _, name in ipairs(names) do
            local e = lock[name]
            print(string.format("  %s%-20s%s %s@ %s  [%s]  %s%s",
                GRN,name,RST, DIM,(e.commit or "?"):sub(1,7),(e.type or "?"),(e.commit_date or "?"),RST))
        end
    else print(DIM.."Brak zainstalowanych bibliotek."..RST) end
    pr()
    print(DIM.."Katalog libs:  "..BIT_HOME..RST)
    print(DIM.."Katalog meta:  "..BIT_META_DIR..RST)
    print(DIM.."Lock file:     "..BIT_LOCK_FILE..RST)
    local size = run_out("du -sh '"..BIT_CACHE_DIR.."' 2>/dev/null | cut -f1")
    if size~="" then print(DIM.."Cache:         "..size..RST) end
end

-- ── Search ────────────────────────────────────────────────────────────────────
local function print_pkg_list(packages, title)
    local lock  = load_lock()
    hr(50); print(CYN..title..RST); hr(50)
    local names = {}
    for k in pairs(packages) do names[#names+1]=k end; table.sort(names)
    if #names==0 then yellow("Brak wyników."); return end
    pr()
    for _, name in ipairs(names) do
        local info  = packages[name]
        local typ   = info.type or "?"
        local entry = lock[name]
        local tag   = ""
        if entry then
            tag = "  "..GRN.."✓ zainstalowany @ "..(entry.commit or "?"):sub(1,7)..RST
        end
        print(string.format("  %s%-24s%s %s[%s]%s%s", GRN,name,RST, DIM,typ,RST, tag))
        if info.url  and info.url  ~="" then print("  "..DIM.."   "..info.url..RST) end
    end
    pr(); print(DIM.."Pakietów: "..#names..RST); hr(50)
end

local function bit_search(query)
    local repo = load_repo()
    if not query or query=="" then
        red("Podaj frazę lub użyj: bit search all"); os.exit(1)
    end
    if query:lower()=="all" then
        print_pkg_list(repo, "Wszystkie pakiety bit:"); return
    end
    local q = query:lower()
    local matches = {}
    for name, info in pairs(repo) do
        local desc = (info.description or ""):lower()
        local typ  = (info.type or ""):lower()
        if name:lower():find(q,1,true) or desc:find(q,1,true) or typ:find(q,1,true) then
            matches[name]=info
        end
    end
    if not next(matches) then
        hr(50); yellow("Brak wyników dla '"..query.."'.")
        print("Wszystkie pakiety: "..CYN.."bit search all"..RST); hr(50)
    else
        print_pkg_list(matches, "Wyniki dla: "..BLD..query..RST)
    end
end

-- ── Installed ─────────────────────────────────────────────────────────────────
local function bit_installed()
    local lock = load_lock()
    hr(50); print(CYN.."Zainstalowane biblioteki:"..RST); hr(50)
    local names = {}
    for k in pairs(lock) do names[#names+1]=k end; table.sort(names)
    if #names==0 then
        yellow("Brak zainstalowanych pakietów.")
        print("  Zainstaluj: "..GRN.."bit install <nazwa>"..RST); hr(50); return
    end
    pr()
    for _, name in ipairs(names) do
        local e    = lock[name]
        local commit = (e.commit or "?"):sub(1,7)
        local cur  = resolve_current(name)
        local mark = cur and (GRN.."✓"..RST) or (RED.."✗"..RST)
        print(string.format("  %s %s%-22s%s %s@ %s%s  %s[%s]  %s%s",
            mark, GRN,name,RST, YEL,commit,RST, DIM,(e.type or "?"),(e.commit_date or "?"),RST))
    end
    pr(); hr(50); print(DIM.."Zainstalowanych: "..#names..RST)
end

-- ── Run ───────────────────────────────────────────────────────────────────────
local function bit_run()
    pr(); print(CYN.."[bit run]"..RST.." Uruchamianie projektu..."); pr()
    print(DIM.."[1/4] Szukanie pliku wejściowego..."..RST)
    local entry = ""; local entry_type = "hl"
    local bithk = io.open("bit.hk","r")
    if bithk then
        for line in bithk:lines() do
            local v = line:match("^%s*%->%s*entry%s*=>%s*(.-)%s*$")
            if v and v~="" then entry=v; break end
        end
        bithk:close()
    end
    for _, p in ipairs({"source-code/main.hl","src/main.hl","cmd/main.hl","main.hl","app/main.hl"}) do
        if entry=="" and run_ok("test -f '"..p.."'") then entry=p end
    end
    if entry=="" then
        for _, p in ipairs({"source-code/Cargo.toml","src/Cargo.toml","Cargo.toml"}) do
            if run_ok("test -f '"..p.."'") then entry=p; entry_type="rust"; break end
        end
    end
    if entry=="" then
        red("Nie znaleziono pliku wejściowego!")
        pr(); pr("Sprawdzone: source-code/main.hl  src/main.hl  main.hl  Cargo.toml"); os.exit(1)
    end
    print(GRN.."Znaleziono:"..RST.." "..entry)
    local is_hl = entry:match("%.hl$")~=nil
    if is_hl then
        print(DIM.."[2/4] Sprawdzanie poprawności..."..RST)
        if not run_ok("hl check '"..entry.."'") then
            red("Błędy w kodzie."); os.exit(1)
        end
        green("Kod poprawny.")
    end
    print(DIM.."[3/4] Sprawdzanie zależności..."..RST)
    bit_init_dirs()
    local bithk2 = io.open("bit.hk","r")
    if bithk2 then
        local in_deps=false
        for line in bithk2:lines() do
            if line:match("^%[dependencies%]") then in_deps=true
            elseif line:match("^%[") then in_deps=false
            elseif in_deps then
                local dep = line:match("^%s*%->%s*(.-)%s*$")
                if dep and dep~="" then
                    dep = dep:match("^(%S+)")
                    if dep and not lock_has(dep) then
                        print("  "..DIM.."instaluje: "..dep..RST)
                        _install_silent(dep)
                    end
                end
            end
        end
        bithk2:close()
    end
    green("Zależności OK.")
    print(DIM.."[4/4] Uruchamianie..."..RST); hr(40)
    if is_hl then os.execute("hl run '"..entry.."'")
    else
        local d = entry:match("^(.*)/[^/]+$") or "."
        os.execute("cd '"..d.."' && cargo run --release")
    end
end

-- ── Help ──────────────────────────────────────────────────────────────────────
local function bit_help()
    bit_header(); pr()
    bold("Uruchamianie projektu:")
    print("  "..GRN.."bit run"..RST.."                    — znajdź + check + zależności + uruchom")
    pr(); bold("Manager pakietów:")
    print("  "..GRN.."bit install "..CYN.."<nazwa> ..."..RST.."      — zainstaluj pakiet(y)")
    print("  "..GRN.."bit remove  "..CYN.."<nazwa> ..."..RST.."      — usuń pakiet(y)")
    print("  "..GRN.."bit upgrade "..CYN.."[nazwa]"..RST.."         — upgrade pakietu (lub wszystkich)")
    print("  "..GRN.."bit verify  "..CYN.."[nazwa]"..RST.."         — weryfikuj checksum")
    print("  "..GRN.."bit list"..RST.."                   — lista pakietów z repo")
    print("  "..GRN.."bit installed"..RST.."              — zainstalowane biblioteki")
    print("  "..GRN.."bit search  "..CYN.."<fraza>"..RST.."         — szukaj pakietu")
    print("  "..GRN.."bit search  "..CYN.."all"..RST.."             — pokaż wszystkie pakiety")
    print("  "..GRN.."bit update"..RST.."                 — aktualizuj listę repo")
    print("  "..GRN.."bit info    "..CYN.."<nazwa>"..RST.."         — info + checksum o pakiecie")
    print("  "..GRN.."bit clean"..RST.."                  — usuń stare wersje i cache")
    pr(); bold("Projekt:")
    print("  "..GRN.."bit workspace"..RST.."              — info o projekcie i zainstalowanych libs")
    pr(); bold("Lokalizacje:")
    print("  "..DIM.."Libs:   "..BIT_HOME..RST)
    print("  "..DIM.."Meta:   "..BIT_META_DIR..RST)
    print("  "..DIM.."Lock:   "..BIT_LOCK_FILE..RST)
    pr(); print("Repozytorium: "..BIT_REPO_URL)
end

-- ══════════════════════════════════════════════════════════════════════════════
-- MAIN
-- ══════════════════════════════════════════════════════════════════════════════
resolve_paths()

local cmd  = arg[1] or ""
local pkgs = {}
for i=2,#arg do pkgs[#pkgs+1]=arg[i] end
local pkg = pkgs[1] or ""

local KNOWN = {run=1,install=1,remove=1,upgrade=1,verify=1,list=1,
               installed=1,search=1,update=1,info=1,clean=1,workspace=1,help=1}

if cmd=="" then bit_help(); os.exit(0) end

if not KNOWN[cmd] then
    bit_header(); pr()
    print(RED.."Nieznana komenda:"..RST.." "..BLD..cmd..RST); pr()
    local kl={}; for k in pairs(KNOWN) do kl[#kl+1]=k end; table.sort(kl)
    print("Dostępne komendy: "..CYN..table.concat(kl,"  ")..RST)
    print("Szczegóły: "..GRN.."bit help"..RST); os.exit(1)
end

if cmd=="install" then
    if #pkgs==0 then red("Podaj nazwę pakietu: bit install <nazwa> [nazwa2 ...]"); os.exit(1) end
    bit_ensure_repo(); for _,p in ipairs(pkgs) do bit_install(p) end; os.exit(0)
end
if cmd=="remove" then
    if #pkgs==0 then red("Podaj nazwę pakietu: bit remove <nazwa> [nazwa2 ...]"); os.exit(1) end
    for _,p in ipairs(pkgs) do bit_remove(p) end; os.exit(0)
end
if cmd=="upgrade" then
    bit_ensure_repo()
    if #pkgs>0 then for _,p in ipairs(pkgs) do bit_upgrade(p) end
    else bit_upgrade("") end; os.exit(0)
end
if cmd=="verify" then
    if #pkgs>0 then for _,p in ipairs(pkgs) do bit_verify(p) end
    else bit_verify("") end; os.exit(0)
end
if cmd=="info" then
    if #pkgs==0 then red("Podaj nazwę pakietu: bit info <nazwa>"); os.exit(1) end
    for _,p in ipairs(pkgs) do bit_info(p) end; os.exit(0)
end

local dispatch = {
    run=bit_run, list=function() bit_ensure_repo(); bit_list() end,
    installed=bit_installed, search=function() bit_ensure_repo(); bit_search(pkg) end,
    update=bit_update, clean=bit_clean, workspace=bit_workspace, help=bit_help,
}
local fn = dispatch[cmd]
if fn then fn() end
