# A set that gives serde a build script acme.nix already gives it.
# Fixture for checks.fixup-sets.
{
  _class = "turnkeyFixups";
  _file = "conflicting/flake.nix#modules.turnkeyFixups.default";

  rust.serde.buildScript.generate = "echo another serde";
}
