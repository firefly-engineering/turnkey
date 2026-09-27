load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "rulessync",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/rulessync",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/mapper:mapper",
        "//src/go/pkg/starlark:starlark",
        "//src/go/pkg/syncconfig:syncconfig",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "rulessync_test",
    srcs = glob(["*_test.go"]),
    # The sync.toml turnkey writes, checked from both sides
    embed_srcs = ["testdata/sync.toml"],
    target_under_test = ":rulessync",
    deps = [
        "//src/go/pkg/conditions:conditions",
        "//src/go/pkg/mapper:mapper",
        "//src/go/pkg/starlark:starlark",
        "//src/go/pkg/syncconfig:syncconfig",
    ],
    visibility = ["PUBLIC"],
)
