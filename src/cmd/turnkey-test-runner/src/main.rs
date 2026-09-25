//! turnkey-test-runner: turnkey's buck2 test runner.
//!
//! Speaks buck2's test-runner protocol (through buck2-test-executor, pinned
//! to the buck2 release turnkey ships), and runs tests exactly as buck2's
//! bundled runner does. Under `tk test` it also lets buck2 reuse recorded
//! results and records fresh passes (docs/specs/test-result-caching.md).

mod args;
mod cache;
mod runner;

use anyhow::{Context, Result};
use buck2_test_executor::Launch;
use clap::Parser;

use crate::args::Config;

#[tokio::main]
async fn main() {
    if let Err(e) = run(Launch::parse()).await {
        eprintln!("turnkey-test-runner: {e:#}");
        std::process::exit(1);
    }
}

async fn run(launch: Launch) -> Result<()> {
    let config = Config::try_parse_from(&launch.runner_args)
        .context("Error parsing test runner arguments")?;

    // SAFETY: `launch` comes from the command line buck2 started us with, so
    // its two socket fds are distinct and only we own them.
    let session = unsafe { buck2_test_executor::start(&launch).await? };

    let address = config.turnkey_test_cache_address.as_deref();
    // Results are recorded only in a local cache: who may write to a shared
    // one isn't decided (docs/specs/test-result-caching.md).
    let local = address.is_some_and(|a| runner::Origin::of(a) == runner::Origin::Local);
    let recorder = if config.turnkey_test_cache.records() && local {
        let address = address.context("--turnkey-test-cache needs --turnkey-test-cache-address")?;
        Some(cache::Recorder::new(
            address,
            config.turnkey_test_cache_instance_name.clone(),
        )?)
    } else {
        None
    };

    runner::Runner::new(session.orchestrator, config, recorder)
        .run_all(session.specs)
        .await?;
    session.server.shutdown().await
}
