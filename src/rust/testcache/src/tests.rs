use super::*;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// How the dev shell describes the cache. The Nix side
/// (nix/buck2/test-cache.nix) is checked against the same file.
const SHELL_CONTRACT: &str = include_str!("../testdata/shell-contract.json");

/// What tk passes turnkey-test-runner and reads back from it. The runner's
/// tests check their side against the same file.
const RUNNER_CONTRACT: &str = include_str!("../testdata/runner-contract.json");

/// An environment of the given variables
fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let vars: BTreeMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |key| vars.get(key).cloned()
}

#[test]
fn from_env_reads_what_the_shell_writes() {
    let contract: Value = serde_json::from_str(SHELL_CONTRACT).unwrap();
    assert_eq!(contract["env"], ENV, "the shell sets another variable");
    for cache in contract["caches"].as_array().unwrap() {
        let descriptor = cache["descriptor"].to_string();
        let config = Config::from_env(&env(&[(ENV, &descriptor)]))
            .unwrap()
            .unwrap();
        assert_eq!(
            config.address, cache["descriptor"]["address"],
            "{descriptor}"
        );
        assert_eq!(
            Value::Bool(config.tls),
            cache["descriptor"]["tls"],
            "{descriptor}"
        );
        // Only the local cache (no endpoint) is tk's to run
        let local = cache["options"]["endpoint"].is_null();
        assert_eq!(config.managed(), local, "{descriptor}");
    }
}

#[test]
fn from_env_without_a_cache() {
    assert_eq!(Config::from_env(&env(&[])), Ok(None));
    assert_eq!(Config::from_env(&env(&[(ENV, "")])), Ok(None));
    for bad in ["grpc://127.0.0.1:1", r#"{"tls": true}"#] {
        assert!(Config::from_env(&env(&[(ENV, bad)])).is_err(), "{bad}");
    }
}

#[test]
fn runner_contract() {
    let contract: Value = serde_json::from_str(RUNNER_CONTRACT).unwrap();
    let address = contract["address"].as_str().unwrap();
    let report = contract["report"].as_str().unwrap();
    for want in contract["plans"].as_array().unwrap() {
        let mode = [Mode::On, Mode::RecordOnly, Mode::ReadOnly, Mode::Off]
            .into_iter()
            .find(|m| want["mode"] == m.as_str())
            .unwrap();
        let origin = [Origin::Local, Origin::Remote]
            .into_iter()
            .find(|o| want["origin"] == o.as_str())
            .unwrap();
        let plan = Plan {
            mode,
            origin,
            address: address.into(),
            unusable: String::new(),
        };
        let args: Vec<String> = serde_json::from_value(want["args"].clone()).unwrap();
        assert_eq!(plan.runner_args(report), args, "{mode:?}/{origin:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report");
    std::fs::write(&path, contract["hits_report"].as_str().unwrap()).unwrap();
    assert_eq!(read_report(&path), contract["hits"].as_i64());
}

#[test]
fn read_report_cases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report");
    assert_eq!(
        read_report(&path),
        None,
        "a missing report reads as nothing"
    );
    for (content, want) in [("3\n", Some(3)), (" 7 ", Some(7)), ("", None), ("x", None)] {
        std::fs::write(&path, content).unwrap();
        assert_eq!(read_report(&path), want, "{content:?}");
    }
}

#[test]
fn store_dir_honours_turnkey_cache_dir() {
    let dir = store_dir(&env(&[(CACHE_DIR_ENV, "/somewhere")])).unwrap();
    assert_eq!(dir, Path::new("/somewhere/test-results"));
}

#[test]
fn store_dir_defaults_to_the_user_cache_dir() {
    let dir = store_dir(&env(&[("HOME", "/home/me"), ("XDG_CACHE_HOME", "/xdg")])).unwrap();
    let want = if cfg!(target_os = "macos") {
        "/home/me/Library/Caches/turnkey/test-results"
    } else {
        "/xdg/turnkey/test-results"
    };
    assert_eq!(dir, Path::new(want));
    assert!(store_dir(&env(&[])).is_err());
    if !cfg!(target_os = "macos") {
        assert!(store_dir(&env(&[("XDG_CACHE_HOME", "relative")])).is_err());
        assert_eq!(
            user_cache_dir(&env(&[("HOME", "/home/me")])).unwrap(),
            Path::new("/home/me/.cache")
        );
    }
}

#[test]
fn max_size_gib_cases() {
    assert_eq!(max_size_gib(&env(&[])), Ok(5));
    assert_eq!(max_size_gib(&env(&[(SIZE_ENV, "")])), Ok(5));
    assert_eq!(max_size_gib(&env(&[(SIZE_ENV, "2")])), Ok(2));
    assert_eq!(max_size_gib(&env(&[(SIZE_ENV, "+3")])), Ok(3));
    for bad in ["0", "-1", "lots", " 2", "99999999999999999999"] {
        assert!(max_size_gib(&env(&[(SIZE_ENV, bad)])).is_err(), "{bad:?}");
    }
}

/// Accepts connections on a loopback port and answers each with `respond`
fn serve(respond: impl Fn(TcpStream) + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let respond = Arc::new(respond);
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { return };
            let respond = respond.clone();
            std::thread::spawn(move || respond(conn));
        }
    });
    address
}

/// Reads the client preface and answers with a SETTINGS frame, as any
/// HTTP/2 (and so gRPC) server does
fn http2_server(mut conn: TcpStream) {
    let mut buf = vec![0; http2_preface().len()];
    if conn.read_exact(&mut buf).is_err() {
        return;
    }
    let _ = conn.write_all(&[0, 0, 0, 0x4, 0, 0, 0, 0, 0]);
    std::thread::sleep(Duration::from_secs(1));
}

fn silent(_: TcpStream) {
    std::thread::sleep(Duration::from_secs(1));
}

/// A loopback address nothing listens on: a port just released
fn unused_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

fn cache(server: &str, address: &str) -> Config {
    Config {
        server: server.into(),
        address: format!("grpc://{address}"),
        tls: false,
    }
}

#[test]
fn remote_tls_cache_must_complete_a_handshake() {
    // Accepts connections but never speaks TLS: a plain TCP probe would
    // pass
    let address = serve(|_| std::thread::sleep(3 * REMOTE_PROBE));
    let remote = Config {
        tls: true,
        ..cache("", &address)
    };
    let err = remote.usable(&env(&[])).unwrap_err();
    assert!(err.contains("TLS handshake"), "{err}");
}

#[test]
fn unreachable_remote_cache_is_unusable() {
    let err = cache("", &unused_address()).usable(&env(&[])).unwrap_err();
    assert!(err.contains("unreachable"), "{err}");
}

#[test]
fn reachable_remote_cache_is_usable() {
    assert_eq!(cache("", &serve(silent)).usable(&env(&[])), Ok(()));
}

#[test]
fn reachable_needs_an_http2_server() {
    assert!(cache("", &serve(http2_server)).reachable(Duration::from_secs(1)));
    assert!(!cache("", &serve(silent)).reachable(Duration::from_millis(100)));
}

#[test]
fn reachable_rejects_non_grpc_addresses() {
    let c = Config {
        address: "http://127.0.0.1:1".into(),
        ..Default::default()
    };
    assert!(!c.reachable(Duration::from_millis(10)));
}

#[test]
fn ensure_fails_fast_when_the_port_is_taken() {
    let store = tempfile::tempdir().unwrap();
    let store_env = env(&[(CACHE_DIR_ENV, &store.path().to_string_lossy())]);
    // Something that isn't the cache holds the port, and the "server"
    // exits at once, as bazel-remote does when it can't bind: a shell
    // refusing bazel-remote's flags
    let c = cache("/bin/sh", &serve(silent));
    let start = Instant::now();
    let err = c.ensure(&store_env).unwrap_err();
    assert!(err.contains("exited on startup"), "{err}");
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "took {:?} to give up",
        start.elapsed()
    );
    assert!(store.path().join("test-results/server.log").exists());
}

#[test]
fn ensure_uses_a_running_server() {
    let c = cache("/nonexistent/bazel-remote", &serve(http2_server));
    assert_eq!(c.ensure(&env(&[])), Ok(()));
}

#[test]
fn ensure_reports_a_server_that_wont_start() {
    let store = tempfile::tempdir().unwrap();
    let store_env = env(&[(CACHE_DIR_ENV, &store.path().to_string_lossy())]);
    let c = cache("/nonexistent/bazel-remote", &unused_address());
    let err = c.ensure(&store_env).unwrap_err();
    assert!(err.contains("starting the test result cache"), "{err}");
}

#[test]
fn concurrent_ensure_starts_one_server() {
    let store = tempfile::tempdir().unwrap();
    let store_env = env(&[(CACHE_DIR_ENV, &store.path().to_string_lossy())]);
    let address = unused_address();
    let c = cache("bazel-remote", &address);
    let starts = Arc::new(Mutex::new(Vec::new()));

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let (c, starts, store_env) = (&c, starts.clone(), &store_env);
                scope.spawn(move || {
                    // Stands in for bazel-remote: records its start, and
                    // serves HTTP/2 prefaces on the address
                    let mut start = |cache: &Config, env: Getenv| {
                        let store = store_dir(env)?;
                        let args = server_args(&store, max_size_gib(env)?, &cache.address[7..]);
                        starts.lock().unwrap().push(args.clone());
                        let listener = TcpListener::bind(&args[5]).map_err(|e| e.to_string())?;
                        std::thread::spawn(move || {
                            for conn in listener.incoming().flatten() {
                                std::thread::spawn(move || http2_server(conn));
                            }
                        });
                        Ok(std::sync::mpsc::channel().1)
                    };
                    c.ensure_with(store_env, &mut start)
                })
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), Ok(()));
        }
    });

    let starts = starts.lock().unwrap();
    assert_eq!(starts.len(), 1, "started {} servers", starts.len());
    let store = store.path().join("test-results");
    assert_eq!(
        starts[0],
        [
            "--dir",
            &store.join("cas").to_string_lossy(),
            "--max_size",
            "5",
            "--grpc_address",
            &address,
            "--http_address",
            "127.0.0.1:0",
            "--idle_timeout",
            "24h",
        ]
    );
}

#[test]
fn plan_applies_the_reuse_policy() {
    let local = cache("bazel-remote", "127.0.0.1:47301");
    let remote = cache("", "cache.example.com:443");
    for (name, cache, forced, up, want) in [
        ("local", &local, false, true, Mode::On),
        ("local, forced re-run", &local, true, true, Mode::RecordOnly),
        ("local, unusable", &local, false, false, Mode::Off),
        (
            "remote: never recorded into",
            &remote,
            false,
            true,
            Mode::ReadOnly,
        ),
        ("remote, forced re-run", &remote, true, true, Mode::Off),
        ("remote, unusable", &remote, false, false, Mode::Off),
    ] {
        let mut usable = || if up { Ok(()) } else { Err("down".to_string()) };
        let plan = cache.plan(forced, &mut usable);
        assert_eq!(plan.mode, want, "{name}");
        assert_eq!(plan.unusable.is_empty(), up, "{name}");
        assert_eq!(
            (plan.origin, plan.address.as_str()),
            (cache.origin(), cache.address.as_str()),
            "{name}"
        );
    }
}

#[test]
fn origin_follows_who_runs_the_cache() {
    // A shared cache reached through a loopback tunnel is still remote
    assert_eq!(cache("", "127.0.0.1:9092").origin(), Origin::Remote);
    assert_eq!(
        cache("bazel-remote", "127.0.0.1:9092").origin(),
        Origin::Local
    );
}

/// Stands in for buck2 in run_tests: records the command line, writes
/// `hits` to the runner's report unless `None`, and exits with `code`
fn fake_buck2(got: &mut Vec<String>, hits: Option<i64>, code: i32) -> impl FnMut(&[String]) -> i32 {
    move |args| {
        *got = args.to_vec();
        for arg in args {
            if let (Some(report), Some(hits)) =
                (arg.strip_prefix("--turnkey-test-cache-report="), hits)
            {
                std::fs::write(report, format!("{hits}\n")).unwrap();
            }
        }
        code
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

/// Runs run_tests_with, the cache usable or not, and returns the exit
/// code, buck2's command line and stderr
fn run(
    cache: &Config,
    args: &[&str],
    usable: bool,
    hits: Option<i64>,
    code: i32,
) -> (i32, Vec<String>, String) {
    let tmp = tempfile::tempdir().unwrap();
    let mut got = Vec::new();
    let mut stderr = Vec::new();
    let mut check = || {
        if usable {
            Ok(())
        } else {
            Err("unreachable".to_string())
        }
    };
    let code = cache.run_tests_with(
        &strings(args),
        false,
        &mut check,
        tmp.path(),
        &mut fake_buck2(&mut got, hits, code),
        &mut stderr,
    );
    // The report doesn't outlive the run
    assert_eq!(
        std::fs::read_dir(tmp.path()).unwrap().count(),
        0,
        "report left behind"
    );
    (code, got, String::from_utf8(stderr).unwrap())
}

#[test]
fn run_tests_puts_the_runner_flags_first_after_the_separator() {
    let local = cache("bazel-remote", "127.0.0.1:47301");
    for (name, args, want) in [
        (
            "no separator",
            &["test", "//:t"][..],
            &["test", "//:t", "--", "FLAGS"][..],
        ),
        (
            "test args of its own",
            &[
                "--isolation-dir",
                "x",
                "test",
                "//:t",
                "--",
                "--test-arg",
                "y",
            ],
            &[
                "--isolation-dir",
                "x",
                "test",
                "//:t",
                "--",
                "FLAGS",
                "--test-arg",
                "y",
            ],
        ),
    ] {
        let (_, got, _) = run(&local, args, true, None, 0);
        let flags = got
            .iter()
            .position(|a| a.starts_with("--turnkey-test-cache="))
            .unwrap_or_else(|| panic!("{name}: no runner flags in {got:?}"));
        // The four flags, collapsed to one placeholder
        let mut collapsed = got[..flags].to_vec();
        collapsed.push("FLAGS".into());
        collapsed.extend_from_slice(&got[flags + 4..]);
        assert_eq!(collapsed, want, "{name}");
    }
}

#[test]
fn run_tests_reports_reused_results() {
    let local = cache("bazel-remote", "127.0.0.1:47301");
    let (code, _, stderr) = run(&local, &["test", "//..."], true, Some(3), 32);
    assert_eq!(code, 32, "buck2's exit code");
    assert!(
        stderr.contains("3 recorded (reused without running)"),
        "{stderr}"
    );
}

#[test]
fn run_tests_without_a_report() {
    // The build failed before any test ran: the runner wrote nothing
    let local = cache("bazel-remote", "127.0.0.1:47301");
    let (_, _, stderr) = run(&local, &["test", "//..."], true, None, 1);
    assert!(!stderr.contains("recorded"), "{stderr}");
}

#[test]
fn run_tests_with_an_unusable_cache() {
    let remote = cache("", "cache.example.com:443");
    let (_, got, stderr) = run(&remote, &["test", "//..."], false, None, 0);
    assert!(
        got.contains(&"--turnkey-test-cache=off".to_string()),
        "{got:?}"
    );
    assert!(
        stderr.contains("without the test result cache: unreachable"),
        "{stderr}"
    );
}

#[test]
fn run_tests_reads_the_temp_dir_from_the_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let local = cache("bazel-remote", &serve(http2_server));
    let tmp_env = env(&[("TMPDIR", &tmp.path().to_string_lossy())]);
    let mut got = Vec::new();
    let mut stderr = Vec::new();
    local.run_tests(
        &strings(&["test", "//..."]),
        false,
        &tmp_env,
        &mut fake_buck2(&mut got, Some(0), 0),
        &mut stderr,
    );
    let report = got
        .iter()
        .find_map(|a| a.strip_prefix("--turnkey-test-cache-report="))
        .unwrap();
    assert!(Path::new(report).starts_with(tmp.path()), "{report}");
    assert!(
        got.contains(&"--turnkey-test-cache=on".to_string()),
        "{got:?}"
    );
}
