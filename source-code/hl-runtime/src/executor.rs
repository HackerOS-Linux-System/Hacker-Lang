use colored::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::thread;

// ─────────────────────────────────────────────────────────────
// Stałe
// ─────────────────────────────────────────────────────────────
const SENTINEL_EXEC: &str = "__HL_X__";
const SENTINEL_COND: &str = "__HL_C__";
const EXEC_TIMEOUT_MS: u64  = 30_000;
const COND_TIMEOUT_MS: u64  = 5_000;
const STDOUT_CHAN_CAP: usize = 4096;
const COND_CACHE_MAX:  usize = 512;

// ─────────────────────────────────────────────────────────────
// ShellKind — backend selector
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind { Bash, Zsh, Dash }

impl Default for ShellKind { fn default() -> Self { Self::Bash } }

impl ShellKind {
    pub fn binary(self) -> &'static str {
        match self { Self::Bash => "bash", Self::Zsh => "zsh", Self::Dash => "dash" }
    }
    fn init_args(self) -> &'static [&'static str] {
        match self {
            Self::Bash => &["--norc", "--noprofile"],
            Self::Zsh  => &["--no-rcs", "--no-globalrcs", "+Z"],
            Self::Dash => &[],
        }
    }
    pub fn supports_double_bracket(self) -> bool {
        matches!(self, Self::Bash | Self::Zsh)
    }
}

// ─────────────────────────────────────────────────────────────
// Wiadomości z wątku I/O stdout
// ─────────────────────────────────────────────────────────────
enum ShellLine {
    Out(String),
    ExecDone(i32),
    CondDone(bool),
    Eof,
}

// ─────────────────────────────────────────────────────────────
// Metryki (lock-free)
// ─────────────────────────────────────────────────────────────
#[derive(Default)]
pub struct ExecMetrics {
    pub exec_count:    AtomicU64,
    pub cond_count:    AtomicU64,
    pub cache_hits:    AtomicU64,
    pub ipc_total_ns:  AtomicU64,
    pub restarts:      AtomicU64,
}

impl ExecMetrics {
    pub fn report(&self) {
        let exec  = self.exec_count.load(Ordering::Relaxed);
        let cond  = self.cond_count.load(Ordering::Relaxed);
        let hits  = self.cache_hits.load(Ordering::Relaxed);
        let ms    = self.ipc_total_ns.load(Ordering::Relaxed) / 1_000_000;
        let rst   = self.restarts.load(Ordering::Relaxed);
        let pct   = if cond > 0 { hits as f64 / cond as f64 * 100.0 } else { 0.0 };
        eprintln!("{}", "━━━ Executor Metrics ━━━━━━━━━━━━━━━━━━━".yellow());
        eprintln!("  exec calls      : {}", exec.to_string().cyan());
        eprintln!("  cond calls      : {}", cond.to_string().cyan());
        eprintln!("  cond cache hits : {} ({:.1}%)", hits.to_string().green(), pct);
        eprintln!("  total IPC time  : {} ms", ms.to_string().yellow());
        eprintln!("  session restarts: {}", rst.to_string().red());
        eprintln!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".yellow());
    }
}

// ─────────────────────────────────────────────────────────────
// Condition cache — LRU z invalidacją przy zmianach zmiennych
// ─────────────────────────────────────────────────────────────
struct CondCache {
    map:   HashMap<u64, bool>,
    order: std::collections::VecDeque<u64>,
}

impl CondCache {
    fn new() -> Self {
        Self {
            map:   HashMap::with_capacity(COND_CACHE_MAX),
            order: std::collections::VecDeque::with_capacity(COND_CACHE_MAX),
        }
    }

    #[inline]
    fn hash(s: &str) -> u64 {
        let mut h = DefaultHasher::new();
        s.hash(&mut h);
        h.finish()
    }

    fn get(&self, cond: &str) -> Option<bool> {
        self.map.get(&Self::hash(cond)).copied()
    }

    fn set(&mut self, cond: &str, val: bool) {
        let k = Self::hash(cond);
        if self.map.len() >= COND_CACHE_MAX {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        self.map.insert(k, val);
        self.order.push_back(k);
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

// ─────────────────────────────────────────────────────────────
// ShellSession — jeden ciągły proces shell + async I/O threads
// ─────────────────────────────────────────────────────────────
struct ShellSession {
    stdin:  ChildStdin,
    rx:     Receiver<ShellLine>,
    child:  Child,
    kind:   ShellKind,
}

impl ShellSession {
    fn spawn(kind: ShellKind, sudo: bool, verbose: bool) -> Option<Self> {
        let mut builder = if sudo {
            let mut b = std::process::Command::new("sudo");
            b.arg(kind.binary());
            b
        } else {
            std::process::Command::new(kind.binary())
        };
        for a in kind.init_args() { builder.arg(a); }

        let mut child = builder
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

        let stdin  = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let stderr = child.stderr.take()?;

        // Wątek stdout — parsuje linie i wysyła przez channel
        let (tx, rx) = mpsc::sync_channel::<ShellLine>(STDOUT_CHAN_CAP);
        {
            let tx2 = tx.clone();
            thread::Builder::new()
            .name(format!("hl-stdout-{}", kind.binary()))
            .spawn(move || {
                let rdr = BufReader::with_capacity(128 * 1024, stdout);
                for line in rdr.lines() {
                    let msg = match line {
                        Err(_) => ShellLine::Eof,
                   Ok(l) => {
                       if let Some(r) = l.strip_prefix(SENTINEL_EXEC) {
                           let code = r.trim_start_matches(':').parse().unwrap_or(0);
                           ShellLine::ExecDone(code)
                       } else if let Some(r) = l.strip_prefix(SENTINEL_COND) {
                           ShellLine::CondDone(r.trim_start_matches(':').trim() == "0")
                       } else {
                           ShellLine::Out(l)
                       }
                   }
                    };
                    let is_eof = matches!(msg, ShellLine::Eof);
                    let _ = tx2.send(msg);
                    if is_eof { break; }
                }
            })
            .ok()?;
        }

        // Wątek stderr — natychmiastowy passthrough
        thread::Builder::new()
        .name(format!("hl-stderr-{}", kind.binary()))
        .spawn(move || {
            let rdr = BufReader::with_capacity(8192, stderr);
            for line in rdr.lines() {
                if let Ok(l) = line { eprintln!("{}", l); }
            }
        })
        .ok()?;

        if verbose {
            eprintln!("{} {:?} session spawned (sudo={})", "[exec]".green(), kind, sudo);
        }

        Some(Self { stdin, rx, child, kind })
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn exec(&mut self, cmd: &str) -> i32 {
        let cmd_s = if !self.kind.supports_double_bracket() {
            adapt_posix(cmd)
        } else {
            cmd.to_string()
        };

        if writeln!(self.stdin, "{}", cmd_s).is_err()
            || writeln!(self.stdin, "echo {}:$?", SENTINEL_EXEC).is_err()
            || self.stdin.flush().is_err()
            {
                return 1;
            }

            let deadline = Instant::now() + Duration::from_millis(EXEC_TIMEOUT_MS);
            loop {
                let rem = deadline.saturating_duration_since(Instant::now());
                if rem.is_zero() {
                    eprintln!("{} exec timeout (30s): {}…", "[!]".yellow(), &cmd[..cmd.len().min(60)]);
                    return 1;
                }
                match self.rx.recv_timeout(rem) {
                    Ok(ShellLine::ExecDone(c)) => return c,
                    Ok(ShellLine::Out(l))      => println!("{}", l),
                    Ok(ShellLine::Eof)         => return 1,
                    Ok(ShellLine::CondDone(_)) => {} // spurious
                    Err(_)                     => return 1,
                }
            }
    }

    fn eval_cond(&mut self, cond: &str) -> bool {
        let cond_s = if !self.kind.supports_double_bracket() {
            adapt_cond_posix(cond)
        } else {
            cond.to_string()
        };

        let script = format!(
            "if {}; then echo {}:0; else echo {}:1; fi",
            cond_s, SENTINEL_COND, SENTINEL_COND
        );

        if writeln!(self.stdin, "{}", script).is_err() || self.stdin.flush().is_err() {
            return false;
        }

        let deadline = Instant::now() + Duration::from_millis(COND_TIMEOUT_MS);
        loop {
            let rem = deadline.saturating_duration_since(Instant::now());
            if rem.is_zero() {
                eprintln!("{} eval_cond timeout (5s): {}", "[!]".yellow(), cond);
                return false;
            }
            match self.rx.recv_timeout(rem) {
                Ok(ShellLine::CondDone(r)) => return r,
                Ok(ShellLine::Out(l))      => println!("{}", l),
                Ok(ShellLine::Eof)         => return false,
                Ok(ShellLine::ExecDone(_)) => {} // spurious
                Err(_)                     => return false,
            }
        }
    }

    fn set_env(&mut self, key: &str, val: &str) {
        let esc = shell_escape(val);
        let _ = writeln!(self.stdin, "export {}={}", key, esc);
        let _ = writeln!(self.stdin, "echo {}:0", SENTINEL_EXEC);
        let _ = self.stdin.flush();
        // Synchronizuj — czekaj na sentinel żeby nie wyprzedzić
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let rem = deadline.saturating_duration_since(Instant::now());
            if rem.is_zero() { break; }
            match self.rx.recv_timeout(rem) {
                Ok(ShellLine::ExecDone(_)) => break,
                Ok(ShellLine::Out(_))      => {}
                _                          => break,
            }
        }
    }
}

impl Drop for ShellSession {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "exit 0");
        let _ = self.stdin.flush();
        let _ = self.child.wait();
    }
}

// ─────────────────────────────────────────────────────────────
// SessionManager — publiczne API dla VM i JIT
// ─────────────────────────────────────────────────────────────
pub struct SessionManager {
    normal:     Option<ShellSession>,
    sudo_sess:  Option<ShellSession>,
    cond_cache: CondCache,
    kind:       ShellKind,
    verbose:    bool,
    pub metrics: Arc<ExecMetrics>,
}

impl SessionManager {
    /// Utwórz z domyślnym shell (Bash)
    pub fn new(verbose: bool) -> Self {
        Self::with_shell(ShellKind::default(), verbose)
    }

    /// Utwórz z wybranym shell
    pub fn with_shell(kind: ShellKind, verbose: bool) -> Self {
        let normal = ShellSession::spawn(kind, false, verbose);
        if normal.is_none() {
            eprintln!("{} WARN: {} session failed — using fallback per-command spawn",
                      "[!]".yellow(), kind.binary());
        }
        Self {
            normal,
            sudo_sess:  None,
            cond_cache: CondCache::new(),
            kind,
            verbose,
            metrics: Arc::new(ExecMetrics::default()),
        }
    }

    // ── Exec ──────────────────────────────────────────────────

    pub fn exec(&mut self, cmd: &str, sudo: bool) -> i32 {
        let t = Instant::now();
        self.metrics.exec_count.fetch_add(1, Ordering::Relaxed);
        let r = if sudo { self.exec_sudo(cmd) } else { self.exec_normal(cmd) };
        self.metrics.ipc_total_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        r
    }

    fn exec_normal(&mut self, cmd: &str) -> i32 {
        if let Some(ref mut s) = self.normal {
            if s.is_alive() { return s.exec(cmd); }
        }
        self.restart_normal();
        self.normal.as_mut()
        .map(|s| s.exec(cmd))
        .unwrap_or_else(|| fallback(cmd, false, self.kind))
    }

    fn exec_sudo(&mut self, cmd: &str) -> i32 {
        if self.sudo_sess.is_none() {
            self.sudo_sess = ShellSession::spawn(self.kind, true, self.verbose);
        }
        if let Some(ref mut s) = self.sudo_sess {
            if s.is_alive() { return s.exec(cmd); }
        }
        self.sudo_sess = ShellSession::spawn(self.kind, true, self.verbose);
        self.sudo_sess.as_mut()
        .map(|s| s.exec(cmd))
        .unwrap_or_else(|| fallback(cmd, true, self.kind))
    }

    fn restart_normal(&mut self) {
        self.metrics.restarts.fetch_add(1, Ordering::Relaxed);
        if self.verbose { eprintln!("{} Restarting shell session...", "[!]".yellow()); }
        self.normal = ShellSession::spawn(self.kind, false, self.verbose);
    }

    // ── eval_cond z cache ─────────────────────────────────────

    /// cond: już po substitute() — zawiera konkretne wartości zmiennych
    pub fn eval_cond(&mut self, cond: &str) -> bool {
        self.metrics.cond_count.fetch_add(1, Ordering::Relaxed);

        if let Some(cached) = self.cond_cache.get(cond) {
            self.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
            return cached;
        }

        let t = Instant::now();
        let result = if let Some(ref mut s) = self.normal {
            if s.is_alive() {
                s.eval_cond(cond)
            } else {
                self.restart_normal();
                self.normal.as_mut()
                .map(|s| s.eval_cond(cond))
                .unwrap_or_else(|| fallback_cond(cond, self.kind))
            }
        } else {
            fallback_cond(cond, self.kind)
        };

        self.metrics.ipc_total_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.cond_cache.set(cond, result);
        result
    }

    // ── env ───────────────────────────────────────────────────

    pub fn set_env(&mut self, key: &str, val: &str) {
        // Zmiana zmiennej env — wyniki eval_cond mogą być nieaktualne
        self.cond_cache.clear();
        if let Some(ref mut s) = self.normal   { s.set_env(key, val); }
        if let Some(ref mut s) = self.sudo_sess { s.set_env(key, val); }
    }

    /// Wywołaj po każdym SetLocal w VM — invaliduje cond cache
    pub fn invalidate_cond_cache(&mut self) {
        self.cond_cache.clear();
    }

    // ── util ─────────────────────────────────────────────────

    pub fn shell_kind(&self) -> ShellKind { self.kind }

    pub fn switch_shell(&mut self, kind: ShellKind) {
        if self.kind == kind { return; }
        self.kind      = kind;
        self.normal    = ShellSession::spawn(kind, false, self.verbose);
        self.sudo_sess = None;
        self.cond_cache.clear();
    }
}

// ─────────────────────────────────────────────────────────────
// Fallback — gdy sesja nie może być uruchomiona
// ─────────────────────────────────────────────────────────────
fn fallback(cmd: &str, sudo: bool, kind: ShellKind) -> i32 {
    let mut c = if sudo {
        let mut c = std::process::Command::new("sudo");
        c.arg(kind.binary());
        c
    } else {
        std::process::Command::new(kind.binary())
    };
    match c.arg("-c").arg(cmd).status() {
        Ok(s)  => s.code().unwrap_or(1),
        Err(e) => { eprintln!("{} fallback exec: {}", "[x]".red(), e); 1 }
    }
}

fn fallback_cond(cond: &str, kind: ShellKind) -> bool {
    let s = format!("if {}; then exit 0; else exit 1; fi", cond);
    matches!(
        std::process::Command::new(kind.binary()).arg("-c").arg(&s).status(),
             Ok(st) if st.code().unwrap_or(1) == 0
    )
}

// ─────────────────────────────────────────────────────────────
// POSIX adapters (dla dash — brak [[]])
// ─────────────────────────────────────────────────────────────
fn adapt_posix(cmd: &str) -> String {
    cmd.replace("[[", "[").replace("]]", "]")
}

fn adapt_cond_posix(cond: &str) -> String {
    let t = cond.trim();
    if t.starts_with("[[") && t.ends_with("]]") {
        let inner = t[2..t.len()-2].trim();
        let fixed = inner.replace(" == ", " = ");
        format!("[ {} ]", fixed)
    } else {
        t.to_string()
    }
}

// ─────────────────────────────────────────────────────────────
// Shell escape
// ─────────────────────────────────────────────────────────────
pub fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

// ─────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_true_false() {
        let mut sm = SessionManager::new(false);
        assert_eq!(sm.exec("true", false), 0);
        assert_eq!(sm.exec("false", false), 1);
    }

    #[test]
    fn cond_cache_works() {
        let mut sm = SessionManager::new(false);
        assert!(sm.eval_cond("[[ 1 == 1 ]]"));
        assert!(sm.eval_cond("[[ 1 == 1 ]]")); // cache hit
        assert_eq!(sm.metrics.cache_hits.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn large_output_no_deadlock() {
        // 100k linii — bash_session.rs deadlockowałby przy ~1000
        let mut sm = SessionManager::new(false);
        let code = sm.exec("seq 1 100000", false);
        assert_eq!(code, 0);
    }

    #[test]
    fn set_env_invalidates_cache() {
        let mut sm = SessionManager::new(false);
        sm.exec("X=foo", false);
        assert!(sm.eval_cond("[[ $X == foo ]]"));
        sm.set_env("X", "bar");
        assert!(sm.eval_cond("[[ $X == bar ]]"));
    }
}
