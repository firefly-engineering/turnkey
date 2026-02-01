load("@prelude//:rules.bzl", "python_library")

python_library(
    name = "buildsystem",
    srcs = [
        "__init__.py",
        "native_library.py",
        "buck2.py",
        "bazel.py",
    ],
    base_module = "python.buildsystem",
    visibility = ["PUBLIC"],
)
