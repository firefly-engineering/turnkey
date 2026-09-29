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
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/toml_edit:toml_edit",
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
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/toml_edit:toml_edit",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
