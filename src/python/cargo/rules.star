# Auto-managed by turnkey. Hash: 7a984399ec557a9e
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "python_library", "python_test")

python_library(
    name = "cargo",
    srcs = [
        "__init__.py",
        "features.py",
        "toml.py",
    ],
    deps = [
        # turnkey:auto-start
        "//src/python/cfg:cfg",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)

python_test(
    name = "test_toml",
    srcs = ["test_toml.py"],
    deps = [
        # turnkey:auto-start
        "//src/python/cfg:cfg",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)

python_test(
    name = "test_features",
    srcs = ["test_features.py"],
    deps = [
        # turnkey:auto-start
        "//src/python/cfg:cfg",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
