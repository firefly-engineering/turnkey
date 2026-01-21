# Auto-managed by turnkey. Hash: 114130d7def66570
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "godeps",
    srcs = [
        "output.go",
        "parser.go",
        "prefetch.go",
        "types.go",
    ],
    deps = [
        # turnkey:auto-start
        "godeps//vendor/golang.org/x/mod/modfile:modfile",
        # turnkey:auto-end
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
        # turnkey:auto-start
        "godeps//vendor/golang.org/x/mod/modfile:modfile",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
