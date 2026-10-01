//! The Nix hashes of the modules' sources, which the godeps cell fetches
//! with

use crate::deps::Dependency;
use gomod::module::{escape_path, escape_version};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// One module version whose source is hashed
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub path: String,
    pub version: String,
}

/// The Nix hash of one module's source (an SRI hash, e.g.
/// "sha256-abc..."), or why it has none
pub type Prefetched = Result<String, String>;

/// Fetches Nix-compatible hashes of Go module sources, all of a module
/// graph's at once
pub trait Prefetcher {
    /// One result per module, in order: a hash, or the error for that
    /// module alone
    fn prefetch(&mut self, mods: &[Module]) -> Vec<Prefetched>;
}

/// Hashes modules' zips from proxy.golang.org, unpacked as Nix fetchzip
/// unpacks them. The godeps cell fetches every module from the proxy
/// (nix/lib/deps-cell/adapters/go.nix), so its hash is the only one that
/// matches; a GitHub archive of the same version hashes differently.
///
/// All modules go to one `nix-prefetch-cached --batch` process, which
/// loads turnkey's prefetch cache (src/rust/prefetch-cache) once and
/// fetches the misses in parallel.
pub struct GoProxyPrefetcher {
    /// The nix-prefetch-cached to run
    pub command: OsString,
    /// Gets nix-prefetch-cached's progress and warnings as they come
    pub logger: Option<Box<dyn Write + Send>>,
    /// Fetch afresh instead of reusing a hash nix-prefetch-cached has
    /// already computed
    pub no_cache: bool,
}

impl GoProxyPrefetcher {
    /// The prefetcher godeps-gen uses: nix-prefetch-cached from PATH,
    /// logging to stderr
    pub fn new(no_cache: bool) -> Self {
        Self {
            command: "nix-prefetch-cached".into(),
            logger: Some(Box::new(std::io::stderr())),
            no_cache,
        }
    }

    /// Run nix-prefetch-cached on `urls`: its stdout, or why it failed
    /// with its stderr
    fn run(&mut self, urls: &[String]) -> Result<String, String> {
        let mut args = vec!["--batch", "--unpack"];
        if self.no_cache {
            args.push("--no-cache");
        }
        let mut child = Command::new(&self.command)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{e}: "))?;

        let input = urls.join("\n") + "\n";
        let mut stdin = child.stdin.take().expect("piped");
        let mut stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let mut logger = self.logger.take();
        // Feed stdin, and tee stderr to the logger, while reading stdout
        let (output, errors, logger) = std::thread::scope(|s| {
            s.spawn(move || {
                // A child that exits early closes its stdin: its exit
                // status says why
                let _ = stdin.write_all(input.as_bytes());
            });
            let tee = s.spawn(move || {
                let mut captured = Vec::new();
                let mut buf = [0; 4096];
                while let Ok(n) = stderr.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    captured.extend_from_slice(&buf[..n]);
                    if let Some(logger) = logger.as_mut() {
                        let _ = logger.write_all(&buf[..n]);
                    }
                }
                (captured, logger)
            });
            let mut output = Vec::new();
            let _ = stdout.read_to_end(&mut output);
            let (errors, logger) = tee.join().expect("stderr reader");
            (output, errors, logger)
        });
        self.logger = logger;

        let stderr = String::from_utf8_lossy(&errors);
        match child.wait() {
            Ok(status) if status.success() => Ok(String::from_utf8_lossy(&output).into_owned()),
            Ok(status) => Err(format!("{status}: {}", stderr.trim())),
            Err(e) => Err(format!("{e}: {}", stderr.trim())),
        }
    }
}

impl Prefetcher for GoProxyPrefetcher {
    /// Hash every module's proxy.golang.org zip in one batch
    fn prefetch(&mut self, mods: &[Module]) -> Vec<Prefetched> {
        if mods.is_empty() {
            return Vec::new();
        }
        let mut results: Vec<Prefetched> = vec![Err(String::new()); mods.len()];
        // The modules that have a URL, and each one's index
        let mut urls = Vec::new();
        let mut at = Vec::new();
        for (i, m) in mods.iter().enumerate() {
            match proxy_zip_url(m) {
                Ok(url) => {
                    urls.push(url);
                    at.push(i);
                }
                Err(e) => results[i] = Err(e),
            }
        }
        if urls.is_empty() {
            return results;
        }

        let lines = self.run(&urls).and_then(|output| {
            let lines: Vec<String> = output
                .strip_suffix('\n')
                .unwrap_or(&output)
                .split('\n')
                .map(str::to_string)
                .collect();
            if lines.len() == urls.len() {
                Ok(lines)
            } else {
                Err(format!("{} results for {} URLs", lines.len(), urls.len()))
            }
        });
        for (j, url) in urls.iter().enumerate() {
            results[at[j]] = match &lines {
                Err(e) => Err(format!("nix-prefetch-cached {url}: {e}")),
                Ok(lines) => match lines[j].strip_prefix("error: ") {
                    Some(reason) => Err(format!("nix-prefetch-cached {url}: {reason}")),
                    None => Ok(lines[j].clone()),
                },
            };
        }
        results
    }
}

/// The proxy.golang.org zip of a module version. The module proxy protocol
/// case-escapes both the path and the version (an upper-case letter is
/// '!' and its lower case).
pub fn proxy_zip_url(m: &Module) -> Result<String, String> {
    let path = escape_path(&m.path).map_err(|e| e.to_string())?;
    let version = escape_version(&m.version).map_err(|e| e.to_string())?;
    Ok(format!("https://proxy.golang.org/{path}/@v/{version}.zip"))
}

/// Fetch the Nix hashes of all `deps` with `p`, in one call. A dependency
/// that gets none is reported to `on_error`; the others still get theirs.
pub fn prefetch_all(
    deps: &mut [Dependency],
    p: &mut dyn Prefetcher,
    mut on_error: impl FnMut(&Dependency, &str),
) {
    // Fetch from the replacement, if any; the dependency keeps its path
    let mods: Vec<Module> = deps
        .iter()
        .map(|d| Module {
            path: d.effective_fetch_path().to_string(),
            version: d.version.clone(),
        })
        .collect();
    let results = p.prefetch(&mods);
    for (i, dep) in deps.iter_mut().enumerate() {
        match results.get(i) {
            None => on_error(dep, &format!("no hash returned for {}", mods[i].path)),
            Some(Err(e)) => on_error(dep, e),
            Some(Ok(hash)) => dep.nix_hash = hash.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    /// A test double: answers from fixed tables and records every call
    #[derive(Default)]
    struct MockPrefetcher {
        hashes: HashMap<String, String>,
        errors: HashMap<String, String>,
        calls: Vec<Vec<Module>>,
    }

    impl Prefetcher for MockPrefetcher {
        fn prefetch(&mut self, mods: &[Module]) -> Vec<Prefetched> {
            self.calls.push(mods.to_vec());
            mods.iter()
                .map(|m| {
                    let key = format!("{} {}", m.path, m.version);
                    if let Some(e) = self.errors.get(&key) {
                        Err(e.clone())
                    } else if let Some(h) = self.hashes.get(&key) {
                        Ok(h.clone())
                    } else {
                        Err("not found".to_string())
                    }
                })
                .collect()
        }
    }

    fn dep(import_path: &str, version: &str) -> Dependency {
        Dependency {
            import_path: import_path.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    fn module(path: &str, version: &str) -> Module {
        Module {
            path: path.into(),
            version: version.into(),
        }
    }

    #[test]
    fn prefetch_all_makes_one_call() {
        let mut mock = MockPrefetcher {
            hashes: HashMap::from([
                ("github.com/foo/bar v1.0.0".into(), "sha256-foo=".into()),
                ("github.com/baz/qux v1.0.0".into(), "sha256-baz=".into()),
            ]),
            ..Default::default()
        };
        let mut deps = vec![
            dep("github.com/foo/bar", "v1.0.0"),
            dep("github.com/baz/qux", "v1.0.0"),
        ];
        prefetch_all(&mut deps, &mut mock, |_, _| {});
        assert_eq!(deps[0].nix_hash, "sha256-foo=");
        assert_eq!(deps[1].nix_hash, "sha256-baz=");
        assert_eq!(mock.calls.len(), 1);
    }

    #[test]
    fn prefetch_all_reports_errors() {
        let mut mock = MockPrefetcher {
            hashes: HashMap::from([("github.com/good/pkg v1.0.0".into(), "sha256-good=".into())]),
            errors: HashMap::from([("github.com/bad/pkg v1.0.0".into(), "fetch failed".into())]),
            ..Default::default()
        };
        let mut deps = vec![
            dep("github.com/good/pkg", "v1.0.0"),
            dep("github.com/bad/pkg", "v1.0.0"),
        ];
        let mut failed = Vec::new();
        prefetch_all(&mut deps, &mut mock, |d, _| {
            failed.push(d.import_path.clone())
        });
        assert_eq!(deps[0].nix_hash, "sha256-good=");
        assert_eq!(deps[1].nix_hash, "");
        assert_eq!(failed, vec!["github.com/bad/pkg"]);
    }

    #[test]
    fn prefetch_all_fetches_the_replacement() {
        let mut mock = MockPrefetcher {
            hashes: HashMap::from([("github.com/fork/lib v1.0.1".into(), "sha256-fork=".into())]),
            ..Default::default()
        };
        let mut deps = vec![dep("github.com/up/lib", "v1.0.1")];
        deps[0].fetch_path = "github.com/fork/lib".into();
        prefetch_all(&mut deps, &mut mock, |_, e| panic!("{e}"));
        assert_eq!(deps[0].nix_hash, "sha256-fork=");
        assert_eq!(deps[0].import_path, "github.com/up/lib");
    }

    #[test]
    fn proxy_zip_url_escapes_path_and_version() {
        let cases = [
            (
                module("github.com/foo/bar", "v1.0.0"),
                "https://proxy.golang.org/github.com/foo/bar/@v/v1.0.0.zip",
            ),
            (
                module("github.com/BurntSushi/toml", "v1.4.0"),
                "https://proxy.golang.org/github.com/!burnt!sushi/toml/@v/v1.4.0.zip",
            ),
            (
                module("github.com/Azure/azure-sdk", "v1.0.0-RC1"),
                "https://proxy.golang.org/github.com/!azure/azure-sdk/@v/v1.0.0-!r!c1.zip",
            ),
        ];
        for (m, want) in cases {
            assert_eq!(proxy_zip_url(&m).as_deref(), Ok(want));
        }
    }

    /// A stand-in nix-prefetch-cached running `script`
    fn stand_in(dir: &tempfile::TempDir, script: &str) -> OsString {
        let path = dir.path().join("nix-prefetch-cached");
        std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.into_os_string()
    }

    fn prefetcher(command: OsString, no_cache: bool) -> GoProxyPrefetcher {
        GoProxyPrefetcher {
            command,
            logger: None,
            no_cache,
        }
    }

    #[test]
    fn fails_a_module_it_cannot_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let command = stand_in(
            &tmp,
            "while read -r url; do echo \"sha256-${url##*/}\"; done\n",
        );
        let got = prefetcher(command, false).prefetch(&[
            module("github.com/foo/bar", "v1.0.0!"),
            module("golang.org/x/mod", "v0.20.0"),
        ]);
        assert!(got[0].is_err(), "{got:?}");
        assert_eq!(got[1], Ok("sha256-v0.20.0.zip".to_string()));
    }

    #[test]
    fn hashes_the_proxy_zips_the_cell_fetches() {
        let tmp = tempfile::tempdir().unwrap();
        // Records its arguments and stdin, and answers one hash per URL
        let calls = tmp.path().join("calls");
        let command = stand_in(
            &tmp,
            &format!(
                "echo \"$@\" >> {calls}\nwhile read -r url; do echo \"$url\" >> {calls}; echo \"sha256-${{url##*/}}\"; done\n",
                calls = calls.display()
            ),
        );
        let mods = [
            module("github.com/BurntSushi/toml", "v1.4.0"),
            module("golang.org/x/mod", "v0.20.0"),
        ];
        for no_cache in [false, true] {
            let got = prefetcher(command.clone(), no_cache).prefetch(&mods);
            assert_eq!(
                got,
                vec![
                    Ok("sha256-v1.4.0.zip".to_string()),
                    Ok("sha256-v0.20.0.zip".to_string())
                ]
            );
        }
        // The URLs nix/lib/deps-cell/fetchers.nix's goproxy fetcher builds,
        // unpacked as fetchzip unpacks them, all in one call
        let urls = "https://proxy.golang.org/github.com/!burnt!sushi/toml/@v/v1.4.0.zip\n\
                    https://proxy.golang.org/golang.org/x/mod/@v/v0.20.0.zip\n";
        assert_eq!(
            std::fs::read_to_string(calls).unwrap(),
            format!("--batch --unpack\n{urls}--batch --unpack --no-cache\n{urls}")
        );
    }

    #[test]
    fn fails_only_the_module_that_failed() {
        let tmp = tempfile::tempdir().unwrap();
        let command = stand_in(
            &tmp,
            "read -r a; read -r b\necho sha256-good=\necho \"error: no such module\"\n",
        );
        let got = prefetcher(command, false).prefetch(&[
            module("example.com/good", "v1.0.0"),
            module("example.com/gone", "v1.0.0"),
        ]);
        assert_eq!(got[0], Ok("sha256-good=".to_string()));
        let err = got[1].as_ref().unwrap_err();
        assert!(
            err.contains("proxy.golang.org/example.com/gone/@v/v1.0.0.zip")
                && err.contains("no such module"),
            "error {err} doesn't name the URL and nix-prefetch-cached's reason"
        );
    }

    #[test]
    fn fails_every_module_when_the_batch_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let command = stand_in(&tmp, "echo 'cache dir unwritable' >&2\nexit 1\n");
        let got = prefetcher(command, false).prefetch(&[
            module("example.com/a", "v1.0.0"),
            module("example.com/b", "v1.0.0"),
        ]);
        for (i, r) in got.iter().enumerate() {
            let err = r.as_ref().unwrap_err();
            assert!(err.contains("cache dir unwritable"), "module {i}: {err}");
        }
    }

    #[test]
    fn logs_stderr_as_it_runs() {
        #[derive(Clone, Default)]
        struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let command = stand_in(&tmp, "read -r a\necho 'fetching it' >&2\necho sha256-x=\n");
        let log = Shared::default();
        let mut p = GoProxyPrefetcher {
            command,
            logger: Some(Box::new(log.clone())),
            no_cache: false,
        };
        assert_eq!(
            p.prefetch(&[module("example.com/a", "v1.0.0")]),
            vec![Ok("sha256-x=".to_string())]
        );
        assert_eq!(
            String::from_utf8(log.0.lock().unwrap().clone()).unwrap(),
            "fetching it\n"
        );
    }
}
