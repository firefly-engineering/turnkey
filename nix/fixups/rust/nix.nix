# The nix crate's build script sets up platform aliases with cfg_aliases;
# the flags stand in for it.
#
# Reference: https://github.com/nix-rust/nix/blob/master/build.rs
{ ... }:

{
  rust.nix = {
    buildScript.skip = true;
    os.linux.rustcFlags = [
      "--cfg"
      "linux"
      "--cfg"
      "linux_android"
    ];
    os.macos.rustcFlags = [
      "--cfg"
      "apple_targets"
      "--cfg"
      "bsd"
    ];
  };
}
