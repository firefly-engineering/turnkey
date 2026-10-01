# turnkey's own fixup set (docs/adr/0003-fixup-sets-are-modules.md): the
# fixups turnkey's repository needs for the crates it locks, one module per
# family. turnkey's flake publishes them as modules.turnkeyFixups.<family>,
# plus `default` importing them all, and imports them itself; they apply to
# no other repository unless it imports them.
let
  families = {
    build-script-skips = ./rust/build-script-skips.nix;
    fuser = ./rust/fuser.nix;
    nix = ./rust/nix.nix;
    ref-cast = ./rust/ref-cast.nix;
    ring = ./rust/ring.nix;
    rustix = ./rust/rustix.nix;
    serde = ./rust/serde.nix;
    thiserror = ./rust/thiserror.nix;
    tree-sitter = ./rust/tree-sitter.nix;
  };
in
families
// {
  default = {
    imports = builtins.attrValues families;
  };
}
