# Serde rustc flags
#
# serde_json's build script detects CPU architecture and sets cfg flags
# for optimized integer parsing. We hardcode the 64-bit path for x86_64.
#
# Reference: https://github.com/serde-rs/json/blob/master/build.rs

{ lib }:

{
  # serde_json uses fast 64-bit arithmetic on x86_64
  serde_json = [ "--cfg" ''fast_arithmetic=\"64\"'' ];
}
