# deps are left empty on purpose: the go-coverage e2e test runs rules sync,
# which must fill them in from the imports.

go_library(
    name = "text",
    package_name = "example.com/lib/text",
    srcs = ["text.go"],
    deps = [],
    visibility = ["PUBLIC"],
)
