//! Command-line arguments.
//!
//! buck2 launches a `test.v2_test_executor` as
//! `<executor> --buck-trace-id <id> --config-entry host=<os> [--config-entry ...]
//!  --executor-fd <fd> --orchestrator-fd <fd> -- ignored --buck-test-info ignored <runner args>`
//! (buck2 app/buck2_test/src/command.rs and unix/executor.rs at the pinned
//! release). Everything after `--` is the runner's own configuration, where
//! `tk test -- <args>` ends up.

use std::os::unix::io::RawFd;
use std::str::FromStr;

use anyhow::{Result, anyhow};
use clap::Parser;

/// How buck2 launches the executor.
#[derive(Debug, Parser)]
pub struct Launch {
    #[clap(long)]
    pub buck_trace_id: Option<String>,

    #[clap(long)]
    pub config_entry: Vec<String>,

    #[clap(long)]
    pub executor_fd: RawFd,

    #[clap(long)]
    pub orchestrator_fd: RawFd,

    /// The runner's own arguments, starting with a placeholder program name.
    #[clap(last = true)]
    pub runner_args: Vec<String>,
}

/// The runner's configuration. Mirrors buck2's bundled runner
/// (app/buck2_test_runner/src/config.rs), so `-- --env`, `-- --timeout` and
/// `-- --test-arg` behave the same.
#[derive(Debug, Parser)]
pub struct Config {
    /// add a list of environment variables using format: --env VAR1=Value1 VAR2='Value 2'
    #[clap(long)]
    pub env: Vec<String>,

    /// Max number of seconds allowed to run a test.
    #[clap(long, default_value = "600")]
    pub timeout: u64,

    /// Ignored arg included for backwards compatibility.
    #[clap(long, hide = true)]
    buck_test_info: String,

    /// turnkey: whether to reuse and record test results. `tk test` sets
    /// this; buck2 called directly leaves it off.
    #[clap(long, value_enum, default_value = "off")]
    pub turnkey_test_cache: crate::cache::Mode,

    /// turnkey: the test result cache (`grpc://host:port`).
    #[clap(long)]
    pub turnkey_test_cache_address: Option<String>,

    /// turnkey: REAPI instance name, as in `buck2_re_client.instance_name`.
    #[clap(long, default_value = "")]
    pub turnkey_test_cache_instance_name: String,

    /// turnkey: file to write the number of hits to once all tests are done,
    /// for `tk test`'s summary.
    #[clap(long)]
    pub turnkey_test_cache_report: Option<std::path::PathBuf>,

    /// Passthrough argments to test binary.
    /// Available as a workaround for when test features are available.
    #[clap(long, num_args=1.., allow_hyphen_values = true)]
    pub test_arg: Vec<String>,
}

/// An `--env NAME=VALUE` argument.
#[derive(Debug, PartialEq, Clone)]
pub struct EnvValue {
    pub name: String,
    pub value: String,
}

impl FromStr for EnvValue {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self> {
        match input.split_once('=') {
            Some((name, value)) => Ok(EnvValue {
                name: name.to_owned(),
                value: value.to_owned(),
            }),
            None => Err(anyhow!(
                "Incorrect syntax for env value. Please use name=value. Input: `{input}`"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_args_as_buck2_sends_them() {
        let launch = Launch::try_parse_from([
            "turnkey-test-runner",
            "--buck-trace-id",
            "abc",
            "--config-entry",
            "host=mac",
            "--executor-fd",
            "3",
            "--orchestrator-fd",
            "4",
            "--",
            "ignored",
            "--buck-test-info",
            "ignored",
            "--env",
            "A=1",
        ])
        .unwrap();
        assert_eq!((launch.executor_fd, launch.orchestrator_fd), (3, 4));
        let config = Config::try_parse_from(&launch.runner_args).unwrap();
        assert_eq!(config.env, vec!["A=1"]);
        assert_eq!(config.timeout, 600);
        assert_eq!(config.turnkey_test_cache, crate::cache::Mode::Off);
    }

    #[test]
    fn test_arg_takes_everything_after_it() {
        // As in buck2's runner: --test-arg accepts hyphenated values and keeps
        // consuming, so runner flags must come before it.
        let config = Config::try_parse_from([
            "ignored",
            "--buck-test-info",
            "ignored",
            "--timeout",
            "5",
            "--test-arg",
            "-test.v",
            "--timeout",
            "7",
        ])
        .unwrap();
        assert_eq!(config.test_arg, vec!["-test.v", "--timeout", "7"]);
        assert_eq!(config.timeout, 5);
    }

    #[test]
    fn env_value_requires_equals() {
        assert_eq!(
            "A=b=c".parse::<EnvValue>().unwrap(),
            EnvValue {
                name: "A".into(),
                value: "b=c".into()
            }
        );
        assert!("A".parse::<EnvValue>().is_err());
    }
}
