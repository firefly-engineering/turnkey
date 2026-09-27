load("@prelude//:rules.bzl", "python_library", "python_test")

python_library(
    name = "cargo",
    srcs = [
        "turnkey/cargo/__init__.py",
        "turnkey/cargo/features.py",
        "turnkey/cargo/semver.py",
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
    deps = [
        ":cargo",
        "//src/python/cfg:cfg",
    ],
)

python_test(
    name = "test_features",
    srcs = ["tests/test_features.py"],
    base_module = "tests",
    deps = [
        ":cargo",
        "//src/python/cfg:cfg",
    ],
)

python_test(
    name = "test_semver",
    srcs = ["tests/test_semver.py"],
    base_module = "tests",
    deps = [
        ":cargo",
        "//src/python/cfg:cfg",
    ],
)

# The feature activation cases src/go/pkg/cargofeatures runs too
python_test(
    name = "test-activation-vectors",
    srcs = ["tests/test_activation_vectors.py"],
    base_module = "tests",
    env = {"TURNKEY_ACTIVATION_VECTORS": "$(location //src/go/pkg/cargofeatures:activation-vectors)"},
    deps = [
        ":cargo",
        "//src/python/cfg:cfg",
    ],
)
