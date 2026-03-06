# cellfresh - detect cell symlink changes and restart buck2 daemon
load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "cellfresh",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/cellfresh",
    srcs = ["cellfresh.go"],
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "cellfresh_test",
    srcs = ["cellfresh_test.go"],
    target_under_test = ":cellfresh",
    deps = [],
    visibility = ["PUBLIC"],
)
