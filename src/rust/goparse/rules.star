# goparse - Go source files' package clause, imports, build constraints and
# embeds, as go/parser and go/build read them
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "goparse",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/tree-sitter:tree-sitter",
        "rustdeps//vendor/tree-sitter-go:tree-sitter-go",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "goparse-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/tempfile:tempfile",
        "rustdeps//vendor/tree-sitter:tree-sitter",
        "rustdeps//vendor/tree-sitter-go:tree-sitter-go",
    ],
)
