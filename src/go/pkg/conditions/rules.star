load("@prelude//:rules.bzl", "export_file", "go_library", "go_test")

go_library(
    name = "conditions",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/conditions",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "conditions_test",
    srcs = glob(["*_test.go"]),
    embed_srcs = ["testdata/split-vectors.json"],
    target_under_test = ":conditions",
    deps = [],
    visibility = ["PUBLIC"],
)

# The split test cases, run by rust-rules-gen's tests too
export_file(
    name = "split-vectors",
    src = "testdata/split-vectors.json",
    visibility = [
        "//src/cmd/rust-rules-gen/...",
    ],
)
