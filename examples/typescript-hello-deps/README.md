# TypeScript External Dependencies Example

This example demonstrates using external npm packages with Buck2.

## Current Status

**Not functional** - requires npm/worker integration.

## Challenge

TypeScript/JavaScript in Buck2 is more complex than Go/Rust/Python:
- No `system_typescript_toolchain` in bundled prelude
- js_library rules require worker-based execution
- npm dependencies need custom handling

## Required Work

See issue `turnkey-ce6` for native TypeScript toolchain support.
See issue `turnkey-3xn` for npm deps cell generation.

## Future Integration

Once native support exists, this would look like:
```python
load("@prelude//:rules.bzl", "ts_binary")

ts_binary(
    name = "hello-deps",
    srcs = ["hello.ts"],
    deps = ["//third-party/npm:chalk"],
)
```
