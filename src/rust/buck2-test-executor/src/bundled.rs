//! What buck2's bundled test runner (app/buck2_test_runner at the pinned
//! release) does with each test: its arguments, the Execute2 request it
//! sends, how it reports the result, and its exit code. A runner built on
//! this crate behaves the same by using these, and adds its own policy on
//! top: the parity suite (src/cmd/check-test-runner-parity) checks that it
//! does.

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};

use crate::proto::buck::host_sharing::{
    HostSharingRequirements, WeightClass, host_sharing_requirements, weight_class,
};
use crate::proto::buck::test::{
    ArgValue, ArgValueContent, ConfiguredTargetHandle, EnvironmentVariable, ExecuteRequest2,
    ExecutionResult2, ExecutionStream, ExternalRunnerSpec, ExternalRunnerSpecValue, TestExecutable,
    TestResult, TestStage, TestStatus, Testing, arg_value_content, execution_status,
    execution_stream, external_runner_spec_value, test_stage,
};

/// Exit code the bundled runner reports when any test did not pass.
pub const FAILURE_EXIT_CODE: i32 = 32;

/// The bundled runner's arguments (app/buck2_test_runner/src/config.rs), so
/// `-- --env`, `-- --timeout` and `-- --test-arg` behave the same. A runner
/// flattens them into its own.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// add a list of environment variables using format: --env VAR1=Value1 VAR2='Value 2'
    #[clap(long)]
    pub env: Vec<String>,

    /// Max number of seconds allowed to run a test.
    #[clap(long, default_value = "600")]
    pub timeout: u64,

    /// Ignored arg included for backwards compatibility.
    #[clap(long, hide = true)]
    buck_test_info: String,

    /// Passthrough argments to test binary.
    /// Available as a workaround for when test features are available.
    #[clap(long, num_args=1.., allow_hyphen_values = true)]
    pub test_arg: Vec<String>,
}

/// An `--env NAME=VALUE` argument.
#[derive(Debug, PartialEq, Clone)]
pub struct EnvValue {
    pub name: String,
    pub value: String,
}

impl FromStr for EnvValue {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self> {
        match input.split_once('=') {
            Some((name, value)) => Ok(EnvValue {
                name: name.to_owned(),
                value: value.to_owned(),
            }),
            None => Err(anyhow!(
                "Incorrect syntax for env value. Please use name=value. Input: `{input}`"
            )),
        }
    }
}

/// The request the bundled runner sends to run `spec`. buck2 computes the
/// test's action digest from it, so it must match exactly for results to be
/// shared. Only whether buck2 may reuse a recorded result is the caller's.
pub fn execute_request(
    spec: ExternalRunnerSpec,
    args: &Args,
    disable_test_execution_caching: bool,
) -> Result<ExecuteRequest2> {
    let target = spec.target.context("spec without a target")?;

    let cmd = spec
        .command
        .into_iter()
        .chain(args.test_arg.iter().map(|arg| verbatim(arg)))
        .map(spec_arg)
        .collect();

    // Sorted by name, later values winning: the spec's env, then `--env`.
    // This is the order buck2's runner produces, and the env is part of the
    // action digest.
    let mut env: BTreeMap<String, ExternalRunnerSpecValue> = spec.env.into_iter().collect();
    for value in &args.env {
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
            seconds: i64::try_from(args.timeout).context("timeout out of range")?,
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
        disable_test_execution_caching,
    })
}

/// The result the bundled runner reports for one test: its status, its
/// stdout and stderr as the details buck2 prints under the test's line, and
/// how long it ran.
pub fn test_result(
    name: String,
    target: ConfiguredTargetHandle,
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
    let details = format!(
        "---- STDOUT ----\n{}\n---- STDERR ----\n{}\n",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::buck::test::{ConfiguredTarget, ExecutionStatus};
    use clap::Parser;

    #[derive(Parser)]
    struct Runner {
        #[clap(flatten)]
        args: Args,
    }

    fn args(extra: &[&str]) -> Args {
        Runner::try_parse_from(
            ["ignored", "--buck-test-info", "ignored"]
                .iter()
                .chain(extra),
        )
        .unwrap()
        .args
    }

    fn verbatim_text(value: &ArgValue) -> &str {
        match value.content.as_ref().and_then(|c| c.value.as_ref()) {
            Some(arg_value_content::Value::SpecValue(ExternalRunnerSpecValue {
                value: Some(external_runner_spec_value::Value::Verbatim(text)),
            })) => text,
            other => panic!("unexpected value {other:?}"),
        }
    }

    fn spec() -> ExternalRunnerSpec {
        ExternalRunnerSpec {
            target: Some(ConfiguredTarget {
                handle: Some(ConfiguredTargetHandle { id: 7 }),
                target: "t".into(),
                ..Default::default()
            }),
            command: vec![verbatim("bin")],
            env: [("B", "spec"), ("A", "spec")]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), verbatim(v)))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn args_as_buck2_sends_them() {
        let args = args(&["--env", "A=1"]);
        assert_eq!(args.env, vec!["A=1"]);
        assert_eq!(args.timeout, 600);
    }

    #[test]
    fn test_arg_takes_everything_after_it() {
        // --test-arg accepts hyphenated values and keeps consuming, so other
        // flags must come before it.
        let args = args(&["--timeout", "5", "--test-arg", "-test.v", "--timeout", "7"]);
        assert_eq!(args.test_arg, vec!["-test.v", "--timeout", "7"]);
        assert_eq!(args.timeout, 5);
    }

    #[test]
    fn env_value_requires_equals() {
        assert_eq!(
            "A=b=c".parse::<EnvValue>().unwrap(),
            EnvValue {
                name: "A".into(),
                value: "b=c".into()
            }
        );
        assert!("A".parse::<EnvValue>().is_err());
    }

    #[test]
    fn requests_sort_the_env_let_env_flags_win_and_append_test_args() {
        let request = execute_request(
            spec(),
            &args(&["--env", "B=flag", "--timeout", "5", "--test-arg", "-v"]),
            true,
        )
        .unwrap();
        let executable = request.test_executable.unwrap();
        let env: Vec<_> = executable
            .env
            .iter()
            .map(|e| (e.key.as_str(), verbatim_text(e.value.as_ref().unwrap())))
            .collect();
        assert_eq!(env, [("A", "spec"), ("B", "flag")]);
        let cmd: Vec<_> = executable.cmd.iter().map(verbatim_text).collect();
        assert_eq!(cmd, ["bin", "-v"]);
        assert_eq!(executable.target.unwrap().id, 7);
        assert_eq!(request.timeout.unwrap().seconds, 5);
        assert!(request.disable_test_execution_caching);
    }

    #[test]
    fn results_carry_status_output_and_duration() {
        let result = ExecutionResult2 {
            status: Some(ExecutionStatus {
                status: Some(execution_status::Status::Finished(1)),
            }),
            stdout: Some(ExecutionStream {
                item: Some(execution_stream::Item::Inline(b"out".to_vec())),
            }),
            execution_time: Some(prost_types::Duration {
                seconds: 2,
                nanos: 0,
            }),
            ..Default::default()
        };
        let reported = test_result("t".into(), ConfiguredTargetHandle { id: 1 }, result).unwrap();
        assert_eq!(reported.status(), TestStatus::Fail);
        assert_eq!(
            reported.details,
            "---- STDOUT ----\nout\n---- STDERR ----\n\n"
        );
        assert_eq!(reported.duration.unwrap().seconds, 2);
        assert_eq!(reported.msg, None);
    }
}
