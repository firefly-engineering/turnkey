load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "gen-rust-buck",
    main = "__main__.py",
    deps = [
        "//python/buck:buck",
        "//python/cargo:cargo",
        "//python/cfg:cfg",
    ],
    visibility = ["PUBLIC"],
)
