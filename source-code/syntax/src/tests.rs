use crate::*;
use std::sync::Mutex;

/// Testy ruszające zmienną środowiskową HL_SYNTAX_PATH nie mogą działać równolegle.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn msgs(p: &Prepared, level: Level) -> Vec<String> {
    p.diags.iter().filter(|d| d.level == level).map(|d| d.message.clone()).collect()
}

// ── Version ─────────────────────────────────────────────────────────────────

#[test]
fn version_parsing_and_ordering() {
    assert_eq!(Version::parse_hackeros("V5.5").unwrap(), Version::new(5, 5));
    assert_eq!(Version::parse_hackeros("v4.1").unwrap(), Version::new(4, 1));
    assert_eq!(Version::parse_hackeros("5").unwrap(), Version::new(5, 0));
    assert!(Version::parse_hackeros("V4.7").unwrap() < Version::parse_hackeros("V5.1").unwrap());
    assert!(Version::parse_hackeros("V5.10").unwrap() > Version::parse_hackeros("V5.9").unwrap());
    assert!(Version::parse_hackeros("Vx").is_err());
    assert!(Version::parse_hackeros("V5..1").is_err());
    assert!(Version::parse_hackeros("V").is_err());
    assert!(Version::parse_hackeros("V1.2.3.4").is_err());
    assert_eq!(Version::parse_hackeros("V5").unwrap().to_string(), "V5");
    assert_eq!(Version::parse_hackeros("V4.7").unwrap().to_string(), "V4.7");
}

#[test]
fn tool_version_display() {
    assert_eq!(ToolVersion::parse("1").unwrap().to_string(), "1");
    assert_eq!(ToolVersion::parse("1.1").unwrap().to_string(), "1.1");
    assert!(ToolVersion::parse("1.1") > ToolVersion::parse("1"));
}

#[test]
fn gen_mapping() {
    assert_eq!(gen_for_version(Version::new(4, 7)), 1);
    assert_eq!(gen_for_version(Version::new(4, 1)), 1);
    assert_eq!(gen_for_version(Version::new(5, 1)), 2);
    assert_eq!(version_for_legacy_gen(1), Version::new(4, 7));
    assert_eq!(version_for_legacy_gen(2), Version::new(5, 1));
}

// ── using ───────────────────────────────────────────────────────────────────

#[test]
fn using_latest_maps_to_gen2_same_line() {
    let src = "#!/usr/bin/env hl\nusing <HackerOS V5.1>\n~> hej\n";
    let p = prepare(src);
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.source, "#!/usr/bin/env hl\nusing <gen 2>\n~> hej\n");
    assert_eq!(p.info.effective_gen, 2);
    assert_eq!(p.info.effective, Version::new(5, 1));
    assert!(p.rewritten);
}

#[test]
fn using_v4_7_is_gen1() {
    let p = prepare("using <HackerOS V4.7>\n~> a\n");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.source, "using <gen 1>\n~> a\n");
    assert_eq!(p.info.effective_gen, 1);
}

#[test]
fn using_future_version_is_clamped_with_warning() {
    let p = prepare("using <HackerOS V5.5>\n~> a\n");
    assert!(!p.has_errors());
    assert_eq!(p.source, "using <gen 2>\n~> a\n");
    assert_eq!(p.info.effective, LATEST_SYNTAX);
    assert_eq!(p.info.declared, Declared::HackerOs(Version::new(5, 5)));
    assert_eq!(msgs(&p, Level::Warning).len(), 1);
}

#[test]
fn using_too_old_is_error_and_passes_through() {
    let src = "using <HackerOS V3.0>\n~> a\n";
    let p = prepare(src);
    assert!(p.has_errors());
    assert_eq!(p.source, src, "błędna dyrektywa ma dojść do parsera bez zmian");
}

#[test]
fn using_is_case_insensitive_and_v_optional() {
    for s in ["using <hackeros v5.1>", "using <HackerOS 5.1>", "using   <  HackerOS   V5.1  >", "using <HackerOS V5.1> ;; komentarz"] {
        let p = prepare(&format!("{}\n~> x\n", s));
        assert!(p.diags.is_empty(), "{} → {:?}", s, p.diags);
        assert_eq!(p.info.effective, Version::new(5, 1), "{}", s);
    }
}

#[test]
fn using_garbage_is_error() {
    for s in ["using <HackerOS>", "using <HackerOS Vx>", "using <foo>", "using HackerOS V5.1", "using <>"] {
        let p = prepare(&format!("{}\n", s));
        assert!(p.has_errors(), "{} powinno być błędem", s);
        assert_eq!(p.source, format!("{}\n", s));
    }
}

#[test]
fn legacy_using_untouched() {
    let src = "using <gen 2>\n~> a\n";
    let p = prepare(src);
    assert!(p.diags.is_empty());
    assert_eq!(p.source, src);
    assert!(!p.rewritten);
    assert_eq!(p.info.declared, Declared::Legacy(2));
    assert_eq!(p.info.effective, Version::new(5, 1));

    let p1 = prepare("using <gen 1>\n");
    assert_eq!(p1.info.effective, Version::new(4, 7));
    assert_eq!(p1.info.effective_gen, 1);
}

#[test]
fn legacy_garbage_left_to_parser_without_our_error() {
    let src = "using <gen x>\n";
    let p = prepare(src);
    assert!(p.diags.is_empty());
    assert_eq!(p.source, src);
}

#[test]
fn no_declaration_means_default() {
    let src = "~> a\n";
    let p = prepare(src);
    assert_eq!(p.source, src);
    assert_eq!(p.info, SyntaxInfo::default());
    assert!(!p.rewritten);
}

#[test]
fn duplicate_new_style_using_is_error() {
    let p = prepare("using <HackerOS V5.1>\nusing <HackerOS V4.7>\n");
    assert_eq!(msgs(&p, Level::Error).len(), 1);
    let p = prepare("using <gen 2>\nusing <HackerOS V5.1>\n");
    assert_eq!(msgs(&p, Level::Error).len(), 1);
}

#[test]
fn legacy_using_repeated_is_never_an_error() {
    // tests/compiler_pipeline.hl ma drugi `using <gen 2>` w heredoc po kodzie
    let src = "using <gen 2>\n~> a\n>> cat <<X\nusing <gen 2>\nX\n";
    let p = prepare(src);
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.source, src);
    let p = prepare("using <HackerOS V5.1>\nusing <gen 2>\n");
    assert!(!p.has_errors());
}

#[test]
fn rolling_is_passed_through() {
    for s in ["using <ROLLING>", "using <rolling>", "using ROLLING"] {
        let src = format!("{}\n~> a\n", s);
        let p = prepare(&src);
        assert!(p.diags.is_empty(), "{}: {:?}", s, p.diags);
        assert_eq!(p.source, src);
        assert_eq!(p.info.declared, Declared::Rolling);
        assert_eq!(p.info.effective_gen, MAX_GEN);
    }
}

#[test]
fn legacy_without_brackets_still_accepted() {
    let p = prepare("using gen 2\n");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.info.declared, Declared::Legacy(2));
}

#[test]
fn using_beyond_parser_window_warns() {
    let src = format!("{}using <HackerOS V4.7>\n", ";; x\n".repeat(10));
    let p = prepare(&src);
    assert_eq!(msgs(&p, Level::Warning).len(), 1);
}

#[test]
fn using_after_code_is_ignored_with_warning() {
    let p = prepare("~> a\nusing <HackerOS V4.7>\n");
    assert_eq!(p.source, "~> a\n\n");
    assert_eq!(p.info.declared, Declared::Default);
    assert_eq!(msgs(&p, Level::Warning).len(), 1);
}

// ── gen ─────────────────────────────────────────────────────────────────────

#[test]
fn gen_is_just_a_version_marker() {
    for (s, shown) in [("gen <1>", "1"), ("gen <1.1>", "1.1"), ("gen <1.0.5>", "1.0.5")] {
        let src = format!("{}\n~> a\n", s);
        let p = prepare(&src);
        assert!(p.diags.is_empty(), "{} → {:?}", s, p.diags);
        assert_eq!(p.source, "\n~> a\n");
        assert_eq!(p.info.tool.unwrap().to_string(), shown);
        assert_eq!(p.info.effective_gen, DEFAULT_GEN, "gen <..> nie wybiera składni");
    }
}

#[test]
fn gen_newer_than_tool_warns_only() {
    let p = prepare("gen <3>\n~> a\n");
    assert!(!p.has_errors());
    assert_eq!(msgs(&p, Level::Warning).len(), 1);
    // Obecna wersja narzędzia to 1.1: równa i starsza — bez ostrzeżenia, nowsza — ostrzeżenie.
    assert!(prepare("gen <1.1>\n").diags.is_empty());
    assert!(prepare("gen <1>\n").diags.is_empty());
    assert_eq!(msgs(&prepare("gen <1.2>\n"), Level::Warning).len(), 1);
    assert_eq!(msgs(&prepare("gen <2>\n"), Level::Warning).len(), 1);
}

#[test]
fn gen_garbage_and_duplicate() {
    assert!(prepare("gen <abc>\n").has_errors());
    assert!(prepare("gen 1\n").has_errors());
    assert!(prepare("gen <1>\ngen <2>\n").has_errors());
}

#[test]
fn gen_works_together_with_using() {
    let p = prepare("gen <1.1>\nusing <HackerOS V4.7>\n~> a\n");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.source, "\nusing <gen 1>\n~> a\n");
    assert_eq!(p.info.tool.unwrap().to_string(), "1.1");
    assert_eq!(p.info.effective_gen, 1);
}

// ── take ────────────────────────────────────────────────────────────────────

#[test]
fn take_from_older_version_from_user_example() {
    let p = prepare("using <HackerOS V5.5>\ntake <HackerOS V4.1/corountimes>\n~> a\n");
    assert!(!p.has_errors(), "{:?}", p.diags);
    assert_eq!(p.source, "using <gen 2>\n\n~> a\n");
    assert_eq!(p.info.takes.len(), 1);
    assert_eq!(p.info.takes[0].feature, "goroutines", "alias `corountimes` → nazwa kanoniczna");
    assert_eq!(p.info.takes[0].requested, "corountimes");
    assert_eq!(p.info.takes_label(), "V4.1/goroutines");
}

#[test]
fn take_gen2_feature_lifts_gen1_file() {
    let p = prepare("using <HackerOS V4.7>\ntake <HackerOS V5.1/arena>\n~> a\n");
    assert!(!p.has_errors(), "{:?}", p.diags);
    assert_eq!(p.info.base_gen, 1);
    assert_eq!(p.info.effective_gen, 2);
    assert_eq!(p.source, "using <gen 2>\n\n~> a\n");
    assert_eq!(msgs(&p, Level::Hint).len(), 1);
}

#[test]
fn take_lifts_legacy_using_line_too() {
    let p = prepare("using <gen 1>\ntake <HackerOS V5.1/switch>\n");
    assert_eq!(p.info.effective_gen, 2);
    assert_eq!(p.source, "using <gen 2>\n\n");
    assert!(p.rewritten);
}

#[test]
fn take_before_using_still_counts() {
    let p = prepare("take <HackerOS V5.1/arena>\nusing <HackerOS V4.7>\n");
    assert_eq!(p.info.effective_gen, 2);
    assert_eq!(p.source, "\nusing <gen 2>\n");
}

#[test]
fn take_old_feature_does_not_change_gen() {
    let p = prepare("using <HackerOS V4.7>\ntake <HackerOS V4.1/goroutines>\n");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.info.effective_gen, 1);
    assert_eq!(p.source, "using <gen 1>\n\n");
}

#[test]
fn take_feature_not_yet_existing_in_that_version() {
    let src = "take <HackerOS V4.1/arena>\n";
    let p = prepare(src);
    assert!(p.has_errors());
    assert_eq!(p.source, src);
    assert!(p.diags[0].message.contains("nie istnieje w V4.1"), "{}", p.diags[0].message);
}

#[test]
fn take_unknown_feature_suggests() {
    let p = prepare("take <HackerOS V4.1/goroutine>\n");
    assert!(p.has_errors());
    assert!(p.diags[0].suggestion.as_deref().unwrap().contains("goroutines"));
}

#[test]
fn take_bad_forms() {
    for s in [
        "take <HackerOS V4.1>",
        "take <HackerOS V4.1/>",
        "take <HackerOS V4.1/a/b>",
        "take HackerOS V4.1/goroutines",
        "take <V4.1/goroutines>",
        "take <HackerOS V9.0/goroutines>",
        "take <HackerOS V3.0/goroutines>",
        "take <not-default/x>",
    ] {
        let p = prepare(&format!("{}\n", s));
        assert!(p.has_errors(), "{} powinno być błędem", s);
        assert_eq!(p.source, format!("{}\n", s));
    }
}

#[test]
fn take_duplicate_warns() {
    let p = prepare("take <HackerOS V4.1/goroutines>\ntake <HackerOS V4.1/coroutines>\n");
    assert_eq!(p.info.takes.len(), 1);
    assert_eq!(msgs(&p, Level::Warning).len(), 1);
}

// ── not-default ─────────────────────────────────────────────────────────────

fn temp_profile_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("hl-syntax-test-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn not_default_unknown_profile_is_error() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Wskazujemy HL_SYNTAX_PATH na pusty katalog, żeby test nie zależał od systemu.
    let d = temp_profile_dir("empty");
    std::env::set_var("HL_SYNTAX_PATH", &d);
    let src = "using <not-default/nie-ma-takiego>\n";
    let p = prepare(src);
    assert!(p.has_errors());
    assert_eq!(p.source, src);
    std::env::remove_var("HL_SYNTAX_PATH");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn not_default_profile_from_file() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let d = temp_profile_dir("prof");
    std::fs::write(
        d.join("moj.syntax"),
        "base = V4.7\nprovides = foo, bar ;; lista\ndescription = test\n",
    )
    .unwrap();
    std::env::set_var("HL_SYNTAX_PATH", &d);
    let p = prepare("using <not-default/moj>\n~> a\n");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.source, "using <gen 1>\n~> a\n");
    assert_eq!(p.info.effective_gen, 1);
    assert_eq!(p.info.short_label(), "not-default/moj");
    assert_eq!(list_profiles().iter().filter(|x| x.name == "moj").count(), 1);
    std::env::remove_var("HL_SYNTAX_PATH");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn not_default_path_traversal_rejected() {
    for s in ["using <not-default/../etc/passwd>", "using <not-default/a/b>", "using <not-default/>", "using <not-default/.hidden>"] {
        let p = prepare(&format!("{}\n", s));
        assert!(p.has_errors(), "{}", s);
    }
}

#[test]
fn profile_file_validation() {
    use std::path::Path;
    let bad = |t: &str| profiles::parse_profile("x", t, Path::new("x.syntax")).is_err();
    assert!(bad(""), "brak base");
    assert!(bad("base = V3.0"));
    assert!(bad("base = V5.1\nfoo = bar"));
    assert!(bad("base V5.1"));
    assert!(!bad("base = V5.1"));
}

// ── ogólne ──────────────────────────────────────────────────────────────────

#[test]
fn line_numbers_are_preserved() {
    let src = "#!/usr/bin/env hl\n;; komentarz\ngen <1>\nusing <HackerOS V5.1>\ntake <HackerOS V4.1/goroutines>\n\n~> hej\n";
    let p = prepare(src);
    assert_eq!(src.lines().count(), p.source.lines().count());
    assert_eq!(p.source.lines().nth(6), Some("~> hej"));
}

#[test]
fn crlf_is_preserved() {
    let p = prepare("using <HackerOS V5.1>\r\n~> a\r\n");
    assert_eq!(p.source, "using <gen 2>\r\n~> a\r\n");
}

#[test]
fn no_trailing_newline() {
    let p = prepare("using <HackerOS V5.1>");
    assert_eq!(p.source, "using <gen 2>");
}

#[test]
fn comments_and_blank_lines_do_not_end_header() {
    let p = prepare("#!/usr/bin/env hl\n;; a\n/// b\n// c\n\nusing <HackerOS V4.7>\n");
    assert!(p.diags.is_empty(), "{:?}", p.diags);
    assert_eq!(p.info.effective_gen, 1);
}

#[test]
fn directive_diag_position() {
    let p = prepare("\n  gen <abc>\n");
    let d = &p.diags[0];
    assert_eq!((d.line, d.col), (2, 3));
}

#[test]
fn ordinary_code_is_never_touched() {
    let src = "% x = 1\n> echo gen <1>\n~> using <HackerOS V5.1>\n:: f <4k> def\n    ~> take <HackerOS V4.1/x>\ndone\n";
    let p = prepare(src);
    assert_eq!(p.source, src);
    assert!(p.diags.is_empty());
    assert!(!p.rewritten);
}

#[test]
fn into_result_and_display() {
    let p = prepare("using <HackerOS V3.0>\n");
    let e = p.into_result().unwrap_err();
    assert!(e.to_string().contains("linia 1"));
    assert!(prepare("using <HackerOS V5.1>\n").into_result().is_ok());
}

#[test]
fn env_pairs() {
    let p = prepare("gen <1.1>\nusing <HackerOS V4.7>\ntake <HackerOS V4.1/goroutines>\n");
    let m: std::collections::HashMap<_, _> = p.info.env_pairs().into_iter().collect();
    assert_eq!(m["HL_SYNTAX"], "HackerOS V4.7");
    assert_eq!(m["HL_SYNTAX_GEN"], "1");
    assert_eq!(m["HL_TOOL_GEN"], "1.1");
    assert_eq!(m["HL_TAKE"], "V4.1/goroutines");
}

#[test]
fn registry_is_consistent() {
    for f in FEATURES {
        assert!(f.gen >= 1 && f.gen <= MAX_GEN, "{}", f.name);
        assert_eq!(gen_for_version(f.since), f.gen, "{}: gen niezgodny z wersją `since`", f.name);
        assert!(find_feature(f.name).is_some());
        for a in f.aliases {
            assert_eq!(find_feature(a).unwrap().name, f.name, "alias {}", a);
        }
    }
    let mut names: Vec<_> = FEATURES.iter().flat_map(|f| std::iter::once(f.name).chain(f.aliases.iter().copied())).collect();
    let n = names.len();
    names.sort();
    names.dedup();
    assert_eq!(n, names.len(), "nazwy/aliasy muszą być unikalne");
    assert_eq!(gen_for_version(LATEST_SYNTAX), MAX_GEN);
    assert_eq!(gen_for_version(GEN1_FINAL), 1);
}

#[test]
fn take_gating_allows_only_taken_features() {
    let p = prepare("using <HackerOS V4.7>\ntake <HackerOS V5.1/arena>\n");
    assert_eq!(p.info.effective_gen, 2);
    assert!(p.info.check_gen2_usage(&["arena"]).is_ok());
    let e = p.info.check_gen2_usage(&["arena", "pipe-var"]).unwrap_err();
    assert!(e.contains("pipe-var") && e.contains("take <HackerOS V5.0/pipe-var>"), "{}", e);
}

#[test]
fn take_gating_is_off_without_lift() {
    // gen 2 z `using` — wszystko wolno; gen 1 bez take — zostaje po stronie parsera.
    assert!(prepare("using <HackerOS V5.1>\n").info.check_gen2_usage(&["arena"]).is_ok());
    assert!(prepare("using <HackerOS V4.7>\n").info.check_gen2_usage(&["arena"]).is_ok());
    assert!(prepare("using <HackerOS V4.7>\ntake <HackerOS V4.1/goroutines>\n").info.check_gen2_usage(&["arena"]).is_ok());
}

#[test]
fn take_gating_features_exist_in_registry() {
    // nazwy zwracane przez hl_parser::gen2_features_used muszą istnieć w rejestrze
    for f in ["pipe-var","coreutils","arith","import-dir","hackeros-api","typed-vars","for-in","while","switch","arena","extern","named-goroutines"] {
        let reg = find_feature(f).unwrap_or_else(|| panic!("brak {} w rejestrze", f));
        assert_eq!(reg.gen, 2, "{}", f);
    }
}
