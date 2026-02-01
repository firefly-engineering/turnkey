# snapshot - file content hashing for change detection
load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "snapshot",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/snapshot",
    srcs = ["snapshot.go"],
    deps = [],
    visibility = ["PUBLIC"],
)
