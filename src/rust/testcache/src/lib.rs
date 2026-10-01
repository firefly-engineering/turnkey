//! testcache: tk test's reuse policy, and the local test result cache
//!
//! Test results are recorded in, and reused from, a local cache-only Remote
//! Execution API server (bazel-remote), one per user per machine, shared by
//! every checkout (docs/specs/test-result-caching.md). tk starts it on
//! demand. [`Config::run_tests`] runs one `tk test` with the cache: it
//! applies the reuse policy, passes turnkey's test runner the flags that say
//! what to do with the cache, and reports the reused results. The runner
//! only obeys those flags.
//!
//! Nothing here reads the environment: tk passes in how to look a variable
//! up ([`Getenv`]).
//!
//! Ported from Go's testcache package (#216), with the same results.

mod server;
mod tls;

pub use server::{http2_preface, server_args};

use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the turnkey dev shell describes the test result cache, as JSON
/// (nix/buck2/test-cache.nix, testdata/shell-contract.json). It is unset
/// when the shell doesn't enable test result caching.
pub const ENV: &str = "TURNKEY_TEST_CACHE";
/// Overrides where turnkey keeps its caches
pub const CACHE_DIR_ENV: &str = "TURNKEY_CACHE_DIR";
/// Overrides the store's size limit, in GiB
pub const SIZE_ENV: &str = "TURNKEY_TEST_CACHE_SIZE_GIB";

/// Looks up an environment variable: tk's, or a test's. As with Go's
/// `os.Getenv`, an empty value is no value.
pub type Getenv<'a> = &'a dyn Fn(&str) -> Option<String>;

fn getenv(env: Getenv, key: &str) -> Option<String> {
    env(key).filter(|value| !value.is_empty())
}

/// The runner's `--turnkey-test-cache` value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Reuses recorded results and records fresh passes
    On,
    /// Runs every test and records fresh passes
    RecordOnly,
    /// Reuses recorded results and never records
    ReadOnly,
    /// Neither reads nor records
    Off,
}

impl Mode {
    /// The flag's value
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::On => "on",
            Mode::RecordOnly => "record-only",
            Mode::ReadOnly => "read-only",
            Mode::Off => "off",
        }
    }
}

/// The runner's `--turnkey-test-cache-origin` value: where the cache lives,
/// which the runner reports on each hit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The cache tk runs on this machine
    Local,
    /// A shared cache the dev shell points at
    Remote,
}

impl Origin {
    /// The flag's value
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Local => "local",
            Origin::Remote => "remote",
        }
    }
}

/// Bounds the store; bazel-remote evicts the least recently used entries
/// beyond it
const DEFAULT_MAX_SIZE_GIB: u64 = 5;

/// The store's size limit: [`SIZE_ENV`] if set, else the default
pub fn max_size_gib(env: Getenv) -> Result<u64, String> {
    let Some(value) = getenv(env, SIZE_ENV) else {
        return Ok(DEFAULT_MAX_SIZE_GIB);
    };
    match parse_go_int(&value) {
        Some(size) if size > 0 => Ok(size as u64),
        _ => Err(format!(
            "{SIZE_ENV}={}: expected a positive number of GiB",
            gostd::strconv::quote(&value)
        )),
    }
}

/// `strconv.Atoi`: an optional sign and decimal digits
fn parse_go_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The test result cache as the dev shell describes it, the same cache the
/// generated .buckconfig's `[buck2_re_client]` section points buck2 at
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Config {
    /// The bazel-remote binary tk runs; the shell names one only for the
    /// local cache
    #[serde(default)]
    pub server: String,
    /// `grpc://host:port`
    #[serde(default)]
    pub address: String,
    /// Whether the cache is reached over TLS; never for the local one
    #[serde(default)]
    pub tls: bool,
}

/// What turnkey's test runner does with the cache in one tk test run
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// What the runner does
    pub mode: Mode,
    /// Where the cache lives
    pub origin: Origin,
    /// The cache's address
    pub address: String,
    /// Why the cache is off for this run; empty when it's in use
    pub unusable: String,
}

impl Plan {
    /// The turnkey-test-runner flags tk passes after `--`. The runner
    /// writes the number of reused results to `report` when it's done.
    pub fn runner_args(&self, report: &str) -> Vec<String> {
        vec![
            format!("--turnkey-test-cache={}", self.mode.as_str()),
            format!("--turnkey-test-cache-address={}", self.address),
            format!("--turnkey-test-cache-origin={}", self.origin.as_str()),
            format!("--turnkey-test-cache-report={report}"),
        ]
    }
}

/// Bounds how long tk waits for a remote cache to accept a connection
/// before running the tests uncached
const REMOTE_PROBE: Duration = Duration::from_secs(2);

/// Runs buck2 with a command line, and returns its exit code as a shell
/// would report it
pub type Buck2<'a> = &'a mut dyn FnMut(&[String]) -> i32;

impl Config {
    /// The cache configuration, from the descriptor in [`ENV`], or `None`
    /// when the dev shell doesn't enable test result caching
    pub fn from_env(env: Getenv) -> Result<Option<Config>, String> {
        let Some(descriptor) = getenv(env, ENV) else {
            return Ok(None);
        };
        let cache: Config = serde_json::from_str(&descriptor).map_err(|e| format!("{ENV}: {e}"))?;
        if cache.address.is_empty() {
            return Err(format!("{ENV} names no address"));
        }
        Ok(Some(cache))
    }

    /// Whether tk runs this cache (a local one) itself. The dev shell names
    /// a server only for the local cache, so this is also where the cache
    /// lives.
    pub fn managed(&self) -> bool {
        !self.server.is_empty()
    }

    /// Where the cache lives
    pub fn origin(&self) -> Origin {
        if self.managed() {
            Origin::Local
        } else {
            Origin::Remote
        }
    }

    /// Runs a `buck2 test` command line (buck2's universal options may come
    /// first) with turnkey's test runner using the cache as the reuse
    /// policy decides for this run. It tells `stderr` why the cache is off,
    /// when it is, and how many results were reused, and returns buck2's
    /// exit code.
    pub fn run_tests(
        &self,
        args: &[String],
        forced: bool,
        env: Getenv,
        buck2: Buck2,
        stderr: &mut dyn Write,
    ) -> i32 {
        self.run_tests_with(
            args,
            forced,
            &mut || self.usable(env),
            &temp_dir(env),
            buck2,
            stderr,
        )
    }

    /// [`Config::run_tests`] with the check that the cache can be used, and
    /// where the report goes, as seams
    fn run_tests_with(
        &self,
        args: &[String],
        forced: bool,
        usable: &mut dyn FnMut() -> Result<(), String>,
        temp_dir: &Path,
        buck2: Buck2,
        stderr: &mut dyn Write,
    ) -> i32 {
        let plan = self.plan(forced, usable);
        if !plan.unusable.is_empty() {
            let _ = writeln!(
                stderr,
                "tk: running tests without the test result cache: {}",
                plan.unusable
            );
        }
        let report = match tempfile::Builder::new()
            .prefix("tk-test-report-")
            .tempfile_in(temp_dir)
        {
            Ok(report) => report.into_temp_path(),
            Err(e) => {
                let _ = writeln!(stderr, "tk: not reporting reused test results: {e}");
                return buck2(&with_runner_args(args, &plan.runner_args("/dev/null")));
            }
        };
        let code = buck2(&with_runner_args(
            args,
            &plan.runner_args(&report.to_string_lossy()),
        ));
        if let Some(hits) = read_report(&report) {
            let _ = writeln!(stderr, "{hits} recorded (reused without running)");
        }
        code
    }

    /// Applies the reuse policy (CONTEXT.md) to one tk test run. A forced
    /// re-run reads nothing. Results are recorded only into the local
    /// cache: who may write to a shared one isn't decided. A cache that
    /// can't be used is off, since buck2 would otherwise retry it for about
    /// 45 s and then fail every cached test without running it.
    pub fn plan(&self, forced: bool, usable: &mut dyn FnMut() -> Result<(), String>) -> Plan {
        let origin = self.origin();
        let mut plan = Plan {
            mode: Mode::Off,
            origin,
            address: self.address.clone(),
            unusable: String::new(),
        };
        if let Err(e) = usable() {
            plan.unusable = e;
            return plan;
        }
        plan.mode = match (origin, forced) {
            (Origin::Local, true) => Mode::RecordOnly,
            (Origin::Local, false) => Mode::On,
            (Origin::Remote, true) => Mode::Off,
            (Origin::Remote, false) => Mode::ReadOnly,
        };
        plan
    }

    /// Starts the local cache if needed. A remote cache has to accept a
    /// connection and, if it takes TLS, complete a handshake as buck2 would,
    /// with the system's roots: a certificate buck2 rejects would otherwise
    /// fail every cached test after buck2's retries.
    pub fn usable(&self, env: Getenv) -> Result<(), String> {
        if self.managed() {
            return self.ensure(env);
        }
        let host_port = self.host_port()?;
        if self.tls {
            return tls::handshake(host_port, REMOTE_PROBE).map_err(|e| {
                format!(
                    "the test result cache at {} doesn't complete a TLS handshake: {e}",
                    self.address
                )
            });
        }
        server::dial(host_port, REMOTE_PROBE)
            .map(drop)
            .map_err(|e| {
                format!(
                    "the test result cache at {} is unreachable: {e}",
                    self.address
                )
            })
    }

    /// The address without its `grpc://` scheme
    fn host_port(&self) -> Result<&str, String> {
        self.address.strip_prefix("grpc://").ok_or_else(|| {
            format!(
                "test cache address {}: expected grpc://host:port",
                gostd::strconv::quote(&self.address)
            )
        })
    }
}

/// `args` with the runner's flags first after `--`, adding one if needed:
/// the runner's `--test-arg` takes every argument after it, so the flags
/// must come before any
fn with_runner_args(args: &[String], runner: &[String]) -> Vec<String> {
    buck2_args::insert_after_separator(args, runner)
}

/// The number of reused results the runner reported, and `None` when it
/// reported nothing (for instance when the build failed before any test
/// ran)
fn read_report(report: &Path) -> Option<i64> {
    let data = std::fs::read(report).ok()?;
    parse_go_int(String::from_utf8_lossy(&data).trim())
}

/// Where recorded results live: one store per user per machine, shared by
/// every checkout and every turnkey repo
pub fn store_dir(env: Getenv) -> Result<PathBuf, String> {
    let base = match getenv(env, CACHE_DIR_ENV) {
        Some(base) => PathBuf::from(base),
        None => user_cache_dir(env)
            .map_err(|e| format!("locating the user cache directory: {e}"))?
            .join("turnkey"),
    };
    Ok(base.join("test-results"))
}

/// `os.UserCacheDir`: `$HOME/Library/Caches` on macOS; elsewhere
/// `$XDG_CACHE_HOME`, which must be absolute, or else `$HOME/.cache`
pub fn user_cache_dir(env: Getenv) -> Result<PathBuf, String> {
    if cfg!(target_os = "macos") {
        let home = getenv(env, "HOME").ok_or("$HOME is not defined")?;
        return Ok(PathBuf::from(format!("{home}/Library/Caches")));
    }
    match getenv(env, "XDG_CACHE_HOME") {
        Some(dir) if !dir.starts_with('/') => Err("path in $XDG_CACHE_HOME is relative".into()),
        Some(dir) => Ok(PathBuf::from(dir)),
        None => {
            let home =
                getenv(env, "HOME").ok_or("neither $XDG_CACHE_HOME nor $HOME are defined")?;
            Ok(PathBuf::from(format!("{home}/.cache")))
        }
    }
}

/// `os.TempDir`: `$TMPDIR`, or else /tmp
fn temp_dir(env: Getenv) -> PathBuf {
    PathBuf::from(getenv(env, "TMPDIR").unwrap_or_else(|| "/tmp".into()))
}

#[cfg(test)]
mod tests;
