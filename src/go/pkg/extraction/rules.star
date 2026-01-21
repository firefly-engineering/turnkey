load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "extraction",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/extraction",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "extraction_test",
    srcs = glob(["*_test.go"]),
    target_under_test = ":extraction",
    deps = [],
    visibility = ["PUBLIC"],
)
