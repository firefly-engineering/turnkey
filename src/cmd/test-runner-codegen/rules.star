# test-runner-codegen - generates turnkey-test-runner's protocol code
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "test-runner-codegen",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/protox:protox",
        "rustdeps//vendor/tonic-prost-build:tonic-prost-build",
    ],
    visibility = ["PUBLIC"],
)
