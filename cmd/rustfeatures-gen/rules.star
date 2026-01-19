# rustfeatures-gen - generate rust-features.toml from workspace Cargo.toml files
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "rustfeatures-gen",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)
