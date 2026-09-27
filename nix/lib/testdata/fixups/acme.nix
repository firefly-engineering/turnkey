# A fixup set as another flake publishes one: flake-parts' flake.modules
# stamps its class and file. Fixture for checks.fixup-sets.
{
  _class = "turnkeyFixups";
  _file = "acme/flake.nix#modules.turnkeyFixups.default";

  rust.serde = {
    buildScript.generate = ctx: "echo serde ${ctx.versionParts.patch} > $OUT_DIR/private.rs";
    patches = [ ./first.patch ];
  };

  rust.ring = {
    buildScript.generate = ctx: "echo ring for ${ctx.platform.os}-${ctx.platform.cpu}";
    nativeLibraries = [
      {
        name =
          ctx: "ring_core_${ctx.versionParts.major}_${ctx.versionParts.minor}_${ctx.versionParts.patch}__";
        staticLib = "out_dir/libring_core.a";
      }
    ];
    os.linux.rustcFlags = [
      "--cfg"
      "linux_like"
    ];
    cpu.x86_64.env.RING_X86 = "1";
    versions = [
      {
        when.atLeast = "0.17";
        rustcFlags = [
          "--cfg"
          "ring_017"
        ];
      }
      {
        when.below = "0.17";
        rustcFlags = [
          "--cfg"
          "ring_016"
        ];
      }
    ];
  };

  # Locked by no repository in the check: an imported set's unused fixup
  # is silent
  rust.unused-in-acme.buildScript.skip = true;

  go."github.com/acme/lib".patches = [ ./first.patch ];
}
