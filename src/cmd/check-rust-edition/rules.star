# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "check-rust-edition",
    visibility = ["PUBLIC"],
)
