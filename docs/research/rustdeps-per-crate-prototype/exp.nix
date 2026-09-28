# One derivation per crate: its dep-rust output plus its own rules.star.
# The index names every crate's store path.
{ flakePath, dataFile }:
let
  flake = builtins.getFlake flakePath;
  system = "aarch64-darwin";
  pkgs = flake.inputs.nixpkgs.legacyPackages.${system};
  inherit (flake.packages.${system}.rustdeps-cell) depPackages;
  data = builtins.fromJSON (builtins.readFile dataFile);
  crate =
    key: dep:
    pkgs.runCommand "rustcrate-${dep.name}-${dep.version}"
      {
        src = dep;
        rules = data.rules.${key};
        passAsFile = [ "rules" ];
      }
      ''
        cp -r $src $out
        chmod u+w $out
        cp $rulesPath $out/rules.star
      '';
in
pkgs.writeText "rustdeps-index.json" (
  builtins.toJSON {
    crates = builtins.mapAttrs crate depPackages;
    inherit (data) aliases;
  }
)
