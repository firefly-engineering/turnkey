# tk CLI - turnkey wrapper for buck2
load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "tk",
    srcs = ["main.go"],
    deps = [
        "//src/go/pkg/syncconfig:syncconfig",
        "//src/go/pkg/syncer:syncer",
    ],
    visibility = ["PUBLIC"],
)
