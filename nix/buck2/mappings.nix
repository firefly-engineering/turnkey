# Buck2 toolchain mappings
#
# Maps turnkey toolchain names to Buck2 toolchain rule specifications.
# Used by the buck2.nix module to generate the toolchains cell.
#
# Each toolchain can specify:
#   - targets: Buck2 toolchain rules to generate in the toolchains cell
#   - implicitDependencies: Other toolchains that must also be included
#   - runtimeDependencies: Packages needed in PATH for Buck2 action execution
#   - skip: true if this is not a Buck2 toolchain (just a dev tool)
#   - alwaysInclude: true if this toolchain should always be generated
#
# dynamicAttrs read packages from the registry through `tool`, so a registry
# without one fails with a message naming it. A toolchain pulled in as an
# implicit dependency needs its registry entries too (go needs python and
# clang), even though nothing declares it.

{ lib }:

let
  tool =
    registry: name:
    registry.${name} or (throw ''
      turnkey: the Buck2 toolchains need `${name}` from the registry, but it has no `${name}` entry.
      A toolchain can need it without declaring it: go, for example, implicitly depends on python and cxx (clang).
      Add `${name}` to turnkey.toolchains.registryExtensions, or to your registry if you replace it.'');
in
{
  # ==========================================================================
  # Language Toolchains
  # ==========================================================================

  go = {
    skip = false;
    targets = [
      {
        name = "go";
        rule = "system_go_toolchain";
        load = "@prelude//toolchains/go:system_go_toolchain.bzl";
        visibility = [ "PUBLIC" ];
      }
      {
        name = "go_bootstrap";
        rule = "system_go_bootstrap_toolchain";
        load = "@prelude//toolchains/go:system_go_bootstrap_toolchain.bzl";
        visibility = [ "PUBLIC" ];
      }
    ];
    # Go needs python for bootstrap scripts and cxx for linking
    implicitDependencies = [
      "python"
      "cxx"
    ];
  };

  rust = {
    skip = false;
    targets = [
      {
        name = "rust";
        rule = "system_rust_toolchain";
        load = "@prelude//toolchains:rust.bzl";
        visibility = [ "PUBLIC" ];
        attrs = {
          default_edition = "2021";
        };
      }
    ];
    # Rust needs CXX for linking and python for build scripts
    implicitDependencies = [
      "cxx"
      "python"
    ];
  };

  python = {
    skip = false;
    targets = [
      {
        name = "python_bootstrap";
        rule = "system_python_bootstrap_toolchain";
        load = "@prelude//toolchains:python.bzl";
        visibility = [ "PUBLIC" ];
        # A store path, not "python3" from PATH: the interpreter then enters
        # the key of every Python action and test, and a toolchain bump
        # changes it.
        dynamicAttrs = registry: {
          interpreter = "${tool registry "python"}/bin/python3";
        };
      }
      {
        name = "python";
        rule = "system_python_toolchain";
        load = "@prelude//toolchains:python.bzl";
        visibility = [ "PUBLIC" ];
        dynamicAttrs = registry: {
          interpreter = "${tool registry "python"}/bin/python3";
        };
      }
    ];
    implicitDependencies = [ ];
  };

  cxx = {
    skip = false;
    targets = [
      {
        name = "cxx";
        rule = "system_cxx_toolchain";
        load = "@prelude//toolchains:cxx.bzl";
        visibility = [ "PUBLIC" ];
        # Dynamic attrs resolved at build time from registry
        # Uses absolute paths so Buck2 can find compilers without PATH
        dynamicAttrs = registry: {
          compiler = "${tool registry "clang"}/bin/clang";
          cxx_compiler = "${tool registry "clang"}/bin/clang++";
          linker = "${tool registry "clang"}/bin/clang++";
        };
      }
    ];
    implicitDependencies = [ ];
    # lld is needed in PATH for Buck2 prelude's -fuse-ld=lld flag on Linux
    runtimeDependencies = [ "lld" ];
  };

  # ==========================================================================
  # Always-Included Toolchains
  # ==========================================================================

  # genrule is needed for most Buck2 builds
  genrule = {
    skip = false;
    alwaysInclude = true;
    targets = [
      {
        name = "genrule";
        rule = "system_genrule_toolchain";
        load = "@prelude//toolchains:genrule.bzl";
        visibility = [ "PUBLIC" ];
      }
    ];
    implicitDependencies = [ ];
  };

  # test toolchain is needed for go_test, rust_test, python_test etc.
  test = {
    skip = false;
    alwaysInclude = true;
    targets = [
      {
        name = "test";
        rule = "noop_test_toolchain";
        load = "@prelude//tests:test_toolchain.bzl";
        visibility = [ "PUBLIC" ];
      }
      {
        name = "remote_test_execution";
        rule = "remote_test_execution_toolchain";
        load = "@prelude//toolchains:remote_test_execution.bzl";
        visibility = [ "PUBLIC" ];
      }
    ];
    implicitDependencies = [ ];
  };

  # ==========================================================================
  # Language Toolchain Meta-Packages
  # ==========================================================================
  # These map *-toolchain meta-packages to the same Buck2 rules as their
  # individual components, enabling toolchain.toml to use versioned
  # meta-packages while still generating the correct Buck2 toolchains.

  rust-toolchain = {
    skip = false;
    targets = [
      {
        name = "rust";
        rule = "system_rust_toolchain";
        load = "@prelude//toolchains:rust.bzl";
        visibility = [ "PUBLIC" ];
        attrs = {
          default_edition = "2021";
        };
      }
    ];
    implicitDependencies = [
      "cxx"
      "python-toolchain"
    ];
  };

  go-toolchain = {
    skip = false;
    targets = [
      {
        name = "go";
        rule = "system_go_toolchain";
        load = "@prelude//toolchains/go:system_go_toolchain.bzl";
        visibility = [ "PUBLIC" ];
      }
      {
        name = "go_bootstrap";
        rule = "system_go_bootstrap_toolchain";
        load = "@prelude//toolchains/go:system_go_bootstrap_toolchain.bzl";
        visibility = [ "PUBLIC" ];
      }
    ];
    implicitDependencies = [
      "python-toolchain"
      "cxx"
    ];
  };

  python-toolchain = {
    skip = false;
    targets = [
      {
        name = "python_bootstrap";
        rule = "system_python_bootstrap_toolchain";
        load = "@prelude//toolchains:python.bzl";
        visibility = [ "PUBLIC" ];
        # A store path, not "python3" from PATH: the interpreter then enters
        # the key of every Python action and test, and a toolchain bump
        # changes it.
        dynamicAttrs = registry: {
          interpreter = "${tool registry "python-toolchain"}/bin/python3";
        };
      }
      {
        name = "python";
        rule = "system_python_toolchain";
        load = "@prelude//toolchains:python.bzl";
        visibility = [ "PUBLIC" ];
        dynamicAttrs = registry: {
          interpreter = "${tool registry "python-toolchain"}/bin/python3";
        };
      }
    ];
    implicitDependencies = [ ];
  };

  typescript-toolchain = {
    skip = false;
    targets = [
      {
        name = "typescript";
        rule = "system_typescript_toolchain";
        load = "@prelude//typescript:toolchain.bzl";
        visibility = [ "PUBLIC" ];
        # Tools come from the declared typescript-toolchain meta-package, not
        # the typescript/nodejs entries' defaults, which move independently
        # (typescript's default became 7, the native compiler with no tsc.js).
        dynamicAttrs = registry: {
          node_path = "${tool registry "typescript-toolchain"}/bin/node";
          tsc_path = "${tool registry "typescript-toolchain"}/lib/node_modules/typescript/bin/tsc";
        };
      }
    ];
    implicitDependencies = [ ];
  };

  solidity-toolchain = {
    skip = false;
    targets = [
      {
        name = "solc";
        rule = "system_solidity_toolchain";
        load = "@prelude//solidity:toolchain.bzl";
        visibility = [ "PUBLIC" ];
        # Tools come from the declared solidity-toolchain meta-package, so
        # the solc and forge Buck2 runs are the ones the dev shell provides.
        dynamicAttrs = registry: {
          solc_path = "${tool registry "solidity-toolchain"}/bin/solc";
          forge_path = "${tool registry "solidity-toolchain"}/bin/forge";
          cast_path = "${tool registry "solidity-toolchain"}/bin/cast";
          anvil_path = "${tool registry "solidity-toolchain"}/bin/anvil";
          jq_path = "${tool registry "jq"}/bin/jq";
        };
      }
    ];
    implicitDependencies = [ ];
  };

  # ==========================================================================
  # Non-Buck2 Tools (skipped)
  # ==========================================================================

  # Nix package manager
  nix = {
    skip = true;
    reason = "Nix package manager, not a Buck2 toolchain";
  };

  # Development tools
  beadwork = {
    skip = true;
    reason = "Issue tracking tool (bw CLI + TUI)";
  };

  jj = {
    skip = true;
    reason = "Jujutsu VCS tool, bundled in vcs-toolchain";
  };

  vcs-toolchain = {
    skip = true;
    reason = "VCS meta-package (jj, git, gh, difftastic, delta)";
  };

  turnkey-composed = {
    skip = true;
    reason = "FUSE composition daemon, not a Buck2 toolchain";
  };

  clang = {
    skip = true;
    reason = "C/C++ compiler, needed in PATH for Buck2 actions but not a toolchain rule";
  };

  lld = {
    skip = true;
    reason = "LLVM linker, needed in PATH for rustc linking but not a toolchain rule";
  };

  # Tools bundled in toolchain meta-packages (skipped when used individually)
  golangci-lint = {
    skip = true;
    reason = "Go linter, bundled in go-toolchain";
  };

  gopls = {
    skip = true;
    reason = "Go LSP server, bundled in go-toolchain";
  };

  cargo-edit = {
    skip = true;
    reason = "Cargo add/rm/upgrade, bundled in rust-toolchain";
  };

  rust-analyzer = {
    skip = true;
    reason = "Rust LSP server, bundled in rust-toolchain";
  };

  uv = {
    skip = true;
    reason = "Python package manager, bundled in python-toolchain";
  };

  ruff = {
    skip = true;
    reason = "Python linter/formatter, bundled in python-toolchain";
  };

  pytest = {
    skip = true;
    reason = "Python test runner, bundled in python-toolchain";
  };

  biome = {
    skip = true;
    reason = "JS/TS linter/formatter, bundled in typescript-toolchain";
  };

  # Rust dependency tool
  reindeer = {
    skip = true;
    reason = "Rust dependency generator, creates Buck2 targets from Cargo.toml but not a toolchain rule";
  };

  # JavaScript/TypeScript
  nodejs = {
    skip = true;
    reason = "Node.js runtime, used as dependency of typescript toolchain";
  };

  typescript = {
    skip = false;
    targets = [
      {
        name = "typescript";
        rule = "system_typescript_toolchain";
        load = "@prelude//typescript:toolchain.bzl";
        visibility = [ "PUBLIC" ];
        # Dynamic attrs resolved at build time from registry
        dynamicAttrs = registry: {
          node_path = "${tool registry "nodejs"}/bin/node";
          tsc_path = "${tool registry "typescript"}/lib/node_modules/typescript/bin/tsc";
        };
      }
    ];
    # TypeScript needs nodejs for running
    implicitDependencies = [ ];
  };

  # Solidity smart contract toolchain
  # Uses 'solc' as the toolchain name (matches registry entry)
  solc = {
    skip = false;
    targets = [
      {
        name = "solc";
        rule = "system_solidity_toolchain";
        load = "@prelude//solidity:toolchain.bzl";
        visibility = [ "PUBLIC" ];
        # Dynamic attrs resolved at build time from registry
        dynamicAttrs = registry: {
          solc_path = "${tool registry "solc"}/bin/solc";
          forge_path = "${tool registry "foundry"}/bin/forge";
          cast_path = "${tool registry "foundry"}/bin/cast";
          anvil_path = "${tool registry "foundry"}/bin/anvil";
          jq_path = "${tool registry "jq"}/bin/jq";
        };
      }
    ];
    implicitDependencies = [ ];
  };

  # Foundry toolkit (forge, cast, anvil)
  foundry = {
    skip = true;
    reason = "Ethereum dev toolkit, used as dependency of solidity toolchain";
  };

  # ==========================================================================
  # Documentation Toolchains
  # ==========================================================================

  mdbook = {
    skip = false;
    targets = [
      {
        name = "mdbook";
        rule = "system_mdbook_toolchain";
        load = "@prelude//mdbook:toolchain.bzl";
        visibility = [ "PUBLIC" ];
        # Dynamic attrs resolved at build time from registry
        dynamicAttrs = registry: {
          mdbook_path = "${tool registry "mdbook"}/bin/mdbook";
          python_path = "${tool registry "python"}/bin/python3";
          # Output served books to .turnkey/books/ to keep source tree clean
          serve_output_dir = ".turnkey/books";
        };
      }
    ];
    implicitDependencies = [ ];
  };

  # mdbook-toolchain is a meta-package bundling mdbook + preprocessors.
  # It generates the same Buck2 toolchain as "mdbook" but resolves paths
  # from the meta-package (which has all binaries in one bin/).
  mdbook-toolchain = {
    skip = false;
    targets = [
      {
        name = "mdbook";
        rule = "system_mdbook_toolchain";
        load = "@prelude//mdbook:toolchain.bzl";
        visibility = [ "PUBLIC" ];
        dynamicAttrs = registry: {
          mdbook_path = "${tool registry "mdbook-toolchain"}/bin/mdbook";
          python_path = "${tool registry "python"}/bin/python3";
          serve_output_dir = ".turnkey/books";
        };
      }
    ];
    implicitDependencies = [ ];
  };

  # ==========================================================================
  # Data Templating Toolchains
  # ==========================================================================

  jrsonnet = {
    skip = false;
    targets = [
      {
        name = "jsonnet";
        rule = "system_jsonnet_toolchain";
        load = "@prelude//jsonnet:toolchain.bzl";
        visibility = [ "PUBLIC" ];
        dynamicAttrs = registry: {
          jsonnet_path = "${tool registry "jrsonnet"}/bin/jrsonnet";
        };
      }
    ];
    implicitDependencies = [ ];
  };
}
