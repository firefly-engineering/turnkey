# deps-extract - Extract dependencies from source files using tree-sitter
load("@prelude//:rules.bzl", "rust_binary", "rust_library", "rust_test")

# The extraction protocol: the JSON deps-extract prints, which rules sync
# reads back with the same types. It is the crate without its default
# features (no tree-sitter grammar), as rules sync depends on it; its deps
# are the protocol's alone, not the crate's, so rules sync leaves them
# alone.
# turnkey:no-sync
rust_library(
    name = "extraction",
    srcs = [
        "src/extraction.rs",
        "src/lib.rs",
    ],
    crate = "deps_extract",
    crate_root = "src/lib.rs",
    default_features = False,
    edition = "2024",
    deps = ["rustdeps//vendor/serde:serde"],
    visibility = ["PUBLIC"],
)

rust_binary(
    name = "deps-extract",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    features = [
        "python",
        "rust",
        "typescript",
        "solidity",
    ],
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
    crate_root = "src/main.rs",
    edition = "2024",
    features = [
        "python",
        "rust",
        "typescript",
        "solidity",
    ],
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
