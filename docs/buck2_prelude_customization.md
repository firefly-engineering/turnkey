# Buck2 Prelude Customization Options

This document explores options for customizing the Buck2 prelude for turnkey's needs,
based on research into Buck2's architecture and how other projects handle customization.

## Background

The Buck2 prelude is a collection of Starlark rules that provide build functionality for
various languages (Go, Rust, Python, C++, etc.). It's essentially the "standard library"
of build rules that ships with Buck2.

Turnkey currently uses `prelude.strategy = "bundled"` which uses the prelude embedded
in the Buck2 binary. As turnkey evolves, we may need prelude modifications for:

- Enhanced Go rules (better Nix integration)
- Custom dependency cell patterns
- Platform/toolchain customizations
- Nix-specific build patterns

## External Cell Origins

Buck2 supports three external cell origins ([docs](https://buck2.build/docs/users/advanced/external_cells/)):

### 1. Bundled Origin

Reserved exclusively for the prelude cell. Uses the prelude embedded in the Buck2 binary.

```ini
[cells]
prelude = prelude

[external_cells]
prelude = bundled
```

**Pros:**
- Simplest setup - no additional files needed
- Always matches the Buck2 version
- Single binary deployment

**Cons:**
- Cannot customize the prelude
- Locked to Buck2's release cycle

### 2. Git Origin

Fetches the prelude from a git repository at a pinned commit.

```ini
[cells]
prelude = prelude

[external_cells]
prelude = git

[external_cell_prelude]
git_origin = https://github.com/facebook/buck2-prelude.git
commit_hash = abc123def456...  # Must be full SHA1, not branch name
```

**Pros:**
- Can pin to specific prelude version
- Works with forks of the prelude
- Standard git workflow for updates

**Cons:**
- Requires network access during initial load
- Must manually track upstream changes
- Commit hash must be updated explicitly

### 3. Path (Non-External)

Use a local directory as the prelude cell (not technically an "external" cell).

```ini
[cells]
prelude = path/to/prelude
```

**Pros:**
- Full control over prelude content
- Can use git submodule or vendored copy
- Works offline

**Cons:**
- Must maintain the prelude copy
- More files in repository

## Turnkey's Current Strategies

Turnkey supports four prelude strategies (defined in `nix/devenv/turnkey/buck2.nix`):

| Strategy | Description | Use Case |
|----------|-------------|----------|
| `bundled` | Buck2's built-in prelude | Default, simplest setup |
| `git` | Git external cell | Pinned upstream version |
| `nix` | Nix derivation | Prelude from Nix store |
| `path` | Filesystem path | Local/vendored prelude |

Example configuration:

```nix
devenv.shells.default = {
  turnkey.buck2 = {
    enable = true;
    prelude.strategy = "git";
    prelude.gitOrigin = "https://github.com/facebook/buck2-prelude.git";
    prelude.commitHash = "abc123...";
  };
};
```

## Customization Approaches

### Approach 1: Extension Cell Pattern (Recommended)

**Used by:** [System Initiative](https://github.com/systeminit/si)

Create a separate cell for custom rules alongside the standard prelude:

```
project/
├── prelude/           # Standard prelude (submodule or external)
├── prelude-custom/    # Custom extensions
│   ├── BUCK
│   ├── platforms/
│   ├── toolchains/
│   └── rules/
└── .buckconfig
```

```ini
[cells]
prelude = prelude
prelude-custom = prelude-custom

[external_cells]
prelude = bundled

[build]
execution_platforms = prelude-custom//platforms:default
```

**Key insight:** System Initiative uses `prelude-si//platforms:default` for execution
platforms, allowing their custom platform definitions while keeping the standard prelude.

Their `prelude-si` contains ([source](https://github.com/systeminit/si/tree/main/prelude-si)):
- Custom rules for Docker, Nix, pnpm, e2e testing
- Platform definitions
- Toolchain configurations
- Language-specific extensions (Rust, Python, Deno)

**Pros:**
- Clean separation of concerns
- Can still use bundled prelude for core rules
- Easy to track what's custom vs standard
- No fork maintenance burden

**Cons:**
- Two cells to manage
- Must understand which rules come from where

### Approach 2: Custom Rules Outside Prelude

Define rules anywhere in your project - they don't need to be in the prelude
([docs](https://buck2.build/docs/rule_authors/writing_rules/)):

```python
# rules/my_rules.bzl
def my_custom_rule_impl(ctx):
    # Implementation
    pass

my_custom_rule = rule(
    impl = my_custom_rule_impl,
    attrs = {
        "src": attrs.source(),
        "deps": attrs.list(attrs.dep()),
    },
)
```

```python
# BUCK
load("//rules:my_rules.bzl", "my_custom_rule")

my_custom_rule(
    name = "my_target",
    src = "input.txt",
)
```

**Pros:**
- No prelude modification needed
- Explicit `load()` makes dependencies clear
- Rules live with the project

**Cons:**
- Must use explicit `load()` statements
- Not globally available like prelude rules

### Approach 3: Custom Toolchains

Define toolchains that return prelude-compatible providers
([docs](https://buck2.build/docs/rule_authors/writing_toolchains/)):

```python
load("@prelude//toolchains:cxx.bzl", "system_cxx_toolchain")

# Customize the C++ toolchain
system_cxx_toolchain(
    name = "cxx",
    compiler_type = "clang",
    cxx_flags = ["-std=c++23", "-Wall", "-Werror"],
    visibility = ["PUBLIC"],
)
```

This is already what turnkey does in the generated toolchains cell. For more control,
define entirely custom toolchain rules that return the expected `*ToolchainInfo` providers.

### Approach 4: Forked Prelude

Maintain a fork of the Buck2 prelude with your modifications.

```ini
[external_cells]
prelude = git

[external_cell_prelude]
git_origin = https://github.com/your-org/buck2-prelude-fork.git
commit_hash = your-fork-commit-hash
```

**Pros:**
- Complete control over all rules
- Can modify any prelude behavior

**Cons:**
- Significant maintenance burden
- Must track upstream changes
- Risk of divergence from upstream

### Approach 5: Patched Buck2 Binary

Build a custom Buck2 with a patched bundled prelude.

**Pros:**
- Single binary deployment
- Changes are invisible to users

**Cons:**
- Must maintain Buck2 build infrastructure
- Rebuild on every Buck2 update
- Most complex approach

## Recommendation for Turnkey

Based on the research, the **Extension Cell Pattern** is recommended:

1. **Keep using bundled prelude** for core language rules
2. **Create a `turnkey-rules` cell** for custom functionality:
   - Nix-specific rules (if needed)
   - Custom dependency management rules
   - Platform/toolchain extensions

This mirrors how we already structure cells:
- `toolchains` - generated toolchain definitions
- `godeps`, `rustdeps`, `pydeps` - dependency cells
- `turnkey-rules` (proposed) - custom build rules

Example future structure:

```ini
[cells]
root = .
prelude = prelude
toolchains = .turnkey/toolchains
turnkey = .turnkey/rules      # New: custom rules cell
godeps = .turnkey/godeps
```

## When to Customize

Consider prelude customization when:

1. **Built-in rules don't support your workflow** - e.g., Nix-specific build patterns
2. **You need enhanced toolchain control** - beyond what system toolchains provide
3. **Platform definitions need modification** - custom constraint values
4. **You're integrating with external systems** - CI/CD, remote execution

For turnkey specifically, potential needs include:
- Enhanced Go rules with better module support
- Rust rules that understand Cargo workspaces
- Python rules with virtual environment support
- Generic "Nix build" rules for hermetic builds

## References

- [Buck2 External Cells Documentation](https://buck2.build/docs/users/advanced/external_cells/)
- [Buck2 Writing Rules](https://buck2.build/docs/rule_authors/writing_rules/)
- [Buck2 Writing Toolchains](https://buck2.build/docs/rule_authors/writing_toolchains/)
- [Buck2 Prelude Repository](https://github.com/facebook/buck2-prelude)
- [System Initiative's prelude-si](https://github.com/systeminit/si/tree/main/prelude-si) - Real-world extension cell example
- [Tweag's Buck2 Tour](https://www.tweag.io/blog/2023-07-06-buck2/) - Good overview of Buck2 architecture

## Related Issues

- `turnkey-vtk`: Symlink version selection (affects how deps cells work)
- `turnkey-ce6`: TypeScript support (may need custom rules)
- `turnkey-yu7`: Revisit gobuckify (already touching prelude code)
