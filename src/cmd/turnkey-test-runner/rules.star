# turnkey-test-runner - turnkey's buck2 test runner
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "turnkey-test-runner",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    # Generated protocol code, built by Nix for the declared buck2 release
    # (nix/packages/test-runner-protocol.nix)
    env = {
        "TURNKEY_TEST_RUNNER_PROTOCOL": read_root_config("turnkey", "test_runner_protocol", ""),
    },
    # Versioned targets: the generated code passes prost and tonic types
    # across tonic-prost, which depends on these exact versioned targets.
    deps = [
        "rustdeps//vendor/prost@0.14.4:prost",
        "rustdeps//vendor/prost-types@0.14.4:prost-types",
        "rustdeps//vendor/tonic@0.14.6:tonic",
        "rustdeps//vendor/tonic-prost@0.14.6:tonic-prost",
    ],
    visibility = ["PUBLIC"],
)
