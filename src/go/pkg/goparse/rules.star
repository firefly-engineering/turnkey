load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "goparse",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/goparse",
    srcs = [
        "constraints.go",
        "doc.go",
        "parser.go",
        "scanner.go",
        "types.go",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "goparse_test",
    srcs = ["parser_test.go"],
    target_under_test = ":goparse",
    visibility = ["PUBLIC"],
)
