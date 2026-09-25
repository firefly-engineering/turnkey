# What test result caching sets up in a shell, from the testCache options.
#
# Pure. One value describes the cache, and both of its readers get it from
# here: the generated .buckconfig (buckconfig.nix's testCache), which buck2
# reads, and the TURNKEY_TEST_CACHE variable, which tk reads. So they can't
# disagree on where the cache is, who runs it or whether it takes TLS. The
# variable's shape is src/go/pkg/testcache/testdata/shell-contract.json,
# which checks.buck2-generators and tk's tests both hold to.
{ lib }:

{
  # The buck2.testCache options (nix/buck2/options.nix)
  testCache,
  # Whether caching can be on at all: a custom prelude turns it off
  enabled,
  # turnkey-test-runner, bazel-remote's binary, and the PATH cached tests get
  runner,
  server,
  path,
  # The local cache's loopback port
  port,
}:

let
  local = testCache.endpoint == null;
  cache =
    if !(testCache.enable && enabled) then
      null
    else
      {
        inherit runner path;
        address = if local then "grpc://127.0.0.1:${toString port}" else testCache.endpoint;
        tls = !local && testCache.tls;
      };
in
{
  # buckconfig.nix's testCache: null when caching is off
  buckconfig = cache;

  # The descriptor tk reads (src/go/pkg/testcache): the address, whether it
  # takes TLS, and, for the local cache only, the server tk runs
  env = lib.optionalAttrs (cache != null) {
    TURNKEY_TEST_CACHE = builtins.toJSON (
      {
        inherit (cache) address tls;
      }
      // lib.optionalAttrs local { inherit server; }
    );
  };
}
