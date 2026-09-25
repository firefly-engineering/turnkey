load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "testcache",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/testcache",
    srcs = ["testcache.go"],
    visibility = ["PUBLIC"],
)

go_test(
    name = "testcache_test",
    srcs = ["testcache_test.go"],
    target_under_test = ":testcache",
    visibility = ["PUBLIC"],
)
