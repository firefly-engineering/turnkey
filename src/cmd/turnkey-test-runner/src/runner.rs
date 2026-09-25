//! Runs every test target buck2 hands over and reports its result.
//!
//! Behaviour matches buck2's bundled runner (app/buck2_test_runner/src/runner.rs
//! at the pinned release): same Execute2 request, same result mapping, same
//! details text, same exit code.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use futures_util::{StreamExt, TryStreamExt, stream};
use tokio::sync::mpsc::UnboundedReceiver;
use tonic::transport::Channel;

use crate::args::{Config, EnvValue};
use crate::cache::{Pass, Recorder};
use crate::proto::buck::data::command_execution_kind;
use crate::proto::buck::host_sharing::{
    HostSharingRequirements, WeightClass, host_sharing_requirements, weight_class,
};
use crate::proto::buck::test::test_orchestrator_client::TestOrchestratorClient;
use crate::proto::buck::test::{
    ArgValue, ArgValueContent, EndOfTestResultsRequest, EnvironmentVariable, ExecuteRequest2,
    ExecutionResult2, ExecutionStream, ExternalRunnerSpec, ExternalRunnerSpecValue,
    ReportTestResultRequest, TestExecutable, TestResult, TestStage, TestStatus, Testing,
    arg_value_content, execute_response2, execution_status, execution_stream,
    external_runner_spec_value, test_stage,
};

/// Exit code reported to buck2 when any test did not pass.
const FAILURE_EXIT_CODE: i32 = 32;

pub struct Runner {
    orchestrator: TestOrchestratorClient<Channel>,
    config: Config,
    recorder: Option<Recorder>,
}

impl Runner {
    pub fn new(
        orchestrator: TestOrchestratorClient<Channel>,
        config: Config,
        recorder: Option<Recorder>,
    ) -> Self {
        Self {
            orchestrator,
            config,
            recorder,
        }
    }

    /// Run every spec until buck2 signals the end of test requests, then
    /// report the overall exit code.
    pub async fn run_all(&self, specs: UnboundedReceiver<ExternalRunnerSpec>) -> Result<()> {
        let specs = stream::unfold(specs, |mut specs| async move {
            specs.recv().await.map(|spec| (spec, specs))
        });
        let all_passed = specs
            .map(|spec| self.run_one(spec))
            // buck2 throttles execution itself, so don't hold requests back.
            .buffer_unordered(10000)
            .try_fold(true, |all_passed, status| async move {
                Ok(all_passed && status == TestStatus::Pass)
            })
            .await?;
        let exit_code = if all_passed { 0 } else { FAILURE_EXIT_CODE };
        self.orchestrator
            .clone()
            .end_of_test_results(EndOfTestResultsRequest { exit_code })
            .await
            .context("reporting the end of test results")?;
        Ok(())
    }

    async fn run_one(&self, spec: ExternalRunnerSpec) -> Result<TestStatus> {
        let target = spec.target.clone().context("spec without a target")?;
        let name = format!("{}//{}:{}", target.cell, target.package, target.target);
        let handle = target.handle.context("spec target without a handle")?;

        let response = self
            .orchestrator
            .clone()
            .execute2(self.execute_request(spec)?)
            .await
            .context("Test execution request failed")?
            .into_inner();
        let result = match response
            .response
            .context("execute response without a result")?
        {
            execute_response2::Response::Result(result) => result,
            // Cancelled tests are not reported.
            execute_response2::Response::Cancelled(_) => return Ok(TestStatus::Omitted),
        };

        if let Some(recorder) = &self.recorder {
            record_if_pass(recorder, &name, &result).await;
        }

        let result = test_result(name, handle, result)?;
        let status = result.status();
        self.orchestrator
            .clone()
            .report_test_result(ReportTestResultRequest {
                result: Some(result),
            })
            .await
            .context("Test result reporting failed")?;
        Ok(status)
    }

    fn execute_request(&self, spec: ExternalRunnerSpec) -> Result<ExecuteRequest2> {
        let target = spec.target.context("spec without a target")?;

        let cmd = spec
            .command
            .into_iter()
            .chain(self.config.test_arg.iter().map(|arg| verbatim(arg)))
            .map(spec_arg)
            .collect();

        // Sorted by name, later values winning: the spec's env, then `--env`.
        // This is the order buck2's runner produces, and the env is part of the
        // action digest.
        let mut env: BTreeMap<String, ExternalRunnerSpecValue> = spec.env.into_iter().collect();
        for value in &self.config.env {
            let EnvValue { name, value } = value.parse()?;
            env.insert(name, verbatim(&value));
        }
        let env = env
            .into_iter()
            .map(|(key, value)| EnvironmentVariable {
                key,
                value: Some(spec_arg(value)),
            })
            .collect();

        Ok(ExecuteRequest2 {
            timeout: Some(prost_types::Duration {
                seconds: i64::try_from(self.config.timeout).context("timeout out of range")?,
                nanos: 0,
            }),
            host_sharing_requirements: Some(HostSharingRequirements {
                requirements: Some(host_sharing_requirements::Requirements::Shared(
                    host_sharing_requirements::Shared {
                        weight_class: Some(WeightClass {
                            value: Some(weight_class::Value::Permits(1)),
                        }),
                    },
                )),
            }),
            test_executable: Some(TestExecutable {
                stage: Some(TestStage {
                    item: Some(test_stage::Item::Testing(Testing {
                        suite: target.target,
                        testcases: Vec::new(),
                        variant: None,
                        repeat_count: None,
                    })),
                }),
                target: target.handle,
                cmd,
                pre_create_dirs: Vec::new(),
                env,
            }),
            executor_override: None,
            required_local_resources: Vec::new(),
            // buck2 reads recorded results only when the runner allows it.
            disable_test_execution_caching: !self.config.turnkey_test_cache.reads(),
        })
    }
}

/// Record a passing local run, so the next run with the same result key is a
/// hit. Anything the runner can't reproduce in full is not recorded: failures,
/// runs that were themselves hits, and tests that declare outputs. Recording
/// problems never fail the test.
async fn record_if_pass(recorder: &Recorder, name: &str, result: &ExecutionResult2) {
    let passed = matches!(
        result.status.as_ref().and_then(|s| s.status),
        Some(execution_status::Status::Finished(0))
    );
    let local_digest = result
        .execution_details
        .as_ref()
        .and_then(|d| d.execution_kind.as_ref())
        .and_then(|k| k.command.as_ref())
        .and_then(|c| match c {
            command_execution_kind::Command::LocalCommand(local) => {
                Some(local.action_digest.as_str())
            }
            _ => None,
        });
    let (Some(action_digest), true, true) = (local_digest, passed, result.outputs.is_empty())
    else {
        return;
    };
    let pass = Pass {
        action_digest,
        stdout: stream_bytes(&result.stdout),
        stderr: stream_bytes(&result.stderr),
        start_time: duration(result.start_time.as_ref()),
        execution_time: duration(result.execution_time.as_ref()),
    };
    if let Err(e) = recorder.record(pass).await {
        eprintln!("turnkey-test-runner: not recording {name}: {e:#}");
    }
}

/// First line of a hit's details.
pub const HIT_MARKER: &str = "recorded: reused the result of an earlier run with the same inputs\n";

/// Whether buck2 served this result from the cache instead of running the test.
fn is_hit(result: &ExecutionResult2) -> bool {
    matches!(
        result
            .execution_details
            .as_ref()
            .and_then(|d| d.execution_kind.as_ref())
            .and_then(|k| k.command.as_ref()),
        Some(command_execution_kind::Command::RemoteCommand(remote)) if remote.cache_hit
    )
}

fn duration(d: Option<&prost_types::Duration>) -> std::time::Duration {
    d.and_then(|d| std::time::Duration::try_from(*d).ok())
        .unwrap_or_default()
}

fn stream_bytes(stream: &Option<ExecutionStream>) -> &[u8] {
    match stream.as_ref().and_then(|s| s.item.as_ref()) {
        Some(execution_stream::Item::Inline(bytes)) => bytes,
        None => &[],
    }
}

fn verbatim(value: &str) -> ExternalRunnerSpecValue {
    ExternalRunnerSpecValue {
        value: Some(external_runner_spec_value::Value::Verbatim(
            value.to_owned(),
        )),
    }
}

fn spec_arg(value: ExternalRunnerSpecValue) -> ArgValue {
    ArgValue {
        content: Some(ArgValueContent {
            value: Some(arg_value_content::Value::SpecValue(value)),
        }),
        format: None,
    }
}

fn stream_text(stream: Option<ExecutionStream>) -> String {
    match stream.and_then(|s| s.item) {
        Some(execution_stream::Item::Inline(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        None => String::new(),
    }
}

fn test_result(
    name: String,
    target: crate::proto::buck::test::ConfiguredTargetHandle,
    result: ExecutionResult2,
) -> Result<TestResult> {
    let status = match result
        .status
        .and_then(|s| s.status)
        .context("execution result without a status")?
    {
        execution_status::Status::Finished(0) => TestStatus::Pass,
        execution_status::Status::Finished(_) => TestStatus::Fail,
        execution_status::Status::TimedOut(_) => TestStatus::Timeout,
    };
    // A hit is marked in the details, the part of a result buck2 prints
    // under the test's line.
    let marker = if is_hit(&result) { HIT_MARKER } else { "" };
    let details = format!(
        "{marker}---- STDOUT ----\n{}\n---- STDERR ----\n{}\n",
        stream_text(result.stdout),
        stream_text(result.stderr)
    );
    Ok(TestResult {
        name,
        status: status as i32,
        msg: None,
        target: Some(target),
        duration: result.execution_time,
        details,
        max_memory_used_bytes: result.max_memory_used_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::buck::data::{CommandExecutionKind, LocalCommand, RemoteCommand};
    use crate::proto::buck::test::{ConfiguredTargetHandle, ExecutionDetails, ExecutionStatus};

    fn pass_with(command: command_execution_kind::Command) -> ExecutionResult2 {
        ExecutionResult2 {
            status: Some(ExecutionStatus {
                status: Some(execution_status::Status::Finished(0)),
            }),
            stdout: Some(ExecutionStream {
                item: Some(execution_stream::Item::Inline(b"out".to_vec())),
            }),
            execution_details: Some(ExecutionDetails {
                execution_kind: Some(CommandExecutionKind {
                    command: Some(command),
                }),
            }),
            ..Default::default()
        }
    }

    fn details(result: ExecutionResult2) -> String {
        test_result("t".into(), ConfiguredTargetHandle { id: 1 }, result)
            .unwrap()
            .details
    }

    #[test]
    fn hits_are_marked_recorded() {
        let hit = pass_with(command_execution_kind::Command::RemoteCommand(
            RemoteCommand {
                cache_hit: true,
                ..Default::default()
            },
        ));
        assert!(details(hit).starts_with("recorded: "));
    }

    #[test]
    fn fresh_runs_are_not_marked() {
        let local = pass_with(command_execution_kind::Command::LocalCommand(
            LocalCommand::default(),
        ));
        assert_eq!(
            details(local),
            "---- STDOUT ----\nout\n---- STDERR ----\n\n"
        );
    }
}
