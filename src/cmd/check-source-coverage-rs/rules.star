# check-source-coverage-rs - Check that all source files are covered by Buck2 targets
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "check-source-coverage-rs",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/glob:glob",
        "rustdeps//vendor/walkdir:walkdir",
        "//src/rust/starlark-parse:starlark-parse",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "check-source-coverage-rs-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/glob:glob",
        "rustdeps//vendor/walkdir:walkdir",
        "//src/rust/starlark-parse:starlark-parse",
    ],
)
