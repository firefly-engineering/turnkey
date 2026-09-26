# Running Tests

Turnkey supports running tests via Buck2.

## Test Commands

```bash
# Run tests for a specific target
tk test //path/to:target-test

# Run all tests
tk test //...

# Run tests matching a pattern
tk test //src/examples/...
```

`tk test` reuses the recorded result of a test whose inputs haven't changed
instead of running it again; see [Test Result Caching](./test-result-caching.md).
Use `tk --rerun test` to run everything.

## Language-Specific Tests

### Go Tests

```bash
tk test //src/go/pkg/mypackage:mypackage_test
```

### Rust Tests

```bash
tk test //src/rust/mycrate:mycrate-test
```

### Python Tests

```bash
tk test //src/python/mymodule:test
```

## Test Output

Only tests that didn't pass are listed, each with its stdout and stderr; the
summary line counts the rest. To list passing tests with their output too:

```bash
tk test //... -- --print-passing-details
```

## Filtering Tests

Arguments after `--` go to the test runner, not the test binary. Pass them
on to the binary with `--test-arg`, which takes every argument after it, so
it comes last:

```bash
# Run specific test function (Go)
tk test //pkg:pkg_test -- --test-arg -test.run=TestSpecificFunction

# Run specific test (Rust)
tk test //crate:crate-test -- --test-arg specific_test_name
```

A filtered run has its own result key, so it doesn't reuse the unfiltered
run's result.

## Continuous Testing

For development, use Buck2's file watching:

```bash
tk test //path/to:target-test --watch
```
