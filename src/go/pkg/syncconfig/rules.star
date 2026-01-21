# Auto-managed by turnkey. Hash: 8338370964de6cbf
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "syncconfig",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig",
    srcs = ["syncconfig.go"],
    deps = [
        # turnkey:auto-start
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "syncconfig_test",
    srcs = ["syncconfig_test.go"],
    target_under_test = ":syncconfig",
    deps = [],
    visibility = ["PUBLIC"],
)
