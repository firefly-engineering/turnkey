# Auto-managed by turnkey. Hash: 562a8597d84e1607
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "rust-hello-deps",
    srcs = ["main.rs"],
    deps = [
        # turnkey:auto-start
        "rustdeps//vendor/itoa:itoa",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
