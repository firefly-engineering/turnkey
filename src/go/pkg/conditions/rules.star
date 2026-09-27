load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "conditions",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/conditions",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "conditions_test",
    srcs = glob(["*_test.go"]),
    target_under_test = ":conditions",
    deps = [],
    visibility = ["PUBLIC"],
)
