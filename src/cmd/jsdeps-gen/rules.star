# jsdeps-gen - generate js-deps.toml from pnpm-lock.yaml
load("@prelude//rust:rust.bzl", "rust_binary")

# Version must match Cargo.toml
VERSION = "0.1.0"

rust_binary(
    name = "jsdeps-gen",
    version = VERSION,
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/base64:base64",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde-saphyr:serde-saphyr",
        "rustdeps//vendor/sha2:sha2",
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)
