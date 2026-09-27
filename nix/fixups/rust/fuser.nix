# fuser's build script picks the mount implementation; without its libfuse
# feature that is the pure-Rust one, which needs no external libfuse.
#
# Reference: https://github.com/cberner/fuser/blob/main/build.rs
{ ... }:

{
  rust.fuser.rustcFlags = [
    "--cfg"
    ''fuser_mount_impl="pure-rust"''
  ];
}
