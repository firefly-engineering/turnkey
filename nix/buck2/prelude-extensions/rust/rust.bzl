# Rust build helpers for Buck2
#
# This module provides wrappers around the prelude's rust rules that add
# common functionality needed for our crates, particularly passing
# Cargo-style environment variables that Buck2 doesn't set automatically.

load("@prelude//:rules.bzl", _rust_binary = "rust_binary", _rust_library = "rust_library", _rust_test = "rust_test")

def rust_binary(
        name,
        version = None,
        cargo_pkg_name = None,
        env = {},
        **kwargs):
    """
    Wrapper around rust_binary that adds Cargo-style environment variables.

    When building with Buck2, Cargo's environment variables like CARGO_PKG_VERSION
    are not automatically set (since Buck2 doesn't run build.rs). This wrapper
    makes it easy to pass these variables explicitly.

    Args:
        name: The target name
        version: Package version (sets CARGO_PKG_VERSION). If provided, the Rust
            code can access it via `option_env!("CARGO_PKG_VERSION")`.
        cargo_pkg_name: Package name (sets CARGO_PKG_NAME). Defaults to `name`.
        env: Additional environment variables to pass to rustc
        **kwargs: All other arguments are passed through to rust_binary

    Example:
        ```starlark
        load("@prelude-extensions//rust:rust.bzl", "rust_binary")

        rust_binary(
            name = "my-tool",
            version = "1.2.3",  # Same as in Cargo.toml
            srcs = glob(["src/**/*.rs"]),
            deps = [...],
        )
        ```

        Then in Rust code:
        ```rust
        fn main() {
            let version = option_env!("CARGO_PKG_VERSION").unwrap_or("unknown");
            println!("my-tool version {}", version);
        }
        ```
    """
    # Build up the environment variables
    cargo_env = {}

    if version != None:
        cargo_env["CARGO_PKG_VERSION"] = version

    if cargo_pkg_name != None:
        cargo_env["CARGO_PKG_NAME"] = cargo_pkg_name
    elif version != None:
        # If version is set but not name, default name to the target name
        cargo_env["CARGO_PKG_NAME"] = name

    # Merge with user-provided env (user env takes precedence)
    merged_env = {}
    merged_env.update(cargo_env)
    merged_env.update(env)

    _rust_binary(
        name = name,
        env = merged_env if merged_env else {},
        **kwargs
    )

def rust_library(
        name,
        version = None,
        cargo_pkg_name = None,
        env = {},
        **kwargs):
    """
    Wrapper around rust_library that adds Cargo-style environment variables.

    See rust_binary for documentation on the version and cargo_pkg_name args.
    """
    cargo_env = {}

    if version != None:
        cargo_env["CARGO_PKG_VERSION"] = version

    if cargo_pkg_name != None:
        cargo_env["CARGO_PKG_NAME"] = cargo_pkg_name
    elif version != None:
        cargo_env["CARGO_PKG_NAME"] = name

    merged_env = {}
    merged_env.update(cargo_env)
    merged_env.update(env)

    _rust_library(
        name = name,
        env = merged_env if merged_env else {},
        **kwargs
    )

def rust_test(
        name,
        version = None,
        cargo_pkg_name = None,
        env = {},
        **kwargs):
    """
    Wrapper around rust_test that adds Cargo-style environment variables.

    See rust_binary for documentation on the version and cargo_pkg_name args.
    """
    cargo_env = {}

    if version != None:
        cargo_env["CARGO_PKG_VERSION"] = version

    if cargo_pkg_name != None:
        cargo_env["CARGO_PKG_NAME"] = cargo_pkg_name
    elif version != None:
        cargo_env["CARGO_PKG_NAME"] = name

    merged_env = {}
    merged_env.update(cargo_env)
    merged_env.update(env)

    _rust_test(
        name = name,
        env = merged_env if merged_env else {},
        **kwargs
    )
