load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "goparse",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/goparse",
    srcs = [
        "constraints.go",
        "doc.go",
        "parser.go",
        "scanner.go",
        "tags.go",
        "types.go",
    ],
    deps = ["//src/go/pkg/conditions:conditions"],
    visibility = ["PUBLIC"],
)

go_test(
    name = "goparse_test",
    srcs = [
        "parser_test.go",
        "tags_test.go",
    ],
    target_under_test = ":goparse",
    deps = ["//src/go/pkg/conditions:conditions"],
    visibility = ["PUBLIC"],
)
