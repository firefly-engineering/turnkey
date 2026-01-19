# Rust hello world example
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "rust-hello",
    srcs = ["main.rs"],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "rust-hello-test",
    srcs = ["main.rs"],
    visibility = ["PUBLIC"],
)
