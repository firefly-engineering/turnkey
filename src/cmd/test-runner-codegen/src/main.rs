//! test-runner-codegen: generates turnkey-test-runner's protocol code.
//!
//! Usage: `test-runner-codegen <protos-dir> <out-dir>`
//!
//! `<protos-dir>` is the layout built by `nix/packages/test-runner-protocol.nix`:
//! `buck2/` holds buck2's test-runner protos, `reapi/` the Remote Execution
//! API and the googleapis files it imports. Protos are compiled with protox,
//! a pure-Rust protobuf compiler, so no `protoc` is needed.

use std::path::PathBuf;

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let protos = PathBuf::from(
        args.next()
            .context("usage: test-runner-codegen <protos-dir> <out-dir>")?,
    );
    let out = PathBuf::from(
        args.next()
            .context("usage: test-runner-codegen <protos-dir> <out-dir>")?,
    );

    let buck2 = protos.join("buck2");
    let reapi = protos.join("reapi");
    let files = [
        buck2.join("test.proto"),
        reapi.join("build/bazel/remote/execution/v2/remote_execution.proto"),
    ];
    let descriptors = protox::compile(files, [&buck2, &reapi]).context("compiling protos")?;

    // The runner is a client of buck2's TestOrchestrator and of the REAPI
    // cache services, and a server of buck2's TestExecutor.
    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .out_dir(&out)
        .compile_fds(descriptors)
        .context("generating Rust code")?;
    Ok(())
}
