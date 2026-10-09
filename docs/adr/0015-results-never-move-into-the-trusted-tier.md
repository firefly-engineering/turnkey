---
status: accepted
---

# Results never move into the trusted tier

The build service runs untrusted code (fork PRs, unapproved branches) in sandboxes, and a sandbox escape would let that code plant a wrong output under a key someone else looks up later; nothing checks an input-addressed output or an action result against its content. Rather than share every result across everyone and bet trunk's integrity on gVisor and the Nix sandbox, we split both caches into a **trusted tier**, written only by builds of approved code on machinery that never runs anything else, and an **untrusted tier** of **bubbles** stacked on top of it. A build reads its **stack** from its own bubble down to the trusted tier at the bottom; results never move down it. A fixed-output derivation's output is the one exception, because the signer can verify it by its content hash.

Decided in [Do we accept the sandbox-escape risk of sharing untrusted-built outputs?](https://github.com/firefly-engineering/turnkey/issues/75); rules in the "Sandbox-escape risk and tiers" section of [`docs/specs/build-service.md`](../specs/build-service.md).

## Considered options

- **One shared namespace with mitigations (the Google model).** Rejected: the most recent Nix sandbox escape (CVE-2026-39860, April 2026) gave root to anyone who could submit a build, and one escape would reach everything trunk ships.
- **Accept the risk now, add tiers later.** Rejected: tiers change the storage layout, the service identities and client configuration, so adding them later is a migration.
- **A fresh VM per build.** Rejected as unnecessary once tiers exist: an escape that persists on a reused instance stays within its tier, and Cloud Run jobs would add start-up latency to every build.

## Consequences

- **Approved code is built once more.** Trunk doesn't reuse PR results; the merge queue rebuilds what changed, and trunk reuses the merge queue's results in full.
- **Bubbles are not integrity boundaries.** Untrusted-tier instances serve every bubble, so an escape can poison other untrusted builds' results (wrong test outcomes), but nothing that ships.
- **Two sets of services and identities** per deployment, one per tier.
