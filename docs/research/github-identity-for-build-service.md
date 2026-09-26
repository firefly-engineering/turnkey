# What GitHub can vouch for, and how it becomes a build-service credential

Research note for turnkey-wit.6 (map turnkey-wit). The question: what can
GitHub prove about who is asking the **build service**? The askers are CI jobs
for PRs (forks included), landing commits, the merge queue, and individual
developers. How does each proof become a credential the service can check
without long-lived client secrets?

The map has settled that every **client** is untrusted for integrity: clients
submit work, never results. So a credential here does not decide who may write
shared results. It decides three things:

- **compute:** who may spend the service's money (Execute, Nix builds);
- **read:** who may read a private repo's artifacts (exactly that repo's
  readers);
- **placement:** where a **client-computed result** goes (developer-private).

- **Researched:** 2026-09-26.
- **GitHub docs:** source of docs.github.com at
  [`github/docs@18945a31`](https://github.com/github/docs/tree/18945a31a4f2d97beb6c5c1a7479102e23c25727)
  (fetched 2026-09-26). Links below are permalinks into that tree, and REST
  facts come from its OpenAPI-derived data, API version `2026-03-10`.
- **GitHub's live OIDC discovery document:**
  [`token.actions.githubusercontent.com/.well-known/openid-configuration`](https://token.actions.githubusercontent.com/.well-known/openid-configuration),
  fetched 2026-09-26.
- **GCP docs:** pages on docs.cloud.google.com, each marked with its own
  "Last updated" date (2026-09-24 or 2026-09-25).
- **buck2:** the pinned commit
  [`6507dd15`](https://github.com/facebook/buck2/tree/6507dd157a6f81a810c48583edf1758dd0c337c5)
  ([`nix/buck2/buck2-source.nix`](../../nix/buck2/buck2-source.nix)).
- **Nix:** 2.34.7, the dev shell's `nix --version`, at tag commit
  [`2c6d06e9`](https://github.com/NixOS/nix/tree/2c6d06e9387cf58167cb5a7ab91cee7333d8d17c).
- **Method:** reading docs and source only. No workflow was run, no token was
  minted, and nothing was deployed. Claims not read directly off a source are
  marked *(inferred)*. Claims that could not be checked are marked
  *(unverified)*.

It builds on
[remote-execution-and-caching.md §3.2](remote-execution-and-caching.md#32-buck2-at-6507dd15)
("Auth": buck2's `$VAR` substitution in `http_headers`, `tls_ca_certs` and
`tls_client_cert`) and does not repeat it.

## 1. TL;DR

| Client kind | What GitHub proves | How the service verifies | What the client presents |
| --- | --- | --- | --- |
| **Actions job, same-repo PR** (`pull_request`) | An OIDC JWT: repo and owner (names and numeric IDs), `event_name=pull_request`, `ref=refs/pull/N/merge`, `head_ref`/`base_ref`, `actor`, `workflow_ref`, `repository_visibility` | JWKS signature, `iss`, `aud` = service, then a policy on the numeric IDs and `event_name` | JWT → exchanged at the service for a **service token** (§5) |
| **Actions job, fork PR** (`pull_request`) | **Nothing usable.** No OIDC token *(inferred, §2.3)*, no secrets, a read-only `GITHUB_TOKEN` | — | Nothing. Anonymous reads of a public repo only. Builds go through the **service-owned client host**, triggered by the service's GitHub App (§3) |
| **Actions job, landing** (`push` to `main`) | OIDC JWT with `event_name=push`, `ref=refs/heads/main`, `ref_protected` | As above; the guarantee is only as strong as the branch's push rules | JWT → service token |
| **Actions job, merge queue** (`merge_group`) | OIDC JWT with `event_name=merge_group` and `ref` = the queue's temporary `gh-readonly-queue/…` branch *(inferred)* | As above | JWT → service token |
| **Actions job, manual** (`workflow_dispatch`) | OIDC JWT; `ref` = whatever branch was dispatched, `actor` = the dispatcher | As above | JWT → service token |
| **Developer laptop** | Nothing unless the user signs in: GitHub App **device flow** → a user access token (8 h, refresh 6 months) scoped to the intersection of user and app access | The service checks the user token with GitHub, lists the repos it reaches, and mints its own token | Service token (bearer for buck2, netrc password for Nix) |
| **Service-owned client host** (Cloud Run job) | Nothing is needed from GitHub for identity: it *is* the service. It acts on a **signed webhook** (HMAC) and fetches source with a 1-hour **installation token** | Webhook signature + `X-GitHub-Delivery` dedup; policy on `author_association` or the collaborator-permission API | Its own GCP service-account identity |

Five points decide the design:

1. **Verify GitHub's JWT in the service itself, not through GCP Workload
   Identity Federation (WIF).** Cloud Run does not support WIF direct resource
   access. The GCP path ends in impersonating one service account and minting
   a Google ID token, which erases the GitHub claims the policy needs (§4).
2. **Clients need a service-minted token that lives as long as a build.** buck2
   substitutes `$VAR` in headers from the *daemon's* environment when it
   creates an RE client. Nix's HTTP binary-cache auth is a `netrc` file (Basic
   auth) or a TLS client certificate: there is no bearer-token setting. A GitHub
   OIDC JWT is short-lived (the docs' examples show `exp − iat = 300 s`;
   lifetime *unverified*), so it cannot be the credential the daemon holds
   (§5).
3. **Fork PRs get no GitHub-vouched identity inside Actions.** That confirms
   the settled design: they run on the service-owned client host, gated by the
   service's GitHub App. GitHub's own "Require approval for all external
   contributors" gate exists for Actions but is **not the default** (the
   default gates first-time contributors only) (§2.3).
4. **A same-repo writer cannot forge `ref=refs/heads/main`** without pushing
   to `main`. They can forge anything keyed only on a workflow file, an
   environment name without branch restrictions, or a `sub` that omits the
   ref. Policy must key on `repository_owner_id`/`repository_id` plus
   `event_name` plus `ref`, never on names or `environment` alone (§2.2).
5. **Merge queue:** free on public org-owned repos. **Private repos need GitHub
   Enterprise Cloud.** Stacked PRs are now a native GitHub feature (public
   preview), exposed as a `stack` object on PRs and webhooks (§6, §7).

## 2. GitHub Actions OIDC

### 2.1 What the token says

- **Standard claims.** `iss` is `https://token.actions.githubusercontent.com`.
  `aud` defaults to the owner's URL and can be set per request.
  `sub` is a composed string. The token also carries `exp`, `iat`, `nbf` and
  `jti`, and is signed with RS256
  ([oidc.md L23-L44](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/security/oidc.md#L23-L44);
  discovery document: `id_token_signing_alg_values_supported: ["RS256"]`,
  `jwks_uri: …/.well-known/jwks`).
- **Custom claims** ([oidc.md L46-L85](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/security/oidc.md#L46-L85)):
  `actor`, `actor_id`, `base_ref`, `head_ref`, `event_name`, `ref`,
  `ref_type`, `repository`, `repository_id`, `repository_owner`,
  `repository_owner_id`, `repository_visibility`, `run_id`, `run_number`,
  `run_attempt`, `runner_environment` (`github-hosted` | `self-hosted`),
  `workflow`, `workflow_ref`, `workflow_sha`, `job_workflow_ref`,
  `job_workflow_sha`, `environment`, and `check_run_id`.
  - The live discovery document also lists `sha`, `ref_protected`,
    `environment_node_id` and `issuer_scope`. The claims table does not
    describe them. `sha` appears in the docs' example token
    ([openid-connect.md L57-L88](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/concepts/security/openid-connect.md#L57-L88)).
    That `ref_protected` is true exactly when `ref` has branch protection is
    *(inferred from the name; unverified)*.
  - `job_workflow_ref` is documented "for jobs using a reusable workflow". Its
    value for a plain workflow is *(unverified)*. The example token shows it
    set.
- **`sub` formats** ([oidc.md L119-L170](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/security/oidc.md#L119-L170)):
  - a job that references an environment:
    `repo:ORG/REPO:environment:NAME`. This wins over everything else;
  - a `pull_request` event: `repo:ORG/REPO:pull_request`. This is the same
    for every PR in the repo; the PR number lives only in `ref`;
  - otherwise: `repo:ORG/REPO:ref:refs/heads/BRANCH` or `…:ref:refs/tags/TAG`.
- **Immutable subjects (new).** Repositories created after **2026-07-15**, and
  repos that opt in, use
  `repo:OWNER@OWNER_ID/REPO@REPO_ID:…`. Older repos keep the name-only format
  unless they opt in, and renames and transfers switch to the new one
  ([oidc.md L172-L186](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/security/oidc.md#L172-L186)).
  A verifier that parses `sub` must accept both formats. Keying on the
  separate `repository_id`/`repository_owner_id` claims avoids parsing `sub`
  at all *(inferred)*.
- **Custom `sub` templates** (`include_claim_keys`, set via REST per org or
  repo) and **repository custom properties** as `repo_property_*` claims
  ([oidc.md L210-L345](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/security/oidc.md#L210-L345)).
  A service verifying the JWT itself can read any claim directly and needs
  neither.
- **Getting a token.** The job needs `permissions: id-token: write`. The
  runner then exposes `ACTIONS_ID_TOKEN_REQUEST_URL` and
  `ACTIONS_ID_TOKEN_REQUEST_TOKEN`, and a step can request a JWT for any
  `audience`
  ([oidc.md L556-L610](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/security/oidc.md#L556-L610)).
  A job can request fresh tokens repeatedly while it runs *(inferred from the
  request API)*.
- **Lifetime.** The docs do not state it. Both example tokens have
  `exp − iat = 300` s
  ([openid-connect.md L85-L87](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/concepts/security/openid-connect.md#L85-L87)).
  Treat it as minutes *(unverified)*.
- **Per event** ([events-that-trigger-workflows.md](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/workflows-and-actions/events-that-trigger-workflows.md)).
  `GITHUB_REF`, and so the `ref` claim *(inferred: the claim is documented as
  "the git ref that triggered the workflow run")*:

  | Event | `ref` | Workflow file comes from | Default `sub` |
  | --- | --- | --- | --- |
  | `pull_request` | `refs/pull/N/merge` (L462) | the PR merge commit | `repo:O/R:pull_request` |
  | `pull_request_target` | default branch (L666-L672) | the default branch (L676) | *(unverified)*: probably `…:ref:refs/heads/main`, which would be indistinguishable from `push` by `sub` |
  | `push` | the updated ref (L784) | the pushed commit, "including workflows that are not merged into the default branch" (L790) | `repo:O/R:ref:refs/heads/B` |
  | `merge_group` | "Ref of the merge group" (L378), i.e. a `gh-readonly-queue/{base}/…` branch | the merge-group commit *(inferred)* | `repo:O/R:ref:refs/heads/gh-readonly-queue/…` *(inferred)* |
  | `workflow_dispatch` | the dispatched branch or tag; any branch can be dispatched once the workflow has run (L1074-L1079) | that branch | `repo:O/R:ref:refs/heads/B` |

  Dependabot *update jobs* can now get OIDC tokens with `event_name=dynamic`
  ([oidc.md L111-L115](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/security/oidc.md#L111-L115)).
  A policy should allowlist `event_name` values.

### 2.2 Can a same-repo branch forge a trusted-looking claim?

GitHub signs what happened; it does not vouch that the workflow code is
trusted. Anyone with write access can push a branch with an edited workflow,
and `push` runs workflows "that are not merged into the default branch"
(L790). So:

- **Not forgeable without pushing to `main`:** `ref=refs/heads/main` on a
  `push` or `workflow_dispatch`. It holds only as far as branch protection or
  rulesets restrict who can push to `main` *(inferred)*.
- **Forgeable by any writer:**
  - **`workflow`/`workflow_ref`/`job_workflow_ref`.** These name a file at the
    run's ref, so on a branch they name the branch's own copy *(inferred)*.
  - **`environment` and `sub = …:environment:X`.** The environment `sub`
    replaces the ref, and an environment with "No restriction" deployment
    branches accepts any branch
    ([deployments-and-environments.md L50-L69](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/workflows-and-actions/deployments-and-environments.md#L50-L69)).
    Branch restrictions are available on public repos, and on private repos
    only on Pro or Team plans (L69).
  - **A `sub` template that omits `ref`,** such as `repository_owner` only.
- **`pull_request_target`** runs the default branch's workflow with secrets
  and a write token, and its `ref` is the default branch. A policy of "ref ==
  main" would therefore admit it, and a workflow that checks out PR code is
  the known "pwn request"
  ([securely-using-pull_request_target.md L16-L50](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/security/securely-using-pull_request_target.md#L16-L50)).
  GitHub will **block `pull_request_target` by default in public repos from
  2026-11-02** (L56-L75).
- **Consequence for the service.** Under the trust rule, a forged "landing"
  identity cannot poison anything, because clients never write results. What
  it can do is spend compute, read, or place client-computed results under
  the wrong key *(inferred from the map's trust rule)*. The policy key is
  therefore (`repository_owner_id`, `repository_id`, `event_name`, `ref`),
  with `event_name` allowlisted. `actor_id` is available for per-user quotas.
  The WIF docs make the same point: use numeric `*_id` fields, because names
  can be re-registered (§4).

### 2.3 Fork PRs

- **Secrets:** "With the exception of `GITHUB_TOKEN`, secrets are not passed
  to the runner when a workflow is triggered from a forked repository"
  ([forked-secrets.md](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/data/reusables/actions/forked-secrets.md)).
- **`id-token`:** for a fork PR (any PR event except `pull_request_target`),
  unless an admin enabled **Send write tokens to workflows from pull
  requests**, "the permissions are adjusted to change any write permissions
  to read only"
  ([workflow-syntax.md L350](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/reference/workflows-and-actions/workflow-syntax.md#L350)).
  `id-token` accepts only `write|none`
  ([github-token-available-permissions.md L12](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/data/reusables/actions/github-token-available-permissions.md#L12)),
  so a fork PR run gets **no OIDC token**. This is *(inferred)*: no page says
  it in one sentence.
  - The "Send write tokens" setting exists only for forks of **private**
    repos
    ([private-repository-forks-options.md](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/data/reusables/actions/private-repository-forks-options.md)).
    Anyone who can fork a private repo is already one of its readers.
  - Dependabot PR runs are treated as fork runs
    ([workflow-runs-dependabot-note.md](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/data/reusables/actions/workflow-runs-dependabot-note.md)).
- **Approval gates (compute, not integrity).** For public repos the setting
  "Approval for running fork pull request workflows from contributors" offers
  three levels: first-time contributors new to GitHub, first-time
  contributors, or **all external contributors** (not a repo or org member).
  "By default, all first-time contributors require approval"
  ([workflows-from-public-fork-setting.md L1-L7](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/data/reusables/actions/workflows-from-public-fork-setting.md#L1-L7),
  [workflow-run-approve-public-fork.md L8](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/data/reusables/actions/workflow-run-approve-public-fork.md#L8)).
  - GitHub warns that the first-time settings are bypassed once any commit by
    the user has been merged (L3).
  - Runs left awaiting approval are deleted after 30 days
    ([approve-runs-from-forks.md L22-L24](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/actions/how-tos/manage-workflow-runs/approve-runs-from-forks.md#L22-L24)).
  - The policy can be read and set via REST
    (`/repos/{o}/{r}/actions/permissions/fork-pr-contributor-approval`, and the
    `/orgs/…` equivalent). `POST /repos/{o}/{r}/actions/runs/{run_id}/approve`
    approves a run.
  - **The settled rule "fork PRs from non-members need approval" corresponds
    to the "all external contributors" level, which a consumer must select;
    it is not GitHub's default.**
- **Alternatives:**
  1. **Nothing:** a fork PR job reads a public repo's artifacts anonymously
     (confidentiality is not a goal) and builds locally. It cannot Execute.
  2. **The service-owned client host** (the settled design). The service's
     GitHub App receives the signed `pull_request` webhook, applies its own
     gate, and builds on its own identity. For the gate it can use the
     webhook's `author_association` (`OWNER`, `MEMBER`, `COLLABORATOR`,
     `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, `FIRST_TIMER`, `NONE`,
     `MANNEQUIN`; pulls schema), the collaborator-permission API, or a
     writer's label or comment.
     - Reusing GitHub's Actions approval (starting when an approved run
       appears, seen via the `workflow_run` webhook) is possible
       *(inferred; unverified)*.
  3. **`pull_request_target`:** avoid. It carries the pwn-request risk above
     and is blocked by default on public repos from 2026-11-02.

## 3. GitHub App

- **Webhooks.** Each delivery is signed as
  `X-Hub-Signature-256: sha256=HMAC-SHA256(secret, body)`, which must be
  compared in constant time
  ([validating-webhook-deliveries.md L44-L54](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/webhooks/using-webhooks/validating-webhook-deliveries.md#L44-L54)).
  Replays are rejected by deduplicating `X-GitHub-Delivery`
  ([best-practices-for-using-webhooks.md L57](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/webhooks/using-webhooks/best-practices-for-using-webhooks.md#L57)).
  - The webhook secret and the App private key are long-lived, but **only the
    service holds them**. Private keys "do not expire"
    ([managing-private-keys-for-github-apps.md L19](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/apps/creating-github-apps/authenticating-with-a-github-app/managing-private-keys-for-github-apps.md#L19)).
  - **The `merge_group` webhook is "App only"**
    ([webhook-events-and-payloads, merge_group](https://docs.github.com/en/webhooks/webhook-events-and-payloads#merge_group),
    rendered page, 2026-09-26). A service that reacts to the merge queue
    directly, rather than through an Actions job, must therefore be a GitHub
    App.
- **Installation tokens.** The App signs a JWT (`exp` ≤ 10 minutes;
  [generating-a-json-web-token-jwt-for-a-github-app.md L22](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app.md#L22))
  and exchanges it at `POST /app/installations/{id}/access_tokens` for a token
  that "expire[s] one hour from the time you create them".
  - The token can be narrowed to at most 500 repositories and to a subset of
    permissions.
  - Since 2026-04-27, new installation tokens are rolling out in a stateless
    `ghs_APPID_JWT` format, longer than 40 characters (REST description of
    that endpoint).
  - The service uses these to fetch private source on the client host, to
    check permissions, and to report checks.
- **"May this user read this repo?"**
  `GET /repos/{o}/{r}/collaborators/{username}/permission` returns the highest
  role from every source (repo, team, org, enterprise) as `admin`, `write`,
  `read` or `none`, plus `role_name`. It needs "Metadata: read" and works with
  installation tokens.
- **Checks.** `POST /repos/{o}/{r}/check-runs` requires a GitHub App
  ("Checks: write"). The Checks API only sees pushes in the repo where the
  check was created: for fork branches `pull_requests` is empty (REST
  description). Reporting per-target results as check runs is therefore an
  App capability.

## 4. Verifying on the service side (GCP)

Three options:

1. **Verify GitHub's JWT directly in the Cloud Run service.**
   - Fetch the JWKS, check the RS256 signature, `iss`, `aud` (a
     service-specific audience the client requests), `exp`/`nbf`, then apply
     the claim policy of §2.2.
   - All claims are available to the service.
   - Cloud Run's own IAM check must then be off for that endpoint
     (`--no-invoker-iam-check` / "Allow public access";
     [Cloud Run: managing access](https://docs.cloud.google.com/run/docs/securing/managing-access),
     last updated 2026-09-24), and the service authenticates every request
     itself.
   - Alternatively, Cloud Run can check a Google ID token in
     `X-Serverless-Authorization` while the app reads `Authorization`
     ([service-to-service](https://docs.cloud.google.com/run/docs/authenticating/service-to-service),
     last updated 2026-09-24).
2. **GCP Workload Identity Federation.**
   - Attribute mapping and CEL attribute conditions, with a **mandatory**
     condition restricting tokens to your org. GitHub uses one issuer for all
     orgs.
   - Google warns against name claims (`repository`, `repository_owner`) and
     recommends the numeric `*_id` claims
     ([WIF with deployment pipelines](https://docs.cloud.google.com/iam/docs/workload-identity-federation-with-deployment-pipelines),
     last updated 2026-09-24).
   - **But:** "Cloud Run doesn't support Workload Identity Federation direct
     resource access. To allow access, use service account impersonation"
     ([supported services](https://docs.cloud.google.com/iam/docs/federated-identity-supported-services),
     last updated 2026-09-25). The documented path is STS token → impersonate
     a service account → `generateIdToken` → `Authorization: Bearer` with a
     roughly 1 h ID token (service-to-service page).
   - So Cloud Run sees **one service account**, not the GitHub claims. You
     would need one service account per trust class, and an attribute
     condition per class, to keep any distinction *(inferred)*.
   - It fits GCS or Artifact Registry access by CI better than it fits the
     build service's own API.
3. **Broker: exchange once, then use a service token.**
   - The client POSTs its GitHub JWT (Actions) or GitHub user token (laptop)
     to the service's token endpoint.
   - The service verifies it (option 1, or GitHub's `POST
     /applications/{client_id}/token` "Check a token" for user tokens), and
     mints its own signed token. The token carries the principal (repo ID and
     event class, or user ID), the readable repo IDs and a quota class, with
     a TTL sized to a build.
   - REAPI and the Nix cache then verify only the service's own signature,
     with no GitHub call per request *(inferred design; this is how the
     constraints in §5 compose)*.

**Recommendation:** option 3, with option 1 as its verifier for Actions. WIF
is out for the service's own API because of the Cloud Run limitation above.

## 5. What clients can present

**buck2 (REAPI) at `6507dd15`:**

- `http_headers` values are `$VAR`-substituted when the gRPC interceptor is
  built, via `std::env::var`, i.e. **the daemon process's environment**
  ([`re_grpc/src/client.rs` L246-L258, L403-L420, L1846-L1848](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L246-L258)).
- The RE client is created lazily and dropped once the last connection handle
  is gone, so a later command builds a new one
  ([`buck2_execute/src/re/manager.rs` L187-L240](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/re/manager.rs#L187-L240)).
  The environment it re-reads is still the daemon's startup environment.
  **A header token therefore cannot be rotated without restarting the
  daemon** *(inferred)*. This ties into turnkey-wit.8 ("Should tk own the
  buck2 daemon's environment?").
- `tls_client_cert` is a *path*. The file is read with `tokio::fs::read`
  whenever a channel config is built
  ([`re_grpc/src/pool.rs` L122-L128](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/pool.rs#L122-L128)).
  So a short-lived client certificate rewritten in place is picked up by the
  next RE client without a daemon restart *(inferred from the lifecycle
  above)*.
- In practice: `http_headers = Authorization:Bearer $TK_BUILD_TOKEN`, carrying
  a service token whose TTL outlives a daemon session. mTLS with a
  broker-issued short-lived certificate is the rotation-friendly alternative.

**Nix 2.34.7:**

- **HTTP(S) binary cache.**
  - curl reads `netrc-file` (default `$NIX_CONF_DIR/netrc`) on every
    transfer ([`filetransfer.cc` L563-L567](https://github.com/NixOS/nix/blob/2c6d06e9387cf58167cb5a7ab91cee7333d8d17c/src/libstore/filetransfer.cc#L563-L567),
    [`filetransfer.hh` L98-L121](https://github.com/NixOS/nix/blob/2c6d06e9387cf58167cb5a7ab91cee7333d8d17c/src/libstore/include/nix/store/filetransfer.hh#L98-L121)).
    That gives HTTP **Basic** auth from `login`/`password`: the service must
    accept a service token as the Basic password.
  - The store also has **`tls-certificate` / `tls-private-key`** settings for
    mTLS
    ([`http-binary-cache-store.hh` L39-L43](https://github.com/NixOS/nix/blob/2c6d06e9387cf58167cb5a7ab91cee7333d8d17c/src/libstore/include/nix/store/http-binary-cache-store.hh#L39-L43),
    [`filetransfer.cc` L577-L585](https://github.com/NixOS/nix/blob/2c6d06e9387cf58167cb5a7ab91cee7333d8d17c/src/libstore/filetransfer.cc#L577-L585)).
  - There is **no bearer-token setting for substituters.**
  - In a multi-user install the daemon substitutes, so the netrc that counts
    is the daemon's *(inferred)*.
- **`access-tokens`** applies only to *fetchers* (GitHub, GitLab and similar
  flake inputs), not to binary caches
  ([`fetch-settings.hh` L28-L60](https://github.com/NixOS/nix/blob/2c6d06e9387cf58167cb5a7ab91cee7333d8d17c/src/libfetchers/include/nix/fetchers/fetch-settings.hh#L28-L60),
  used in [`github.cc` L406, L581](https://github.com/NixOS/nix/blob/2c6d06e9387cf58167cb5a7ab91cee7333d8d17c/src/libfetchers/github.cc#L406)).
- **`s3://` stores** use the AWS default credential provider chain and SigV4,
  including session tokens
  ([`s3-binary-cache-store.md` L44-L50](https://github.com/NixOS/nix/blob/2c6d06e9387cf58167cb5a7ab91cee7333d8d17c/src/libstore/s3-binary-cache-store.md#L44-L50)).
  This is an AWS-shaped option, not a GCP one.
- **Cloud Run consequence** *(inferred)*: Nix cannot send
  `X-Serverless-Authorization` or a Google ID token, so the Nix face must run
  with Cloud Run's invoker IAM check off and authenticate Basic/mTLS itself.
  Anonymous reads of a public repo's Nix cache need no credential at all.

## 6. Merge queue

- **Availability:** "any public repository owned by an organization, or …
  private repositories owned by organizations using GitHub Enterprise Cloud"
  ([gated-features/merge-queue.md](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/data/reusables/gated-features/merge-queue.md)).
  Repos owned by user accounts are excluded. `firefly-engineering/turnkey`
  qualifies. A consumer with a private repo on Free or Team does not.
- **Semantics**
  ([managing-a-merge-queue.md L45-L115](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue.md#L45-L115)):
  - A writer enqueues a PR that already passed the required checks.
  - The queue creates a temporary branch `gh-readonly-queue/{base}/…` holding
    base, the PRs ahead, and this PR, then dispatches `merge_group`
    (`checks_requested`).
  - It merges when the required checks pass on that branch.
  - Build concurrency ranges from 1 to 100.
  - A failure ejects the PR and rebuilds the groups behind it; jumping the
    queue rebuilds everything in flight.
  - Workflows must list `merge_group` explicitly, or required checks never
    report
    ([merge-group-event-with-required-checks.md](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/data/reusables/actions/merge-group-event-with-required-checks.md)).
- **Identity** *(inferred)*:
  - A `merge_group` job runs in the base repo with full trust: OIDC
    available, `ref` = the queue branch. This holds even if the PR came from
    a fork, because a writer enqueued it.
  - The code it builds is exactly what will land. That makes it the natural
    point for "what gates a merge" (turnkey-wit.11).
  - Its `ref` is not `main`, so a policy on landing refs must name
    `gh-readonly-queue/*` separately.

## 7. Stacked PRs

- **Native stacks (public preview; `pr-stacks` on github.com and GHEC):**
  - A stack is a chain of same-repo PRs. **Cross-fork stacks are not
    supported.**
  - Every PR in a stack is evaluated against the rules and required checks of
    the stack's base, and Actions triggers "as if each pull request in the
    stack targets the base of the stack"
    ([stacked-pull-requests.md L12-L60](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/pull-requests/reference/stacked-pull-requests.md#L12-L60)).
  - Stacks go through merge queues in order
    ([L78-L80](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/pull-requests/reference/stacked-pull-requests.md#L78-L80)).
- **Detection:**
  - The REST PR resource and `pull_request` webhook payloads carry a nullable
    `stack` object with `id`, `number`, `size`, `position` (1 = bottom) and
    `base.{ref,sha}`
    ([stacked-pull-requests-apis-and-webhooks.md L24-L58](https://github.com/github/docs/blob/18945a31a4f2d97beb6c5c1a7479102e23c25727/content/pull-requests/reference/stacked-pull-requests-apis-and-webhooks.md#L24-L58);
    pulls schema).
  - In Actions it is `github.event.pull_request.stack`.
  - A Stacks API exists at `/repos/{o}/{r}/stacks[/{n}[/add|/unstack]]`, and
    GraphQL has read-only `stack`/`stackEntry`.
- **Without native stacks:** a PR is stacked when its `base.ref` is another
  open PR's `head.ref` in the same repo. `GET /repos/{o}/{r}/pulls?head=OWNER:BRANCH`
  answers that in one call.
- Whether the OIDC `base_ref` claim of a stacked PR is the direct parent or
  the stack base is *(unverified)*.

## 8. Open points and caveats

- **Unverified:**
  - GitHub OIDC token lifetime;
  - the default `sub` for `pull_request_target`;
  - `job_workflow_ref` on non-reusable workflows;
  - the exact meaning of `ref_protected`;
  - the OIDC `ref`/`sub` for `merge_group`.

  A one-off workflow that prints the decoded token for each event would
  settle all five. That needs a real run.
- **The buck2 daemon-environment conclusion (§5) is read from code, not
  observed.** It bears directly on turnkey-wit.8 and turnkey-wit.14.
- **GitHub user tokens are opaque.** The service must call GitHub to verify
  one (`POST /applications/{client_id}/token` needs the App's client
  credentials). That is another reason to exchange once and mint a service
  token.
- **Nothing here contradicts the map's settled decisions.** Two facts
  constrain them:
  - The approval rule for non-member fork PRs matches a GitHub setting that
    must be chosen explicitly; it is not GitHub's default.
  - Merge queue, the natural merge gate, needs GitHub Enterprise Cloud for
    private repos.
