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
    deps = [":cfg"],
)
