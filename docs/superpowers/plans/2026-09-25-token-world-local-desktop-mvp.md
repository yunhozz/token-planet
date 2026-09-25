# Token World Local Desktop MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the local-first Token World desktop client that reads Codex and Claude Code usage metadata, shows honest totals and source status, and grows a distinctive planet on macOS and native Windows.

**Architecture:** The React client renders a compact world window and detailed planet view. Rust owns source discovery, parsing, the SQLite ledger, token accounting, and the platform tray; only typed, content-free summaries cross the Tauri boundary. The local growth engine consumes confirmed counts and records incomplete source coverage separately.

**Tech Stack:** Tauri 2, React with recommended TypeScript, Rust, rusqlite, serde/serde_json, chrono, Vitest, and pnpm (frontend language and package manager are review choices).

**Spec:** `docs/specs/token-world-mvp.md`

## Global Constraints

- Platforms are macOS menu bar and native Windows tray; WSL support is outside MVP.
- Agents are Codex and Claude Code only.
- Raw logs, prompts, messages, tool output, and full source paths never leave the device.
- Missing or ambiguous token fields stay unknown; never turn them into zero.
- Count Codex authoritative per-response totals once; do not add cached-input or reasoning breakdowns again.
- Count Claude usage only when the local record shape is recognized and the required usage fields are unambiguous.
- All source parsers are read-only and versioned; errors never include raw log lines.
- The frontend displays an incomplete label whenever an enabled source cannot be measured.

## Review Focus

- Re-scanning a Codex cumulative snapshot must not add its existing thread total again; pin this in Task 2's snapshot test.
- A Claude transcript with no usage object must produce `usage unavailable`, not a measured zero; pin this in Task 2's parser test.
- Replaying the same source record after restart must not change totals; pin this in Task 3's SQLite idempotency test.
- A partial final JSONL line must remain pending until completed and must not be treated as corrupt usage; pin this in Task 3's scanner test.
- The tray must open the same compact world window on macOS and Windows while exposing platform-specific accessibility labels; pin this in Task 5's platform smoke checklist.

## File Structure

- `apps/desktop/src-tauri/src/domain/usage.rs`: source and coverage types, canonical token categories, and known-subtotal rules.
- `apps/desktop/src-tauri/src/collectors/codex.rs`: Codex JSONL discovery, response records, and cumulative-only fallback.
- `apps/desktop/src-tauri/src/collectors/claude_code.rs`: Claude Code JSONL discovery and explicitly supported usage shapes.
- `apps/desktop/src-tauri/src/storage/ledger.rs`: SQLite schema, file checkpoints, record IDs, and daily aggregates.
- `apps/desktop/src-tauri/src/growth.rs`: contribution-credit calculation and deterministic world stage mapping.
- `apps/desktop/src-tauri/src/platform/tray.rs`: shared tray menu/window toggle; small OS-specific modules configure macOS and Windows behavior.
- `apps/desktop/src-tauri/src/platform/macos.rs` and `windows.rs`: app activation, menu-bar/tray window behavior, and platform smoke-test hooks.
- `apps/desktop/src-tauri/src/lib.rs`: Tauri setup and typed commands.
- `apps/desktop/src/types/usage.ts`: frontend mirror of usage and coverage types.
- `apps/desktop/src/components/SourceStatus.tsx`: source discovery and completeness display.
- `apps/desktop/src/components/UsageSummary.tsx`: user's known/complete local totals.
- `apps/desktop/src/components/PlanetScene.tsx`: deterministic SVG planet layers and milestone view.
- `apps/desktop/src/App.tsx`: compact view and expanded world navigation.
- `apps/desktop/vitest.config.ts` and `src/test-setup.ts`: frontend component test configuration.
- `apps/desktop/package.json`: app scripts and frontend dependencies.
- Rust tests live beside their modules; frontend component tests live under `apps/desktop/src/components/__tests__/`.

## Tasks

### Task 0: Align with the remote repository and create the desktop shell

**Files:** create the empty `apps/desktop/` directory and initialize it with the Tauri template; preserve `docs/` at the repository root.

**Interfaces:** produces a runnable Tauri 2 + React client for later tasks.

- [ ] Check remote refs and the default branch before creating the first commit; base work on the remote default branch if one exists and preserve all remote commits.
- [ ] Create the empty `apps/desktop/` directory, run `pnpm create tauri-app@latest` from it, and select React, TypeScript, and pnpm if those recommendations are approved.
- [ ] Run the generated client with `pnpm --dir apps/desktop tauri dev`; confirm the starter window opens on macOS.
- [ ] Add Vitest, React Testing Library, `user-event`, and `jsdom` to the desktop package; configure the `test` script and `vitest.config.ts`.
- [ ] Record the exact Rust, Node, pnpm, and Tauri CLI versions in the project toolchain files so both platforms build from the same versions.

### Task 1: Define the canonical local usage contract

**Files:** create `apps/desktop/src-tauri/src/domain/usage.rs`, `apps/desktop/src/types/usage.ts`, and Rust unit tests in `usage.rs`.

**Interfaces:**

```rust
pub enum Agent {
    Codex,
    ClaudeCode,
}

pub enum UsageCoverage {
    Complete,
    Partial,
    Unavailable,
    Unsupported,
    UserDisabled,
}

pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub coverage: UsageCoverage,
}

pub fn known_subtotal(usages: &[TokenUsage]) -> Option<u64> {
    let mut known = usages.iter().filter_map(|usage| usage.total_tokens);
    let first = known.next()?;
    Some(known.fold(first, u64::saturating_add))
}
```

```rust
#[test]
fn missing_usage_is_not_zero() {
    let usage = TokenUsage {
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        total_tokens: None,
        coverage: UsageCoverage::Unavailable,
    };
    assert_eq!(known_subtotal(&[usage]), None);
}
```

- [ ] Write Rust tests for complete, partial, unknown, and disabled source states, including a case where a missing field is not equal to `Some(0)`.
- [ ] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml domain::usage` and confirm the new tests fail before implementation.
- [ ] Implement the canonical types and `known_subtotal()` so it returns `None` when no source total is known and `Some(0)` for a measured zero; never fill missing values with zero.
- [ ] Add matching TypeScript discriminated unions and type-check with `pnpm --dir apps/desktop build`.

### Task 2: Implement versioned Codex and Claude Code adapters

**Files:** create `apps/desktop/src-tauri/src/collectors/mod.rs`, `codex.rs`, `claude_code.rs`, and inline adapter fixtures/tests.

**Interfaces:** each adapter exposes `parse_line(line: &str) -> Result<Option<ParsedRecord>, ParseError>`; `None` means the valid JSONL row is unrelated to usage, and `Some` contains only event identity, timestamp, usage values, and coverage. It has no conversation-text field. A recognized row with an unrecognized usage shape returns a record with `Unsupported` coverage.

```rust
pub enum ParseError {
    InvalidJson,
    InvalidIdentity,
}

pub struct ParsedRecord {
    pub agent: Agent,
    pub event_key: String,
    pub occurred_at_utc: DateTime<Utc>,
    pub usage: TokenUsage,
}
```

The Codex fixture uses only synthetic values:

```json
{"type":"token_usage_record","payload":{"response_id":"r1","usage":{"input_tokens":30,"cached_input_tokens":8,"output_tokens":12,"reasoning_output_tokens":4,"total_tokens":42}}}
```

Expected normalized total: `42`, counted once for response `r1`. `TokenUsage.total_tokens` is the adapter's normalized eligible total; category fields remain breakdowns and `known_subtotal()` sums normalized totals only.

- [ ] Add synthetic Codex fixtures with `token_usage_record.payload.usage.total_tokens`, breakdown fields, duplicate response IDs, and cumulative-only token-count events.
- [ ] Add synthetic Claude fixtures for a complete recognized usage record, missing `usage`, and an unknown future field layout.
- [ ] Write parser tests proving Codex uses `usage.total_tokens` once, ignores cached/reasoning breakdowns as add-ons, and returns unavailable for a Claude record without recognized usage.
- [ ] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml collectors` and confirm the parser tests fail before implementation.
- [ ] Implement only the versioned field mappings from the fixtures; skip duplicate Codex response IDs and keep cumulative-only records on a separate delta path.
- [ ] Re-run the parser tests and inspect logs to confirm parse errors report file status and line position only, never raw JSON.
- [ ] Before enabling Claude usage claims for a Windows release, compare the parser with a real native Windows Claude Code transcript locally; add only key names and synthetic values to the fixture, never transcript text.

### Task 3: Add source discovery and the SQLite deduplication ledger

**Files:** create `apps/desktop/src-tauri/src/storage/ledger.rs`, `apps/desktop/src-tauri/src/collectors/discovery.rs`, and module tests.

**Interfaces:** `scan_sources(config, ledger) -> Result<ScanSummary, ScanError>`; `ScanSummary` includes per-agent status, confirmed aggregate values, and last scan time, but no source paths or record text.

```rust
pub struct SourceConfig {
    pub codex_root: Option<PathBuf>,
    pub claude_root: Option<PathBuf>,
}

pub struct Ledger {
    connection: rusqlite::Connection,
}

impl Ledger {
    pub fn insert(&mut self, record: &ParsedRecord) -> Result<(), ScanError>;
    pub fn daily_total(
        &self,
        agent: Agent,
        bucket_date: &str,
    ) -> Result<Option<u64>, ScanError>;
}

pub enum ScanError {
    Database,
    SourceIo,
}

pub fn scan_sources(
    config: &SourceConfig,
    ledger: &mut Ledger,
) -> Result<ScanSummary, ScanError>;

pub struct ScanSummary {
    pub codex: TokenUsage,
    pub claude_code: TokenUsage,
    pub confirmed_subtotal: Option<u64>,
    pub complete_total: Option<u64>,
    pub scanned_at_utc: DateTime<Utc>,
}
```

The ledger uses a local-only event key derived from source session and response/message identity; it never syncs that key. Its durable schema is:

```sql
CREATE TABLE source_checkpoint (
  source_id TEXT PRIMARY KEY,
  file_fingerprint TEXT NOT NULL,
  byte_offset INTEGER NOT NULL,
  parser_version INTEGER NOT NULL
);
CREATE TABLE daily_agent_total (
  agent TEXT NOT NULL,
  bucket_date TEXT NOT NULL,
  total_tokens INTEGER,
  coverage TEXT NOT NULL,
  PRIMARY KEY (agent, bucket_date)
);
CREATE TABLE usage_record (
  event_key TEXT PRIMARY KEY,
  agent TEXT NOT NULL,
  bucket_date TEXT NOT NULL,
  occurred_at_utc TEXT NOT NULL,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_read_tokens INTEGER,
  cache_write_tokens INTEGER,
  total_tokens INTEGER,
  coverage TEXT NOT NULL,
  parser_version INTEGER NOT NULL
);
```

```rust
let record = codex::parse_line(
    r#"{"type":"token_usage_record","payload":{"response_id":"r1","usage":{"input_tokens":30,"cached_input_tokens":8,"output_tokens":12,"reasoning_output_tokens":4,"total_tokens":42}}}"#
)?
    .expect("the fixture contains a usage event");
ledger.insert(&record)?;
ledger.insert(&record)?;
assert_eq!(ledger.daily_total(Agent::Codex, "2026-09-25")?, Some(42));
```

- [ ] Test default roots and override rules for `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `%USERPROFILE%` defaults, and user-selected custom folders using temporary directories.
- [ ] Define a SQLite schema for source checkpoints, deduplicated record IDs, daily per-agent totals, and coverage state; do not store full filenames or transcript content.
- [ ] Test a record inserted twice and a restarted scanner; both must yield the same daily total as one insertion.
- [ ] Test an incomplete final JSONL line; retain its starting byte offset and parse it only after the next scan sees a line terminator.
- [ ] Implement one transaction per scanned batch and advance a checkpoint only after all parsed records in that batch are committed.
- [ ] Trigger a complete scan on launch, an incremental scan every 60 seconds while the app runs, and an incremental scan from the manual refresh action.
- [ ] Re-run the storage tests and confirm a read-only scan never changes source files.

### Task 4: Build local totals and planet growth

**Files:** create `apps/desktop/src-tauri/src/growth.rs`, `apps/desktop/src/components/UsageSummary.tsx`, `SourceStatus.tsx`, and `PlanetScene.tsx`; update `apps/desktop/src/App.tsx` and `apps/desktop/src/types/usage.ts`.

**Interfaces:** Rust returns the typed `WorldSnapshot` below; React renders the snapshot without reading files or computing token totals.

```rust
pub struct WorldSnapshot {
    pub usage: ScanSummary,
    pub growth_credit: f64,
    pub stage: u8,
    pub progress_to_next: f64,
}
```

```rust
pub fn contribution_credit(known_tokens: u64, k: f64) -> f64 {
    (1.0 + known_tokens as f64 / k).log2()
}

#[test]
fn high_daily_usage_has_diminishing_marginal_credit() {
    assert!(contribution_credit(1_000_000, 100_000)
        < 2.0 * contribution_credit(500_000, 100_000));
}
```

- [ ] Agree `K`, day boundary, milestone pace, and stage thresholds from example curves before coding this task; use the reviewed settings as named configuration values.
- [ ] Write growth tests for zero known tokens, a positive known subtotal, missing-source coverage, and a high-contributor diminishing-return comparison.
- [ ] Run the focused growth tests and confirm the curve tests fail before implementation.
- [ ] Implement `credit = log2(1 + known_enabled_tokens / K)` per member-day, sum the known credits, and preserve incomplete coverage as a separate field.
- [ ] Implement deterministic SVG layers for proto-planet, crust/land plates, oceans/clouds, and moon/orbit; stage and progress are driven only by `WorldSnapshot`.
- [ ] Add React tests that verify missing Codex or Claude data displays an incomplete status instead of `0` and that world totals contain no member breakdown.
- [ ] Run `pnpm --dir apps/desktop exec vitest run` and `pnpm --dir apps/desktop build`.

### Task 5: Add macOS menu-bar and Windows tray behavior

**Files:** create `apps/desktop/src-tauri/src/platform/tray.rs`, `macos.rs`, and `windows.rs`; update `apps/desktop/src-tauri/src/lib.rs`, `tauri.conf.json`, and `capabilities/default.json`.

**Interfaces:** the tray menu has `Open Token World`, source status, sync status (local-only in this plan), and `Quit`; a tray click toggles the compact window.

```rust
let tray = tauri::tray::TrayIconBuilder::with_id("token-world")
    .menu(&menu)
    .on_menu_event(handle_menu_event)
    .build(app)?;
```

- [ ] Write a platform-independent test for menu action IDs and compact-window toggle state.
- [ ] Implement Tauri 2 tray creation in Rust and show the React compact window when the icon is clicked.
- [ ] Configure macOS as a menu-bar app and Windows as a native notification-area tray app; keep the detailed planet view reachable from the compact window.
- [ ] Verify the macOS menu-bar click, keyboard navigation, screen-reader labels, reopen-after-hide, and Quit action on a Mac.
- [ ] Verify Windows tray click, keyboard navigation, screen-reader labels, reopen-after-hide, and Quit action on native Windows; do not claim Windows support based only on macOS compilation.

### Task 6: Package and document local-first behavior

**Files:** update `apps/desktop/README.md` and platform packaging configuration; add no additional collection sources.

- [ ] Document local data roots, custom folder selection, supported parser versions, and unknown/incomplete statuses.
- [ ] Build the macOS bundle with `pnpm --dir apps/desktop tauri build` and verify it launches from Applications without a shell-provided environment.
- [ ] Build the Windows bundle on native Windows with the same command and verify the app starts without WSL.
- [ ] Inspect the Tauri command surface and capabilities; confirm React cannot read arbitrary files and that no log text, source path, or session identifier appears in app logs.
- [ ] Review all acceptance checks against the spec and stop with a release note if Claude Code usage is still unverified on native Windows.
