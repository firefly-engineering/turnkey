# The pinned buck2 release (docs/adr/0002-turnkey-owns-the-buck2-version.md).
#
# Each turnkey revision ships exactly one buck2 release: the prebuilt binary
# and the upstream prelude built with it, both from turnkey's own toolbox,
# and the buck2 source revision they come from. Anything that mirrors one of
# buck2's wire formats, such as the test runner's protocol code, is generated
# from that source revision. This record is the single place that names
# them; they are bumped together.
#
# A bump follows docs/developer-manual/src/contributing/bumping-buck2.md. It
# is gated by the test-runner parity suite: with the new release in the dev
# shell, run `python3 src/cmd/check-test-runner-parity/__main__.py`, require
# it to report every scenario as matching, and put its summary line in the
# commit that bumps the pin.
#
# Callers get the pinned release through turnkey's flake lib,
# `turnkeyLib.pinnedBuck2Release system`, which binds `registry` to turnkey's
# own (never a consumer's, so the binary and prelude can't drift from each
# other). `version` and `protos` don't need it, so a plain
# `import ./buck2-source.nix { inherit pkgs lib; }` still reads those.
{
  pkgs,
  lib,
  registry ? throw "turnkey: the pinned buck2 release needs turnkey's registry; use turnkeyLib.pinnedBuck2Release system",
}:

let
  pinned = {
    # Release date: the version key of toolbox's `buck2` and `buck2-prelude`
    version = "2026-09-15";
    # buck2 commit the release was built from
    rev = "6507dd157a6f81a810c48583edf1758dd0c337c5";
    protosHash = "sha256-xaGN1+FuT8DDiKI/Ww8cGrozx0Shvdyj9deH/ZycAdg=";
    # Prelude commit the release was built with (its `prelude_hash` asset)
    preludeRev = "4d101dce3482c35b32f9f1e7072b354ae789d256";
  };

  # The entry for the pinned release in the registry's `name` tool
  fromRegistry =
    name:
    registry.${name}.versions.${pinned.version}
      or (throw "turnkey: toolbox has no ${name} ${pinned.version}, the pinned buck2 release");

  # The date key alone doesn't name a build exactly, so each of toolbox's
  # entries is checked against the commit the release records for it. A
  # mismatch is fixed in toolbox.
  checkRev =
    name: pkg: attr: expected:
    let
      rev = pkg.passthru.${attr} or null;
    in
    if rev == expected then
      pkg
    else
      throw "turnkey: toolbox's ${name} ${pinned.version} is commit ${toString rev}, but the pinned buck2 release records ${expected}";

  # toolbox's buck2 doesn't record its source commit yet (turnkey-s2a), so
  # the binary is checked only once it does
  buck2 =
    let
      pkg = fromRegistry "buck2";
    in
    if pkg.passthru ? rev then checkRev "buck2" pkg "rev" pinned.rev else pkg;

  upstreamPrelude = checkRev "buck2-prelude" (fromRegistry "buck2-prelude") "preludeRev" pinned.preludeRev;
in
{
  inherit (pinned) version;

  # The pinned buck2 binary
  inherit buck2;

  # The upstream prelude built with the pinned release
  inherit upstreamPrelude;

  # turnkey's prelude: the upstream one with turnkey's patches and extensions
  prelude = import ./prelude.nix { inherit pkgs lib upstreamPrelude; };

  # buck2's test-runner protocol files at the pinned source revision
  protos = pkgs.fetchFromGitHub {
    owner = "facebook";
    repo = "buck2";
    inherit (pinned) rev;
    sparseCheckout = [
      "app/buck2_test_proto"
      "app/buck2_data"
      "app/buck2_host_sharing_proto"
    ];
    hash = pinned.protosHash;
  };
}
