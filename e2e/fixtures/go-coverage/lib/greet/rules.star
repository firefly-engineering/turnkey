# deps are left empty on purpose: the go-coverage e2e test runs rules sync,
# which must fill them in from the imports.

go_library(
    name = "greet",
    package_name = "example.com/lib/greet",
    srcs = ["greet.go"],
    deps = [],
    visibility = ["PUBLIC"],
)

go_test(
    name = "greet_test",
    # The internal test (package greet) and the external one (greet_test)
    srcs = [
        "greet_external_test.go",
        "greet_test.go",
    ],
    target_under_test = ":greet",
    # Compiled in through //go:embed
    embed_srcs = ["testdata/cases.txt"],
    # Copied next to the test binary, read at run time
    resources = ["testdata/golden.txt"],
    deps = [],
)
