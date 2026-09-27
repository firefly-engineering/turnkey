load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "pydepscell",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/pydepscell",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [
        "//src/go/pkg/conditional:conditional",
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/pep508:pep508",
        "//src/go/pkg/starlark:starlark",
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "pydepscell_test",
    srcs = glob(["*_test.go"]),
    target_under_test = ":pydepscell",
    deps = [
        "//src/go/pkg/conditions:conditions",
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
    ],
    visibility = ["PUBLIC"],
)
