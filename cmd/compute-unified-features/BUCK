load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "compute-unified-features",
    main = "__main__.py",
    deps = [
        "//python/cargo:cargo",
        "//python/cfg:cfg",
    ],
    visibility = ["PUBLIC"],
)
