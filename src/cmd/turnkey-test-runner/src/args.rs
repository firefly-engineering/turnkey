//! The runner's own arguments: what buck2 passes after `--` in the
//! executor's command line (buck2_test_executor::Launch), where
//! `tk test -- <args>` ends up.

use std::str::FromStr;

use anyhow::{Result, anyhow};
use clap::Parser;

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

    /// turnkey: where the test result cache lives, for reporting hits.
    /// `tk test` knows it from the dev shell.
    #[clap(long, value_enum, default_value = "local")]
    pub turnkey_test_cache_origin: crate::runner::Origin,

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

    /// What `tk test` passes and reads back, shared with tk's tests. buck2
    /// hands the file to the test (rules.star); a Cargo build finds it in
    /// the source tree.
    fn runner_contract() -> serde_json::Value {
        let path = std::env::var("TURNKEY_RUNNER_CONTRACT").unwrap_or_else(|_| {
            let manifest_dir = option_env!("CARGO_MANIFEST_DIR")
                .expect("TURNKEY_RUNNER_CONTRACT is unset outside a Cargo build");
            format!("{manifest_dir}/../../go/pkg/testcache/testdata/runner-contract.json")
        });
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn parses_what_tk_passes() {
        use clap::ValueEnum;
        let contract = runner_contract();
        for plan in contract["plans"].as_array().unwrap() {
            let args = plan["args"].as_array().unwrap().iter();
            let config = Config::try_parse_from(
                ["ignored", "--buck-test-info", "ignored"]
                    .into_iter()
                    .chain(args.map(|arg| arg.as_str().unwrap())),
            )
            .unwrap();
            let mode = config.turnkey_test_cache.to_possible_value().unwrap();
            let origin = config
                .turnkey_test_cache_origin
                .to_possible_value()
                .unwrap();
            assert_eq!(mode.get_name(), plan["mode"]);
            assert_eq!(origin.get_name(), plan["origin"]);
            assert_eq!(
                config.turnkey_test_cache_address.as_deref(),
                contract["address"].as_str()
            );
            assert_eq!(
                config.turnkey_test_cache_report.as_deref(),
                contract["report"].as_str().map(std::path::Path::new)
            );
        }
    }

    #[test]
    fn writes_the_report_tk_reads() {
        let contract = runner_contract();
        let hits = contract["hits"].as_u64().unwrap() as usize;
        assert_eq!(crate::runner::hits_report(hits), contract["hits_report"]);
    }

    #[test]
    fn runner_args_as_buck2_sends_them() {
        let config =
            Config::try_parse_from(["ignored", "--buck-test-info", "ignored", "--env", "A=1"])
                .unwrap();
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
