# Running Tests

Turnkey supports running tests via Buck2.

## Test Commands

```bash
# Run tests for a specific target
tk test //path/to:target-test

# Run all tests
tk test //...

# Run tests matching a pattern
tk test //examples/...
```

## Language-Specific Tests

### Go Tests

```bash
tk test //go/pkg/mypackage:mypackage_test
```

### Rust Tests

```bash
tk test //rust/mycrate:mycrate-test
```

### Python Tests

```bash
tk test //python/mymodule:test
```

## Test Output

Test results are displayed in the console. For detailed output:

```bash
tk test //... -- --nocapture
```

## Filtering Tests

Pass arguments after `--` to the test runner:

```bash
# Run specific test function (Go)
tk test //pkg:pkg_test -- -run TestSpecificFunction

# Run specific test (Rust)
tk test //crate:crate-test -- specific_test_name
```

## Continuous Testing

For development, use Buck2's file watching:

```bash
tk test //path/to:target-test --watch
```
