# The parity harness: `nix run .#parity -- <tool>` runs a Go tool and its
# Rust port on the same inputs and diffs what they produce (#209, from the
# decision on #204). Run by hand: no flake check, no CI job.
#
# Temporary: each port's switch deletes its case file from ./cases, and
# the tk switch deletes this directory and the flake's apps.parity.
#
# Each case file names the flake packages of its tool's Go and Rust
# versions. Only their derivation paths are embedded here, without their
# string context: a derivation path with its context makes the harness
# depend on the whole build-time closure of every tool, which Nix then
# fetches or builds. Evaluating the paths writes the .drv files, and
# parity.py builds the two packages of the tool it is asked about. See
# README.md.
{
  pkgs,
  lib,
  # The flake's packages, which the case files' tool.go and tool.rust name
  packages,
  # The tree the cases' inputs are read from: turnkey's own source
  source,
}:

let
  caseFiles = lib.filterAttrs (name: type: type == "regular" && lib.hasSuffix ".toml" name) (
    builtins.readDir ./cases
  );

  toolPackages = lib.mapAttrs' (
    file: _:
    let
      tool = (builtins.fromTOML (builtins.readFile (./cases + "/${file}"))).tool;
      drvOf =
        side: name:
        if packages ? ${name} then
          builtins.unsafeDiscardStringContext packages.${name}.drvPath
        else
          throw "parity: cases/${file}: tool.${side} names packages.${name}, which the flake doesn't have";
    in
    lib.nameValuePair (lib.removeSuffix ".toml" file) {
      go = drvOf "go" tool.go;
      rust = if tool ? rust then drvOf "rust" tool.rust else null;
    }
  ) caseFiles;

  config = pkgs.writeText "parity-config.json" (
    builtins.toJSON {
      casesDir = "${./cases}";
      source = "${source}";
      # What the launched tool finds on PATH after the stubs: the real
      # tools a case doesn't stub (godeps-gen's go list)
      toolsPath = lib.makeBinPath [
        pkgs.coreutils
        pkgs.go
        pkgs.git
      ];
      sslCertFile = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      packages = toolPackages;
    }
  );
in
pkgs.writeShellApplication {
  name = "parity";
  text = ''
    exec ${lib.getExe pkgs.python3} ${./parity.py} --config ${config} "$@"
  '';
  meta = {
    description = "Diff a turnkey Go tool against its Rust port on the same inputs";
    mainProgram = "parity";
  };
}
