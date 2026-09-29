load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "cargofeatures",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/cargofeatures",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "cargofeatures_test",
    srcs = glob(["*_test.go"]),
    embed_srcs = ["testdata/activation-vectors.json"],
    target_under_test = ":cargofeatures",
    deps = [],
    visibility = ["PUBLIC"],
)
