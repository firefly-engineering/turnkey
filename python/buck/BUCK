load("@prelude//:rules.bzl", "python_library")

python_library(
    name = "buck",
    srcs = [
        "__init__.py",
        "generator.py",
    ],
    deps = [
        "//python/cfg:cfg",
        "//python/cargo:cargo",
    ],
    visibility = ["PUBLIC"],
)
