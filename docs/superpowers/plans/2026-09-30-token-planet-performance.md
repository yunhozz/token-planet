# Token Planet Performance Implementation Plan

> **For agentic workers:** Yunho's Harness is active. Route code edits through `yunho_team_coder`; the development lead owns integration and the completion decision.

**Goal:** Reduce repeated local and remote work and make unavoidable waits visible.

**Architecture:** Keep the existing collection, account, and sync contracts. Add local query indexes, reuse HTTP connections with bounded waits, reduce repeated membership queries and scene calculations, and add accessible pending feedback.

**Tech Stack:** React 19, TypeScript, Tauri 2, Rust, SQLite, reqwest.

**Spec:** User request on 2026-09-30: investigate bottlenecks, optimize feasible areas, and provide loader UI for remaining waits. CEO confirmed implementation authorization.

## Global Constraints

- Preserve account isolation, source rewrite detection, token counting, and transaction semantics.
- Keep the existing visual design and add no dependencies.
- Do not add or run tests, execute builds, or commit changes during this task.
- Evidence is source inspection, not a measured speedup or runtime verification.

## Review Focus

- Account changes must invalidate old asynchronous frontend responses.
- Existing scan and window ordering must remain intact.
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

The development lead reviewed the changed source and focused diffs. Query indexes are added after the corresponding tables exist; both remote clients clone one pooled HTTP client; membership results carry context and stale effect responses are rejected; source pending state clears in `finally`; the existing global reduced-motion rule disables the spinner. Group refresh renders a status beside existing content. No tests, builds, benchmarks, or runtime checks were executed; successful compilation and measured speedup remain unconfirmed. No commit was created.

## Remaining Bottlenecks

Native setup scans before the window appears and the worker immediately scans again. Scan-heavy commands hold the ledger lock during collection. Processed log prefixes are hashed on every scan to detect in-place rewrites. Each changed file currently rebuilds daily totals inside its transaction; changing this requires a failure-safe collection transaction design. Growth journal preparation reads account history repeatedly. Shop requests can overlap. Remote commands serialize through the sync gate, and remote journal responses include the full history. These remain candidates for a later focused change with behavioral verification and measurement.
