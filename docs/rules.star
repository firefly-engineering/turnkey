# Documentation build targets
#
# Build individual books:
#   tk build //docs/user-manual:user-manual
#   tk build //docs/developer-manual:developer-manual
#
# Build both:
#   tk build //docs/user-manual:user-manual //docs/developer-manual:developer-manual
#
# Serve for development:
#   tk run //docs/user-manual:user-manual
#   tk run //docs/developer-manual:developer-manual

# Export both book targets for convenience
# Note: Use individual targets above rather than this filegroup for building
# since both books output a directory named "book"
export_file(
    name = "README",
    src = "README.md",
    visibility = ["PUBLIC"],
)
