load("@prelude//typescript:typescript.bzl", "typescript_library", "typescript_test")

typescript_test(
    name = "esm",
    main = "esm.mts",
    srcs = ["esm.mts"],
    npm_deps = [
        "jsdeps//:@tkfixture/cyc-a",
        "jsdeps//:@tkfixture/host",
        "jsdeps//:@tkfixture/plugin",
        "jsdeps//:@tkfixture/wrapper",
        "jsdeps//:@types/micromatch",
        "jsdeps//:micromatch",
        "jsdeps//:p-limit",
        # turnkey:preserve-start
        # Not imported: required by name, and node's own types
        "jsdeps//:picomatch",
        "jsdeps//:@types/node",
        # turnkey:preserve-end
    ],
)

typescript_test(
    name = "cjs",
    main = "cjs.cts",
    srcs = ["cjs.cts"],
    npm_deps = [
        "jsdeps//:@tkfixture/cyc-a",
        "jsdeps//:@tkfixture/host",
        "jsdeps//:@tkfixture/plugin",
        "jsdeps//:@tkfixture/wrapper",
        "jsdeps//:@types/micromatch",
        "jsdeps//:micromatch",
        "jsdeps//:p-limit",
        # turnkey:preserve-start
        # Not imported: required by name, and node's own types
        "jsdeps//:picomatch",
        "jsdeps//:@types/node",
        # turnkey:preserve-end
    ],
)

typescript_library(
    name = "undeclared",
    srcs = ["undeclared.ts"],
    npm_deps = [
        # turnkey:preserve-start
        # The fixture's direct deps, none of which declares what it imports
        "jsdeps//:@tkfixture/cyc-a",
        "jsdeps//:@types/micromatch",
        "jsdeps//:@types/node",
        "jsdeps//:micromatch",
        # turnkey:preserve-end
    ],
)
