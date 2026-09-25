load("@prelude//:rules.bzl", "export_file", "go_library", "go_test")

go_library(
    name = "testcache",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/testcache",
    srcs = ["testcache.go"],
    visibility = ["PUBLIC"],
)

go_test(
    name = "testcache_test",
    srcs = ["testcache_test.go"],
    embed_srcs = [
        "testdata/runner-contract.json",
        "testdata/shell-contract.json",
    ],
    target_under_test = ":testcache",
    visibility = ["PUBLIC"],
)

# What tk passes turnkey-test-runner and reads back, checked from both sides
export_file(
    name = "runner-contract",
    src = "testdata/runner-contract.json",
    visibility = ["//src/cmd/turnkey-test-runner/..."],
)
