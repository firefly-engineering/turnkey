# rustdeps-gen - generate rust-deps.toml from Cargo.lock
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "rustdeps-gen",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/base64:base64",
        "rustdeps//vendor/cargo-lock:cargo-lock",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/glob:glob",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "rustdeps-gen-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/base64:base64",
        "rustdeps//vendor/cargo-lock:cargo-lock",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/glob:glob",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
