# buck2-test-executor - the executor side of buck2's test-runner protocol
load("@prelude//:rules.bzl", "rust_library", "rust_test")

# Generated protocol code, built by Nix for the pinned buck2 release
# (nix/packages/test-runner-protocol.nix)
_ENV = {
    "TURNKEY_TEST_RUNNER_PROTOCOL": read_root_config("turnkey", "test_runner_protocol", ""),
}

# Versioned targets for crates whose types cross into tonic and tonic-prost,
# which depend on these exact versioned targets.
_DEPS = [
    "rustdeps//vendor/anyhow:anyhow",
    "rustdeps//vendor/clap:clap",
    "rustdeps//vendor/futures-util@0.3.34:futures-util",
    "rustdeps//vendor/hyper-util@0.1.21:hyper-util",
    "rustdeps//vendor/prost@0.14.4:prost",
    "rustdeps//vendor/prost-types@0.14.4:prost-types",
    "rustdeps//vendor/tokio@1.50.0:tokio",
    "rustdeps//vendor/tonic@0.14.6:tonic",
    "rustdeps//vendor/tonic-prost@0.14.6:tonic-prost",
    "rustdeps//vendor/tower@0.5.3:tower",
]

rust_library(
    name = "buck2-test-executor",
    srcs = glob(["src/**/*.rs"]),
    crate = "buck2_test_executor",
    edition = "2024",
    env = _ENV,
    deps = _DEPS,
    visibility = ["PUBLIC"],
)

rust_test(
    name = "buck2-test-executor-test",
    srcs = glob(["src/**/*.rs"]),
    crate = "buck2_test_executor",
    edition = "2024",
    env = _ENV,
    deps = _DEPS,
)
