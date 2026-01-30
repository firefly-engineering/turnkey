load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "buckgen",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/buckgen",
    srcs = [
        "config.go",
        "doc.go",
        "normalize.go",
        "render.go",
    ],
    deps = [
        "//src/go/pkg/goparse:goparse",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "buckgen_test",
    srcs = ["render_test.go"],
    deps = [
        "//src/go/pkg/goparse:goparse",
    ],
    target_under_test = ":buckgen",
    visibility = ["PUBLIC"],
)
