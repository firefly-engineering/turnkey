# deps are left empty on purpose: the go-coverage e2e test runs rules sync,
# which must fill them in from the imports.

go_library(
    name = "dump",
    package_name = "example.com/app/dump",
    srcs = ["dump.go"],
    deps = [],
    visibility = ["PUBLIC"],
)
