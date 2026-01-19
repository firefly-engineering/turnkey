# Documentation build targets
#
# Build individual books:
#   tk build //docs/user-manual:user-manual
#   tk build //docs/developer-manual:developer-manual
#
# Serve for development:
#   tk run //docs/user-manual:user-manual
#   tk run //docs/developer-manual:developer-manual

# Alias to build all documentation
filegroup(
    name = "all",
    srcs = [
        "//docs/user-manual:user-manual",
        "//docs/developer-manual:developer-manual",
    ],
    visibility = ["PUBLIC"],
)
