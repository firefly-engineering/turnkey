load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "conditional",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/conditional",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/starlark:starlark",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "conditional_test",
    srcs = glob(["*_test.go"]),
    target_under_test = ":conditional",
    deps = [
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/starlark:starlark",
    ],
    visibility = ["PUBLIC"],
)
