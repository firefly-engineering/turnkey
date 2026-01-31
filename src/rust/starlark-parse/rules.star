# starlark-parse - Parse Starlark/Buck2 build files using tree-sitter
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "starlark-parse",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/tree-sitter:tree-sitter",
        "rustdeps//vendor/tree-sitter-starlark:tree-sitter-starlark",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "starlark-parse-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/tree-sitter:tree-sitter",
        "rustdeps//vendor/tree-sitter-starlark:tree-sitter-starlark",
    ],
)
