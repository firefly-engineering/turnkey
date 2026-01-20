# rustdeps-gen - generate rust-deps.toml from Cargo.lock
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "rustdeps-gen",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/base64:base64",
        "rustdeps//vendor/cargo-lock:cargo-lock",
        "rustdeps//vendor/clap:clap",
    ],
    visibility = ["PUBLIC"],
)
