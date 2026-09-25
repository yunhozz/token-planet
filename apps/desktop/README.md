# Token World desktop

Token World turns confirmed Codex and Claude Code token usage into a growing solo or shared planet. This directory contains the Tauri 2, React, and Rust desktop client. Solo use needs no account.

## Run locally

Use Node 22.21.0, npm 10.9.4, the Rust toolchain in `rust-toolchain.toml`, and the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
npm run tauri -- dev
```

From the repository root, prefix npm commands with `npm --prefix apps/desktop`. Build a macOS application bundle on a Mac with `npm run tauri -- build --bundles app`. A native Windows machine with the Tauri prerequisites is required to build and check the Windows tray app.

To enable the shared-world controls in a development build, set `TOKEN_WORLD_SUPABASE_URL` and `TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY` before starting Tauri. The publishable key is a public client key; never embed a Supabase secret or service-role key. Without these values, solo use remains available. The hosted project and custom SMTP are not configured yet.

## Local sources and storage

| Agent | macOS default | Native Windows default | Override |
| --- | --- | --- | --- |
| Codex | `~/.codex/sessions/**/*.jsonl` | `%USERPROFILE%\.codex\sessions\**\*.jsonl` | `CODEX_HOME` + `/sessions` |
| Claude Code | `~/.claude/projects/**/*.jsonl` | `%USERPROFILE%\.claude\projects\**\*.jsonl` | `CLAUDE_CONFIG_DIR` + `/projects` |

In the detailed view, **폴더 선택** lets you choose the `sessions` or `projects` folder directly if your installation uses another location. The native picker and selected path stay in Rust; the path is saved only in the local SQLite settings. The UI receives a usage summary, never the path. WSL collection is outside this version.

Choosing a different folder replaces that agent's prior local aggregate and checkpoints, then scans the new folder. Original agent files are never changed. A changed file is rescanned when its previously processed bytes differ; this protects against in-place rewrites at the cost of reading the processed prefix during each scan.

The local SQLite ledger lives in Tauri's OS-managed application-local-data directory as `usage-ledger.sqlite3`. It stores deduplication keys, file checkpoints, day totals, source preferences, an installation ID, pending daily aggregates, and a cached world summary. It does not store transcript text, prompts, tool output, or complete source filenames. Codex and Claude Code JSONL files are read only. A launch scan is followed by periodic scans; the circular arrow triggers a manual scan.

## Counting and coverage

- Codex parser version 1 uses each recognized `token_usage_record.payload.usage.total_tokens` once per response. Cache and reasoning fields are breakdowns and are not added again. For files without response records, cumulative `event_msg` token snapshots use a separate delta path.
- Claude Code parser version 1 recognizes final assistant records with input, output, cache-read, and cache-creation token fields. It adds those four categories once. Claude Code's local transcript schema is internal and may change. A record without complete, unambiguous usage remains unknown. Native Windows Claude Code collection is still a release validation gate.
- The app distinguishes confirmed totals from incomplete coverage. Source status reports a missing folder, denied access, unsupported format, unavailable usage, or partial coverage separately. An unfinished JSONL line waits for completion; a measured zero remains different from an unknown count. Missing usage is never silently set to zero.
- In the detailed view, **사용 안 함** excludes a source and its prior records from the selected-source total and planet growth without deleting the local ledger. **다시 포함** restores it.
- A member-day's confirmed enabled-source totals are combined before applying `log2(1 + tokens / 100,000)`. The solo planet changes at cumulative credits 5, 20, 50, and 100, with visual progress between milestones. The day boundary uses the world creator's IANA timezone fixed when the world is created.

When sharing is enabled, Rust rebuilds daily aggregates in the creator's timezone and queues changed snapshots with increasing revisions. The first upload and subsequent successful scans run about every 60 seconds. Connection failures retain the local queue and retry after 5, 10, 20, 40, then 60 seconds. The app also retains the queue across restarts. Pausing stops uploads while preserving local collection and pending aggregates. Deleting synced usage pauses sharing and excludes all records through the current creator-timezone day from future uploads, so old history is not restored on resume. Later days can be shared after resuming. Leaving clears the local shared-world queue. Signing out removes the session and cached world view but retains local revision metadata, so the same account can safely resume without duplicating already shared rows; another account starts with a separate queue. Signing out does not delete already shared server aggregates.

## Privacy and current release status

Only the Rust process scans local JSONL and opens the native folder picker. The React window invokes narrow commands for usage, source settings, world membership, invitations, and sync controls. No arbitrary file read command or filesystem plugin permission is exposed to React. When configured, Supabase Auth access and refresh tokens are stored through the operating system's credential store in Rust, not in React storage. Raw logs, prompts, local paths, and session IDs are not part of the aggregate RPC body. The server accepts self-reported aggregates from an authenticated client; a modified client can fabricate usage, so the MVP does not promise fraud-resistant totals. Copying the same agent logs to another installation can count them twice under the selected device-scoped rule.

A macOS release `.app` bundle has been built and launched through Launch Services. Its compact window showed the planet and distinct source status. Launch after copying the bundle to Applications remains unchecked. Native Windows build, tray click and keyboard behavior, and a real native Windows Claude Code transcript with usage fields remain to be checked before claiming a cross-platform release. macOS status-bar icon click and keyboard behavior also need a direct manual check; the automated UI surface could inspect the app window but not the status-bar icon.
