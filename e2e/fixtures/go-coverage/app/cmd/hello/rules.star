# deps are left empty on purpose: the go-coverage e2e test runs rules sync,
# which must fill them in from the imports.

go_binary(
    name = "hello",
    srcs = ["main.go"],
    deps = [],
    visibility = ["PUBLIC"],
)
