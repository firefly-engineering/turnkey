# A second published set, fixing some of the same crates. Fixture for
# checks.fixup-sets.
{
  _class = "turnkeyFixups";
  _file = "other/flake.nix#modules.turnkeyFixups.default";

  rust.serde = {
    rustcFlags = [
      "--cfg"
      "from_other"
    ];
    patches = [ ./second.patch ];
  };
}
