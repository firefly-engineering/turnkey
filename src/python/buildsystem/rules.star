load("@prelude//:rules.bzl", "python_library")

python_library(
    name = "buildsystem",
    srcs = [
        "turnkey/buildsystem/__init__.py",
        "turnkey/buildsystem/native_library.py",
        "turnkey/buildsystem/buck2.py",
        "turnkey/buildsystem/bazel.py",
    ],
    base_module = "",
    visibility = ["PUBLIC"],
)
