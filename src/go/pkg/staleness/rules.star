# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "staleness",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/staleness",
    srcs = ["staleness.go"],
    visibility = ["PUBLIC"],
)

go_test(
    name = "staleness_test",
    srcs = ["staleness_test.go"],
    target_under_test = ":staleness",
    visibility = ["PUBLIC"],
)
