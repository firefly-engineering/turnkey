# Syncer package for tk sync
load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "syncer",
    package_name = "github.com/firefly-engineering/turnkey/go/pkg/syncer",
    srcs = ["syncer.go"],
    deps = [
        "//go/pkg/staleness:staleness",
        "//go/pkg/syncconfig:syncconfig",
    ],
    visibility = ["PUBLIC"],
)
