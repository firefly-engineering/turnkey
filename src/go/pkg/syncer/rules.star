# Auto-managed by turnkey. Hash: 0002ac08d9f5e083
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "syncer",
    srcs = ["syncer.go"],
    deps = [
        # turnkey:auto-start
        "//src/go/pkg/staleness:staleness",
        "//src/go/pkg/syncconfig:syncconfig",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
