load("@prelude//:rules.bzl", "python_library")

python_library(
    name = "buck",
    srcs = [
        "__init__.py",
        "generator.py",
    ],
    base_module = "python.buck",
    deps = [
        "//src/python/buildsystem:buildsystem",
        "//src/python/cargo:cargo",
        "//src/python/cfg:cfg",
    ],
    visibility = ["PUBLIC"],
)
