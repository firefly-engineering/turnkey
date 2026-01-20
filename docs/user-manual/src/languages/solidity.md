# Solidity Support

Turnkey provides Solidity smart contract support with Buck2 integration, including compilation, testing with Foundry, and dependency management.

## Setup

Add to `toolchain.toml`:

```toml
[toolchains]
solidity = {}
foundry = {}
```

Enable Solidity dependencies in `flake.nix` (if using external libraries):

```nix
turnkey.toolchains.buck2.solidity = {
  enable = true;
  depsFile = ./solidity-deps.toml;
};
```

## Project Structure

```
my-project/
├── foundry.toml              # Foundry configuration
├── solidity-deps.toml        # Generated dependency manifest
├── src/
│   └── contracts/
│       ├── MyToken.sol
│       └── rules.star
└── test/
    ├── MyToken.t.sol
    └── rules.star
```

## Build Rules

### solidity_library

Compile Solidity source files:

```python
load("@prelude//solidity:solidity.bzl", "solidity_library")

solidity_library(
    name = "my_token",
    srcs = ["MyToken.sol"],
    deps = ["//soldeps:openzeppelin_contracts"],
    solc_version = "0.8.20",  # Optional: specify compiler version
    optimizer = True,
    optimizer_runs = 200,
)
```

### solidity_contract

Extract a specific contract from compiled sources:

```python
load("@prelude//solidity:solidity.bzl", "solidity_contract")

solidity_contract(
    name = "my_token_artifact",
    contract = "MyToken",  # Contract name in source
    deps = [":my_token"],
)
```

This produces:
- `{name}.abi` - Contract ABI (JSON)
- `{name}.bin` - Deployment bytecode

### solidity_test

Run tests with Foundry's `forge test`:

```python
load("@prelude//solidity:solidity.bzl", "solidity_test")

solidity_test(
    name = "my_token_test",
    srcs = ["MyToken.t.sol"],
    deps = [
        "//src/contracts:my_token",
        "//soldeps:forge-std",
    ],
    fuzz_runs = 256,  # Optional: fuzz test iterations
)
```

## External Dependencies

### OpenZeppelin and npm packages

Reference npm packages via the `soldeps` cell:

```python
solidity_library(
    name = "my_token",
    srcs = ["MyToken.sol"],
    deps = ["//soldeps:openzeppelin_contracts"],
)
```

In your Solidity file:

```solidity
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
```

Import remappings are auto-generated based on the dependency structure.

### Foundry git dependencies

Dependencies declared in `foundry.toml` are also supported:

```toml
[dependencies]
forge-std = "github:foundry-rs/forge-std"
solady = "github:vectorized/solady"
```

## Compiler Version

You can specify the Solidity compiler version per-target:

```python
solidity_library(
    name = "legacy_contract",
    srcs = ["Legacy.sol"],
    solc_version = "0.7.6",  # Use older compiler
)

solidity_library(
    name = "modern_contract",
    srcs = ["Modern.sol"],
    solc_version = "0.8.20",  # Use newer compiler
)
```

## Building and Testing

```bash
# Build contracts
tk build //src/contracts:my_token

# Run tests
tk test //test:my_token_test

# Build all Solidity targets
tk build //... --target-platforms //platforms:solidity
```

## Forge Integration

The `solidity_test` rule wraps Foundry's `forge test`, supporting:
- Unit tests
- Fuzz testing
- Fork testing (with `fork_url` attribute)
- Gas reports

```python
solidity_test(
    name = "integration_test",
    srcs = ["Integration.t.sol"],
    deps = [":my_token"],
    fork_url = "https://eth-mainnet.g.alchemy.com/v2/...",  # Optional
)
```
