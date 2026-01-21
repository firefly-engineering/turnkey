# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

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
