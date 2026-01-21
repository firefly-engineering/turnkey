# Auto-managed by turnkey. Hash: 562a8597d84e1607
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "rust-hello-deps",
    srcs = ["main.rs"],
    edition = "2024",
    deps = [
        "rustdeps//vendor/itoa:itoa",
    ],
    visibility = ["PUBLIC"],
)
