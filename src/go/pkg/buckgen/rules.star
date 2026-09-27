load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "buckgen",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/buckgen",
    srcs = [
        "config.go",
        "doc.go",
        "render.go",
    ],
    deps = [
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/goparse:goparse",
        "//src/go/pkg/starlark:starlark",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "buckgen_test",
    srcs = ["render_test.go"],
    deps = [
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/goparse:goparse",
    ],
    target_under_test = ":buckgen",
    visibility = ["PUBLIC"],
)
