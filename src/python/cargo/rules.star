load("@prelude//:rules.bzl", "python_library", "python_test")

python_library(
    name = "cargo",
    srcs = [
        "turnkey/cargo/__init__.py",
        "turnkey/cargo/features.py",
        "turnkey/cargo/toml.py",
    ],
    base_module = "",
    deps = [
        "//src/python/cfg:cfg",
    ],
    visibility = ["PUBLIC"],
)

python_test(
    name = "test_toml",
    srcs = ["tests/test_toml.py"],
    base_module = "tests",
    deps = [":cargo"],
)

python_test(
    name = "test_features",
    srcs = ["tests/test_features.py"],
    base_module = "tests",
    deps = [":cargo"],
)
