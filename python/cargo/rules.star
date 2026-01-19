load("@prelude//:rules.bzl", "python_library", "python_test")

python_library(
    name = "cargo",
    srcs = [
        "__init__.py",
        "features.py",
        "toml.py",
    ],
    deps = [
        "//python/cfg:cfg",
    ],
    visibility = ["PUBLIC"],
)

python_test(
    name = "test_toml",
    srcs = ["test_toml.py"],
    deps = [":cargo"],
)

python_test(
    name = "test_features",
    srcs = ["test_features.py"],
    deps = [":cargo"],
)
