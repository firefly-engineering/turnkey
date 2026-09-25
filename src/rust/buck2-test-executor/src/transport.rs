//! gRPC over the two Unix sockets buck2 passes to the executor.
//!
//! Each socket carries exactly one HTTP/2 connection: buck2 is the client of
//! our TestExecutor service on one, and the server of its TestOrchestrator
//! service on the other. Neither side ever reconnects. Mirrors buck2's
//! buck2_grpc crate (channel.rs and server.rs at the pinned release).

use std::os::unix::io::{FromRawFd, RawFd};
use std::os::unix::net::UnixStream as StdUnixStream;

use anyhow::{Context, Result, anyhow};
use futures_util::{StreamExt, future, stream};
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tonic::transport::{Channel, Endpoint, Server, Uri};
use tower::service_fn;

use crate::proto::buck::test::test_executor_server::{TestExecutor, TestExecutorServer};

/// Take ownership of a socket file descriptor inherited from buck2.
///
/// # Safety
/// `fd` must be an open Unix stream socket that nothing else owns.
pub unsafe fn inherited_socket(fd: RawFd) -> Result<UnixStream> {
    // SAFETY: guaranteed by the caller.
    let std = unsafe { StdUnixStream::from_raw_fd(fd) };
    std.set_nonblocking(true)
        .with_context(|| format!("setting fd {fd} non-blocking"))?;
    UnixStream::from_std(std).with_context(|| format!("adopting fd {fd}"))
}

/// A gRPC channel over an already-connected socket.
pub async fn channel(io: UnixStream) -> Result<Channel> {
    let mut io = Some(hyper_util::rt::TokioIo::new(io));
    // The URI only fills in request headers; the connection already exists.
    Endpoint::try_from("http://orchestrator.invalid")?
        .connect_with_connector(service_fn(move |_: Uri| {
            future::ready(
                io.take()
                    .ok_or_else(|| "cannot reconnect after connection loss".to_owned()),
            )
        }))
        .await
        .context("connecting to the test orchestrator")
}

/// A running TestExecutor server; shut it down once all results are reported.
pub struct ServerHandle {
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<Result<()>>,
}

impl ServerHandle {
    pub async fn shutdown(self) -> Result<()> {
        let _ = self.shutdown.send(());
        self.task.await.context("joining the executor server")?
    }
}

/// Serve `executor` on an already-connected socket.
pub fn serve<T: TestExecutor>(io: UnixStream, executor: T) -> ServerHandle {
    let (shutdown, shutdown_rx) = oneshot::channel();
    // Yield the one connection, then never end: the server must not exit just
    // because no new connections arrive.
    let incoming =
        stream::once(future::ready(Ok::<_, std::io::Error>(io))).chain(stream::pending());
    let task = tokio::spawn(async move {
        Server::builder()
            .add_service(TestExecutorServer::new(executor))
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(|e| anyhow!("executor server failed: {e}"))
    });
    ServerHandle { shutdown, task }
}
