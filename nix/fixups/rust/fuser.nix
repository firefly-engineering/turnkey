# fuser's build script picks the mount implementation; without its libfuse
# feature that is the pure-Rust one, which needs no external libfuse. That
# cfg is all it produces, so the flag stands in for it.
#
# Reference: https://github.com/cberner/fuser/blob/main/build.rs
{ ... }:

{
  rust.fuser.buildScript.skip = true;
  rust.fuser.rustcFlags = [
    "--cfg"
    ''fuser_mount_impl="pure-rust"''
  ];
}
