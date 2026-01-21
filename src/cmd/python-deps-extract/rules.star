load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "python-deps-extract",
    package_name = "github.com/firefly-engineering/turnkey/src/cmd/python-deps-extract",
    srcs = glob(["*.go"]),
    deps = [
        "//src/go/pkg/extraction:extraction",
    ],
    visibility = ["PUBLIC"],
)
