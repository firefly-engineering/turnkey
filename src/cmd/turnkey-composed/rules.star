# turnkey-composed - FUSE composition daemon for Turnkey
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "turnkey-composed",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/composition:composition",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/ctrlc:ctrlc",
        "rustdeps//vendor/env_logger:env_logger",
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    visibility = ["PUBLIC"],
)
