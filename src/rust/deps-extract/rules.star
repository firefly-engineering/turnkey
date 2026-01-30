# deps-extract - Extract dependencies from source files using tree-sitter
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "deps-extract",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tree-sitter:tree-sitter",
        "rustdeps//vendor/tree-sitter-python:tree-sitter-python",
        "rustdeps//vendor/tree-sitter-rust:tree-sitter-rust",
        "rustdeps//vendor/tree-sitter-solidity:tree-sitter-solidity",
        "rustdeps//vendor/tree-sitter-typescript:tree-sitter-typescript",
        "rustdeps//vendor/walkdir:walkdir",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "deps-extract-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tree-sitter:tree-sitter",
        "rustdeps//vendor/tree-sitter-python:tree-sitter-python",
        "rustdeps//vendor/tree-sitter-rust:tree-sitter-rust",
        "rustdeps//vendor/tree-sitter-solidity:tree-sitter-solidity",
        "rustdeps//vendor/tree-sitter-typescript:tree-sitter-typescript",
        "rustdeps//vendor/walkdir:walkdir",
    ],
)
