load("@prelude//jsonnet:jsonnet.bzl", "jsonnet_library", "jsonnet_test")

# Build configuration for development environment
jsonnet_library(
    name = "config-dev",
    srcs = [
        "config.jsonnet",
        "common.libsonnet",
    ],
    out = "config-dev.json",
    ext_strs = {"env": "development"},
    visibility = ["PUBLIC"],
)

# Build configuration for production environment
jsonnet_library(
    name = "config-prod",
    srcs = [
        "config.jsonnet",
        "common.libsonnet",
    ],
    out = "config-prod.json",
    ext_strs = {"env": "production"},
    visibility = ["PUBLIC"],
)

# Jsonnet library for common functions (used as dep in test)
jsonnet_library(
    name = "common",
    srcs = ["common.libsonnet"],
    visibility = ["PUBLIC"],
)

# Test the common library using assertions
jsonnet_test(
    name = "common-test",
    src = "common_test.jsonnet",
    deps = [":common"],
)

# Test config output matches expected (golden file mode)
jsonnet_test(
    name = "config-dev-test",
    src = "config.jsonnet",
    golden = "config-dev.expected.json",
    deps = [":common"],
    ext_strs = {"env": "development"},
)
