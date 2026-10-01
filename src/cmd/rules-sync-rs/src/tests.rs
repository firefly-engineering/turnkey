//! src/cmd/rules-sync's tests, ported

use super::*;
use serde_json::Value;
use std::path::Path;

/// A Rust project whose one rules.star holds `rules_star`: its root
fn rules_project(rules_star: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::Builder::new()
        .prefix("rules-sync-test")
        .tempdir()
        .unwrap();
    let root = dir.path().to_string_lossy().into_owned();
    for (rel, content) in [
        (".buckconfig", ""),
        (
            ".turnkey/sync.toml",
            "[[languages]]\nname = \"rust\"\ncell = \"rustdeps\"\ndeps_file = \"rust-deps.toml\"\n",
        ),
        ("Cargo.toml", "[workspace]\nmembers = [\"lib\"]\n"),
        ("lib/Cargo.toml", "[package]\nname = \"lib\"\n"),
        ("lib/rules.star", rules_star),
    ] {
        let path = Path::new(&root).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    (dir, root)
}

fn env() -> Env {
    Env {
        path: None,
        cwd: "/".to_string(),
    }
}

/// Runs rules-sync with `args`: its exit code, and the report it printed
fn run_report(args: &[&str]) -> (i32, Value) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = run(&args, &env(), &mut stdout, &mut stderr);
    let report = serde_json::from_slice(&stdout).unwrap_or_else(|err| {
        panic!(
            "report {:?}: {err} (stderr: {})",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        )
    });
    (code, report)
}

const UNREADABLE: &str = "_DEPS = []

rust_library(
    name = \"lib\",
    deps = _DEPS,
)
";

/// The report lists a target whose deps sync can't read as unreadable,
/// and one a "# turnkey:no-sync" comment opts out as opted out.
#[test]
fn reports_unreadable_deps() {
    let (_dir, root) = rules_project(UNREADABLE);
    let (code, report) = run_report(&["--project-root", &root, "--dry-run", "--force"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(
        report,
        serde_json::json!({"results": [{
            "path": format!("{root}/lib/rules.star"),
            "updated": false,
            "skipped": false,
            "unreadable": [{"target": "lib", "attribute": "deps"}],
        }]})
    );

    let (_dir, root) = rules_project(&format!("# turnkey:no-sync\n{}", &UNREADABLE[12..]));
    let (_, report) = run_report(&["--project-root", &root, "--dry-run", "--force"]);
    assert_eq!(
        report,
        serde_json::json!({"results": [{
            "path": format!("{root}/lib/rules.star"),
            "updated": false,
            "skipped": false,
            "opted_out": ["lib"],
        }]})
    );
}

/// The directory argument limits sync to the rules.star files under it,
/// after `--` too, as tk passes it.
#[test]
fn syncs_the_directory_given() {
    let (_dir, root) = rules_project(UNREADABLE);
    std::fs::create_dir(format!("{root}/empty")).unwrap();
    let empty = format!("{root}/empty");
    let (code, report) = run_report(&["--project-root", &root, "--force", "--", &empty]);
    assert_eq!((code, report), (0, serde_json::json!({"results": []})));
}

/// An error that stops sync is in the report, with where it stopped, and
/// rules-sync exits 1.
#[test]
fn reports_errors() {
    let (_dir, root) = rules_project(UNREADABLE);
    std::fs::write(format!("{root}/.turnkey/sync.toml"), "").unwrap();
    let (code, report) = run_report(&["--project-root", &root]);
    assert_eq!(code, 1);
    assert_eq!(
        report,
        serde_json::json!({"results": null, "error": {
            "stage": "setup",
            "message": ".turnkey/sync.toml lists no [[languages]]: re-enter the turnkey shell to regenerate it",
        }})
    );

    let (_dir, root) = rules_project(UNREADABLE);
    let missing = format!("{root}/missing");
    let (code, report) = run_report(&["--project-root", &root, "--force", &missing]);
    assert_eq!(code, 1);
    assert_eq!(
        report,
        serde_json::json!({"results": null, "error": {
            "stage": "sync",
            "message": format!("lstat {missing}: no such file or directory"),
        }})
    );
}

/// Bad usage prints no report and exits 2.
#[test]
fn usage() {
    for args in [
        &[][..],
        &["--project-root", "a", "b", "c"],
        &["--bogus"],
        &["-h"],
        &["--project-root"],
        &["---project-root=a"],
        &["--project-root", "a", "--force=maybe"],
    ] {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run(&args, &env(), &mut stdout, &mut stderr);
        assert_eq!((code, stdout.len()), (2, 0), "{args:?}");
    }
}

/// Flags as Go's flag package reads them
#[test]
fn flags() {
    let parse =
        |args: &[&str]| parse_flags(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(
        parse(&[
            "-project-root=r",
            "--dry-run",
            "-force=false",
            "--verbose=1",
            "d",
            "--force"
        ]),
        Ok(Flags {
            project_root: "r".into(),
            dry_run: true,
            verbose: true,
            force: false,
            args: vec!["d".into(), "--force".into()],
        })
    );
    assert_eq!(
        parse(&["--project-root", "--force", "-", "x"]),
        Ok(Flags {
            project_root: "--force".into(),
            args: vec!["-".into(), "x".into()],
            ..Flags::default()
        })
    );
    assert_eq!(
        parse(&["--bogus"]),
        Err(Some("flag provided but not defined: -bogus".into()))
    );
    assert_eq!(
        parse(&["--force=maybe"]),
        Err(Some(
            "invalid boolean value \"maybe\" for -force: parse error".into()
        ))
    );
    assert_eq!(parse(&["-help"]), Err(None));
    assert_eq!(parse(&["-=x"]), Err(Some("bad flag syntax: -=x".into())));
}
