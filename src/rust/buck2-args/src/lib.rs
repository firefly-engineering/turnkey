//! buck2-args: buck2's command line, as tk reads and rewrites it
//!
//! tk passes everything that isn't one of its own subcommands to buck2.
//! On the way it finds buck2's subcommand past the universal options before
//! it ([`subcommand`]), decides from it whether the command reads the build
//! graph, so that tk syncs first ([`needs_sync`]), keeps isolation
//! directories hidden ([`transform_isolation_dir`]), and injects the
//! per-developer arguments of `.turnkey/local.toml` after `--`
//! ([`apply_local_overrides`], [`LocalConfig`]).
//!
//! The command line is that of the pinned buck2 release
//! (docs/adr/0002-turnkey-owns-the-buck2-version.md).
//!
//! Ported from Go's buck2args and localconfig packages and the Go tk's
//! command line handling (#216), with the same results.

mod local;

pub use local::{DEFAULT_CONFIG_PATH, Error, LocalConfig, TargetOverride, match_target};

/// buck2's universal options that take their value as the next argument,
/// as `buck2 --help` lists them. Their `--name=value` forms are one
/// argument.
const GLOBAL_OPTIONS_WITH_VALUE: &[&str] = &[
    "--isolation-dir",
    "-v",
    "--verbose",
    "--oncall",
    "--client-metadata",
    "--setting",
    "--agent-context",
];

/// Returns buck2's subcommand in `args` and its index, skipping the
/// universal options before it, or `None` when there is none. In
/// `--isolation-dir x test //...`, it is `test`, at 2.
pub fn subcommand<S: AsRef<str>>(args: &[S]) -> Option<(&str, usize)> {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_ref();
        if !arg.starts_with('-') {
            return Some((arg, i));
        }
        if GLOBAL_OPTIONS_WITH_VALUE.contains(&arg) {
            i += 1; // its value
        }
        i += 1;
    }
    None
}

/// buck2's subcommands that don't read the build graph, which tk passes
/// through without syncing
pub const PASS_THROUGH_COMMANDS: &[&str] = &[
    "clean", "kill", "killall", "status", "log", "rage", "help", "docs", "init",
];

/// Whether tk syncs before running buck2's `subcommand` (`""` for none):
/// every one but the pass-through commands, unknown ones included (the safe
/// default)
pub fn needs_sync(subcommand: &str) -> bool {
    !PASS_THROUGH_COMMANDS.contains(&subcommand)
}

/// Rewrites `--isolation-dir` values so that buck2's isolation directories
/// are dot directories, which Go, Cargo and pytest ignore:
///
/// - `--isolation-dir=foo` becomes `--isolation-dir=.turnkey-foo`, and
///   `--isolation-dir foo` becomes `--isolation-dir .turnkey-foo`;
/// - a value that starts with a dot is left as it is;
/// - without `--isolation-dir`, buck2 uses `.turnkey`, from the
///   buckconfig.
///
/// The arguments after `--` are the target's or the test's, not buck2's,
/// and are left as they are.
pub fn transform_isolation_dir<S: AsRef<str>>(args: &[S]) -> Vec<String> {
    let mut result = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_ref();
        if arg == "--" {
            result.extend(args[i..].iter().map(|a| a.as_ref().to_string()));
            return result;
        }
        if let Some(value) = arg.strip_prefix("--isolation-dir=") {
            result.push(format!(
                "--isolation-dir={}",
                transform_isolation_dir_value(value)
            ));
        } else if arg == "--isolation-dir" && i + 1 < args.len() {
            result.push("--isolation-dir".to_string());
            result.push(transform_isolation_dir_value(args[i + 1].as_ref()));
            i += 1; // the value, consumed
        } else {
            result.push(arg.to_string());
        }
        i += 1;
    }
    result
}

/// An isolation directory's name, as tk passes it to buck2: unchanged when
/// it starts with a dot, else prefixed with `.turnkey-`
pub fn transform_isolation_dir_value(value: &str) -> String {
    if value.starts_with('.') {
        value.to_string()
    } else {
        format!(".turnkey-{value}")
    }
}

/// `args` with `inserted` right after the first `--`, before the arguments
/// already there, or after a `--` added at the end when there is none
pub fn insert_after_separator<S: AsRef<str>, T: AsRef<str>>(
    args: &[S],
    inserted: &[T],
) -> Vec<String> {
    let args = args.iter().map(|a| a.as_ref().to_string());
    let inserted = inserted.iter().map(|a| a.as_ref().to_string());
    let mut result: Vec<String> = args.collect();
    match result.iter().position(|a| a == "--") {
        Some(separator) => {
            let tail = result.split_off(separator + 1);
            result.extend(inserted);
            result.extend(tail);
        }
        None => {
            result.push("--".to_string());
            result.extend(inserted);
        }
    }
    result
}

/// A local override applied to a command line
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedOverride {
    /// buck2's subcommand: `run`, `build` or `test`
    pub subcommand: String,
    /// The target the override is for: the first `//` argument after the
    /// subcommand, before any `--`
    pub target: String,
    /// The override's arguments
    pub override_args: Vec<String>,
    /// The command line, with the override's arguments injected after `--`
    pub args: Vec<String>,
}

/// Applies the local override for the command line's target, if `config`
/// has one: for `run`, `build` and `test`, the override of the first
/// argument starting with `//` after the subcommand (before any `--`) is
/// injected right after `--`. `None` when nothing applies.
pub fn apply_local_overrides<S: AsRef<str>>(
    config: &LocalConfig,
    args: &[S],
) -> Option<AppliedOverride> {
    if args.is_empty() || !config.has_overrides() {
        return None;
    }
    let (subcommand, index) = subcommand(args)?;
    if !matches!(subcommand, "run" | "build" | "test") {
        return None;
    }
    let target = args[index + 1..]
        .iter()
        .map(AsRef::as_ref)
        .take_while(|arg| *arg != "--")
        .find(|arg| arg.starts_with("//"))?;
    let found = config.get_override(subcommand, target)?;
    if found.args.is_empty() {
        return None;
    }
    Some(AppliedOverride {
        subcommand: subcommand.to_string(),
        target: target.to_string(),
        override_args: found.args.clone(),
        args: insert_after_separator(args, &found.args),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command line, and its subcommand and the subcommand's index
    type SubcommandCase<'a> = (&'a [&'a str], Option<(&'a str, usize)>);

    #[test]
    fn subcommand_skips_the_universal_options() {
        let cases: &[SubcommandCase] = &[
            (&["test", "//..."], Some(("test", 0))),
            (
                &["--isolation-dir", "x", "test", "//..."],
                Some(("test", 2)),
            ),
            (&["--isolation-dir=x", "build"], Some(("build", 1))),
            (
                &["-v", "2", "--oncall", "me", "run", "//:bin"],
                Some(("run", 4)),
            ),
            (
                &["-v=2", "--client-metadata", "k=v", "test"],
                Some(("test", 3)),
            ),
            (
                &[
                    "--setting",
                    "a.b=c",
                    "--agent-context",
                    "intent=build",
                    "targets",
                ],
                Some(("targets", 4)),
            ),
            (&["--help"], None),
            (&["--isolation-dir", "test"], None),
            (&[], None),
        ];
        for (args, want) in cases {
            assert_eq!(subcommand(args), *want, "subcommand({args:?})");
        }
    }

    #[test]
    fn only_the_pass_through_commands_skip_sync() {
        for command in PASS_THROUGH_COMMANDS {
            assert!(!needs_sync(command), "{command}");
        }
        for command in [
            "build", "run", "test", "query", "targets", "bxl", "", "unknown",
        ] {
            assert!(needs_sync(command), "{command:?}");
        }
    }

    #[test]
    fn isolation_dir_values() {
        for (input, want) in [
            ("foo", ".turnkey-foo"),
            ("v2", ".turnkey-v2"),
            (".custom", ".custom"),
            (".turnkey", ".turnkey"),
            (".turnkey-foo", ".turnkey-foo"),
            ("", ".turnkey-"),
        ] {
            assert_eq!(transform_isolation_dir_value(input), want, "{input:?}");
        }
    }

    #[test]
    fn isolation_dir_flags() {
        let cases: &[(&[&str], &[&str])] = &[
            (&["build", "//foo:bar"], &["build", "//foo:bar"]),
            (
                &["--isolation-dir=test", "build", "//foo:bar"],
                &["--isolation-dir=.turnkey-test", "build", "//foo:bar"],
            ),
            (
                &["--isolation-dir=.custom", "build", "//foo:bar"],
                &["--isolation-dir=.custom", "build", "//foo:bar"],
            ),
            (
                &["--isolation-dir", "test", "build", "//foo:bar"],
                &["--isolation-dir", ".turnkey-test", "build", "//foo:bar"],
            ),
            (
                &["--isolation-dir", ".custom", "build", "//foo:bar"],
                &["--isolation-dir", ".custom", "build", "//foo:bar"],
            ),
            (
                &["build", "--isolation-dir=foo", "//target"],
                &["build", "--isolation-dir=.turnkey-foo", "//target"],
            ),
            (
                &["-v", "--isolation-dir=test", "build", "--show-output"],
                &[
                    "-v",
                    "--isolation-dir=.turnkey-test",
                    "build",
                    "--show-output",
                ],
            ),
            (
                &["build", "--isolation-dir=foo"],
                &["build", "--isolation-dir=.turnkey-foo"],
            ),
            (&["build", "--isolation-dir"], &["build", "--isolation-dir"]),
            (
                &[
                    "--isolation-dir=x",
                    "run",
                    "//tool",
                    "--",
                    "--isolation-dir=y",
                    "--isolation-dir",
                    "z",
                ],
                &[
                    "--isolation-dir=.turnkey-x",
                    "run",
                    "//tool",
                    "--",
                    "--isolation-dir=y",
                    "--isolation-dir",
                    "z",
                ],
            ),
        ];
        for (input, want) in cases {
            assert_eq!(transform_isolation_dir(input), *want, "{input:?}");
        }
    }

    #[test]
    fn inserts_after_the_first_separator() {
        let cases: &[(&[&str], &[&str])] = &[
            (&["run", "//:t"], &["run", "//:t", "--", "A", "B"]),
            (
                &["run", "//:t", "--", "x", "--", "y"],
                &["run", "//:t", "--", "A", "B", "x", "--", "y"],
            ),
            (&["--"], &["--", "A", "B"]),
        ];
        for (args, want) in cases {
            assert_eq!(insert_after_separator(args, &["A", "B"]), *want, "{args:?}");
        }
    }

    fn config(toml: &str) -> LocalConfig {
        LocalConfig::parse(toml.as_bytes()).unwrap()
    }

    #[test]
    fn applies_the_override_of_the_first_target() {
        let config = config(
            r#"
[run."//docs/user-manual"]
args = ["-n", "localhost"]

[test."//src/..."]
args = ["--verbose"]

[build."//empty:args"]
args = []
"#,
        );
        let applied = apply_local_overrides(
            &config,
            &["--isolation-dir", "x", "run", "//docs/user-manual"],
        )
        .unwrap();
        assert_eq!(applied.subcommand, "run");
        assert_eq!(applied.target, "//docs/user-manual");
        assert_eq!(applied.override_args, ["-n", "localhost"]);
        assert_eq!(
            applied.args,
            [
                "--isolation-dir",
                "x",
                "run",
                "//docs/user-manual",
                "--",
                "-n",
                "localhost"
            ]
        );

        let applied = apply_local_overrides(
            &config,
            &["test", "--flag", "//src/a:b", "//other", "--", "x"],
        )
        .unwrap();
        assert_eq!(
            applied.args,
            [
                "test",
                "--flag",
                "//src/a:b",
                "//other",
                "--",
                "--verbose",
                "x"
            ]
        );

        for args in [
            &["run", "//other:target"][..],
            &["targets", "//docs/user-manual"],
            &["run", "--", "//docs/user-manual"],
            &["build", "//empty:args"],
            &["run"],
            &["--help"],
            &[],
        ] {
            assert_eq!(apply_local_overrides(&config, args), None, "{args:?}");
        }
    }

    #[test]
    fn nothing_applies_without_overrides() {
        assert_eq!(
            apply_local_overrides(&config(""), &["run", "//docs/user-manual"]),
            None
        );
    }
}
