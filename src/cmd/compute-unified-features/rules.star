# Auto-managed by turnkey. Hash: 6688b0e1e72fe516
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "compute-unified-features",
    deps = [
        # turnkey:auto-start
        "//src/python/cargo:cargo",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
