# The pinned buck2 release (docs/adr/0002-turnkey-owns-the-buck2-version.md).
#
# Each turnkey revision ships exactly one buck2 release: the prebuilt binary
# and the upstream prelude built with it, both from turnkey's own toolbox,
# and the buck2 source revision they come from. Anything that mirrors one of
# buck2's wire formats, such as the test runner's protocol code, is generated
# from that source revision. This record is the single place that names
# them; they are bumped together.
#
# A bump is gated by the test-runner parity suite: with the new release in
# the dev shell, run `python3 src/cmd/check-test-runner-parity/__main__.py`,
# require it to report every scenario as matching, and put its summary line
# in the commit that bumps the pin.
{ pkgs, lib }:

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

  # The entry for the pinned release in a registry's `name` tool
  fromRegistry =
    registry: name:
    registry.${name}.versions.${pinned.version}
      or (throw "turnkey: toolbox has no ${name} ${pinned.version}, the pinned buck2 release");
in
{
  inherit (pinned) version;

  # The pinned buck2 binary. `registry` is turnkey's own
  # (turnkeyLib.defaultTellerRegistry system), never a consumer's.
  buck2 = registry: fromRegistry registry "buck2";

  # The upstream prelude built with the pinned release. The date key alone
  # doesn't name a prelude exactly, so toolbox's entry is checked against the
  # release's own prelude commit; a mismatch is fixed in toolbox.
  upstreamPrelude =
    registry:
    let
      prelude = fromRegistry registry "buck2-prelude";
      rev = prelude.passthru.preludeRev or null;
    in
    if rev == pinned.preludeRev then
      prelude
    else
      throw "turnkey: toolbox's buck2-prelude ${pinned.version} is commit ${toString rev}, but buck2 ${pinned.version} was built with ${pinned.preludeRev}";

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
