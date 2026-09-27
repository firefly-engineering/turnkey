load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "mapper",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/mapper",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [
        "//src/go/pkg/cargocfg:cargocfg",
        "//src/go/pkg/cargofeatures:cargofeatures",
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/extraction:extraction",
        "//src/go/pkg/starlark:starlark",
        "//src/go/pkg/syncconfig:syncconfig",
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
        "godeps//vendor/golang.org/x/mod/modfile:modfile",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "mapper_test",
    srcs = glob(["*_test.go"]),
    target_under_test = ":mapper",
    deps = [
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/extraction:extraction",
        "//src/go/pkg/godeps:godeps",
        "//src/go/pkg/starlark:starlark",
    ],
    visibility = ["PUBLIC"],
)
