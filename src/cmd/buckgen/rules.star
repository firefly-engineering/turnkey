go_binary(
    name = "buckgen",
    srcs = ["main.go"],
    deps = [
        "//src/go/pkg/buckgen:buckgen",
    ],
    visibility = ["PUBLIC"],
)
