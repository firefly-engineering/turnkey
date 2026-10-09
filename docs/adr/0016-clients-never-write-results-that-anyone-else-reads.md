---
status: accepted
---

# Clients never write results that anyone else reads

Every client of the build service is untrusted: laptops, CI runners, same-repo and fork PRs alike. A client may upload content-store blobs, which are verified by hash, and submit Nix derivations, which are work, not results. Results anyone else reads come only from the service: action results from its executor, store paths from its Nix builder, built in a sandbox and signed with a key the builder never holds. A client may write results only into a **bubble** that nobody outside its own domain reads, such as a developer's bubble shared by their own machines. The trust root is the service and its operators. Because a sandbox escape could still plant results, results are also tiered ([ADR 0015](0015-results-never-move-into-the-trusted-tier.md)): the code under build doesn't matter to the trusted tier's integrity only because untrusted code never builds there.

Settled while charting [Incremental builds everywhere for turnkey repos on GitHub](https://github.com/firefly-engineering/turnkey/issues/60) as "clients submit work, never results", and restated in [Do we accept the sandbox-escape risk of sharing untrusted-built outputs?](https://github.com/firefly-engineering/turnkey/issues/75). Recorded by [Record the trust rule as an ADR](https://github.com/firefly-engineering/turnkey/issues/67).

## Considered options

- **Scoped writes for everyone** (the original "virtual buckets"): each build writes its own scope, and trunk's is written by builds of reviewed code running on clients. Rejected as the source of shared results, because a client can forge an entry: a PR iteration uploads a forged result under a legitimate action key, and a later, clean iteration of the same PR, or anyone stacked on it, gets a hit on it. Partly adopted as **bubbles**: a client may still write its own owner-only bubble, where a forged entry fools only its author.
- **CI as the only writer** (`docs/research/remote-execution-and-caching.md` §4a). Rejected: laptops and PRs could never contribute, and CI jobs run untrusted PR code anyway, so "CI wrote it" proves nothing.
- **Results only from the service's own executor and builder.** Chosen.

## Consequences

- **Remote execution is the core.** Results that aren't the client's own come only from the executor, so sharing work means executing it remotely.
- **Client-computed results stay with their author.** Local work (darwin actions before a darwin executor, offline builds, `local_only` actions) is never read by anyone else. It stays in the client's local cache, or in an owner-only bubble.
- **The test runner can't publish passes to others.** [Test runner writes passes to a remote cache (G7)](https://github.com/firefly-engineering/turnkey/issues/52) holds only for an owner-only bubble; shared test results come from tests the executor runs. The **Reuse policy**'s "written only into the local cache" ([ADR 0001](0001-runner-recorded-native-test-caching.md)) is consistent with this rule; widening it to an owner-only bubble would be a new decision, not a correction.
