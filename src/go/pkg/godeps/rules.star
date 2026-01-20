# godeps - Go dependency parsing and prefetching library
load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "godeps",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/godeps",
    srcs = [
        "output.go",
        "parser.go",
        "prefetch.go",
        "types.go",
    ],
    deps = [
        "godeps//vendor/golang.org/x/mod/modfile:modfile",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "godeps_test",
    srcs = [
        "integration_test.go",
        "output_test.go",
        "parser_test.go",
        "prefetch_test.go",
    ],
    deps = [
        "godeps//vendor/golang.org/x/mod/modfile:modfile",
    ],
    resources = ["//src/testdata:godeps_fixtures"],
    target_under_test = ":godeps",
)
