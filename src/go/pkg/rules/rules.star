# Auto-managed by turnkey. Hash: 4a36ae7ce5af5da5
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "rules",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/rules",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
        "godeps//vendor/go.starlark.net/syntax:syntax",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "rules_test",
    srcs = glob(["*_test.go"]),
    target_under_test = ":rules",
    deps = [
        # turnkey:auto-start
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
        "godeps//vendor/go.starlark.net/syntax:syntax",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
