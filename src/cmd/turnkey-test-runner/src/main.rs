//! turnkey-test-runner: turnkey's buck2 test runner.
//!
//! Speaks buck2's test-runner protocol, pinned to the buck2 release turnkey
//! ships, and runs tests exactly as buck2's bundled runner does. Under
//! `tk test` it also lets buck2 reuse recorded results and records fresh
//! passes (docs/specs/test-result-caching.md).

mod args;
mod cache;
mod executor;
#[allow(dead_code)]
mod proto;
mod runner;
mod transport;

use anyhow::{Context, Result};
use clap::Parser;

use crate::args::{Config, Launch};
use crate::proto::buck::test::test_orchestrator_client::TestOrchestratorClient;

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

    // SAFETY: buck2 passes two distinct socket fds that only we own.
    let executor_io = unsafe { transport::inherited_socket(launch.executor_fd)? };
    let orchestrator_io = unsafe { transport::inherited_socket(launch.orchestrator_fd)? };

    let (spec_sender, specs) = tokio::sync::mpsc::unbounded_channel();
    let server = transport::serve(executor_io, executor::Executor::new(spec_sender));
    let orchestrator = TestOrchestratorClient::new(transport::channel(orchestrator_io).await?);

    let recorder = if config.turnkey_test_cache.records() {
        let address = config
            .turnkey_test_cache_address
            .as_deref()
            .context("--turnkey-test-cache needs --turnkey-test-cache-address")?;
        Some(cache::Recorder::new(
            address,
            config.turnkey_test_cache_instance_name.clone(),
        )?)
    } else {
        None
    };

    runner::Runner::new(orchestrator, config, recorder)
        .run_all(specs)
        .await?;
    server.shutdown().await
}
