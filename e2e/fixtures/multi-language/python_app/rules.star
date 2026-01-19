# Python binary
python_binary(
    name = "hello-python",
    main = "hello.py",
    deps = ["pydeps//vendor/six:six"],
    visibility = ["PUBLIC"],
)
