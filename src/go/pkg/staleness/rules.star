# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "staleness",
    srcs = [
        "cache.go",
        "imports.go",
        "python.go",
        "rust.go",
        "srclist.go",
        "staleness.go",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "staleness_test",
    srcs = [
        "cache_test.go",
        "imports_test.go",
        "python_test.go",
        "rust_test.go",
        "srclist_test.go",
        "staleness_test.go",
    ],
    visibility = ["PUBLIC"],
)
