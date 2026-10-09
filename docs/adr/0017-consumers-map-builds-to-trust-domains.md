---
status: accepted
---

# Consumers map builds to trust domains; the service only enforces them

The build service runs untrusted code in sandboxes, and a sandbox escape would let that code plant a wrong output under a key someone else looks up later; nothing checks an input-addressed output or an action result against its content. So results are partitioned into **trust domains**: sets of builds that share one writable layer (their **bubble**), each reading a **stack** of bubbles below it, with results never moving from one bubble to another. Which builds belong to which domain, what each domain stacks on, and which domains are **isolated** (run on machinery no other domain's builds touch) is the consumer's policy, not the service's: the service can't know what a consumer trusts, and can't enforce its CI or merge rules. The service provides the mechanism, attaches each build to a domain from claims it verifies itself, and ships a default policy. Signer-verified fixed-output outputs are the one exception to "never moves": they are shared by every domain, because the signer checks them by content hash.

This supersedes [ADR 0015](0015-results-never-move-into-the-trusted-tier.md), which hard-coded one trusted tier for "approved code" (merge-queue runs and protected-branch pushes) and fixed the catalogue of bubbles; those are now turnkey's default policy. Decided in [Do we accept the sandbox-escape risk of sharing untrusted-built outputs?](https://github.com/firefly-engineering/turnkey/issues/75) and revised when [What gates a merge, and what runs after landing?](https://github.com/firefly-engineering/turnkey/issues/71) was ruled out of scope. Rules: the "Sandbox-escape risk and trust domains" and "Result sharing" sections of [`docs/specs/build-service.md`](../specs/build-service.md).

## Considered options

- **One shared namespace with mitigations (the Google model).** Rejected: the most recent Nix sandbox escape (CVE-2026-39860, April 2026) gave root to anyone who could submit a build, and one escape would reach every result.
- **A service-defined trusted tier** ([ADR 0015](0015-results-never-move-into-the-trusted-tier.md)). Rejected: "trusted" is a consumer's judgement about its own repo, which the service can neither know nor enforce.
- **A fresh VM per build.** Rejected as unnecessary: an escape that persists on a reused instance stays within its isolation boundary, and Cloud Run jobs would add start-up latency to every build.

## Consequences

- **Only isolated domains are integrity boundaries.** Non-isolated domains share machinery, so an escape in one can poison the others' bubbles.
- **A deployment runs one set of services and identities per isolated domain.**
- **Under the default policy, landing code is built once more**: the mainline domain doesn't read PR bubbles, so trunk doesn't reuse PR results.
