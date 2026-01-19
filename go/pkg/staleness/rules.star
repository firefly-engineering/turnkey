# Staleness detection library for turnkey sync

load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "staleness",
    package_name = "github.com/firefly-engineering/turnkey/go/pkg/staleness",
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
    target_under_test = ":staleness",
)
