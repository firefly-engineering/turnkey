# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//solidity:solidity.bzl", "solidity_contract", "solidity_library", "solidity_test")

# Compile the Counter contract source
solidity_library(
    name = "counter_lib",
    srcs = ["src/Counter.sol"],
    optimizer = True,
    optimizer_runs = 200,
    visibility = ["PUBLIC"],
)

# Extract the Counter contract artifacts (ABI, bytecode)
solidity_contract(
    name = "counter",
    contract = "Counter",
    lib = ":counter_lib",
    visibility = ["PUBLIC"],
)

# Run Foundry tests
solidity_test(
    name = "counter_test",
    srcs = ["test/Counter.t.sol"],
    deps = [":counter_lib"],
    fuzz_runs = 256,
    visibility = ["PUBLIC"],
)
