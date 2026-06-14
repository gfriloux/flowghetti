//! Golden tests: run the full pipeline on each fixture in `tests/fixtures/` and
//! compare the rendered DOT against `tests/golden/<name>.dot`.
//!
//! Regenerate the golden files intentionally with `just bless` (sets `BLESS=1`),
//! then review the diff.

use flowghetti::render::ThemeChoice;
use std::path::Path;

#[test]
fn golden_fixtures() {
    let bless = std::env::var_os("BLESS").is_some();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = root.join("tests/fixtures");
    let golden_dir = root.join("tests/golden");

    let mut names: Vec<String> = std::fs::read_dir(&fixtures)
        .expect("tests/fixtures should exist")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no fixtures found in tests/fixtures");

    let mut failures = Vec::new();
    for name in &names {
        let actual = flowghetti::run(&fixtures.join(name), "LR", ThemeChoice::Light, false, None)
            .unwrap_or_else(|err| panic!("pipeline failed on fixture {name}: {err}"));
        let golden = golden_dir.join(format!("{name}.dot"));

        if bless {
            std::fs::write(&golden, &actual).expect("should write golden file");
            continue;
        }

        match std::fs::read_to_string(&golden) {
            Ok(expected) if expected == actual => {}
            Ok(_) => failures.push(name.clone()),
            Err(_) => panic!("missing golden {} — run `just bless`", golden.display()),
        }
    }

    assert!(
        failures.is_empty(),
        "golden mismatch for {failures:?} — review and run `just bless` if intended"
    );
}

/// The dark palette is exercised end-to-end on one representative fixture
/// (`dalim_cluster`: load balancer + compute nodes, database/secure/neutral
/// edges). The dark golden lives alongside the light ones as `<name>.dark.dot`.
#[test]
fn golden_dark_theme() {
    let bless = std::env::var_os("BLESS").is_some();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("tests/fixtures/dalim_cluster");
    let golden = root.join("tests/golden/dalim_cluster.dark.dot");

    let actual = flowghetti::run(&fixture, "LR", ThemeChoice::Dark, false, None)
        .unwrap_or_else(|err| panic!("pipeline failed on dark fixture: {err}"));

    if bless {
        std::fs::write(&golden, &actual).expect("should write dark golden file");
        return;
    }

    let expected = std::fs::read_to_string(&golden).unwrap_or_else(|_| {
        panic!(
            "missing dark golden {} — run `just bless`",
            golden.display()
        )
    });
    assert_eq!(
        expected, actual,
        "dark golden mismatch — review and `just bless`"
    );
}

/// The legend is exercised on the smallest fixture (`basique`) with `--legend`;
/// the golden lives as `<name>.legend.dot`.
#[test]
fn golden_legend() {
    let bless = std::env::var_os("BLESS").is_some();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("tests/fixtures/basique");
    let golden = root.join("tests/golden/basique.legend.dot");

    let actual = flowghetti::run(&fixture, "LR", ThemeChoice::Light, true, None)
        .unwrap_or_else(|err| panic!("pipeline failed on legend fixture: {err}"));

    if bless {
        std::fs::write(&golden, &actual).expect("should write legend golden file");
        return;
    }

    let expected = std::fs::read_to_string(&golden).unwrap_or_else(|_| {
        panic!(
            "missing legend golden {} — run `just bless`",
            golden.display()
        )
    });
    assert_eq!(
        expected, actual,
        "legend golden mismatch — review and `just bless`"
    );
}

/// A `--title` is exercised on the smallest fixture (`basique`); the golden
/// lives as `<name>.title.dot`.
#[test]
fn golden_title() {
    let bless = std::env::var_os("BLESS").is_some();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("tests/fixtures/basique");
    let golden = root.join("tests/golden/basique.title.dot");

    let actual = flowghetti::run(
        &fixture,
        "LR",
        ThemeChoice::Light,
        false,
        Some("staging / eu-west-1"),
    )
    .unwrap_or_else(|err| panic!("pipeline failed on title fixture: {err}"));

    if bless {
        std::fs::write(&golden, &actual).expect("should write title golden file");
        return;
    }

    let expected = std::fs::read_to_string(&golden).unwrap_or_else(|_| {
        panic!(
            "missing title golden {} — run `just bless`",
            golden.display()
        )
    });
    assert_eq!(
        expected, actual,
        "title golden mismatch — review and `just bless`"
    );
}
