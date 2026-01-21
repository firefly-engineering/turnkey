# tk CLI - turnkey wrapper for buck2
load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "tk",
    srcs = glob(["*.go"]),
    deps = [
        "//src/go/pkg/localconfig:localconfig",
        "//src/go/pkg/rules:rules",
        "//src/go/pkg/syncconfig:syncconfig",
        "//src/go/pkg/syncer:syncer",
    ],
    visibility = ["PUBLIC"],
)
