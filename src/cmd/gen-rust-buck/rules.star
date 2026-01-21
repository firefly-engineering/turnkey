# Auto-managed by turnkey. Hash: ae0fbb851223616a
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "gen-rust-buck",
    deps = [
        # turnkey:auto-start
        "//src/python/buck:buck",
        "//src/python/cargo:cargo",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
