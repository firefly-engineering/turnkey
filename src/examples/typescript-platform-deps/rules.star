load("@prelude//typescript:typescript.bzl", "typescript_binary")

typescript_binary(
    name = "typescript-platform-deps",
    main = "main.ts",
    srcs = ["main.ts"],
    npm_deps = [
        # turnkey:preserve-start
        # Not imported: the example only checks that the platform's
        # optional dependencies reach node_modules
        "jsdeps//:chokidar",
        # turnkey:preserve-end
    ],
    visibility = ["PUBLIC"],
)
