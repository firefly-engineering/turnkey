# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "python_library", "python_test")

python_library(
    name = "cfg",
    srcs = [
        "__init__.py",
        "evaluator.py",
        "parser.py",
        "target.py",
    ],
    visibility = ["PUBLIC"],
)

python_test(
    name = "test",
    srcs = ["test_parser.py"],
    visibility = ["PUBLIC"],
)
