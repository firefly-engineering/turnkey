//! The runner's own arguments: what buck2 passes after `--` in the
//! executor's command line (buck2_test_executor::Launch), where
//! `tk test -- <args>` ends up.

use clap::Parser;

/// The runner's configuration: buck2's bundled runner's arguments, so
/// `-- --env`, `-- --timeout` and `-- --test-arg` behave the same, and
/// turnkey's.
#[derive(Debug, Parser)]
pub struct Config {
    #[clap(flatten)]
    pub bundled: buck2_test_executor::bundled::Args,

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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `tk test` passes and reads back, shared with tk's tests. buck2
    /// hands the file to the test (rules.star); a Cargo build finds it in
    /// the source tree.
    fn runner_contract() -> serde_json::Value {
        let path = std::env::var("TURNKEY_RUNNER_CONTRACT").unwrap_or_else(|_| {
            let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
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
    fn reads_the_label_the_test_caching_helper_sets() {
        let labels = &runner_contract()["labels"];
        assert_eq!(crate::runner::CACHEABLE_LABEL, labels["cacheable"]);
    }

    #[test]
    fn writes_the_report_tk_reads() {
        let contract = runner_contract();
        let hits = contract["hits"].as_u64().unwrap() as usize;
        assert_eq!(crate::runner::hits_report(hits), contract["hits_report"]);
    }

    #[test]
    fn runner_args_as_buck2_sends_them() {
        // The bundled runner's flags and turnkey's, together, with
        // --test-arg last: it consumes everything after it.
        let config = Config::try_parse_from([
            "ignored",
            "--buck-test-info",
            "ignored",
            "--env",
            "A=1",
            "--turnkey-test-cache=on",
            "--test-arg",
            "--turnkey-test-cache=off",
        ])
        .unwrap();
        assert_eq!(config.bundled.env, vec!["A=1"]);
        assert_eq!(config.bundled.timeout, 600);
        assert_eq!(config.bundled.test_arg, vec!["--turnkey-test-cache=off"]);
        assert_eq!(config.turnkey_test_cache, crate::cache::Mode::On);
    }
}
