load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "cargocfg",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/cargocfg",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = ["//src/go/pkg/conditions:conditions"],
    visibility = ["PUBLIC"],
)

go_test(
    name = "cargocfg_test",
    srcs = glob(["*_test.go"]),
    embed_srcs = ["testdata/cfg-vectors.json"],
    target_under_test = ":cargocfg",
    deps = ["//src/go/pkg/conditions:conditions"],
    visibility = ["PUBLIC"],
)
