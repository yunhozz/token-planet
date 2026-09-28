# Token Planet Growth Journal Implementation Plan

> **For agentic workers:** Implement this plan task-by-task in the current session. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a private, account-wide growth journal that shows daily confirmed usage, growth milestones, and planet reset credits across cycles and devices.

**Architecture:** Derive per-device absolute daily records from the existing account-owned local usage ledger, retain them in an account-scoped SQLite outbox/cache, and synchronize them through authenticated self-only Supabase RPCs. The desktop UI merges the newest per-device record, groups days by account cycle, and computes growth once after summing confirmed tokens across devices and agents.

**Tech Stack:** React 19, TypeScript, Tauri 2, Rust, SQLite, Supabase PostgreSQL/RPC.

**Spec:** `docs/superpowers/specs/2026-09-28-token-planet-growth-journal-design.md`

## Global Constraints

- Keep existing Codex and Claude Code collection, growth formula, and planet reset rules unchanged.
- Never upload source conversations, prompts, responses, local file paths, session identifiers, or event deduplication keys.
- Store journal rows for the account owner only; do not add daily records to group APIs or group responses.
- Use the account planet's fixed IANA timezone for day buckets.
- Apply `log2(1 + confirmed tokens / 100,000)` once per account, cycle, and day after merging devices and enabled agents.
- Keep unknown totals null, omit unrecorded dates, and retain confirmed zero distinctly.
- Keep the local change set uncommitted for the user.

## Review Focus

- Two devices contribute to one cycle/day: combine confirmed tokens before calculating growth.
- A local pending row and its server copy share a key: select one newest absolute revision, never sum both.
- A reset occurs during a local date: classify each event by cycle so the date can appear in both cycles.
- A source is disabled or a historical total changes: replace the affected absolute daily row so stale server values disappear.
- A device syncs after account journal deletion: reject old generation rows and rebuild only from records after the deletion time.

---

### Task 1: Local growth journal model and outbox

**Files:**
- Create: `apps/desktop/src-tauri/src/domain/growth_journal.rs`
- Create: `apps/desktop/src-tauri/src/storage/growth_journal.rs`
- Modify: `apps/desktop/src-tauri/src/domain/mod.rs`
- Modify: `apps/desktop/src-tauri/src/storage/mod.rs`
- Modify: `apps/desktop/src-tauri/src/storage/ledger.rs`
- Modify: `apps/desktop/src-tauri/src/storage/planet_accounts.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Produce serializable `GrowthJournalCycle`, `GrowthJournalEntry`, and `GrowthJournal` models. An entry is keyed by device, cycle, date, and agent, and carries revision, token total, coverage, deletion generation, and payload hash only.
- Produce `Ledger::prepare_growth_journal()`, `Ledger::growth_journal()`, `Ledger::pending_growth_journal_entries()`, and `Ledger::apply_growth_journal_state(...)`.
- Keep account identity from `planet_account_id`; use existing `usage_record`, `planet_usage_owner`, `planet_wallet_credit`, and current cycle settings as inputs.

- [x] Add account-scoped SQLite tables for cycle metadata, daily absolute rows, remote cache, and deletion generation/cutoff.
- [x] Derive cycle boundaries from activation, current cycle, and wallet-credit timestamps; classify owned source records by timestamp and fixed planet timezone.
- [x] Aggregate confirmed token totals and collection coverage by device/cycle/date/agent; preserve null versus zero and increment revision only when the sealed payload changes.
- [x] Recompute affected dates after scans, source disablement, and record correction; preserve the journal outbox across account switches and restarts.
- [x] Merge cached server rows with local rows by full key and highest revision; apply a newer server deletion generation before exposing cached rows.
- [x] Review local SQL and serialized payload fields to confirm no source identity or raw record data is included.

### Task 2: Private Supabase API and account sync

**Files:**
- Create: `supabase/migrations/202609280001_growth_journal.sql`
- Create: `apps/desktop/src-tauri/src/commands/growth_journal.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/sync/client.rs`
- Modify: `apps/desktop/src-tauri/src/sync/worker.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Add self-only RPCs `get_my_growth_journal`, `upsert_my_growth_journal`, and `delete_my_growth_journal` over private cycle/day/deletion tables.
- `upsert_my_growth_journal` accepts absolute cycle metadata and pending day rows, validates user-owned payload fields, enforces generation and monotonic revision/hash, and returns the account's canonical journal state.
- Add Tauri commands `get_growth_journal` and `delete_growth_journal`; the first reads through the authenticated API when available and falls back to account-scoped local cache.
- Integrate pending journal rows into `sync_once`; honor existing sync pause state and apply remote deletion generations before upload.

- [x] Add private tables with account foreign keys and checks for day, agent, coverage, nonnegative confirmed counts, revisions, hashes, and deletion generation.
- [x] Add authenticated wrappers that derive the owner from `auth.uid()`; revoke direct table access and do not expose these rows through world RPCs.
- [x] Add strict Rust request/response models and RPC methods; upload only absolute aggregate rows and cycle boundary metadata.
- [x] Pull canonical rows, cache them, acknowledge matching local revisions, and keep failed/offline entries queued for retry.
- [x] Implement deletion as a server-side generation increment and cutoff, remove stored day/cycle history, then rebuild only post-cutoff local rows while retaining the current cycle identity.
- [x] Review grants, function search paths, input field allowlists, and group API diffs for account isolation.

### Task 3: Growth journal navigation and presentation

**Files:**
- Create: `apps/desktop/src/components/GrowthJournal.tsx`
- Modify: `apps/desktop/src/types/usage.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/App.css`

**Interfaces:**
- `GrowthJournal` receives a `GrowthJournal` payload and callbacks for loading a selected cycle and deleting the account journal.
- The native command returns cycle metadata and merged day rows; the component computes display-level day totals, per-cycle cumulative milestones, and reset rows from those records.

- [x] Add a detailed-view entry point and a journal view that defaults to the current cycle's most recent seven dates.
- [x] Add previous/next seven-day navigation and cycle selection; selecting a previous cycle opens the seven-day range ending on that cycle's last date.
- [x] Render only dates with journal rows, show per-agent collection status and confirmed totals, and distinguish unavailable totals from confirmed zero.
- [x] Compute first-crossed `5`, `20`, `50`, and `100` growth-credit dates from current confirmed daily aggregates without inventing event times.
- [x] Show reset time and the existing wallet-credit amount per completed cycle; identify the current cycle as in progress.
- [x] Add the explicit personal-journal deletion action and confirmation, then refresh the view from the deletion response.
- [x] Review the rendered states for empty history, overlapping reset dates, unavailable sources, offline local rows, and account switching.
