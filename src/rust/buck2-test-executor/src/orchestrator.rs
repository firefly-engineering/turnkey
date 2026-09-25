//! The calls a runner makes to buck2's test orchestrator, as a port, so a
//! runner's tests can stand in for buck2. buck2's TestOrchestrator client is
//! the production adapter.

use anyhow::{Context, Result};
use tonic::transport::Channel;

use crate::proto::buck::test::test_orchestrator_client::TestOrchestratorClient;
use crate::proto::buck::test::{
    EndOfTestResultsRequest, ExecuteRequest2, ExecuteResponse2, ReportTestResultRequest, TestResult,
};

pub trait Orchestrator {
    /// Have buck2 run (or reuse) one test.
    fn execute(
        &self,
        request: ExecuteRequest2,
    ) -> impl Future<Output = Result<ExecuteResponse2>> + Send;
    /// Report one test's result.
    fn report(&self, result: TestResult) -> impl Future<Output = Result<()>> + Send;
    /// Say every result is reported, with the run's exit code.
    fn end(&self, exit_code: i32) -> impl Future<Output = Result<()>> + Send;
}

impl Orchestrator for TestOrchestratorClient<Channel> {
    async fn execute(&self, request: ExecuteRequest2) -> Result<ExecuteResponse2> {
        Ok(self
            .clone()
            .execute2(request)
            .await
            .context("Test execution request failed")?
            .into_inner())
    }

    async fn report(&self, result: TestResult) -> Result<()> {
        self.clone()
            .report_test_result(ReportTestResultRequest {
                result: Some(result),
            })
            .await
            .context("Test result reporting failed")?;
        Ok(())
    }

    async fn end(&self, exit_code: i32) -> Result<()> {
        self.clone()
            .end_of_test_results(EndOfTestResultsRequest { exit_code })
            .await
            .context("reporting the end of test results")?;
        Ok(())
    }
}
