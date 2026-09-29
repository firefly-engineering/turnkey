# materialize - keep a deps cell's directory in line with its cell index
load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "materialize",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/materialize",
    srcs = ["materialize.go"],
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "materialize_test",
    srcs = ["materialize_test.go"],
    target_under_test = ":materialize",
    deps = [],
    visibility = ["PUBLIC"],
)
