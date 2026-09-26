//! turnkey-test-runner: turnkey's buck2 test runner.
//!
//! Speaks buck2's test-runner protocol (through buck2-test-executor, pinned
//! to the buck2 release turnkey ships), and runs tests exactly as buck2's
//! bundled runner does. Unlike it, it prints a passing test's output only
//! with `-- --print-passing-details`. Under `tk test` it also lets buck2
//! reuse recorded results and records fresh passes
//! (docs/specs/test-result-caching.md).

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

    // Whether to record at all is tk's decision (the mode); it never asks
    // for recording into a shared cache.
    let recorder = if config.turnkey_test_cache.records() {
        let address = config
            .turnkey_test_cache_address
            .as_deref()
            .context("--turnkey-test-cache needs --turnkey-test-cache-address")?;
        Some(cache::Recorder::connect(address)?)
    } else {
        None
    };

    runner::Runner::new(session.orchestrator, config, recorder)
        .run_all(session.specs)
        .await?;
    session.server.shutdown().await
}
