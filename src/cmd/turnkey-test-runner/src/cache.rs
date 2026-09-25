//! Recording passing test results in the local test result cache.
//!
//! buck2 looks test results up natively (supports_test_execution_caching),
//! but never uploads a test it ran locally. The runner fills that gap: after
//! a local pass, it writes an ActionResult under the action digest buck2
//! computed and reported, so the next lookup of the same digest is a hit
//! (docs/adr/0001-runner-recorded-native-test-caching.md).

use std::time::Duration;

use anyhow::{Context, Result, bail};
use tonic::transport::{Channel, Endpoint};

use buck2_test_executor::proto::build::bazel::remote::execution::v2::action_cache_client::ActionCacheClient;
use buck2_test_executor::proto::build::bazel::remote::execution::v2::{
    ActionResult, Digest, ExecutedActionMetadata, UpdateActionResultRequest,
};

/// Stdout and stderr are recorded inline in the ActionResult. Beyond this
/// size a result isn't recorded at all, rather than recorded incompletely.
const MAX_INLINE_OUTPUT: usize = 1024 * 1024;

/// What the runner does with the cache for a test run. `tk test` chooses it
/// by the reuse policy (src/go/pkg/testcache); the runner only obeys it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    /// Neither read nor record: tests run as under buck2's bundled runner.
    Off,
    /// Reuse recorded results, and record fresh passes.
    On,
    /// Run every test, and record fresh passes.
    RecordOnly,
    /// Reuse recorded results, and never record: for a shared cache.
    ReadOnly,
}

impl Mode {
    pub fn reads(self) -> bool {
        matches!(self, Mode::On | Mode::ReadOnly)
    }

    pub fn records(self) -> bool {
        matches!(self, Mode::On | Mode::RecordOnly)
    }
}

/// The REAPI instance name buck2 uses. turnkey never sets
/// `buck2_re_client.instance_name`, so it is buck2's default, the empty name
/// (docs/specs/test-result-caching.md).
const INSTANCE_NAME: &str = "";

/// A passing local run, as reported by buck2.
pub struct Pass<'a> {
    /// buck2's action digest, `<sha256 hex>:<size>`.
    pub action_digest: &'a str,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
    /// When the test started, since the Unix epoch.
    pub start_time: Duration,
    pub execution_time: Duration,
}

pub struct Recorder {
    client: ActionCacheClient<Channel>,
}

impl Recorder {
    /// Connect lazily to the cache at `address` (`grpc://host:port`, as in
    /// the `[buck2_re_client]` config).
    pub fn new(address: &str) -> Result<Self> {
        let uri = address
            .strip_prefix("grpc://")
            .map(|rest| format!("http://{rest}"))
            .with_context(|| format!("unsupported cache address `{address}`: expected grpc://"))?;
        let channel = Endpoint::try_from(uri)?.connect_lazy();
        Ok(Self {
            client: ActionCacheClient::new(channel),
        })
    }

    pub async fn record(&self, pass: Pass<'_>) -> Result<()> {
        if pass.stdout.len() + pass.stderr.len() > MAX_INLINE_OUTPUT {
            bail!("output larger than {MAX_INLINE_OUTPUT} bytes; not recorded");
        }
        let completed = pass.start_time + pass.execution_time;
        let result = ActionResult {
            exit_code: 0,
            stdout_raw: pass.stdout.to_vec(),
            stderr_raw: pass.stderr.to_vec(),
            // buck2 takes a hit's duration from these timestamps.
            execution_metadata: Some(ExecutedActionMetadata {
                worker: "turnkey-test-runner".to_owned(),
                execution_start_timestamp: Some(timestamp(pass.start_time)),
                execution_completed_timestamp: Some(timestamp(completed)),
                ..Default::default()
            }),
            ..Default::default()
        };
        self.client
            .clone()
            .update_action_result(UpdateActionResultRequest {
                instance_name: INSTANCE_NAME.to_owned(),
                action_digest: Some(parse_digest(pass.action_digest)?),
                action_result: Some(result),
                ..Default::default()
            })
            .await
            .context("writing the action result")?;
        Ok(())
    }
}

/// Parse buck2's `<hash>:<size>` digest notation.
pub fn parse_digest(digest: &str) -> Result<Digest> {
    let (hash, size) = digest
        .split_once(':')
        .with_context(|| format!("malformed action digest `{digest}`"))?;
    Ok(Digest {
        hash: hash.to_owned(),
        size_bytes: size
            .parse()
            .with_context(|| format!("malformed action digest size in `{digest}`"))?,
    })
}

fn timestamp(since_epoch: Duration) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: since_epoch.as_secs() as i64,
        nanos: since_epoch.subsec_nanos() as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_buck2_digests() {
        let digest = parse_digest("28d8aace45:147").unwrap();
        assert_eq!(digest.hash, "28d8aace45");
        assert_eq!(digest.size_bytes, 147);
        assert!(parse_digest("28d8aace45").is_err());
        assert!(parse_digest("28d8aace45:x").is_err());
    }

    #[test]
    fn modes_read_and_record() {
        assert!(Mode::On.reads() && Mode::On.records());
        assert!(!Mode::RecordOnly.reads() && Mode::RecordOnly.records());
        assert!(Mode::ReadOnly.reads() && !Mode::ReadOnly.records());
        assert!(!Mode::Off.reads() && !Mode::Off.records());
    }
}
