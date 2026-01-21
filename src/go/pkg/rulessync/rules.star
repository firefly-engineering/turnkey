load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "rulessync",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/rulessync",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [
        "//src/go/pkg/extraction:extraction",
        "//src/go/pkg/mapper:mapper",
        "//src/go/pkg/starlark:starlark",
    ],
    visibility = ["PUBLIC"],
)
