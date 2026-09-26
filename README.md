# Turnkey

Turnkey is a polyglot development environment and build system designed for seamless integration of Go, Rust, and Python projects. It leverages **Nix** for hermetic environments and **Buck2** for fast, incremental builds.

## Getting Started

### Prerequisites

1.  **Nix**: Install Nix (with flake support enabled).
2.  **Direnv**: Install `direnv` and hook it into your shell.

### Setup

1.  Clone the repository.
2.  Enter the directory and allow direnv to load the environment:
    ```bash
    direnv allow
    ```
    This will automatically download and configure all necessary tools (Go, Rust, Python, Buck2, etc.) in a hermetic environment.


## Tools

Turnkey provides two main CLI tools to streamline development:

### `tk` - The Build Wrapper

`tk` is a smart wrapper around **Buck2**. It ensures that your build graph and generated files (like `rules.star` and dependency cells) are always up-to-date before running any build commands.

**Usage:**
- `tk build //...`: Build all targets (syncs automatically).
- `tk test //...`: Run all tests.
- `tk sync`: Explicitly sync generated files.
- `tk check`: Check for staleness (useful for CI).

### `tw` - The Tool Wrapper

`tw` is a wrapper for native language tools (`go`, `cargo`, `uv`). It monitors dependency files (like `go.mod`, `Cargo.toml`) and automatically triggers a sync if they change after running a command. This keeps your Nix files and Buck2 rules in sync with your native package managers.

**Usage:**
- `tw go get github.com/pkg/errors`: Add a Go dependency and sync.
- `tw cargo add serde`: Add a Rust dependency and sync.
- `tw uv add requests`: Add a Python dependency and sync.

## Workflow

Work is tracked in [GitHub Issues](https://github.com/firefly-engineering/turnkey/issues), prioritised in the [turnkey project](https://github.com/orgs/firefly-engineering/projects/3).

- **Find work**: `gh issue list --search "-is:blocked no:assignee"`
- **Claim work**: `gh issue edit <n> --add-assignee @me`
- **Submit work**: commit with `Fixes #<n>`, then push

See `AGENTS.md` for detailed workflow instructions.
