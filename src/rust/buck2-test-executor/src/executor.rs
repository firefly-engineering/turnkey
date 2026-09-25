//! The TestExecutor service: buck2 sends one spec per test target, then says
//! there are no more.

use std::sync::Mutex;

use tokio::sync::mpsc::UnboundedSender;
use tonic::{Request, Response, Status};

use crate::proto::buck::test::test_executor_server::TestExecutor;
use crate::proto::buck::test::{
    Empty, ExternalRunnerSpec, ExternalRunnerSpecRequest, UnstableHeapDumpRequest,
    UnstableHeapDumpResponse,
};

pub struct Executor {
    specs: Mutex<Option<UnboundedSender<ExternalRunnerSpec>>>,
}

impl Executor {
    pub fn new(specs: UnboundedSender<ExternalRunnerSpec>) -> Self {
        Self {
            specs: Mutex::new(Some(specs)),
        }
    }
}

#[tonic::async_trait]
impl TestExecutor for Executor {
    async fn external_runner_spec(
        &self,
        request: Request<ExternalRunnerSpecRequest>,
    ) -> Result<Response<Empty>, Status> {
        let spec = request
            .into_inner()
            .test_spec
            .ok_or_else(|| Status::invalid_argument("missing `test_spec`"))?;
        let specs = self.specs.lock().expect("spec sender lock");
        let sender = specs.as_ref().ok_or_else(|| {
            Status::failed_precondition("spec received after end of test requests")
        })?;
        sender
            .send(spec)
            .map_err(|_| Status::internal("test runner stopped"))?;
        Ok(Response::new(Empty {}))
    }

    async fn end_of_test_requests(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Empty>, Status> {
        // Dropping the sender lets the runner finish once in-flight tests do.
        self.specs.lock().expect("spec sender lock").take();
        Ok(Response::new(Empty {}))
    }

    async fn unstable_heap_dump(
        &self,
        _request: Request<UnstableHeapDumpRequest>,
    ) -> Result<Response<UnstableHeapDumpResponse>, Status> {
        Err(Status::unimplemented("heap dumps are not supported"))
    }
}
