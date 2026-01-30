# Auto-managed by turnkey.

load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "prefetchcache",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/prefetchcache",
    srcs = [
        "cache.go",
    ],
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "prefetchcache_test",
    srcs = [
        "cache_test.go",
    ],
    target_under_test = ":prefetchcache",
    deps = [],
    visibility = ["PUBLIC"],
)
