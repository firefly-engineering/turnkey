load("@prelude//:rules.bzl", "export_file", "go_library", "go_test")

go_library(
    name = "starlark",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/starlark",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = [
        "godeps//vendor/go.starlark.net/syntax:syntax",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "starlark_test",
    srcs = glob(["*_test.go"]),
    embed_srcs = ["testdata/syntax-vectors.json"],
    target_under_test = ":starlark",
    deps = ["godeps//vendor/go.starlark.net/syntax:syntax"],
    visibility = ["PUBLIC"],
)

# The syntax trees go.starlark.net builds, run by the rules-star crate's
# tests too
export_file(
    name = "syntax-vectors",
    src = "testdata/syntax-vectors.json",
    visibility = ["//src/rust/rules-star/..."],
)
