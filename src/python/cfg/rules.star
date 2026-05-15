load("@prelude//:rules.bzl", "python_library", "python_test")

python_library(
    name = "cfg",
    srcs = [
        "turnkey/cfg/__init__.py",
        "turnkey/cfg/evaluator.py",
        "turnkey/cfg/parser.py",
        "turnkey/cfg/target.py",
    ],
    base_module = "",
    visibility = ["PUBLIC"],
)

python_test(
    name = "test",
    srcs = ["tests/test_parser.py"],
    base_module = "tests",
    deps = [":cfg"],
)
