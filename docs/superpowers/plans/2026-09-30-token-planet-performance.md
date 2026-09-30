# Token Planet Performance Implementation Plan

> **For agentic workers:** Yunho's Harness is active. Route code edits through `yunho_team_coder`; the development lead owns integration and the completion decision.

**Goal:** Reduce repeated local and remote work and make unavoidable waits visible.

**Architecture:** Keep the existing collection, account, and sync contracts. Add local query indexes, reuse HTTP connections with bounded waits, reduce repeated membership queries and scene calculations, and add accessible pending feedback. Show the loading window before background collection, aggregate changes once per source, and recompute derived planet state without collecting logs again.

**Tech Stack:** React 19, TypeScript, Tauri 2, Rust, SQLite, reqwest.

**Spec:** User request on 2026-09-30: investigate bottlenecks, optimize feasible areas, and provide loader UI for remaining waits. CEO confirmed implementation authorization.

## Global Constraints

- Preserve account isolation, source rewrite detection, token counting, and atomic checkpoint/aggregate consistency.
- Keep the existing visual design and add no dependencies.
- The initial source-only review preceded later user authorization for tests and builds; current verification is recorded below.
- Distinguish passing tests/builds from native runtime checks and measured performance results.

## Review Focus

- Account changes must invalidate old asynchronous frontend responses.
- Final startup window mode and user-controlled hidden/detail states must be preserved when scanning moves into the background.
- Source actions must prevent overlapping conflicting requests.
- Network timeout errors must preserve existing retry and offline paths; timed-out mutations may already have reached the server.
- Loaders must expose status text and respect reduced motion.

### Task 1: SQLite queries

**Files:** `apps/desktop/src-tauri/src/storage/ledger.rs`, `storage/planet_accounts.rs`.

- [x] Add idempotent indexes for usage records by source/agent/kind and usage ownership by account/event.
- [x] Inspect schema initialization and query predicates in source.

### Task 2: Network waits

**Files:** `apps/desktop/src-tauri/src/sync/client.rs`, `sync/auth.rs`, shared HTTP helper if needed.

- [x] Reuse a pooled client with a 5-second connect timeout and a 20-second request timeout.
- [x] Preserve transport-error mapping and mutation request identity.
- [x] Inspect callers to confirm retry and offline behavior are retained.

### Task 3: Frontend work and pending feedback

**Files:** `apps/desktop/src/App.tsx`, `components/SourceStatus.tsx`, `components/PlanetScene.tsx`, `components/LoadingStatus.tsx`, `src/App.css`.

- [x] Key membership queries by stable account/world/role identity, refresh on member-count changes, and retry on manual refresh.
- [x] Memoize scene tiles across avatar animation updates.
- [x] Provide accessible loading feedback for startup, source operations, refresh, shop, journal, and group lookup.
- [x] Inspect pending-action cleanup, error display, and reduced-motion rules. Keep existing group content mounted during background refresh.

## Completion Evidence

The development lead reviewed the changed source and focused diffs. Query indexes are added after the corresponding tables exist; both remote clients clone one pooled HTTP client; membership results carry context and stale effect responses are rejected; source pending state clears in `finally`; the existing global reduced-motion rule disables the spinner. Group refresh renders a status beside existing content. At this initial review checkpoint, no tests, builds, benchmarks, or runtime checks had been executed; later work and verification are recorded below. No commit was created.

### Follow-up: collection and startup

- [x] Show the loading popup before background account restoration and the initial scan. Reuse that scan for the first worker cycle.
- [x] Restore saved account identity before publishing usage, preserve hidden/detail window states, and retain scan failures for startup retry.
- [x] Collect each source in one transaction with per-file savepoints. Rebuild daily totals once only when usage rows changed; identical record UPSERTs do not trigger rebuilding.
- [x] Reuse the validated prefix digest and extend it with complete raw JSONL lines from the same file handle. Continue verifying all previously processed bytes, including same-size rewrites, and clear prior rows on truncation to an empty file.
- [x] Recompute derived planet snapshots after remote merges and reset without rereading logs. Use the collector's latest usage under the ledger lock and refuse this path after a collector failure.

The follow-up source review covered savepoint rollback, root transaction rollback, account restoration errors, missed frontend error events, hidden windows, and ledger/snapshot lock ordering. The coder ran `git diff --check` successfully. At that follow-up checkpoint, no tests, builds, benchmarks, or native UI runtime checks had been performed. Logs appended during remote synchronization are reflected by the next actual collection scan rather than by redundant scans after remote merges.

### Follow-up: journal reuse and shop requests

- [x] Reuse a successfully prepared growth journal only when SQLite `total_changes` and `data_version` still match. Actual local writes or external commits require preparation again. Do not cache a preparation that observed an external commit during its work.
- [x] Skip identical setting UPSERTs so unchanged source status and unchanged remote settings do not invalidate journal reuse needlessly.
- [x] Coalesce in-flight shop state requests for one user/cycle across automatic reload, synchronization events, retry, and explicit refresh. Purchase/equip success invalidates earlier reads; failures release pending requests for retry.
- [x] Show shop refresh feedback beside existing content and explain synchronization waits using the existing loaders.

These changes add no dependencies or schema contracts. Journal preparation still reads full history after actual DB changes; rendering and remote synchronization may also read full history. Read-request coalescing does not merge purchases or equip mutations. The serialized synchronization gate remains in place.

An independent read-only review of shop request generations/retry and journal cache invalidation found no material issues requiring correction. That review itself ran no tests or builds; subsequent verification is recorded below.

## Verification

The user later authorized focused tests and builds. The development lead reports:

- `cargo test --offline --locked`: 101 Rust tests passed, including the scan rollback/truncation and journal cache regression cases.
- `cargo check`: passed.
- Strict Clippy reports two existing `clone_on_copy` findings in `storage/growth_journal.rs`; rerunning with `-A clone_on_copy` passed. No changes were made for these unrelated findings.
- `npm --prefix apps/desktop test`: all 73 tests across 8 files passed, including same-context shop request coalescing, retry after a shared read fails, and preserving a confirmed purchase against an older read.
- `npm --prefix apps/desktop run build`: TypeScript and Vite build passed.
- `rustfmt +stable --check --edition 2021` passed for the changed Rust files; `git diff --check` passed. A repository-wide `cargo fmt --check` still reports pre-existing formatting differences outside those files.

No native Tauri runtime check or performance benchmark was performed, so real-window behavior and measured speedup remain unverified. No commit was created.

## Remaining Bottlenecks

Scan-heavy manual commands still hold the ledger lock during collection. Every actual scan still hashes processed log prefixes to detect arbitrary same-size in-place rewrites; the redundant reread after parsing and the redundant scans after remote merges have been removed. Changed sources still perform a full daily aggregate rebuild once per source. A safe incremental design must cover historical snapshot suppression by new response records, source rewrites, and duplicate event relocation between sources, which can affect dates beyond the appended rows. Growth journal preparation reads history when DB changes, and remote journal responses/rendering still include full history. Superseded shop contexts can have outstanding server requests because Tauri invocations are not canceled, though their results are excluded. Remote commands remain serialized through the sync gate to preserve upload ordering against pause, deletion, leaving, and account changes; loader feedback covers the wait. These remain candidates for a focused change with behavioral verification and measurement.
