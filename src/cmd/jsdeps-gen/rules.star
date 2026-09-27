# jsdeps-gen - generate js-deps.toml from pnpm-lock.yaml
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "jsdeps-gen",
    srcs = glob(["src/**/*.rs", "VERSION.txt"]),
    edition = "2024",
    deps = [
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/base64:base64",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde-saphyr:serde-saphyr",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/sha2:sha2",
    ],
    visibility = ["PUBLIC"],
)
