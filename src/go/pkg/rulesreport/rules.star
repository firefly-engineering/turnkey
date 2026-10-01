load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "rulesreport",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/rulesreport",
    srcs = ["rulesreport.go"],
    visibility = ["PUBLIC"],
)
