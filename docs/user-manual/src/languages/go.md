# Go Support

Turnkey provides comprehensive Go support with Buck2 integration.

## Setup

Add to `toolchain.toml`:

```toml
[toolchains]
go = {}
godeps-gen = {}
```

Enable Go dependencies in `flake.nix`:

```nix
turnkey.toolchains.buck2.go = {
  enable = true;
  depsFile = ./go-deps.toml;
};
```

## Project Structure

```
my-project/
├── go.mod
├── go.sum
├── go-deps.toml          # Generated from go.mod
├── cmd/
│   └── myapp/
│       ├── main.go
│       └── rules.star
└── pkg/
    └── mylib/
        ├── lib.go
        └── rules.star
```

## Build Rules

In `rules.star`:

```python
load("@prelude//go:go.bzl", "go_binary", "go_library")

go_binary(
    name = "myapp",
    srcs = ["main.go"],
    deps = ["//pkg/mylib:mylib"],
)
```

## External Dependencies

Reference third-party packages via the `godeps` cell:

```python
go_library(
    name = "mylib",
    srcs = ["lib.go"],
    deps = ["godeps//github.com/pkg/errors:errors"],
)
```

## Auto-Sync

The `go` command is wrapped to auto-sync dependencies:

```bash
go get github.com/some/package  # Triggers sync
go mod tidy                      # Triggers sync
```
