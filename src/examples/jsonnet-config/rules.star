load("@prelude//jsonnet:jsonnet.bzl", "jsonnet_library")

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
