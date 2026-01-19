# Installation

## Prerequisites

Before installing Turnkey, ensure you have:

- **Nix** with flakes enabled
- **direnv** (recommended) for automatic environment activation

### Enabling Nix Flakes

If you haven't enabled flakes, add to `~/.config/nix/nix.conf`:

```
experimental-features = nix-command flakes
```

## Adding Turnkey to Your Project

### New Project

Use the Turnkey template to create a new project:

```bash
nix flake init -t github:firefly-engineering/turnkey
```

### Existing Project

Add Turnkey to your `flake.nix` inputs:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
  };

  outputs = { self, nixpkgs, turnkey, ... }: {
    # Your flake configuration
  };
}
```

## Verifying Installation

After setup, enter the development shell:

```bash
nix develop
```

You should see the welcome message and have access to your declared toolchains.
