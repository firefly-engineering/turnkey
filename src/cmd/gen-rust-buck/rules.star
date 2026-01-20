load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "gen-rust-buck",
    main = "__main__.py",
    deps = [
        "//src/python/buck:buck",
        "//src/python/cargo:cargo",
        "//src/python/cfg:cfg",
    ],
    visibility = ["PUBLIC"],
)
