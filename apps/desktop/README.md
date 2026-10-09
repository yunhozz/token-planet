# Token Planet desktop

Token Planet turns confirmed Codex and Claude Code token usage into independent personal planets, with optional private group comparison. This directory contains the Tauri 2, React, and Rust desktop client. Solo use needs no account.

## Current multiplayer policy (2026-10-08)

The [approved policy](../../docs/superpowers/specs/2026-10-08-token-planet-multiplayer-policy-design.md) defines independent personal planets and private groups of up to 10 members. Group members see exact individual current planet tokens, lifetime tokens, growth credits, and ranks. They cannot edit or grow another member's planet.

Current device-bound anonymous Auth uses owner-issued invitations implemented in this branch under the [invite lifecycle design](../../docs/superpowers/specs/2026-10-08-token-planet-invite-lifecycle-design.md). Invitations expire after 7 days (168 hours), permit one successful join, and can be revoked by the owner before use. The server stores only a digest of each code. The new client replaces the reusable personal-code join path. Hosted migration and native A/B app validation remain pending. Losing the stored session loses the same member identity; account recovery is deferred. New read-only visits and joint goals are also deferred.

Group responses exclude Auth tokens, wallet balances and credit history; original logs, prompts, paths and original agent session identifiers stay local. See the [verification record](../../docs/2026-10-08-multiplayer-verification.md) for document evidence and runtime status. Existing build/test notes are historical evidence and do not prove the current two-user flow.

## Run locally

Use Node 22.21.0, npm 10.9.4, the Rust toolchain in `rust-toolchain.toml`, and the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
npm run tauri -- dev
```

From the repository root, prefix npm commands with `npm --prefix apps/desktop`. Build a macOS application bundle on a Mac with `npm run tauri -- build --bundles app`. A native Windows machine with the Tauri prerequisites is required to build and check the Windows tray app.

### Local Supabase

On each macOS machine, install and start [Docker Desktop](https://docs.docker.com/desktop/setup/install/mac-install/), then install the Node/npm and Rust versions noted above and the [Tauri macOS prerequisites](https://v2.tauri.app/start/prerequisites/#macos). Clone this repository; its `supabase/config.toml` and migrations are already tracked, so do not run `supabase init` or copy keys from another machine.

From the repository root, install desktop dependencies and start the local Supabase services used by desktop Auth:

If a DB-only `token-planet` local stack is already running, stop it first; rerunning `start` will not add Auth to that stack. This preserves its database backup and volume. Do not use `--no-backup`:

```sh
npx --yes supabase@2.118.0 stop --project-id token-planet
```

```sh
npm --prefix apps/desktop ci
npx --yes supabase@2.118.0 start --exclude realtime,storage-api,imgproxy,postgres-meta,studio,edge-runtime,logflare,vector,supavisor,mailpit
npm --prefix apps/desktop run dev:local
```

`dev:local` reads `API_URL` and `PUBLISHABLE_KEY` from the local CLI status, requires an HTTP loopback URL and a non-empty publishable key, then overrides only `TOKEN_PLANET_SUPABASE_URL` and `TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY` for `tauri dev`. This keeps a hosted URL inherited from a shell or launchd from being used by the local launch. The CLI status also contains secret keys; the launcher does not display or save its output or those keys.

Shared-world entry creates a device-bound Supabase anonymous user. The app keeps its session in the operating system credential store. The member's nickname may match another member's; the Supabase user ID distinguishes them. The world owner issues an invitation and shares its 64-character hexadecimal code; the friend pastes it with a nickname when joining. An invitation expires after 168 hours and is consumed by one successful join. The owner can revoke an unused invitation. The reusable 10-character personal-code flow is historical and is no longer the new client's join path. If this Mac loses the session, the same member identity and its shared-world access cannot be recovered.

Each Mac has its own local database and Auth users. Git carries the configuration and migrations, not this local data. The local launcher reads that Mac's generated publishable key, so no `TOKEN_PLANET_SUPABASE_*` values need to be copied between machines. Two Macs using separate local stacks cannot exchange invitations or join the same shared world; that requires both apps to use one reachable Supabase project. To stop the local stack without deleting its data, run `npx --yes supabase@2.118.0 stop --project-id token-planet` from the repository root.

### Hosted Supabase development

For hosted development, create an ignored, machine-local settings file from the tracked template on each Mac:

```sh
cp apps/desktop/.env.example apps/desktop/.env.local
```

Set `TOKEN_PLANET_SUPABASE_URL` and `TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY` in `.env.local`, then run from the repository root:

```sh
npm --prefix apps/desktop run dev:hosted
```

The launcher requires a non-empty publishable key and an HTTPS URL, then overrides only those two settings for `tauri dev`. The publishable key is a public client key; never put a Supabase secret or service-role key in this file. Local development with `dev:local` uses the local Supabase CLI and does not need `.env.local`. Running `npm run tauri -- dev` without hosted settings still allows solo use. For hosted sharing, apply the latest migrations and enable Anonymous Sign-Ins in Supabase Authentication → Sign In / Providers; no SMTP or sender domain is needed.

## Installation identity and local data

The Token Planet release uses a new application identifier and a new keychain service. An existing installation's SQLite file and keychain entry are left in place, but the new app does not read them. The new installation creates a new anonymous Supabase user. Its identity does not inherit the previous user's shared-world memberships or ownership, so old worlds may no longer be accessible from the new installation. There is no automatic account, local-data, or world transfer.

## Releases and production migrations

GitHub Actions builds macOS DMG and Windows MSI artifacts for a matching `vX.Y.Z` tag and creates a draft GitHub Release after both platform builds pass. A person reviews the draft and completes the manual checks in [`docs/release-acceptance.md`](../../docs/release-acceptance.md) before publishing it. The production Supabase migration workflow is a separate manual operation. See the [deployment guide](../../docs/deployment.md) for required Actions variables, protected environment secrets, and operating steps.

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
- The app distinguishes confirmed totals from incomplete coverage. Source status reports a missing folder, denied access, unsupported format, unavailable usage, or partial coverage separately. An unresolved source or checkpoint failure also marks known shared-world day totals incomplete, including past days. An unfinished JSONL line waits for completion; a measured zero remains different from an unknown count. Missing usage is never silently set to zero.
- In the detailed view, **사용 안 함** excludes a source and its prior records from the selected-source total and planet growth without deleting the local ledger. **다시 포함** restores it.
- A member-day's confirmed enabled-source totals are combined before applying `log2(1 + tokens / 100,000)`. The solo planet changes at cumulative credits 5, 20, 50, and 100, with visual progress between milestones. The day boundary uses the world creator's IANA timezone fixed when the world is created.

When sharing is enabled, Rust rebuilds daily aggregates in the creator's timezone and queues changed snapshots with increasing revisions. The first upload and subsequent successful scans run about every 60 seconds. Connection failures retain the local queue and retry after 5, 10, 20, 40, then 60 seconds. The app also retains the queue across restarts. Pausing stops uploads on this installation while preserving local collection and pending aggregates. Deleting synced usage removes this device identity's server aggregates, pauses sharing, and excludes all records through the current creator-timezone day from future uploads. Later days can be shared after explicitly resuming. Leaving removes membership and shared aggregates, clears this installation's queue, and retains a minimal server cutoff for that member and world so rejoining cannot restore old history from this device. There is no account-switch or sign-out flow: losing the stored anonymous session loses this member identity and its shared-world access.

Planet profiles, cycles, objects, wallets, and synced metrics are isolated by the anonymous Supabase user ID for shared use. On first sharing, the device identity adopts an unclaimed local-only planet. Usage records retain the identity that first collected them, including after source rescans. Upgrades from the older single-planet storage preserve previously shared data in a local `legacy` archive because its owner cannot be reliably inferred; the current member restores its canonical server planet instead. This prevents old data from being uploaded under a new identity, but unsynced legacy progress is retained only in that archive.

## Privacy and current release status

Only the Rust process scans local JSONL and opens the native folder picker. The React window invokes narrow commands for usage, source settings, world membership, invitations, and sync controls. No arbitrary file read command or filesystem plugin permission is exposed to React. When configured, Supabase Auth access and refresh tokens are stored through the operating system's credential store in Rust, not in React storage. Raw logs, prompts, local paths, and session IDs are not part of the aggregate RPC body. The server accepts self-reported aggregates from an authenticated client; a modified client can fabricate usage, so the MVP does not promise fraud-resistant totals. Copying the same agent logs to another installation can count them twice under the selected device-scoped rule.

A macOS release `.app` bundle has been built and launched through Launch Services. Its compact window showed the planet and distinct source status. Launch after copying the bundle to Applications remains unchecked. Native Windows build, tray click and keyboard behavior, and a real native Windows Claude Code transcript with usage fields remain to be checked before claiming a cross-platform release. macOS status-bar icon click and keyboard behavior also need a direct manual check; the automated UI surface could inspect the app window but not the status-bar icon.
