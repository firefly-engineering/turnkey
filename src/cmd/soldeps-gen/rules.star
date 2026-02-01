# soldeps-gen - generate solidity-deps.toml from foundry.toml
load("@prelude//rust:rust.bzl", "rust_binary")

# Version must match Cargo.toml
VERSION = "0.1.0"

rust_binary(
    name = "soldeps-gen",
    version = VERSION,
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/serde-saphyr:serde-saphyr",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/ureq:ureq",
    ],
    visibility = ["PUBLIC"],
)
