# Code Style

## Nix

### Formatting

- 2 space indentation
- Multi-line function parameters
- Aligned braces

```nix
{
  config,
  pkgs,
  lib,
  ...
}:
```

### Module Pattern

```nix
{
  options = {
    # Option definitions
  };

  config = lib.mkIf cfg.enable {
    # Implementation
  };
}
```

### Naming

- Use descriptive attribute names
- camelCase for local variables
- kebab-case for package names

## Starlark (Buck2)

### Rule Definitions

```python
def _my_rule_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of my_rule.

    Args:
        ctx: Analysis context from Buck2
    """
    pass

my_rule = rule(
    impl = _my_rule_impl,
    attrs = {
        "srcs": attrs.list(attrs.source()),
    },
    doc = "Short description of the rule.",
)
```

### Naming

- snake_case for functions and variables
- PascalCase for providers
- _prefix for private functions

## Go

Standard Go formatting with `gofmt`.

### Package Comments

```go
// Package syncer provides dependency synchronization.
package syncer
```

## Commit Messages

Use conventional commits:

```
feat: add zig toolchain support
fix: correct Python deps cell generation
docs: update troubleshooting guide
refactor: simplify registry pattern
```

Prefix types:
- `feat:` - New features
- `fix:` - Bug fixes
- `docs:` - Documentation
- `refactor:` - Code restructuring
- `test:` - Test additions
- `chore:` - Maintenance
