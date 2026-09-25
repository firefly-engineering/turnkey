# turnkey-test-runner - turnkey's buck2 test runner
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

# Generated protocol code, built by Nix for the declared buck2 release
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

rust_binary(
    name = "turnkey-test-runner",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    env = _ENV,
    deps = _DEPS,
    visibility = ["PUBLIC"],
)

rust_test(
    name = "turnkey-test-runner-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    env = _ENV,
    deps = _DEPS,
)
