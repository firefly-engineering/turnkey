# Rules package - utilities for managing rules.star files
go_library(
    name = "rules",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/rules",
    srcs = glob(["*.go"]),
    deps = [
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "rules_test",
    srcs = glob(["*_test.go"]),
    deps = [":rules"],
    visibility = ["PUBLIC"],
)
