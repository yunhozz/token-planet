# Token World MVP Product and Technical Specification

**Status:** Approved product direction; the decision gates below remain for plan review before implementation.

## Product goal

Token World is a private, cooperative desktop app where Codex and Claude Code usage helps a shared planet develop. The first world can belong to one person; its owner can invite a private group of up to 10 people.

## Confirmed scope and constraints

- Product name: Token World.
- Stack: Tauri 2, React, and Rust.
- Platforms: macOS menu bar app and native Windows system tray app. WSL collection is out of MVP.
- Sources: Codex and Claude Code only.
- Core metric: the combined number of eligible tokens known from the enabled sources.
- Raw logs, message text, prompts, tool output, and source file paths stay on each device.
- The shared service receives only per-member, per-device, per-day, per-agent aggregate counts and sync metadata. The service can read those aggregates; other members see the world total, not member-level totals or a leaderboard.
- A missing, unreadable, or unrecognized source is shown as unknown or incomplete. It is never silently converted to zero.
- The desktop app parses records locally and stores its deduplication ledger locally.
- Store the local ledger and pending sync outbox under Tauri's OS-managed application-local-data directory; keep it separate from the original agent logs.

## Local data sources and evidence

The Codex default session directory is `~/.codex/sessions` on macOS and `%USERPROFILE%/.codex/sessions` on Windows; `CODEX_HOME` can relocate it. The default files are JSONL. A metadata-only sample on the current Mac contained `token_usage_record` entries with `payload.usage`, `turn_token_usage`, and `thread_token_usage`; the usage record included `total_tokens`, with cached-input, cache-write, and reasoning fields as breakdowns. The collector should count a per-response `usage.total_tokens` once when present, and must not add cached or reasoning breakdowns to it. Cumulative turn or thread snapshots are fallback inputs only when per-response records are absent, and require a separate snapshot-delta adapter.

Claude Code CLI sessions are documented at `~/.claude/projects/<project>/<session-id>.jsonl` by default; `%USERPROFILE%/.claude/projects` is the native Windows equivalent, and `CLAUDE_CONFIG_DIR` relocates the root. The session JSONL schema is internal and can change between releases. The default Claude Code directory was absent on the current Mac, so no local Claude transcript structure could be confirmed here. Claude Code's documented OpenTelemetry `api_request` data includes input, output, cache-read, and cache-creation token fields, but requires telemetry configuration. MVP collection must therefore recognize supported local usage records when they exist, display unknown/incomplete when they do not, and avoid enabling telemetry or uploading transcripts automatically.

The Tauri app should check environment overrides, default paths, and a user-selected custom path. It should read only usage metadata needed by the parser and must not persist transcript text or full record bodies.

| Collection approach | Benefits | Costs | Recommendation |
|---|---|---|---|
| Poll supported local records at startup and every 60 seconds | Simple across both operating systems; local checkpoints keep repeat scans small | Up to one minute of delay | Use for MVP, with a manual refresh action |
| Watch files for changes | Near-immediate updates | Platform event behavior and missed notifications require a fallback scan | Consider later if polling feels stale |
| Configure Claude Code OpenTelemetry export | Documented request usage fields | Requires user configuration and careful control of telemetry output | Do not enable automatically in MVP |

For Claude Code, prefer a recognized local usage record when present. If its internal session file has no unambiguous usage values, show unavailable rather than asking to enable telemetry. A future opt-in telemetry adapter can be evaluated separately.

## User experience

1. On first launch, detect each source separately and show `ready`, `not found`, `permission denied`, `unsupported format`, or `usage unavailable`.
2. Let the user mark an absent source as not used, select a custom source folder, or leave it unresolved. Only explicitly disabled sources are excluded from the user's selected-source metric.
3. Show a confirmed known subtotal with an incomplete label when an enabled source cannot be read. Show a complete total only when all enabled sources have recognized usage data.
4. Show the user's own precise totals on their device. Show the group planet's aggregate progress and source-coverage status to members; do not expose member token totals or rankings.
5. A macOS menu-bar icon and Windows tray icon open a compact world window. The compact view shows world stage/progress, the user's local known total and source status, sync status, and an `Open world` action for the detailed planet view. Scan at startup, then incrementally every 60 seconds while the app is running; include a manual refresh action.

## Growth model proposal

Use a distinct planetary identity rather than plant growth: a dark proto-planet gains a mineral crust, land plates and oceans, a cloud/atmosphere layer, then a small moon or orbital ring. Render deterministic vector layers so a given world state always looks the same.

The recommended fairness rule is diminishing per-member, per-day returns:

```text
member_day_credit = log2(1 + known_enabled_tokens / K)
world_credit = sum(member_day_credit across members and day buckets)
```

Combine a member's enabled-source and device totals for that day bucket before applying the curve, so using multiple devices does not grant multiple diminishing-return allowances. The known portion contributes while missing sources remain explicitly marked incomplete. `K` is fixed at 100,000 confirmed tokens per member-day by user choice. The day bucket uses the world creator's IANA timezone fixed when the world is created; later device or member timezone changes do not rebucket history. The approved cumulative stage thresholds are 5, 20, 50, and 100 credits. At 100,000 confirmed tokens per member-day, one member earns 1 credit daily; a 10-member group at that rate earns 10 credits daily. Visual progress changes continuously between stage thresholds. No individual ranking or exact member totals are shown to the group.

For a normalized curve, `T/K = 0, 1, 3, 7, 15` yields `0, 1, 2, 3, 4` credits. With `K = 100,000`, those points represent 0, 100,000, 300,000, 700,000, and 1,500,000 confirmed tokens in one member-day.

| Choice | Benefits | Costs | Recommendation |
|---|---|---|---|
| Linear token growth | Easy to explain and verify | High-volume users dominate the shared planet | Do not use for MVP |
| Per-member, per-day diminishing returns | Each person can contribute; marginal influence falls as one person's daily usage grows | Requires a fixed day boundary and milestone pace | Use the logarithmic curve above |
| Equal credit for any active member-day | Strongest participation parity | Planet growth no longer reflects token volume well | Consider only if testing shows the logarithmic model still feels unfair |

## Collaboration and synchronization proposal

- A person starts with a solo world. Sharing requires signing in and creating or joining a private world.
- A shared world has 1–10 members. There is no public directory or friend discovery in MVP.
- Recommended invitation: a hard-to-guess, revocable, single-use link/code with a defined expiry. The expiry period remains a review choice.
- Each installation has a stable device ID. The client sends absolute daily snapshots keyed by member, device, the approved day bucket, and agent, with a monotonic revision and idempotency hash. The server replaces the same snapshot on retry rather than adding it twice.
- Recommended MVP dedupe is device-scoped: it handles scanner replays, app restarts, and network retries without sending event identifiers. If a user manually copies the same historical source logs to another device, those device snapshots can count the same usage twice. Account-scoped pseudonymous event hashes could prevent that but would add persistent per-event identifiers to server data; that choice remains for review.
- The server stores world membership and aggregates, but no session IDs, prompts, conversation text, filesystem paths, or raw logs.
- Disabling sharing stops future uploads. MVP should include a way to delete the user's synced aggregate records and leave or dissolve a world.

| Choice | Benefits | Costs | Recommendation |
|---|---|---|---|
| Email-based invitation | Easy to revoke for a known person | Requires collecting and storing invitee email before they join | Defer for MVP |
| Private single-use link/code | Works for friends and a solo-first flow without contact discovery | The owner must share the code safely; expiry policy must be chosen | Use with a one-use limit and 10-member cap |
| Public group directory | Easy to discover worlds | Adds moderation and privacy work | Defer |

## Token accounting and data status

- Codex: use the source's authoritative `total_tokens` field once per response record. Cached input and reasoning are reported as breakdowns and are not added again. Use a documented, versioned fallback for cumulative-only files.
- Claude Code: count only a recognized usage record with a complete, unambiguous input/output/cache mapping. The current official monitoring fields are input, output, cache read, and cache creation tokens; when using those fields, sum each category once. Do not assume an absent field means zero.
- Normalize each recognized source record to one `total_tokens` value in its adapter. `known_subtotal` sums those normalized values; it never adds category breakdowns to a source-provided total.
- Store each numeric category as nullable and store an explicit coverage state. Record dedupe IDs and parser version stay local. The server receives only aggregates and coverage needed to distinguish a known subtotal from a complete total.
- The UI must distinguish a measured zero from unknown, unsupported, unreadable, and user-disabled.

## MVP boundary

**MVP:** source discovery and status; local parsing and dedupe; local SQLite ledger; local totals; solo world; deterministic planet stages; macOS menu-bar and native Windows tray entrypoints; account and private 1–10 member worlds; invite/join; aggregate-only sync; opt-out and deletion; offline queue and sync status.

**Later:** public worlds, global leaderboards, achievements, chat, generated planet artwork, mobile clients, WSL collection, additional agents, social discovery, and automated telemetry configuration.

## Decision gates before implementation

1. Use TypeScript for the React frontend and npm as the user-selected package manager; commit `package-lock.json` for reproducible installs.
2. Select the authentication/backend provider. Options and tradeoffs are recorded in the shared-world plan; recommendation: managed PostgreSQL with row-level access policies.
3. Use `K = 100,000`, a world-creator IANA timezone fixed at world creation, and cumulative stage thresholds of 5, 20, 50, and 100 credits, as selected by the user. Use the per-member, per-day logarithmic curve above, with no hard contribution cap.
4. Confirm invitation expiry and whether the owner may transfer ownership or must dissolve the world when leaving.
5. Confirm device-scoped dedupe for MVP or choose account-scoped pseudonymous event hashes to deduplicate copied history across devices. Recommendation: device-scoped dedupe, with the copied-history limitation disclosed.
6. Confirm whether each account may have one shared world or multiple shared worlds. Recommendation: one shared world per account in MVP, while keeping a solo world local until the user creates or joins a shared world.
7. Before claiming Claude Code collection works for a given release, validate the adapter against a real native Windows Claude Code transcript that contains usage fields. Until then, missing or ambiguous usage remains explicitly unavailable.

## Primary references

- [Tauri create a project](https://v2.tauri.app/start/create-project/)
- [Tauri tray API](https://v2.tauri.app/reference/javascript/api/namespacetray/)
- [Claude Code session files](https://code.claude.com/docs/en/sessions)
- [Claude Code directory and configuration](https://code.claude.com/docs/en/claude-directory)
- [Claude Code usage monitoring fields](https://code.claude.com/docs/en/monitoring-usage)
- [Codex session listing implementation](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/list.rs)
- [Codex token usage rendering](https://github.com/openai/codex/blob/main/codex-rs/tui/src/token_usage.rs)
