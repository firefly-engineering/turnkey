load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "buck2args",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/buck2args",
    srcs = ["buck2args.go"],
    visibility = ["PUBLIC"],
)

go_test(
    name = "buck2args_test",
    srcs = ["buck2args_test.go"],
    target_under_test = ":buck2args",
    visibility = ["PUBLIC"],
)
