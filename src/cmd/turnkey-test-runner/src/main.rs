//! turnkey-test-runner: turnkey's buck2 test runner.
//!
//! Speaks buck2's test-runner protocol, pinned to the buck2 release turnkey
//! ships, and records passing test results so `tk test` can reuse them
//! (docs/specs/test-result-caching.md).

// Running tests arrives with turnkey-vnm.2; until then the protocol code is
// only compiled.
#[allow(dead_code)]
mod proto;

fn main() {
    eprintln!("turnkey-test-runner: not yet runnable");
    std::process::exit(2);
}
