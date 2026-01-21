# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//solidity:solidity.bzl", "solidity_contract", "solidity_library", "solidity_test")

solidity_library(
    name = "counter_lib",
    srcs = ["src/Counter.sol"],
    visibility = ["PUBLIC"],
)

solidity_contract(
    name = "counter",
    visibility = ["PUBLIC"],
)

solidity_test(
    name = "counter_test",
    srcs = ["test/Counter.t.sol"],
    visibility = ["PUBLIC"],
)
