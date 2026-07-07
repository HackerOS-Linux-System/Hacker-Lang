use anyhow::{bail, Result};
use std::process::{Command, Stdio};
use hl_parser::ast::{Node, ExternRuntime};
use crate::env::{Env, Value};
use crate::executor::exec_nodes;
use crate::config::load_config;

/// Wykonaj blok extern
pub fn exec_extern_def(
    file: &str,
    runtime: &ExternRuntime,
    body:    &[Node],
    env:     &mut Env,
) -> Result<crate::executor::ExecResult> {
    // 1. Ewaluuj ciało bloku żeby zebrać argumenty i zmienne env
    //    Argumenty: _arg_0, _arg_1, ... zdefiniowane w bloku
    //    Env vars: _env_KEY=VALUE
    exec_nodes(body, env)?;

    // 2. Zbierz argumenty pozycyjne z env
    let mut args: Vec<String> = Vec::new();
    let mut i = 0usize;
    loop {
        let key = format!("_arg_{}", i);
        let val = env.get_var(&key).to_string_val();
        if val.is_empty() { break; }
        args.push(val);
        i += 1;
    }

    // 3. Zbierz zmienne env (_env_KEY)
    let mut extra_env: Vec<(String, String)> = Vec::new();
    for (k, v) in &env.vars {
        if let Some(env_key) = k.strip_prefix("_env_") {
            extra_env.push((env_key.to_string(), v.to_string_val()));
        }
    }

    // 4. Rozwiąż ścieżkę pliku
    let resolved_path = resolve_extern_file(file, runtime, env);

    // 5. Uruchom odpowiedni runtime
    let result = match runtime {
        ExternRuntime::Shell  => run_shell(&resolved_path, &args, &extra_env),
        ExternRuntime::Python => run_python(&resolved_path, &args, &extra_env),
        ExternRuntime::Java   => run_java(&resolved_path, &args, &extra_env),
        ExternRuntime::Elf    => run_elf(&resolved_path, &args, &extra_env),
        ExternRuntime::So     => run_so(&resolved_path, &args, &extra_env),
    }?;

    // 6. Zapisz wynik do env
    env.set_var("_extern_result",   Value::String(result.stdout.clone().unwrap_or_default().trim().to_string()));
    env.set_var("_extern_exit",     Value::Number(result.exit_code as f64));
    env.last_exit = result.exit_code;

    Ok(result)
}

// ── Rozwiązywanie ścieżki ────────────────────────────────────────────────────

fn resolve_extern_file(file: &str, runtime: &ExternRuntime, env: &mut Env) -> String {
    let expanded = env.interpolate(file);

    // Absolutna ścieżka — użyj bezpośrednio
    if expanded.starts_with('/') { return expanded; }

    // Ścieżka względna — sprawdź czy istnieje
    if expanded.contains('/') {
        return expanded;
    }

    // Tylko nazwa — szukaj w kolejności:
    // 1. Bieżący katalog
    let cwd_path = std::path::Path::new(&expanded);
    if cwd_path.exists() { return expanded; }

    // 2. bit libs aktywnego środowiska lub globalnego
    let cfg = load_config();
    let libs_dir = cfg.effective_libs_dir();
    let in_libs = libs_dir.join(&expanded);
    if in_libs.exists() { return in_libs.display().to_string(); }

    // 3. Dla ELF — szukaj w PATH (exec sam to zrobi)
    if matches!(runtime, ExternRuntime::Elf) {
        if let Ok(path) = which::which(&expanded) {
            return path.display().to_string();
        }
    }

    // 4. Dla .so — /usr/lib, /usr/local/lib
    if matches!(runtime, ExternRuntime::So) {
        for dir in &["/usr/lib", "/usr/local/lib", "/lib"] {
            let p = std::path::Path::new(dir).join(&expanded);
            if p.exists() { return p.display().to_string(); }
        }
    }

    // Fallback — zwróć jak jest i niech runtime sam obsłuży błąd
    expanded
}

// ── Shell runtime ─────────────────────────────────────────────────────────────

fn run_shell(
    file:      &str,
    args:      &[String],
    extra_env: &[(String, String)],
) -> Result<crate::executor::ExecResult> {
    let cfg  = load_config();
    let shell = cfg.shell_cmd().to_string();

    // Sprawdź czy plik istnieje
    if !file.is_empty() && !std::path::Path::new(file).exists() {
        // Może to inline script? Sprawdź czy nie ma '/' — wtedy traktuj jako komendę
        if file.contains('/') {
            bail!("[extern shell] Skrypt nie istnieje: '{}'", file);
        }
        // Komenda w PATH — uruchom bezpośrednio
        let mut cmd = build_command_with_env(file, args, extra_env);
        let status = cmd.status()?;
        return Ok(crate::executor::ExecResult {
            exit_code: status.code().unwrap_or(1),
            stdout: None,
        });
    }

    let mut cmd = Command::new(&shell);
    if !file.is_empty() { cmd.arg(file); }
    cmd.args(args);
    apply_env(&mut cmd, extra_env);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status()?;
    Ok(crate::executor::ExecResult {
        exit_code: status.code().unwrap_or(1),
        stdout: None,
    })
}

// ── Python runtime ────────────────────────────────────────────────────────────

fn run_python(
    file:      &str,
    args:      &[String],
    extra_env: &[(String, String)],
) -> Result<crate::executor::ExecResult> {
    let cfg = load_config();
    let python = cfg.python_cmd().to_string();

    // Sprawdź czy python3 istnieje
    if which::which(&python).is_err() {
        bail!(
            "[extern python] '{}' nie znaleziony.\n\
             Zainstaluj: sudo apt install python3\n\
             Lub ustaw w config.hk: [extern] python = /path/to/python3",
            python
        );
    }

    let mut cmd = Command::new(&python);
    if !file.is_empty() { cmd.arg(file); }
    cmd.args(args);
    apply_env(&mut cmd, extra_env);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status()?;
    Ok(crate::executor::ExecResult {
        exit_code: status.code().unwrap_or(1),
        stdout: None,
    })
}

// ── Java runtime ──────────────────────────────────────────────────────────────

fn run_java(
    file:      &str,
    args:      &[String],
    extra_env: &[(String, String)],
) -> Result<crate::executor::ExecResult> {
    let cfg  = load_config();
    let java = cfg.java_cmd().to_string();

    if which::which(&java).is_err() {
        bail!(
            "[extern java] '{}' nie znaleziony.\n\
             Zainstaluj: sudo apt install default-jre\n\
             Lub ustaw w config.hk: [extern] java = /path/to/java",
            java
        );
    }

    let mut cmd = Command::new(&java);

    if file.ends_with(".jar") {
        // JAR: java -jar plik.jar args
        cmd.arg("-jar").arg(file);
    } else if file.ends_with(".class") {
        // .class: java NazwaKlasy (bez .class)
        let class_name = file.trim_end_matches(".class");
        cmd.arg(class_name);
    } else if !file.is_empty() {
        // Traktuj jako klasę lub moduł
        cmd.arg(file);
    }

    cmd.args(args);
    apply_env(&mut cmd, extra_env);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status()?;
    Ok(crate::executor::ExecResult {
        exit_code: status.code().unwrap_or(1),
        stdout: None,
    })
}

// ── ELF runtime ───────────────────────────────────────────────────────────────

fn run_elf(
    file:      &str,
    args:      &[String],
    extra_env: &[(String, String)],
) -> Result<crate::executor::ExecResult> {
    if file.is_empty() {
        bail!("[extern elf] Brak nazwy binarki");
    }

    let mut cmd = build_command_with_env(file, args, extra_env);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    match cmd.status() {
        Ok(status) => Ok(crate::executor::ExecResult {
            exit_code: status.code().unwrap_or(1),
            stdout: None,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!(
                "[extern elf] Binarka '{}' nie znaleziona w PATH.\n\
                 Sprawdź ścieżkę lub dodaj do PATH.",
                file
            );
        }
        Err(e) => bail!("[extern elf] Błąd uruchamiania '{}': {}", file, e),
    }
}

// ── .so runtime ───────────────────────────────────────────────────────────────
//
// Szuka symbolu "hl_extern_call" w bibliotece .so
// Sygnatura: extern "C" fn hl_extern_call(argc: i32, argv: *const *const i8) -> i32

fn run_so(
    file:      &str,
    args:      &[String],
    _extra_env: &[(String, String)],
) -> Result<crate::executor::ExecResult> {
    if file.is_empty() {
        bail!("[extern so] Brak nazwy biblioteki .so");
    }

    if !std::path::Path::new(file).exists() {
        bail!(
            "[extern so] Biblioteka .so nie znaleziona: '{}'\n\
             Sprawdź ścieżkę lub zainstaluj przez bit.",
            file
        );
    }

    // Załaduj .so przez dlopen (libloading)
    // SAFETY: ładujemy symbol zgodnie z deklarowaną sygnaturą
    #[cfg(target_os = "linux")]
    {
        use std::ffi::{CString, CStr};
        use std::os::raw::{c_int, c_char};

        extern "C" {
            fn dlopen(filename: *const c_char, flag: c_int) -> *mut std::ffi::c_void;
            fn dlsym(handle: *mut std::ffi::c_void, symbol: *const c_char) -> *mut std::ffi::c_void;
            fn dlclose(handle: *mut std::ffi::c_void) -> c_int;
            fn dlerror() -> *const c_char;
        }

        const RTLD_NOW: c_int   = 0x00002;
        const RTLD_LOCAL: c_int = 0x00000;

        let c_file = CString::new(file).unwrap();
        let handle = unsafe { dlopen(c_file.as_ptr(), RTLD_NOW | RTLD_LOCAL) };

        if handle.is_null() {
            let err = unsafe {
                let e = dlerror();
                if e.is_null() { "nieznany błąd".to_string() }
                else { CStr::from_ptr(e).to_string_lossy().to_string() }
            };
            bail!("[extern so] dlopen('{}') failed: {}", file, err);
        }

        let sym_name = CString::new("hl_extern_call").unwrap();
        let sym_ptr  = unsafe { dlsym(handle, sym_name.as_ptr()) };

        if sym_ptr.is_null() {
            unsafe { dlclose(handle); }
            bail!(
                "[extern so] Symbol 'hl_extern_call' nie znaleziony w '{}'.\n\
                 Biblioteka musi eksportować:\n\
                 extern \"C\" fn hl_extern_call(argc: i32, argv: *const *const i8) -> i32",
                file
            );
        }

        // Przygotuj argv
        type HlExternCallFn = unsafe extern "C" fn(c_int, *const *const c_char) -> c_int;
        let func: HlExternCallFn = unsafe { std::mem::transmute(sym_ptr) };

        let c_args: Vec<CString> = args.iter()
            .filter_map(|a| CString::new(a.as_str()).ok())
            .collect();
        let c_ptrs: Vec<*const c_char> = c_args.iter().map(|s| s.as_ptr()).collect();

        let exit_code = unsafe {
            func(c_ptrs.len() as c_int, c_ptrs.as_ptr())
        };

        unsafe { dlclose(handle); }

        return Ok(crate::executor::ExecResult {
            exit_code: exit_code as i32,
            stdout: None,
        });
    }

    #[cfg(not(target_os = "linux"))]
    bail!("[extern so] Obsługiwane tylko na Linux");
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_command_with_env(
    prog:      &str,
    args:      &[String],
    extra_env: &[(String, String)],
) -> Command {
    let mut cmd = Command::new(prog);
    cmd.args(args);
    apply_env(&mut cmd, extra_env);
    cmd
}

fn apply_env(cmd: &mut Command, extra_env: &[(String, String)]) {
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
}
