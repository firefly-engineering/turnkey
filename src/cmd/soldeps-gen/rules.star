# soldeps-gen - generate solidity-deps.toml from foundry.toml
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "soldeps-gen",
    srcs = glob(["src/**/*.rs", "VERSION.txt"]),
    edition = "2024",
    deps = [
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/base64:base64",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/flate2:flate2",
        "rustdeps//vendor/serde-saphyr:serde-saphyr",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/sha1:sha1",
        "rustdeps//vendor/sha2:sha2",
        "rustdeps//vendor/tar:tar",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/ureq:ureq",
    ],
    visibility = ["PUBLIC"],
)
