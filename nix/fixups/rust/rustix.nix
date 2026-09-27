# rustix's build script picks its backend and OS-family aliases per target;
# off Linux it always uses the libc backend. The flags stand in for it.
#
# Reference: https://github.com/bytecodealliance/rustix/blob/main/build.rs
{ ... }:

{
  rust.rustix = {
    buildScript.skip = true;
    os.linux.rustcFlags = [
      "--cfg"
      "libc"
      "--cfg"
      "linux_like"
      "--cfg"
      "linux_kernel"
    ];
    os.macos.rustcFlags = [
      "--cfg"
      "libc"
      "--cfg"
      "apple"
      "--cfg"
      "bsd"
    ];
  };
}
