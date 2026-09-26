load("@prelude//:rules.bzl", "python_library", "python_test")

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

python_test(
    name = "test_generator",
    srcs = ["tests/test_generator.py"],
    base_module = "tests",
    deps = [":buck"],
)
