# Auto-managed by turnkey. Hash: f388892cc619788f
# Manual sections marked with turnkey:preserve-start/end are not modified.

python_binary(
    name = "hello-python",
    main = "hello.py",
    deps = ["pydeps//vendor/six:six"],
    visibility = ["PUBLIC"],
)
