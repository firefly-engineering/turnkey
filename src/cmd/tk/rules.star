# Auto-managed by turnkey. Hash: e94159bedae8df97
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "tk",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        "//src/go/pkg/localconfig:localconfig",
        "//src/go/pkg/rules:rules",
        "//src/go/pkg/syncconfig:syncconfig",
        "//src/go/pkg/syncer:syncer",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
