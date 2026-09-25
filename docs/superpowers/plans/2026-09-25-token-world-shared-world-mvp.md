# Token World Shared World MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a solo Token World owner create or join a private group of up to 10 people and grow one planet from aggregate-only usage snapshots.

**Architecture:** The desktop Rust process sends idempotent daily snapshots for each linked device and agent. A managed relational service owns authentication, membership, invitations, and aggregate storage; row-level access rules protect every world. Other group members receive only the planet's combined totals and progress.

**Tech Stack:** Tauri 2 + React + Rust client; Supabase PostgreSQL, Auth, and row-level access policies for the shared service (user-selected).

**Spec:** `docs/specs/token-world-mvp.md`

## Global Constraints

- Shared worlds have 1–10 members, begin as a solo world, and are private by default.
- No public directory, member leaderboard, or exact member token totals.
- Server data is limited to account/world membership, device identifiers, aggregate daily per-agent counts, coverage, and sync metadata.
- Never upload raw JSONL, prompts, conversation bodies, tool output, local file paths, or session IDs.
- Sync is opt-in and can be paused; the user can delete their synced aggregates and leave a world.
- The service may read the per-member aggregates as disclosed in the approved privacy proposal.
- Retries replace a device's snapshot at the same key; they never add a duplicate.
- Unknown source usage remains explicitly incomplete, not zero.

## Review Focus

- A replayed upload must not increase the world total; pin this in Task 3's idempotency test.
- A user from another world must not read or mutate a world aggregate; pin this in Task 2's authorization tests.
- A stale device revision must not overwrite a newer snapshot; pin this in Task 3's conflict test.
- An expired, revoked, or already-used invite must not add a member; pin this in Task 3's invite tests.
- A member leaving or deleting an account must not leave their aggregate visible in the world; pin this in Task 4's deletion test.

## File Structure

The selected Supabase implementation uses:

- `supabase/migrations/0001_worlds_and_memberships.sql`: worlds, membership roles, membership constraints, and row-level policies.
- `supabase/migrations/0002_daily_usage_snapshots.sql`: per-device daily agent snapshots, uniqueness keys, and aggregate access policies.
- `supabase/migrations/0003_invites.sql`: hashed invite codes, expiry, revocation, and single-use constraints.
- `supabase/tests/world_access.sql`: cross-world and role access tests.
- `apps/desktop/src-tauri/src/sync/aggregate.rs`: privacy-safe upload DTO and payload validation.
- `apps/desktop/src-tauri/src/sync/client.rs`: authenticated upload, retry, backoff, and revision handling.
- `apps/desktop/src-tauri/src/storage/outbox.rs`: local pending snapshot queue and sent revision state.
- `apps/desktop/src-tauri/src/commands/sharing.rs`: opt-in, pause, leave, and deletion commands.
- `apps/desktop/src/components/SharingSetup.tsx`: sign-in and create/join steps.
- `apps/desktop/src/components/InvitePanel.tsx`: invite creation, copy, revocation, and acceptance.
- `apps/desktop/src/components/SyncStatus.tsx`: last successful sync and queued state.
- `apps/desktop/src/lib/sharing.ts`: typed frontend commands for sharing state.

## Provider and account options for review

| Approach | Benefits | Costs | Recommendation |
|---|---|---|---|
| Managed PostgreSQL + Auth + row-level policies (Supabase candidate) | Relational member/day/tool aggregates, SQL constraints, centralized auth and access policy | Provider dependency; desktop auth redirect and token storage need careful setup | Recommended for a 1–10 member MVP |
| Firebase Auth + Firestore | Fast auth and realtime data flow | Aggregate-per-member/day access rules and reporting are less natural; denormalized rules need careful testing | Suitable if realtime-first experience becomes the priority |
| Custom Rust API + PostgreSQL | Full control of invite, privacy, retention, and account flows | Requires operating, monitoring, securing, and updating a backend from day one | Defer until managed-provider limits are known |

The user selected email verification-code sign-in inside the desktop app across both platforms. Recommended invitation is a private, single-use, revocable random code/link with a 7-day expiry and a 10-member cap. Confirm invite expiry and world-owner departure behavior before implementation.

## Tasks

### Task 0: Approve provider and privacy contract

**Files:** update this plan and `docs/specs/token-world-mvp.md` only after review; no implementation files yet.

- [x] Select Supabase, Firebase, or custom Rust API using the comparison above. The user selected Supabase.
- [x] Confirm email magic-link sign-in or select a different cross-platform sign-in method. The user selected an email verification code entered in the app.
- [ ] Confirm invite expiry, one-use behavior, and whether an owner may transfer ownership or must dissolve the world on departure.
- [ ] Confirm device-scoped dedupe for MVP or select account-scoped pseudonymous event hashes; device-scoped dedupe is recommended and does not catch a user manually copying the same source history onto another device.
- [ ] Confirm one shared world per account or multiple shared worlds; one shared world per account is recommended for MVP, while solo progress remains local until sharing starts.
- [ ] Use the reviewed `K = 100,000`, the world creator's fixed IANA timezone, and cumulative thresholds 5, 20, 50, and 100 credits before defining the server growth aggregate.
- [ ] Inspect remote Git refs and branch protection before creating provider files; preserve any existing remote history.

### Task 1: Define the aggregate-only sync contract

**Files:** create `apps/desktop/src-tauri/src/sync/aggregate.rs`, `apps/desktop/src-tauri/src/storage/outbox.rs`, and Rust contract tests.

**Interfaces:**

```rust
pub struct DailyUsageSnapshot {
    pub device_id: String,
    pub bucket_date: String,
    pub bucket_policy_version: u16,
    pub agent: Agent,
    pub schema_version: u16,
    pub revision: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub coverage: UsageCoverage,
    pub payload_hash: String,
}
```

```rust
fn sample_snapshot() -> DailyUsageSnapshot {
    DailyUsageSnapshot {
        device_id: "device-1".into(),
        bucket_date: "2026-09-25".into(),
        bucket_policy_version: 1,
        agent: Agent::Codex,
        schema_version: 1,
        revision: 1,
        input_tokens: Some(30),
        output_tokens: Some(12),
        cache_read_tokens: Some(8),
        cache_write_tokens: Some(0),
        total_tokens: Some(42),
        coverage: UsageCoverage::Complete,
        payload_hash: "synthetic-hash".into(),
    }
}

#[test]
fn snapshot_serialization_excludes_source_identity() {
    let value = serde_json::to_value(sample_snapshot()).unwrap();
    assert!(value.get("session_id").is_none());
    assert!(value.get("source_path").is_none());
    assert!(value.get("prompt").is_none());
    assert!(value.get("transcript").is_none());
}
```

- [ ] Add serialization tests that assert this DTO has no session ID, raw path, message, prompt, or transcript field.
- [ ] Add tests for complete, partial, and unavailable snapshots; missing numeric values must serialize as absent/null with explicit coverage, never as zero.
- [ ] Add tests that the same device/day/agent/revision produces the same hash and that a changed count requires a higher revision.
- [ ] Implement the DTO and calculate its hash from stable aggregate fields in sorted field order.
- [ ] Implement a local outbox that holds the newest snapshot per device/day/agent until the server acknowledges its revision.

### Task 2: Create worlds, memberships, and access policies

**Files:** provider-native schema and access tests; Supabase paths are listed in File Structure if selected.

**Interfaces:** `World { id, owner_id, name, created_at }`; `WorldMember { world_id, user_id, role, joined_at }`; the database enforces one-to-ten members and one membership per user per world.

For the recommended PostgreSQL provider, the initial schema contract is:

```sql
CREATE TABLE worlds (
  id UUID PRIMARY KEY,
  owner_id UUID NOT NULL,
  name TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE world_members (
  world_id UUID NOT NULL REFERENCES worlds(id) ON DELETE CASCADE,
  user_id UUID NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('owner', 'member')),
  joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (world_id, user_id)
);
CREATE TABLE daily_usage_snapshots (
  world_id UUID NOT NULL REFERENCES worlds(id) ON DELETE CASCADE,
  user_id UUID NOT NULL,
  device_id UUID NOT NULL,
  bucket_date DATE NOT NULL,
  bucket_policy_version SMALLINT NOT NULL,
  agent TEXT NOT NULL CHECK (agent IN ('codex', 'claude_code')),
  schema_version SMALLINT NOT NULL,
  revision BIGINT NOT NULL,
  input_tokens BIGINT,
  output_tokens BIGINT,
  cache_read_tokens BIGINT,
  cache_write_tokens BIGINT,
  total_tokens BIGINT,
  coverage TEXT NOT NULL CHECK (coverage IN ('complete', 'partial', 'unavailable', 'unsupported', 'user_disabled')),
  payload_hash TEXT NOT NULL,
  PRIMARY KEY (world_id, user_id, device_id, bucket_date, agent),
  FOREIGN KEY (world_id, user_id) REFERENCES world_members(world_id, user_id)
);
```

The production schema also stores:

```sql
CREATE TABLE world_invites (
  id UUID PRIMARY KEY,
  world_id UUID NOT NULL REFERENCES worlds(id) ON DELETE CASCADE,
  code_hash TEXT NOT NULL UNIQUE,
  expires_at TIMESTAMPTZ NOT NULL,
  revoked_at TIMESTAMPTZ,
  used_at TIMESTAMPTZ
);
```

- [ ] Write schema tests for solo-world creation, joining a second member, rejecting an eleventh member, and preventing duplicate membership.
- [ ] Write authorization tests where a member of World A attempts to read or mutate World B; expect denial.
- [ ] Write a world-creation test that proves the owner also has a `world_members` row with role `owner`.
- [ ] Implement world and membership tables, owner/member roles, and database constraints.
- [ ] Implement access policies so members can read only their own world shell; aggregate access is limited to the world aggregate API/view, not other member rows.
- [ ] Add a membership-checked `get_world_summary(world_id)` function that returns only summed world values, milestone progress, and group-level coverage.
- [ ] Run provider-native policy tests and confirm cross-world reads and writes are rejected.

### Task 3: Implement invites and idempotent aggregate sync

**Files:** provider invite and usage schemas/functions, `apps/desktop/src-tauri/src/sync/client.rs`, and Rust/provider integration tests.

**Interfaces:** `create_invite(world_id) -> InviteLink`; `accept_invite(code) -> WorldSummary`; `upload_snapshot(world_id, snapshot) -> Ack`; the authenticated user ID comes from the session, and `Ack` contains the stored revision only.

```text
upload_snapshot(snapshot):
  authorize snapshot.user_id from the authenticated account
  verify current world membership
  if stored.revision > snapshot.revision: return stored revision without mutation
  if stored.revision == snapshot.revision and stored.payload_hash == snapshot.payload_hash: return stored revision
  if stored.revision == snapshot.revision: reject revision conflict
  otherwise replace the snapshot and return the new revision
```

- [ ] Test invite acceptance for valid, expired, revoked, already-used, and full-world cases.
- [ ] Store only a cryptographic hash of each invite code; enforce expiry, one use, revocation, and the 10-member cap in the database transaction.
- [ ] Test identical snapshot retry, older revision retry, newer replacement, and simultaneous devices with different device IDs.
- [ ] Under the recommended device-scoped policy, test that two distinct device IDs are additive and same-device retries remain idempotent; if account-scoped hashes are selected, replace this expected behavior with a copied-history dedupe test.
- [ ] Implement an upsert key of `member_id + device_id + bucket_date + agent`; reject stale revisions and treat same-revision identical payloads as acknowledged.
- [ ] Aggregate a member's enabled-agent totals across all their devices for each approved day bucket, apply the reviewed diminishing-return curve once per member-day, then sum those credits for the world.
- [ ] Return only world totals, progress, last-update time, and group-level coverage; keep per-member rows unavailable to other members.
- [ ] Test the request and response bodies contain no raw record fields, session IDs, paths, or per-member token breakdowns.

### Task 4: Connect sharing controls and world UI

**Files:** create `apps/desktop/src-tauri/src/commands/sharing.rs`, `apps/desktop/src/components/SharingSetup.tsx`, `InvitePanel.tsx`, `SyncStatus.tsx`, and `apps/desktop/src/lib/sharing.ts`; update `apps/desktop/src/components/PlanetScene.tsx` and `apps/desktop/src/App.tsx`.

**Interfaces:** `get_sharing_state()`, `enable_sharing()`, `pause_sharing()`, `create_invite()`, `join_world(code)`, `leave_world()`, and `delete_synced_usage()` are typed Tauri commands.

- [ ] Add component tests for solo state, signed-out state, shared state, sync queued, paused, and failed sync states.
- [ ] Add a test that only a world aggregate and coverage badge reach `PlanetScene`; member rows never reach the group UI.
- [ ] Implement opt-in sign-in, world creation/join, invite copy/revoke, and member count display.
- [ ] Implement a pause control that stops new uploads but retains local collection and pending snapshots until resume or explicit deletion.
- [ ] Implement leave and delete flows; verify server aggregates for that user are removed from group calculations after completion.
- [ ] Run the client tests and provider integration tests against a disposable development project.

### Task 5: Verify offline sync, privacy, and platform behavior

**Files:** update `apps/desktop/src-tauri/src/sync/client.rs`, `apps/desktop/src-tauri/src/storage/outbox.rs`, privacy documentation, and platform acceptance checklist.

- [ ] Test offline queue retention, retry with exponential backoff, acknowledgement cleanup, and app restart during a pending upload.
- [ ] Test that a changed local historical scan sends a higher revision and replaces the previous daily snapshot without double-counting.
- [ ] Verify account tokens are stored using the selected platform's secure storage, not plain frontend local storage.
- [ ] Inspect network payloads from both macOS and native Windows; confirm only approved aggregates and IDs are sent.
- [ ] Verify invite acceptance, pause, resume, leave, and delete on macOS and native Windows.
- [ ] Document that self-reported client aggregates can be altered by a modified client; MVP does not promise fraud-proof usage accounting.

### Task 6: Release acceptance

- [ ] Review every schema field and endpoint payload against the privacy constraints in the spec.
- [ ] Confirm a user can run solo without an account and can later enable sharing without uploading historical raw files.
- [ ] Confirm the group planet shows aggregate progress only and never a member leaderboard.
- [ ] Confirm missing sources produce a visible incomplete state and do not become zero in the server aggregate.
- [ ] Complete macOS and native Windows release smoke checks before publishing the MVP.
