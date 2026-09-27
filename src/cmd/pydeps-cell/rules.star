load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "pydeps-cell",
    srcs = ["main.go"],
    deps = ["//src/go/pkg/pydepscell:pydepscell"],
    visibility = ["PUBLIC"],
)
