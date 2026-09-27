load("@prelude//:rules.bzl", "export_file", "go_library", "go_test")

go_library(
    name = "pep508",
    package_name = "github.com/firefly-engineering/turnkey/src/go/pkg/pep508",
    srcs = glob(["*.go"], exclude = ["*_test.go"]),
    deps = ["//src/go/pkg/conditions:conditions"],
    visibility = ["PUBLIC"],
)

go_test(
    name = "pep508_test",
    srcs = glob(["*_test.go"]),
    embed_srcs = ["testdata/pep508-vectors.json"],
    target_under_test = ":pep508",
    deps = ["//src/go/pkg/conditions:conditions"],
    visibility = ["PUBLIC"],
)

# The PEP 508 cases, run by pydeps-gen's tests too
export_file(
    name = "pep508-vectors",
    src = "testdata/pep508-vectors.json",
    visibility = ["//src/cmd/pydeps-gen/..."],
)
