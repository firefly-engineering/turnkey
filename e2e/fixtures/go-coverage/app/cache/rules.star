# deps are left empty on purpose: the go-coverage e2e test runs rules sync,
# which must fill them in from the imports.

go_library(
    name = "cache",
    package_name = "example.com/app/cache",
    srcs = ["cache.go"],
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "cache_test",
    srcs = ["cache_test.go"],
    target_under_test = ":cache",
    deps = [],
)
