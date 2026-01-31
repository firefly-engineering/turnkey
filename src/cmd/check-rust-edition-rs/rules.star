# check-rust-edition-rs - Check Rust edition consistency
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "check-rust-edition-rs",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/glob:glob",
        "rustdeps//vendor/toml:toml",
        "//src/rust/starlark-parse:starlark-parse",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "check-rust-edition-rs-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/glob:glob",
        "rustdeps//vendor/toml:toml",
        "//src/rust/starlark-parse:starlark-parse",
    ],
)
