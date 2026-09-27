# wrap - runs native tools for tw, syncing the deps they change

load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "wrap",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/wrap",
    srcs = ["wrap.go"],
    deps = [
        "//src/go/pkg/snapshot:snapshot",
        "//src/go/pkg/syncconfig:syncconfig",
        "//src/go/pkg/syncer:syncer",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "wrap_test",
    srcs = ["wrap_test.go"],
    target_under_test = ":wrap",
    deps = ["//src/go/pkg/syncconfig:syncconfig"],
    visibility = ["PUBLIC"],
)
