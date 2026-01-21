# Auto-managed by turnkey. Hash: f388892cc619788f
# Manual sections marked with turnkey:preserve-start/end are not modified.

python_binary(
    name = "hello-python",
    deps = [
        # turnkey:auto-start
        "pydeps//vendor/six:six",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
