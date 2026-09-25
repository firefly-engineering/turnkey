//! Runs every test target buck2 hands over and reports its result.
//!
//! Each test is run and reported as buck2's bundled runner would
//! (buck2_test_executor::bundled); on top, the runner applies the mode tk
//! chose, marks hits, and records passes.

use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use futures_util::{StreamExt, TryStreamExt, stream};
use tokio::sync::mpsc::UnboundedReceiver;

use buck2_test_executor::Orchestrator;
use buck2_test_executor::bundled;
use buck2_test_executor::proto::buck::data::command_execution_kind;
use buck2_test_executor::proto::buck::test::{
    ConfiguredTargetHandle, ExecutionResult2, ExecutionStream, ExternalRunnerSpec, TestResult,
    TestStatus, execute_response2, execution_status, execution_stream, test_result,
};

use crate::args::Config;
use crate::cache::{ActionCache, Pass, Recorder};

/// Label the test-caching helper gives every target it caches: the targets of
/// cache-safe rules, minus those labelled `no-test-cache`. Only such targets
/// are recorded: buck2 reports an action digest for every local run,
/// cacheable or not, so the label is the only way to tell.
pub const CACHEABLE_LABEL: &str = "turnkey-cacheable";

pub struct Runner<O, C> {
    orchestrator: O,
    config: Config,
    recorder: Option<Recorder<C>>,
    /// Tests whose result buck2 reused instead of running them.
    hits: AtomicUsize,
    /// Where the cache in use lives, for reporting hits.
    origin: Origin,
}

impl<O: Orchestrator, C: ActionCache> Runner<O, C> {
    pub fn new(orchestrator: O, config: Config, recorder: Option<Recorder<C>>) -> Self {
        let origin = config.turnkey_test_cache_origin;
        Self {
            orchestrator,
            config,
            origin,
            recorder,
            hits: AtomicUsize::new(0),
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
        let exit_code = if all_passed {
            0
        } else {
            bundled::FAILURE_EXIT_CODE
        };
        if let Some(report) = &self.config.turnkey_test_cache_report {
            let hits = self.hits.load(Ordering::Relaxed);
            std::fs::write(report, hits_report(hits))
                .with_context(|| format!("writing {}", report.display()))?;
        }
        self.orchestrator.end(exit_code).await
    }

    async fn run_one(&self, spec: ExternalRunnerSpec) -> Result<TestStatus> {
        let target = spec.target.clone().context("spec without a target")?;
        let name = format!("{}//{}:{}", target.cell, target.package, target.target);
        let handle = target.handle.context("spec target without a handle")?;
        let mode = self.config.turnkey_test_cache;
        let cacheable = spec.labels.iter().any(|label| label == CACHEABLE_LABEL);

        let response = self
            .orchestrator
            // buck2 reads recorded results only when the mode allows it.
            .execute(bundled::execute_request(
                spec,
                &self.config.bundled,
                !mode.reads(),
            )?)
            .await?;
        let result = match response
            .response
            .context("execute response without a result")?
        {
            execute_response2::Response::Result(result) => result,
            // Cancelled tests are not reported.
            execute_response2::Response::Cancelled(_) => return Ok(TestStatus::Omitted),
        };

        if is_hit(&result) {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
        if let (Some(recorder), true) = (&self.recorder, mode.records() && cacheable) {
            record_if_pass(recorder, &name, &result).await;
        }

        let result = test_result(name, handle, result, self.origin)?;
        let status = result.status();
        self.orchestrator.report(result).await?;
        Ok(status)
    }
}

/// Record a passing local run, so the next run with the same result key is a
/// hit. Anything the runner can't reproduce in full is not recorded: failures,
/// runs that were themselves hits, and tests that declare outputs. Recording
/// problems never fail the test.
async fn record_if_pass<C: ActionCache>(
    recorder: &Recorder<C>,
    name: &str,
    result: &ExecutionResult2,
) {
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

/// The report `tk test` reads the number of hits from
/// (src/go/pkg/testcache/testdata/runner-contract.json).
pub fn hits_report(hits: usize) -> String {
    format!("{hits}\n")
}

/// Where the test result cache in use lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Origin {
    /// This machine's cache.
    Local,
    /// A shared cache elsewhere.
    Remote,
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Origin::Local => "local",
            Origin::Remote => "remote",
        }
    }
}

/// First line of a hit's details.
fn hit_marker(origin: Origin) -> &'static str {
    match origin {
        Origin::Local => "recorded: reused the result of an earlier run with the same inputs\n",
        Origin::Remote => {
            "recorded, remote: reused the result of an earlier run with the same inputs\n"
        }
    }
}

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

/// The bundled runner's result for one test, with a hit marked as such. The
/// marker goes in the details, the part of a result buck2 prints under the
/// test's line. The hit's provenance goes in `msg`, which buck2 keeps in the
/// event log but doesn't print, and its duration (the original run's) moves
/// there too: the console shows no duration for a test that didn't run.
fn test_result(
    name: String,
    target: ConfiguredTargetHandle,
    result: ExecutionResult2,
    origin: Origin,
) -> Result<TestResult> {
    let hit = is_hit(&result);
    let original_duration = duration(result.execution_time.as_ref());
    let mut reported = bundled::test_result(name, target, result)?;
    if hit {
        let provenance = serde_json::json!({
            "turnkey_test_cache": {
                "hit": true,
                "origin": origin.as_str(),
                "original_duration_us": original_duration.as_micros() as u64,
            }
        });
        reported.details = format!("{}{}", hit_marker(origin), reported.details);
        reported.msg = Some(test_result::OptionalMsg {
            msg: provenance.to_string(),
        });
        reported.duration = None;
    }
    Ok(reported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use buck2_test_executor::proto::buck::data::{
        CommandExecutionKind, LocalCommand, RemoteCommand,
    };
    use buck2_test_executor::proto::buck::test::{
        ConfiguredTarget, ExecutionDetails, ExecutionStatus,
    };
    use buck2_test_executor::proto::buck::test::{
        ExecuteRequest2, ExecuteResponse2, TestStage, test_stage,
    };
    use buck2_test_executor::proto::build::bazel::remote::execution::v2::UpdateActionResultRequest;

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
        test_result(
            "t".into(),
            ConfiguredTargetHandle { id: 1 },
            result,
            Origin::Local,
        )
        .unwrap()
        .details
    }

    fn hit() -> ExecutionResult2 {
        let mut result = pass_with(command_execution_kind::Command::RemoteCommand(
            RemoteCommand {
                cache_hit: true,
                ..Default::default()
            },
        ));
        result.execution_time = Some(prost_types::Duration {
            seconds: 1,
            nanos: 500_000_000,
        });
        result
    }

    #[test]
    fn hits_carry_provenance_in_msg_and_no_duration() {
        let reported = test_result(
            "t".into(),
            ConfiguredTargetHandle { id: 1 },
            hit(),
            Origin::Local,
        )
        .unwrap();
        assert_eq!(reported.duration, None);
        let msg: serde_json::Value = serde_json::from_str(&reported.msg.unwrap().msg).unwrap();
        assert_eq!(
            msg,
            serde_json::json!({"turnkey_test_cache": {
                "hit": true, "origin": "local", "original_duration_us": 1_500_000u64
            }})
        );
    }

    #[test]
    fn remote_hits_say_so() {
        let reported = test_result(
            "t".into(),
            ConfiguredTargetHandle { id: 1 },
            hit(),
            Origin::Remote,
        )
        .unwrap();
        assert!(reported.details.starts_with("recorded, remote: "));
    }

    /// Stands in for buck2's orchestrator: answers each test with a canned
    /// result, keyed by target name, and keeps what the runner sent.
    #[derive(Default)]
    struct FakeBuck2 {
        results: BTreeMap<String, execute_response2::Response>,
        requests: std::sync::Mutex<Vec<ExecuteRequest2>>,
        reported: std::sync::Mutex<Vec<TestResult>>,
        exit_code: std::sync::Mutex<Option<i32>>,
    }

    impl Orchestrator for &FakeBuck2 {
        async fn execute(&self, request: ExecuteRequest2) -> Result<ExecuteResponse2> {
            let suite = match request
                .test_executable
                .as_ref()
                .and_then(|e| e.stage.as_ref())
            {
                Some(TestStage {
                    item: Some(test_stage::Item::Testing(testing)),
                }) => testing.suite.clone(),
                _ => anyhow::bail!("request without a testing stage"),
            };
            let response = self.results.get(&suite).cloned();
            self.requests.lock().unwrap().push(request);
            Ok(ExecuteResponse2 { response })
        }

        async fn report(&self, result: TestResult) -> Result<()> {
            self.reported.lock().unwrap().push(result);
            Ok(())
        }

        async fn end(&self, exit_code: i32) -> Result<()> {
            *self.exit_code.lock().unwrap() = Some(exit_code);
            Ok(())
        }
    }

    fn config(args: &[&str]) -> Config {
        use clap::Parser;
        Config::try_parse_from(
            ["ignored", "--buck-test-info", "ignored"]
                .iter()
                .chain(args),
        )
        .unwrap()
    }

    fn spec(target: &str, id: i64) -> ExternalRunnerSpec {
        ExternalRunnerSpec {
            target: Some(ConfiguredTarget {
                handle: Some(ConfiguredTargetHandle { id }),
                cell: "root".into(),
                package: "pkg".into(),
                target: target.into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn finished(exit: i32) -> execute_response2::Response {
        let mut result = pass_with(command_execution_kind::Command::LocalCommand(
            LocalCommand::default(),
        ));
        result.status = Some(ExecutionStatus {
            status: Some(execution_status::Status::Finished(exit)),
        });
        execute_response2::Response::Result(result)
    }

    /// Stands in for the test result cache: keeps every recorded result.
    #[derive(Default)]
    struct FakeCache {
        written: std::sync::Mutex<Vec<UpdateActionResultRequest>>,
    }

    impl ActionCache for &FakeCache {
        async fn update(&self, request: UpdateActionResultRequest) -> Result<()> {
            self.written.lock().unwrap().push(request);
            Ok(())
        }
    }

    impl FakeCache {
        /// The digests recorded, in hash order.
        fn digests(&self) -> Vec<String> {
            let mut digests: Vec<_> = self
                .written
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.action_digest.as_ref().unwrap().hash.clone())
                .collect();
            digests.sort();
            digests
        }
    }

    /// Run `specs` through a runner talking to `buck2`, without a cache.
    async fn run(buck2: &FakeBuck2, config: Config, specs: Vec<ExternalRunnerSpec>) {
        run_recording(buck2, None, config, specs).await
    }

    /// Run `specs` through a runner talking to `buck2` and recording into
    /// `cache`, as main.rs sets it up: a recorder only when the mode records.
    async fn run_recording(
        buck2: &FakeBuck2,
        cache: Option<&FakeCache>,
        config: Config,
        specs: Vec<ExternalRunnerSpec>,
    ) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        for spec in specs {
            sender.send(spec).unwrap();
        }
        drop(sender);
        let recorder = cache
            .filter(|_| config.turnkey_test_cache.records())
            .map(Recorder::new);
        Runner::new(buck2, config, recorder)
            .run_all(receiver)
            .await
            .unwrap();
    }

    /// A local run of the test with digest `<hash>:10`, exiting with `exit`.
    fn local_run(hash: &str, exit: i32) -> execute_response2::Response {
        let mut result = pass_with(command_execution_kind::Command::LocalCommand(
            LocalCommand {
                action_digest: format!("{hash}:10"),
                ..Default::default()
            },
        ));
        result.status = Some(ExecutionStatus {
            status: Some(execution_status::Status::Finished(exit)),
        });
        result.start_time = Some(prost_types::Duration {
            seconds: 100,
            nanos: 0,
        });
        result.execution_time = Some(prost_types::Duration {
            seconds: 2,
            nanos: 500_000_000,
        });
        execute_response2::Response::Result(result)
    }

    fn with_result(
        response: execute_response2::Response,
        change: impl FnOnce(&mut ExecutionResult2),
    ) -> execute_response2::Response {
        let execute_response2::Response::Result(mut result) = response else {
            unreachable!()
        };
        change(&mut result);
        execute_response2::Response::Result(result)
    }

    /// buck2 answering one test per name with `results`.
    fn buck2_answering(results: Vec<(&str, execute_response2::Response)>) -> FakeBuck2 {
        FakeBuck2 {
            results: results
                .into_iter()
                .map(|(name, response)| (name.to_owned(), response))
                .collect(),
            ..Default::default()
        }
    }

    /// Targets of a cache-safe rule, which the test-caching helper labels.
    fn specs(names: &[&str]) -> Vec<ExternalRunnerSpec> {
        names
            .iter()
            .zip(1..)
            .map(|(name, id)| cacheable(spec(name, id)))
            .collect()
    }

    fn cacheable(mut spec: ExternalRunnerSpec) -> ExternalRunnerSpec {
        spec.labels.push(CACHEABLE_LABEL.to_owned());
        spec
    }

    #[tokio::test]
    async fn records_only_passing_local_runs_without_outputs() {
        let buck2 = buck2_answering(vec![
            ("passes", local_run("passes", 0)),
            ("fails", local_run("fails", 1)),
            ("hit", execute_response2::Response::Result(hit())),
            (
                "has-outputs",
                with_result(local_run("has-outputs", 0), |r| {
                    r.outputs = vec![Default::default()]
                }),
            ),
        ]);
        let cache = FakeCache::default();
        run_recording(
            &buck2,
            Some(&cache),
            config(&["--turnkey-test-cache=on"]),
            specs(&["passes", "fails", "hit", "has-outputs"]),
        )
        .await;
        assert_eq!(cache.digests(), ["passes"]);
    }

    #[tokio::test]
    async fn a_recorded_result_is_what_buck2_needs_to_serve_a_hit() {
        let buck2 = buck2_answering(vec![(
            "passes",
            with_result(local_run("passes", 0), |r| {
                r.stderr = Some(ExecutionStream {
                    item: Some(execution_stream::Item::Inline(b"err".to_vec())),
                })
            }),
        )]);
        let cache = FakeCache::default();
        run_recording(
            &buck2,
            Some(&cache),
            config(&["--turnkey-test-cache=on"]),
            specs(&["passes"]),
        )
        .await;

        let written = cache.written.lock().unwrap();
        let [request] = written.as_slice() else {
            panic!("expected one recorded result, got {}", written.len())
        };
        // buck2's instance name, which turnkey leaves at its default
        assert_eq!(request.instance_name, "");
        let digest = request.action_digest.as_ref().unwrap();
        assert_eq!((digest.hash.as_str(), digest.size_bytes), ("passes", 10));
        let result = request.action_result.as_ref().unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout_raw, b"out");
        assert_eq!(result.stderr_raw, b"err");
        assert!(result.output_files.is_empty() && result.output_directories.is_empty());
        // buck2 takes a hit's duration from these timestamps.
        let metadata = result.execution_metadata.as_ref().unwrap();
        let start = metadata.execution_start_timestamp.unwrap();
        let completed = metadata.execution_completed_timestamp.unwrap();
        assert_eq!((start.seconds, start.nanos), (100, 0));
        assert_eq!((completed.seconds, completed.nanos), (102, 500_000_000));
    }

    #[tokio::test]
    async fn output_too_large_to_inline_is_not_recorded() {
        let buck2 = buck2_answering(vec![(
            "chatty",
            with_result(local_run("chatty", 0), |r| {
                r.stdout = Some(ExecutionStream {
                    item: Some(execution_stream::Item::Inline(vec![b'x'; 2 * 1024 * 1024])),
                })
            }),
        )]);
        let cache = FakeCache::default();
        run_recording(
            &buck2,
            Some(&cache),
            config(&["--turnkey-test-cache=on"]),
            specs(&["chatty"]),
        )
        .await;
        assert!(cache.digests().is_empty());
        // Not recording never fails the test.
        assert_eq!(*buck2.exit_code.lock().unwrap(), Some(0));
    }

    #[tokio::test]
    async fn only_modes_that_record_record() {
        for (mode, recorded) in [
            ("on", true),
            ("record-only", true),
            ("read-only", false),
            ("off", false),
        ] {
            let buck2 = buck2_answering(vec![("passes", local_run("passes", 0))]);
            let cache = FakeCache::default();
            run_recording(
                &buck2,
                Some(&cache),
                config(&[&format!("--turnkey-test-cache={mode}")]),
                specs(&["passes"]),
            )
            .await;
            assert_eq!(!cache.digests().is_empty(), recorded, "mode {mode}");
        }
    }

    #[tokio::test]
    async fn only_targets_of_cache_safe_rules_are_recorded() {
        let buck2 = buck2_answering(vec![
            ("cache-safe", local_run("cache-safe", 0)),
            ("unpatched-rule", local_run("unpatched-rule", 0)),
        ]);
        let cache = FakeCache::default();
        run_recording(
            &buck2,
            Some(&cache),
            config(&["--turnkey-test-cache=on"]),
            vec![cacheable(spec("cache-safe", 1)), spec("unpatched-rule", 2)],
        )
        .await;
        assert_eq!(cache.digests(), ["cache-safe"]);
    }

    #[tokio::test]
    async fn reports_every_result_and_fails_the_run_on_any_failure() {
        let buck2 = FakeBuck2 {
            results: BTreeMap::from([
                ("passes".into(), finished(0)),
                ("fails".into(), finished(1)),
            ]),
            ..Default::default()
        };
        run(
            &buck2,
            config(&[]),
            vec![spec("passes", 1), spec("fails", 2)],
        )
        .await;

        let mut reported: Vec<_> = buck2
            .reported
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.name.clone(), r.status(), r.target.unwrap().id))
            .collect();
        reported.sort();
        assert_eq!(
            reported,
            [
                ("root//pkg:fails".to_owned(), TestStatus::Fail, 2),
                ("root//pkg:passes".to_owned(), TestStatus::Pass, 1),
            ]
        );
        assert_eq!(
            *buck2.exit_code.lock().unwrap(),
            Some(bundled::FAILURE_EXIT_CODE)
        );
    }

    #[tokio::test]
    async fn a_run_where_everything_passes_exits_zero() {
        let buck2 = FakeBuck2 {
            results: BTreeMap::from([("passes".into(), finished(0))]),
            ..Default::default()
        };
        run(&buck2, config(&[]), vec![spec("passes", 1)]).await;
        assert_eq!(*buck2.exit_code.lock().unwrap(), Some(0));
    }

    #[tokio::test]
    async fn cancelled_tests_are_not_reported() {
        let buck2 = FakeBuck2 {
            results: BTreeMap::from([(
                "cancelled".into(),
                execute_response2::Response::Cancelled(Default::default()),
            )]),
            ..Default::default()
        };
        run(&buck2, config(&[]), vec![spec("cancelled", 1)]).await;
        assert!(buck2.reported.lock().unwrap().is_empty());
        // An omitted test isn't a pass.
        assert_eq!(
            *buck2.exit_code.lock().unwrap(),
            Some(bundled::FAILURE_EXIT_CODE)
        );
    }

    #[tokio::test]
    async fn caching_is_off_unless_tk_turns_it_on() {
        let buck2 = FakeBuck2 {
            results: BTreeMap::from([("t".into(), finished(0))]),
            ..Default::default()
        };
        run(&buck2, config(&[]), vec![spec("t", 1)]).await;
        assert!(buck2.requests.lock().unwrap()[0].disable_test_execution_caching);
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
