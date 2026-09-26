# Can niks3 run on a metadata store that scales to zero?

Research for ticket `turnkey-wit.4` of the build-service map `turnkey-wit`.
The question: can [niks3](https://github.com/Mic92/niks3) (MIT) be refactored
to run on a metadata store whose idle cost is its stored bytes only, with GCS as
the object store? If not, what else could be the Nix-cache half of the build
service?

Constraints from the map (settled, not reopened here): one Nix cache, no scopes;
only the service's Nix builder writes store paths, signed with a service-held
key; every client reads (anonymously for public repos, authenticated for private
ones); scale to zero is a hard requirement; GCP first.

- **Researched:** 2026-09-26.
- **niks3:** `Mic92/niks3@e64338d7` (commit "bump version 1.13.0",
  2026-09-23). Unless stated otherwise every niks3 link is a permalink at
  [`e64338d7`](https://github.com/Mic92/niks3/tree/e64338d774d97249ce0ace37a59767e72b5b52fb).
- **minio-go** (niks3's S3 client, `go.mod` pins `v7.3.0`):
  [`minio/minio-go@ce0e323c`](https://github.com/minio/minio-go/tree/ce0e323c55c64964e6ad820ef0c6f5b286446aae) (tag `v7.3.0`).
- **Nix:** tag `2.35.2`,
  [`NixOS/nix@2c73b59d`](https://github.com/NixOS/nix/tree/2c73b59da29606068c0c98db015dd3a66955525d).
- **Attic:** [`zhaofengli/attic@7a19204d`](https://github.com/zhaofengli/attic/tree/7a19204df10d606c5070e6bb72615c3461900c05).
- **harmonia:** [`nix-community/harmonia@63989bd7`](https://github.com/nix-community/harmonia/tree/63989bd7eb7e1986d7e5c4d8f1988260da2dc24f);
  **nix-serve-ng:** [`aristanetworks/nix-serve-ng@2c30a4d5`](https://github.com/aristanetworks/nix-serve-ng/tree/2c30a4d56da4e24021682b63e72ba92d4c4bf00a).
- **Vendor docs** (GCS, Spanner, Firestore, Cloud Run, Neon, Litestream) were
  read on 2026-09-26; Google pages carried "last updated 2026-09-24/25".
- **Method:** source reading and vendor docs only. Nothing was deployed, and no
  request was sent to GCS. Statements not read off a source are marked
  *(inferred)*; statements that could not be checked are marked *(unverified)*.

## 1. TL;DR

**Don't refactor niks3 onto another database, and don't fork it.** Two
findings decide it:

1. **niks3's Postgres use is not a thin key-value layer.** The data is
   key-value shaped (object key → direct refs), but the GC's correctness under
   concurrent pushes rests on Postgres itself: recursive CTEs for
   reachability, three PL/pgSQL functions that commit a push atomically and
   re-check its closure, row locks (`FOR UPDATE`, `FOR SHARE`), session advisory
   locks, a trigger, and a tombstone and grace-period protocol that upstream
   models in Quint ([§2](#2-how-niks3-uses-postgres)). Porting it to Firestore
   or SQLite means re-deriving that protocol, not swapping a driver.
2. **Most of what that machinery protects is not needed here.** niks3 exists so
   that many remote, semi-trusted uploaders (CI jobs holding only a token) can
   push straight to S3 through presigned URLs while a GC runs. In the build
   service the only writer is the service's own Nix builder, which holds bucket
   credentials. Nix already writes NARs, narinfos and signatures to an S3 store
   by itself ([§4](#4-is-niks3s-upload-design-needed-here)). What remains
   valuable is **GC** and a **read proxy for private caches**.

**Recommendation** ([§7](#7-recommendation)):

- **Write:** the builder pushes with Nix's own S3 binary-cache store pointed at
  GCS's XML API (HMAC key), signing with `secret-key`. *(GCS end to end is
  unverified; smoke-test it first.)*
- **Read:** public caches read GCS (or a CDN) directly; private caches go
  through a small stateless read proxy on Cloud Run.
- **GC:** a scheduled Cloud Run job does a mark-and-sweep over the bucket. The
  roots are recently written or touched narinfos plus named pins. A lease held
  in GCS (create-if-absent preconditions, strong consistency) serialises the
  job against builder uploads. No database; idle cost is the bucket.
- **Fallback if niks3's features are wanted as-is:** upstream niks3, unpatched,
  on [Neon](https://neon.com/docs/introduction/scale-to-zero) serverless
  Postgres over a direct (unpooled) connection. No code change, and it scales
  to zero, but it puts the metadata outside GCP (see §8).

Candidate verdicts ([§3](#3-refactor-candidates)): **Spanner no** (100 PU
minimum, "no suspend mode"). **Bigtable no.** **Firestore feasible but weeks
of work** that upstream is unlikely to take. **SQLite + Litestream** needs a
single writer, which Cloud Run cannot guarantee. **Neon** works with zero code.
**No database** works because there is one writer.

## 2. How niks3 uses Postgres

### 2.1 Where it sits

The README's architecture: clients ask the server for an upload and get
presigned S3 URLs; they PUT NARs and narinfos straight to S3; the server
records references in Postgres for GC; reads go straight to S3 or a CDN, or
optionally through niks3 as a read proxy
([`README.md`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/README.md)).

- The server connects and runs goose migrations **at startup**, before it
  serves anything, read proxy included
  ([`server/server.go` L195-L203](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/server.go#L195-L203),
  [`server/pg/pg.go` L17-L40](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/pg.go#L17-L40)).
  So even a read-only niks3 instance wakes the database on every cold start.
- Queries are generated by sqlc with the `postgresql` engine and `pgx/v5`
  ([`sqlc.yml`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/sqlc.yml)):
  32 named queries in
  [`server/pg/query.sql`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/query.sql),
  plus three function files. 16 non-test server files touch the pool or
  queries *(counted with grep)*.
- `modernc.org/sqlite` is in `go.mod`, but only for the client-side
  post-build-hook queue
  ([`hook/queue.go`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/hook/queue.go)),
  not the server.

### 2.2 Schema

From [`20241026095416_initial_model.sql`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/migrations/20241026095416_initial_model.sql)
and later migrations:

| Table | Role |
|---|---|
| `closures(key, updated_at)` | GC roots: one row per pushed root narinfo; `updated_at` decides retention. |
| `objects(key, refs[], deleted_at, first_deleted_at, size)` | Every bucket object (narinfo, NAR, `.ls`, log, realisation) with its **direct** refs; tombstone columns for the grace period. |
| `pending_closures(id, key, started_at, roots[])` | In-flight uploads ("pushes", `roots` added in [`20260923120000_add_pushes.sql`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/migrations/20260923120000_add_pushes.sql)). |
| `pending_objects(pending_closure_id, key, refs[], size)` | Objects a pending push promised to upload. |
| `multipart_uploads(pending_closure_id, object_key, upload_id)` | Open S3 multipart uploads, so stale ones can be aborted. |
| `pins(name, narinfo_key → closures.key, store_path)` | Named roots exempt from GC ([`20251218171726_add_pins.sql`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/migrations/20251218171726_add_pins.sql)). |
| `object_stats` | Single-row running totals kept by a trigger ([`20260628120000_add_object_size_and_stats.sql`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/migrations/20260628120000_add_object_size_and_stats.sql), [`functions/2_object_stats_trigger.sql`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/functions/2_object_stats_trigger.sql)). |

The data model itself is a key → refs graph: key-value shaped.

### 2.3 Upload bookkeeping (write path)

1. **Create a push** in one transaction
   ([`pending_closure.go` L169-L278](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pending_closure.go#L169-L278)):
   insert the pending row, look up which objects already exist, and bulk-insert
   the missing ones with `COPY FROM`
   ([`query.sql` L14-L15](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/query.sql#L14-L15)).
   Live objects are skipped. The server can optionally `StatObject` them in S3
   to catch drift (L232-L249).
2. **Tombstoned objects** (marked by GC but not yet deleted) are not reused.
   The server polls until the tombstone is 30 s old, then asks the client to
   re-upload them, unless another push resurrected them meanwhile
   ([`waitForDeletion` L128-L167](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pending_closure.go#L128-L167)).
3. **Presigned URLs** are handed out: a single PUT for small objects, and a
   multipart upload with presigned part URLs for large NARs
   ([L283-L385](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pending_closure.go#L283-L385)).
   Validity is 5 h (L24).
4. **Each completed upload is registered** straight away with an upsert that
   merges ref arrays and clears tombstones
   ([`query.sql` L53-L67](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/query.sql#L53-L67)).
5. **Signing:** the client sends narinfo metadata. The server signs only keys
   that belong to the pending push and returns the signatures
   ([`uploads.go` L596-L640](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/uploads.go#L596-L640)).
   The key never leaves the server.
6. **Commit** is PL/pgSQL
   ([`functions/3_commit_push.sql`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/functions/3_commit_push.sql),
   [`functions/1_commit_pending_closure.sql`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/functions/1_commit_pending_closure.sql)).
   It walks the push's roots recursively over live objects plus its own
   pending objects. If anything under them was collected since the push began,
   it raises "Push object missing" and the handler returns 409
   ([`pushes.go` L80-L110](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pushes.go#L80-L110)).
   Otherwise it upserts `closures`, moves pending objects into `objects` (merge
   refs, resurrect tombstones, `xmax = 0` to detect insert), and deletes the
   pending rows, all in one transaction.

### 2.4 Garbage collection

`runGarbageCollection`
([`closure.go` L172-L278](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/closure.go#L172-L278)):

1. Take a **session advisory lock** on a dedicated connection. It serialises GC
   across replicas; the in-process `GCTaskStore` only dedupes inside one
   process ([L17-L56](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/closure.go#L17-L56)).
2. Expire pending pushes older than `failed-uploads-older-than` (default 6 h,
   longer than the 5 h URL validity). Their objects are inserted as already
   tombstoned so that half-uploaded files are swept too
   ([`query.sql` L80-L118](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/query.sql#L80-L118)).
3. Delete `closures` older than `older-than`, except pinned ones
   ([L139-L143](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/query.sql#L139-L143)).
4. **Mark:** one statement with a recursive CTE computes reachability from all
   closures. Every live object that is unreachable and not pending gets
   tombstoned, under `FOR UPDATE`
   ([`MarkStaleObjects` L179-L220](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/query.sql#L179-L220)).
5. **Sweep:** objects tombstoned for longer than the grace period are deleted
   from S3 in batches of 1000. Rows are dropped on success and un-tombstoned on
   failure ([`objects_model.go`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/objects_model.go)).
6. `VACUUM ANALYZE` the GC tables
   ([`closure.go` L292-L305](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/closure.go#L292-L305)).

The race between pushes and GC is modelled in Quint
([`spec/pushes.qnt`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/spec/pushes.qnt),
invariant `safe = closureComplete and noDuplicateUploads`, L169-L175). The git
history shows the protocol was hardened step by step: "fix concurrent GC and
commit race with grace period", "use first_deleted_at for grace period…",
"Fix critical GC bug where resurrected objects could be deleted" (2025-10), and
"pins: make pin creation robust against concurrent GC" (2026-05, `FOR SHARE`
in
[`query.sql` L231-L236](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pg/query.sql#L231-L236)).

### 2.5 Other Postgres-only features

- **Build-farm leader election** over a session advisory lock held for the
  length of an NDJSON stream
  ([`farm.go` L14-L41](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/farm.go#L14-L41)).
  Not needed here.
- The `object_stats` trigger (metrics only).

### 2.6 Verdict: SQL-heavy or thin KV?

**Thin KV in data, SQL-heavy in correctness.** The Postgres features it relies
on:

- recursive CTEs (mark, commit check, closure listing);
- PL/pgSQL transactions;
- `ON CONFLICT` with array merges;
- row locks and advisory locks;
- `COPY`, a trigger, and `VACUUM`.

A port has to rebuild each of these guarantees, and then re-check them against
the Quint model.

## 3. Refactor candidates

"Idle cost" means cost while no build runs.

| Candidate | Scales to zero? | Effort to port niks3 | Correctness under concurrent upload + GC | Verdict |
|---|---|---|---|---|
| **Cloud SQL** (context) | No; settled in the map | none | as upstream | out |
| **Cloud Spanner** | **No.** Minimum 100 processing units, and "Spanner doesn't have a suspend mode… even when you are not running a workload, Spanner frequently performs background work" ([compute capacity](https://docs.cloud.google.com/spanner/docs/compute-capacity)) | high | fine (external consistency) | out |
| **Bigtable** | No (provisioned nodes) *(not re-checked)* | high, and no multi-row transactions | poor | out |
| **Firestore (native)** | **Yes.** Billed only for operations, storage and egress, with a free tier ([billing example](https://docs.cloud.google.com/firestore/docs/billing-example), [quotas](https://docs.cloud.google.com/firestore/native/docs/quotas)) | **high (weeks)** *(inferred)*: replace 32 queries, 3 functions and a trigger; turn the recursive CTEs into app-side BFS over batched gets; advisory lock becomes a lease doc | achievable with transactions, but the protocol must be redesigned and re-modelled | possible, poor value |
| **SQLite + Litestream → GCS** | Yes, if the DB is restored on cold start | **medium** *(inferred)*: SQLite has recursive CTEs; arrays become JSON or a join table; PL/pgSQL moves to Go; locks go away (one writer) | Only with **exactly one writer process**, which Cloud Run does not guarantee (below) | risky |
| **Neon** (serverless Postgres, outside GCP) | **Yes.** Compute suspends after 5 min idle and wakes "within a few hundred milliseconds"; storage is billed ([scale to zero](https://neon.com/docs/introduction/scale-to-zero), [pricing](https://neon.com/pricing)) | **none** (connection string) | as upstream, **if** a direct connection is used (below) | works; outside GCP |
| **No database** | Yes | new small GC tool, no niks3 code *(inferred: a few hundred lines)* | Fine with one writer plus a GCS lease (§6) | **recommended** |

### Notes per candidate

**Firestore.**

- Transactions are limited to 270 s, with a 60 s idle expiry
  ([quotas](https://docs.cloud.google.com/firestore/native/docs/quotas)).
- A commit that moves thousands of pending objects in one transaction may hit
  the per-commit write limits. The current limit was not found on the quotas
  page *(unverified)*.
- Every GC mark reads every object document: O(objects) reads per run,
  priced per 100k ([billing example](https://docs.cloud.google.com/firestore/docs/billing-example)).
- Pricing figures on that page are an example; check the region's rates
  before costing.

**SQLite + Litestream.**

- Litestream: "It is your responsibility to ensure you do not have multiple
  applications replicating concurrently", or restores can fail
  ([tips](https://litestream.io/tips/)).
- Cloud Run's max-instances is a soft cap: it "can be exceeded for a brief
  period due to circumstances such as traffic spikes"
  ([max instances](https://docs.cloud.google.com/run/docs/configuring/max-instances)).
  Setting `max-instances=1` therefore does not prevent two writers. Fencing
  with a GCS lease would be needed anyway, and at that point the lease alone
  (§6) is enough.
- Cold start must restore the whole database from GCS *(inferred: seconds to
  minutes, depending on object count)*.

**Neon.**

- niks3 takes a *session* advisory lock for GC (and farm leadership). Neon's
  pooled (PgBouncer transaction-mode) endpoint lists "Session-level advisory
  locks" as unsupported
  ([connection pooling](https://neon.com/docs/connect/connection-pooling)).
  **Use the direct connection string.**
- The Free plan caps storage at 0.5 GB per project. Paid plans have "no
  monthly minimum": $0.106/CU-hour (Launch) and $0.35/GB-month storage
  ([pricing](https://neon.com/pricing)).
- Cold path *(inferred)*: Cloud Run cold start, then niks3 connects and runs
  migrations, then Neon wakes (a few hundred ms). This happens on the write
  path, and on the read path when the read proxy is used (§2.1).

## 4. Is niks3's upload design needed here?

Mostly no. niks3's write path solves one problem: **uploaders that are remote
and hold no bucket credentials.**

- Presigned URLs, pending pushes, multipart bookkeeping and OIDC write scopes
  exist so a CI job with a token can PUT directly to S3.
- The server-side signing endpoint exists so the signing key never reaches
  that job.

In the build service the only writer is the service's own Nix builder, inside
the trust boundary, holding bucket credentials and the signing key (map Notes).
Nix already does everything that path provides:

- **NAR + narinfo + signing.** Nix's binary-cache store writes the NAR (unless
  the file already exists), checks that every reference is valid, signs with
  the configured `secret-key`/`secret-keys`, and only then writes the narinfo
  ([`binary-cache-store.cc` L265-L269, L280-L298](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/binary-cache-store.cc#L265-L298),
  signers at [L36-L43](https://github.com/NixOS/nix/blob/2c73b59da29606068c0c98db015dd3a66955525d/src/libstore/binary-cache-store.cc#L36-L43)).
  **The narinfo is the commit point.** The reference check may be answered from
  Nix's narinfo cache ("typically they'll already be cached", L282-L283), so
  it is not a guarantee against a concurrent GC.
- **S3-compatible endpoints.** The S3 store's `endpoint` setting is "for
  S3-compatible services", and `secret-key` is the signing key path
  ([Nix 2.35.2 manual, S3 binary cache store](https://nix.dev/manual/nix/latest/store/types/s3-binary-cache-store)).

What niks3 would still add:

| niks3 part | Still valuable? |
|---|---|
| Presigned upload, pending pushes, multipart tracking | No: the builder has credentials. |
| OIDC/API-token write auth | No: there are no external writers. |
| Server-side signing | No: Nix signs in the builder. |
| Narinfo generation | No: Nix generates narinfos. |
| **GC** (reachability, retention, pins, grace) | **Yes.** This is the real gap. |
| **Read proxy** (`--enable-read-proxy`, OIDC read scope, optional 307 to presigned GET via `--read-redirect-ttl`) | **Yes, for private caches.** The code itself uses no DB ([`proxy.go` L219](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/proxy.go#L219), route at [`server.go` L320](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/server.go#L320)), but the binary requires Postgres at startup (§2.1). |
| Pins | Nice to have: a named root is easy to model without a DB (§6). |
| Metrics / cache stats | Nice to have. |

## 5. GCS as niks3's object store

niks3 talks to S3 through minio-go (`minio.New` with static V4 credentials, or
`credentials.NewIAM`,
[`server.go` L205-L218](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/server.go#L205-L218)).
Against GCS's XML API:

- **Multi-object delete** (`POST ?delete`, used by `RemoveObjectsWithResult` in
  the sweep) is **not** in GCS's XML API
  ([XML API overview](https://docs.cloud.google.com/storage/docs/xml-api/overview)).
  minio-go handles this: when the endpoint host is exactly
  `storage.googleapis.com` it falls back to single DELETEs
  ([`api-remove.go` L33-L39, L565-L572](https://github.com/minio/minio-go/blob/ce0e323c55c64964e6ad820ef0c6f5b286446aae/api-remove.go#L33-L39),
  [`pkg/s3utils/utils.go` L278-L284](https://github.com/minio/minio-go/blob/ce0e323c55c64964e6ad820ef0c6f5b286446aae/pkg/s3utils/utils.go#L278-L284)).
  So `--s3-endpoint` must be `storage.googleapis.com`, not a custom domain.
- **Multipart uploads** (initiate, upload part, complete, abort, list parts)
  are supported by the XML API
  ([XML API overview](https://docs.cloud.google.com/storage/docs/xml-api/overview),
  [migrating from S3](https://docs.cloud.google.com/storage/docs/migrating)).
  **Presigned `UploadPart` URLs** (niks3 presigns each part,
  [`pending_multipart.go` L134-L166](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/server/pending_multipart.go#L134-L166))
  were not tested against GCS *(unverified)*.
- **V4 signatures with HMAC keys** are supported. However, "chunked transfer
  encoding and V4 signatures can't be used simultaneously"
  ([migrating](https://docs.cloud.google.com/storage/docs/migrating)).
  minio-go already special-cases Google for streaming uploads
  ([`api-put-object-streaming.go`](https://github.com/minio/minio-go/blob/ce0e323c55c64964e6ad820ef0c6f5b286446aae/api-put-object-streaming.go)).
- **Credentials on Cloud Run:** `--s3-use-iam` uses minio's AWS IAM provider
  (the EC2 metadata endpoint `169.254.169.254`,
  [`pkg/credentials/iam_aws.go` L91](https://github.com/minio/minio-go/blob/ce0e323c55c64964e6ad820ef0c6f5b286446aae/pkg/credentials/iam_aws.go#L91)).
  It cannot use a GCP service account's metadata token, so a **service-account
  HMAC key** stored as a secret is needed *(inferred)*.
- **Listing:** GCS's XML API accepts `list-type=2`
  ([XML API overview](https://docs.cloud.google.com/storage/docs/xml-api/overview)).
- Upstream tests run against RustFS, not GCS
  ([`CONTRIBUTING.md`](https://github.com/Mic92/niks3/blob/e64338d774d97249ce0ace37a59767e72b5b52fb/CONTRIBUTING.md)),
  and the issue tracker has no GCS reports (search for "gcs" and "google
  cloud" on 2026-09-26 returned nothing).

**Verdict:** niks3 should work against GCS's S3 interop with an HMAC key and
the exact `storage.googleapis.com` endpoint. It does not need a native GCS
backend, but it is untested there *(unverified)*. The same interop question
applies to Nix's own S3 store (§7).

## 6. The "no database" design

Only the builder writes, so the bucket plus a lease can hold all GC state.
This is a design sketch *(inferred)*. It is built on two GCS guarantees:

- **Strong consistency:** an object is readable "as soon as you receive a
  success response", and listing and metadata updates are strongly consistent.
  The exception is publicly cached objects
  ([consistency](https://docs.cloud.google.com/storage/docs/consistency)).
- **Create-if-absent:** `x-goog-if-generation-match: 0` "only proceeds if no
  object with the specified name exists", and the XML API supports it
  ([preconditions](https://docs.cloud.google.com/storage/docs/request-preconditions)).

The design:

- **Roots:** narinfos whose `updated` time falls within the retention window,
  plus pins stored as `_pins/<name>` objects that name a narinfo. The builder
  "touches" a root it reuses by patching its metadata. That refreshes
  `updated`, and metadata updates are strongly consistent.
- **Graph:** GET every narinfo and parse `References:` and `URL:`. The edges
  are narinfo → NAR, narinfo → referenced narinfos, and narinfo → `.ls`, log
  and realisation files. This is O(narinfos) Class-B reads per GC run.
  Keeping refs in custom metadata so a listing returns them would cut the
  reads, but that needs a custom writer *(inferred)*.
- **Mutual exclusion with uploads** (a Dekker-style write-then-check; the lease
  names and TTLs are illustrative):
  - The builder creates `_gc/uploads/<id>` (create-if-absent) **before** asking
    the cache which paths it has. It then checks that `_gc/lock` is absent,
    and deletes its marker after the last narinfo is written.
  - The GC job creates `_gc/lock` (create-if-absent), then lists
    `_gc/uploads/`. If any fresh marker exists, it backs off.
  - Both sides write before they read, and GCS is strongly consistent, so at
    least one sees the other.
  - Markers and the lock need a TTL with renewal, so that a crashed holder does
    not block forever. A holder that outlives its TTL without renewing
    reopens the race; renewal must fail closed.
- **Sweep order:** delete narinfos before their NARs, so a narinfo never points
  at a missing NAR.
- **Why this is simpler than niks3:** GC is exclusive of uploads rather than
  concurrent with them. That removes tombstones, grace periods and the
  commit-time closure re-check. It is acceptable because GC is rare, and in a
  scale-to-zero service it runs when nothing else does *(inferred)*.
- **Verification:** the protocol should be model-checked before it is trusted.
  niks3's Quint spec is a starting point.

## 7. Recommendation

1. **Nix-cache half = GCS bucket + builder writes + GC job + optional read
   proxy.** No database.
   - **Write:** `nix copy --to 's3://<bucket>?endpoint=storage.googleapis.com&secret-key=…'`
     from the builder. *(Unverified against GCS; the first smoke test should
     cover large-NAR multipart and credentials from an HMAC key.)*
   - **Read:** public caches read GCS or a CDN directly. Mind the consistency
     caveat for cached public objects: once GC deletes a narinfo, a CDN may
     keep serving it until its TTL expires, so NAR deletion should lag narinfo
     deletion by at least the cache TTL *(inferred)*.
   - **Private caches:** a stateless proxy. Either write a small one, or reuse
     niks3's `proxy.go` behind a **fetch-and-patch** patch that makes Postgres
     optional when only the read proxy runs (per turnkey's "Importing External
     Software" policy).
   - **GC:** a Cloud Run job as in §6.
2. **Fallback:** upstream niks3, unmodified, on Neon over a direct connection.
   It works today and scales to zero, but it runs niks3's whole remote-upload
   machinery for a single trusted writer, and its metadata lives outside GCP.
3. **Not recommended:** porting niks3 to Firestore or SQLite. It costs weeks,
   re-opens a race that took upstream a year and a formal model to close, and
   is unlikely to be accepted upstream (§8).

## 8. Alternatives and upstream appetite

| Option | Metadata store | Read path | Scale to zero? | Fit |
|---|---|---|---|---|
| **Attic** | Postgres or SQLite through sqlx ([`server/Cargo.toml` L79-L80](https://github.com/zhaofengli/attic/blob/7a19204df10d606c5070e6bb72615c3461900c05/server/Cargo.toml#L79-L80)) | **Every narinfo read hits the DB** ([`binary_cache.rs` L142](https://github.com/zhaofengli/attic/blob/7a19204df10d606c5070e6bb72615c3461900c05/server/src/api/binary_cache.rs#L142)), and narinfos are signed at read time (L154-L157). Multi-chunk NARs are reassembled and streamed by the server; only single-chunk NARs redirect ([L195-L250](https://github.com/zhaofengli/attic/blob/7a19204df10d606c5070e6bb72615c3461900c05/server/src/api/binary_cache.rs#L195-L250)) | Worse than niks3: the DB and server sit on the read path | poor |
| **harmonia** | none; serves a live `/nix/store` ("serves your /nix/store as a binary cache over http", [README](https://github.com/nix-community/harmonia/blob/63989bd7eb7e1986d7e5c4d8f1988260da2dc24f/README.md)) | through the server | Needs a persistent Nix store: disk plus a running host | poor |
| **nix-serve-ng** | same model: a drop-in `nix-serve` replacement ([README](https://github.com/aristanetworks/nix-serve-ng/blob/2c30a4d56da4e24021682b63e72ba92d4c4bf00a/README.md)) | through the server | as harmonia | poor |
| **`nix copy --to s3://…` + GC job** | none | direct from the bucket | **yes** | **recommended** (§7) |
| **niks3 + Neon** | Postgres, external | direct, or via proxy | yes (Neon suspends) | fallback |

**Upstream appetite for a pluggable metadata backend (niks3):** no signal
either way, leaning against *(inferred)*.

- GitHub Discussions are disabled on the repo.
- Searches of issues and PRs for "sqlite", "database", "backend",
  "serverless", "pluggable", "gcs", "google cloud", "cloud run", "lambda" and
  "D1" (2026-09-26) found no request for another metadata store.
- Recent work deepens the Postgres coupling: build claims were added as an
  `UNLOGGED` table and then dropped (2026-09); farm leadership uses an advisory
  lock; pushes brought the PL/pgSQL `commit_push` (2026-09-23).
- A backend abstraction would touch most of `server/` during active churn.
  Ask the maintainer before investing. (Per instructions, no issue was opened.)

## 9. Contradictions with settled map decisions

- **None of substance.** The map note "niks3 needs Postgres → explore
  refactoring it onto a scale-to-zero store" is answered **no**: the finding is
  to drop niks3 from the write path rather than refactor it.
- **"GCP first":** the only zero-code niks3 route (Neon) puts the metadata
  store outside GCP. The recommended design stays inside GCP.

## Not covered or not verified

- Nothing was deployed. Nix's S3 store and niks3 were not run against GCS.
  Multipart, presigned part URLs and HMAC credentials remain untested there.
- Firestore's per-commit write limit, and current per-region Firestore and GCS
  operation prices, were not confirmed (the GCS pricing page could not be read).
- The §6 lease protocol is a sketch. It has not been model-checked.
- Cold-start latency of niks3 on Cloud Run + Neon was not measured.
- The size of GC state at realistic object counts (narinfo reads per GC run,
  SQLite restore time) was not estimated with data.
