# turnkey-test-runner - turnkey's buck2 test runner
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

# Versioned targets for crates whose types cross into tonic, as in
# //src/rust/buck2-test-executor, which the protocol types come from.
_DEPS = [
    "//src/rust/buck2-test-executor:buck2-test-executor",
    "rustdeps//vendor/anyhow:anyhow",
    "rustdeps//vendor/clap:clap",
    "rustdeps//vendor/futures-util@0.3.34:futures-util",
    "rustdeps//vendor/prost-types@0.14.4:prost-types",
    "rustdeps//vendor/serde_json:serde_json",
    "rustdeps//vendor/tokio@1.50.0:tokio",
    "rustdeps//vendor/tonic@0.14.6:tonic",
]

rust_binary(
    name = "turnkey-test-runner",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = _DEPS,
    visibility = ["PUBLIC"],
)

rust_test(
    name = "turnkey-test-runner-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = _DEPS,
)
