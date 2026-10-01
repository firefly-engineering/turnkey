load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "rules-sync",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        "//src/go/pkg/rulesreport:rulesreport",
        "//src/go/pkg/rulessync:rulessync",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
