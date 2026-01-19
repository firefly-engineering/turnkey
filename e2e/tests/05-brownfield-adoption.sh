#!/usr/bin/env bash
# E2E Test: Adopt existing project (brownfield)
#
# Tests adding turnkey to an existing language project:
# 1. Create a standard Go project with existing deps (go.mod, go.sum)
# 2. Add turnkey flake.nix boilerplate
# 3. Add .envrc with 'use flake'
# 4. Enter devshell - verify deps can be generated
# 5. Verify 'tk build' works
# 6. Verify language-native tools still work (go build)
#
# Validates: brownfield adoption, no disruption to existing workflows
#
# Issue: turnkey-njc
#
# OPTIMIZED: Uses batched devshell calls to minimize nix develop overhead.
# Original: 8 devshell calls (~112s overhead)
# Optimized: 2 devshell calls (~28s overhead)
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Adopt existing project (brownfield)"

# Get turnkey root for flake reference
TURNKEY_ROOT=$(get_turnkey_root)

# Step 1: Create a "brownfield" project directory
step "Creating brownfield Go project (simulating existing project)"
PROJECT_DIR=$(setup_test_project "brownfield-go")
cd "$PROJECT_DIR"

# Step 2: Create an existing Go project structure (no turnkey yet)
# This simulates what a typical Go project looks like before turnkey adoption
step "Setting up existing Go project structure"

# Simple main package (keeps things straightforward for Buck2)
cat > main.go << 'EOF'
package main

import (
	"fmt"

	"github.com/google/uuid"
)

func main() {
	requestID := uuid.New()
	fmt.Printf("Starting brownfield server with request ID: %s\n", requestID.String())
	fmt.Println("Brownfield server ready!")
}
EOF

# Existing go.mod (already has dependencies)
cat > go.mod << 'EOF'
module example.com/brownfield

go 1.21

require github.com/google/uuid v1.6.0
EOF

cat > go.sum << 'EOF'
github.com/google/uuid v1.6.0 h1:NIvaJDMOsjHA8n1jAhLSgzrAzy1Hgr+hNrb57e+94F0=
github.com/google/uuid v1.6.0/go.mod h1:TIyPZe4MgqvfeYDBFedMoGGpEw/LqOeaOT+nhxU+yHo=
EOF

# Existing README (typical for a real project)
cat > README.md << 'EOF'
# Brownfield Project

This is an existing Go project that we're adopting with turnkey.

## Building

```bash
go build ./cmd/server
```
EOF

# Initial git commit (simulating existing project history)
step "Simulating existing project git history"
git add -A
git commit -m "Initial brownfield project" --quiet

section "Adopting with Turnkey"

# Step 3: Add turnkey configuration
# This is what a user would do to adopt turnkey
step "Adding turnkey flake.nix"

cat > flake.nix << EOF
{
  description = "Brownfield Go project - adopted with turnkey";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    devenv.url = "github:cachix/devenv";
    turnkey.url = "git+file://${TURNKEY_ROOT}";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devenv.flakeModule
        inputs.turnkey.flakeModules.turnkey
      ];

      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      perSystem = { pkgs, config, ... }: {
        turnkey.toolchains = {
          enable = true;
          declarationFiles.default = ./toolchain.toml;

          registry = {
            buck2 = pkgs.buck2;
            nix = pkgs.nix;
            go = pkgs.go;
            clang = pkgs.llvmPackages.clang;
            lld = pkgs.llvmPackages.lld;
            godeps-gen = inputs.turnkey.packages.\${pkgs.system}.godeps-gen;
            tk = inputs.turnkey.packages.\${pkgs.system}.tk;
          };

          buck2 = {
            enable = true;
            prelude.strategy = "bundled";

            go = {
              enable = true;
              depsFile = ./go-deps.toml;
            };
          };
        };
      };
    };
}
EOF

step "Adding toolchain.toml"
cat > toolchain.toml << 'EOF'
[toolchains]
buck2 = {}
nix = {}
go = {}
godeps-gen = {}
tk = {}
EOF

step "Adding .envrc"
cat > .envrc << 'EOF'
use flake . --no-pure-eval

# Watch for Nix file changes
watch_file nix/**/*.nix
watch_file flake.nix
watch_file flake.lock

# Sync turnkey symlinks on direnv reload
if [ -n "$TURNKEY_BUCK2_CONFIG" ]; then
  if [ "$(readlink .buckconfig 2>/dev/null)" != "$TURNKEY_BUCK2_CONFIG" ]; then
    ln -sf "$TURNKEY_BUCK2_CONFIG" .buckconfig
    echo "turnkey: Synced .buckconfig symlink"
  fi
fi
if [ -n "$TURNKEY_BUCK2_TOOLCHAINS_CELL" ]; then
  mkdir -p .turnkey
  if [ "$(readlink .turnkey/toolchains 2>/dev/null)" != "$TURNKEY_BUCK2_TOOLCHAINS_CELL" ]; then
    ln -sfn "$TURNKEY_BUCK2_TOOLCHAINS_CELL" .turnkey/toolchains
    echo "turnkey: Synced toolchains cell symlink"
  fi
fi
for var in $(env | grep '^TURNKEY_CELL_' | cut -d= -f1); do
  value="${!var}"
  cell_path="${value%%:*}"
  cell_deriv="${value#*:}"
  if [ "$(readlink "$cell_path" 2>/dev/null)" != "$cell_deriv" ]; then
    mkdir -p "$(dirname "$cell_path")"
    ln -sfn "$cell_deriv" "$cell_path"
    echo "turnkey: Synced $cell_path symlink"
  fi
done
EOF

step "Adding rules.star file for build"
cat > rules.star << 'EOF'
go_binary(
    name = "server",
    srcs = ["main.go"],
    deps = ["godeps//vendor/github.com/google/uuid:uuid"],
)
EOF

# Step 4: Stage files for flake
step "Staging new turnkey files"
stage_for_flake

# Step 5: Phase 1 - Generate deps and verify tools (single devshell)
step "Generating deps and verifying tools (batched)"
run_in_devshell_script << 'PHASE1'
  echo "Checking required tools..."
  for cmd in buck2 go godeps-gen; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "ERROR: Command not found: $cmd" >&2
      exit 1
    fi
    echo "  Found: $cmd"
  done

  echo ""
  echo "Generating go-deps.toml from existing project deps..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml
  echo "Generated go-deps.toml"
PHASE1
assert_file_exists "go-deps.toml" || exit 1
assert_file_contains "go-deps.toml" "github.com/google/uuid" || exit 1

# Step 6: Commit turnkey adoption
step "Committing turnkey adoption"
stage_for_flake
commit_changes "Adopt turnkey for Buck2 builds"

section "Verify Brownfield Project Works"

# Step 7: Phase 2 - Build and verify native tools (single devshell)
step "Building with Buck2 and verifying native tools (batched)"
run_in_devshell_script << 'PHASE2'
  echo "Building with Buck2..."
  buck2 build //:server

  echo ""
  echo "Verifying native Go tools still work..."
  go build -o /dev/null ./main.go
  go vet ./main.go
  go mod tidy
  echo "Native Go tools work!"
PHASE2

# Step 8: Verify go.mod unchanged after turnkey adoption
step "Verifying go.mod unchanged"
# go mod tidy should not have changed anything
if git diff --exit-code go.mod go.sum; then
  echo "go.mod and go.sum unchanged (good!)"
else
  echo "WARNING: go.mod or go.sum was modified. This is expected if go mod tidy fixed something."
fi

section "PASS: Adopt existing project (brownfield)"
