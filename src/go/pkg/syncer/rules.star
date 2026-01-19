# Syncer package for tk sync
load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "syncer",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/syncer",
    srcs = ["syncer.go"],
    deps = [
        "//src/go/pkg/staleness:staleness",
        "//src/go/pkg/syncconfig:syncconfig",
    ],
    visibility = ["PUBLIC"],
)
