load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "starlark",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/starlark",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [
        "godeps//vendor/go.starlark.net/syntax:syntax",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "starlark_test",
    srcs = glob(["*_test.go"]),
    target_under_test = ":starlark",
    deps = [],
    visibility = ["PUBLIC"],
)
