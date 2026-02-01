# soldeps-gen - generate solidity-deps.toml from foundry.toml
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "soldeps-gen",
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
