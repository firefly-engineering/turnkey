# Auto-managed by turnkey. Hash: 7a984399ec557a9e
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "python_library")

python_library(
    name = "buck",
    srcs = [
        "__init__.py",
        "generator.py",
    ],
    deps = [
        # turnkey:auto-start
        "//src/python/cfg:cfg",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
