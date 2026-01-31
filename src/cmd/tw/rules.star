# tw - native tool wrapper with auto-sync
load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "tw",
    srcs = ["main.go"],
    deps = [
        "//src/go/pkg/snapshot:snapshot",
        "//src/go/pkg/syncconfig:syncconfig",
        "//src/go/pkg/syncer:syncer",
    ],
    visibility = ["PUBLIC"],
)
