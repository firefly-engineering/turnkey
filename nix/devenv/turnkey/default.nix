{ pkgs, lib, config, ... }:

let
  cfg = config.turnkey;
in
{
  options.turnkey = {
    packages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [];
      description = "Packages resolved from toolchain declarations";
    };
  };

  config = lib.mkIf (cfg.packages != []) {
    packages = cfg.packages;
  };
}
