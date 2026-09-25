//! The executor side of buck2's test-runner protocol, at the buck2 release
//! turnkey pins (nix/buck2/buck2-source.nix).
//!
//! buck2 launches a `test.v2_test_executor` with two inherited sockets. On
//! one, buck2 is the client of our TestExecutor service and sends one spec
//! per test target, then says there are no more. On the other, it serves
//! TestOrchestrator, which runs tests and takes their results. [`start`]
//! sets up both and hands back the specs as a stream and a client for the
//! orchestrator; what to do with them is the runner's business.
//!
//! The protocol code is generated from buck2's own protos at the pinned
//! revision ([`proto`]); anything here that mirrors buck2 cites the buck2
//! source it follows.

mod executor;
pub mod proto;
mod transport;

use std::os::unix::io::RawFd;

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;
use tonic::transport::Channel;

use crate::proto::buck::test::ExternalRunnerSpec;
use crate::proto::buck::test::test_orchestrator_client::TestOrchestratorClient;

/// How buck2 launches the executor:
/// `<executor> --buck-trace-id <id> --config-entry host=<os> [--config-entry ...]
///  --executor-fd <fd> --orchestrator-fd <fd> -- ignored --buck-test-info ignored <runner args>`
/// (buck2 app/buck2_test/src/command.rs and unix/executor.rs at the pinned
/// release). Everything after `--` is the runner's own configuration, where
/// `tk test -- <args>` ends up.
#[derive(Debug, clap::Parser)]
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

/// A started executor.
pub struct Session {
    /// One spec per test target. The stream ends once buck2 has signalled the
    /// end of test requests.
    pub specs: UnboundedReceiver<ExternalRunnerSpec>,
    /// buck2's orchestrator: runs tests, takes their results.
    pub orchestrator: TestOrchestratorClient<Channel>,
    /// The TestExecutor server; shut it down once every result is reported.
    pub server: Server,
}

/// The running TestExecutor server.
pub struct Server(transport::ServerHandle);

impl Server {
    pub async fn shutdown(self) -> Result<()> {
        self.0.shutdown().await
    }
}

/// Serve TestExecutor and connect to the orchestrator, over the sockets
/// `launch` names.
///
/// # Safety
/// `launch.executor_fd` and `launch.orchestrator_fd` must be distinct open
/// Unix stream sockets that nothing else in the process owns. They are when
/// `launch` was parsed from the command line buck2 started this process with.
pub async unsafe fn start(launch: &Launch) -> Result<Session> {
    // SAFETY: guaranteed by the caller.
    let executor_io = unsafe { transport::inherited_socket(launch.executor_fd)? };
    let orchestrator_io = unsafe { transport::inherited_socket(launch.orchestrator_fd)? };

    let (spec_sender, specs) = tokio::sync::mpsc::unbounded_channel();
    let server = transport::serve(executor_io, executor::Executor::new(spec_sender));
    let orchestrator = TestOrchestratorClient::new(transport::channel(orchestrator_io).await?);
    Ok(Session {
        specs,
        orchestrator,
        server: Server(server),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

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
        assert_eq!(
            launch.runner_args,
            ["ignored", "--buck-test-info", "ignored", "--env", "A=1"]
        );
    }
}
