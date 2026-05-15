load("@prelude//:rules.bzl", "python_library")

python_library(
    name = "buck",
    srcs = [
        "turnkey/buck/__init__.py",
        "turnkey/buck/generator.py",
    ],
    base_module = "",
    deps = [
        "//src/python/buildsystem:buildsystem",
        "//src/python/cargo:cargo",
        "//src/python/cfg:cfg",
    ],
    visibility = ["PUBLIC"],
)
