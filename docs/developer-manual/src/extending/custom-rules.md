# Custom Rules

Create custom Buck2 rules for your project.

## Project-Local Rules

For rules specific to your project, create a `rules/` directory:

```
my-project/
├── rules/
│   ├── myrule.bzl
│   └── rules.star
└── rules.star
```

In `rules/rules.star`:

```python
# Export rules from this cell
load(":myrule.bzl", "my_rule")
```

Reference from your project:

```python
load("//rules:myrule.bzl", "my_rule")

my_rule(
    name = "example",
    # ...
)
```

## Prelude-Level Rules

For rules you want available across multiple projects, add them as prelude extensions. See [Prelude Extensions](./prelude-extensions.md).

## Rule Structure

### Basic Rule

```python
def _my_rule_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("output.txt")

    ctx.actions.run(
        cmd_args("echo", "hello", ">", out.as_output()),
        category = "my_rule",
    )

    return [DefaultInfo(default_output = out)]

my_rule = rule(
    impl = _my_rule_impl,
    attrs = {
        "srcs": attrs.list(attrs.source()),
    },
)
```

### With Toolchain

```python
def _my_rule_impl(ctx):
    toolchain = ctx.attrs._toolchain[MyToolchainInfo]
    # Use toolchain...

my_rule = rule(
    impl = _my_rule_impl,
    attrs = {
        "_toolchain": attrs.toolchain_dep(
            default = "toolchains//:mytool",
            providers = [MyToolchainInfo],
        ),
    },
)
```

### With RunInfo

```python
def _my_binary_impl(ctx):
    out = ctx.actions.declare_output(ctx.label.name)
    # Build the binary...

    run_info = RunInfo(args = cmd_args(out))

    return [
        DefaultInfo(default_output = out),
        run_info,  # Makes it runnable with `buck2 run`
    ]
```

## Best Practices

1. Use categories in `ctx.actions.run()` for build output
2. Declare all outputs explicitly
3. Use hidden deps for non-output dependencies
4. Provide sensible defaults for optional attrs
