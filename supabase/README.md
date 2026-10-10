# Token Planet shared-world database

## Current multiplayer policy (2026-10-08)

The [approved policy](../docs/superpowers/specs/2026-10-08-token-planet-multiplayer-policy-design.md) keeps each member's planet independent. Private groups have at most 10 members and expose exact individual current/lifetime tokens, growth credits and ranks through membership-checked group responses. Group membership grants no permission to edit another planet.

Current authentication is device-bound anonymous Auth. This branch implements the [invite lifecycle](../docs/superpowers/specs/2026-10-08-token-planet-invite-lifecycle-design.md): owner-issued invitations expire after 7 days (168 hours), permit one successful join, and support owner revocation before use. Codes are stored only as digests. The forward migration `20261008090000_invite_lifecycle.sql` replaces personal-code joining; the new client uses the invitation APIs. On 2026-10-09, this migration was applied to the user-designated hosted target `mafckfpptzcqxborwkkw` and its RPC/table grants were checked read-only (E5 in the verification record). Independent-session concurrency and native A/B app validation remain pending. Same-member recovery after session loss, new read-only visits, and joint goals remain deferred.

Original logs, prompts, paths and original agent session identifiers stay local. Auth access/refresh tokens, wallet balance and credit history are excluded from group responses. The [verification record](../docs/2026-10-08-multiplayer-verification.md) separates P01–P05 document evidence from V01–V13 runtime evidence; earlier SQL/test checkpoints do not prove current two-user behavior or the 10-member capacity boundary in practice.

The earlier migration chain and 121 pgTAP assertions were verified locally with Supabase CLI 2.118.0 and Docker (PostgreSQL 17). Exact-SHA remote CI has since passed the full migration replay and SQL suites in a disposable GitHub-hosted database; see E3 in the verification record. The CI SQL suite exercised invite RPC grant/role boundaries, including issuance privileges, denial of legacy personal-code joining and anonymous RPC access, and direct-table/non-owner denial. The designated hosted target’s invite RPC/table grants were subsequently verified read-only on 2026-10-09 (E5); that evidence is limited to that target. Independent-session concurrency and native A/B app validation remain release gates.

### Local service selection

DB-only SQL/CI checks do not prove Auth or Realtime behavior. The CI replay runner below uses its own disposable project; group-chat validation uses only the already-running approved scratch instance described in [Group chat local verification](#group-chat-local-verification).

For a separately approved, fresh default repository-local `token-planet` project, run from the repository root with Auth, Kong, PostgREST, and Realtime enabled:

```sh
npx --yes supabase@2.118.0 start --workdir . --exclude storage-api,imgproxy,postgres-meta,studio,edge-runtime,logflare,vector,supavisor,mailpit
```

An already-running DB-only project will not gain services by rerunning `start`. Do not implicitly stop/reset another project or reuse occupied ports. The chat QA target below is already running and must not be restarted or reset by the verification harness.

The desktop creates a Supabase anonymous user when someone first starts sharing. Its session is stored in the operating system credential store, and its user ID identifies the member; nicknames may repeat. Losing that session loses access to the same member identity. Each Mac is a separate member, even when both use the same hosted project. Email, password, OTP, SMTP, and Mailpit are not part of this flow.

The world and membership tables enforce one shared world per account and at most 10 members. Authenticated members can read their world shell and their own usage snapshot rows. The `get_world_planets(uuid)` API replaces the retired `get_world_summary(uuid)` and returns member profiles, derived planet state, token totals, growth, coverage, and ranks after checking membership. `get_my_planet_state()` and `upsert_my_planet_state(jsonb,jsonb)` isolate private planet and wallet state by `auth.uid()`. Raw logs, paths, prompts, session IDs, and wallet credits are excluded from group responses. Tests cover account isolation, device retries and addition, canonical reset timestamp round trips, wallet deduplication, cooldown, coverage, and access control.

The database requires a recognized world timezone and keeps it fixed. A world owner's account cannot be deleted while the world still points to it; ownership must be transferred or the world dissolved first. Earlier migrations introduced invitations and then reusable personal codes. The current branch's forward invite lifecycle migration disables personal-code joining and implements owner-issued, expiring, single-use, revocable invitations while retaining the world capacity limit. Other migrations implement idempotent per-device uploads, ownership transfer, leave, and deletion of a member's synced aggregates.

For hosted use, apply the latest migrations and enable **Anonymous Sign-Ins** in the project's Authentication → Sign In / Providers settings. The local equivalent is `auth.enable_anonymous_sign_ins` in `supabase/config.toml`. A publishable key remains in the desktop configuration; never put a secret or service-role key there. The hosted setting and an actual cross-device invitation must be checked before calling hosted sharing ready. See the [Supabase anonymous sign-ins guide](https://supabase.com/docs/guides/auth/auth-anonymous).

## CI migration replay

`.github/workflows/supabase-migrations.yml` stages the original SQL into a new temporary Supabase project, replays the complete migration chain, checks the generated migration history against its manifest, and runs every top-level SQL suite under `supabase/tests`.

The staging manifest uses `history_mode: synthetic_ci_only`. It records each source filename, original version string, synthetic version, staged filename, and SQL SHA-256. SQL bytes are copied unchanged; the repository's migration filenames, SQL, configuration, and existing database history are not rewritten. Synthetic history is solely for the disposable CI database and must not be applied to deployments or used to update existing databases. The repository's ordinary `supabase db reset` still reads the original filenames, so this CI-only path does not change the filename-order behavior of a normal local reset.

### Run locally

Use an already-installed Supabase CLI **2.119.0** by setting `SUPABASE_BIN` to its absolute executable path. The runner creates its own temporary project ID and work directory. It refuses Supabase project/credential overrides, database URLs, non-local Docker contexts, and occupied local ports `56432` or `56430`.

The local Docker daemon must be available through a Unix socket, and the pinned Postgres image `public.ecr.aws/supabase/postgres:17.6.1.171` must already be cached. The local runner does not install the CLI or pull images. The GitHub-hosted workflow checks the Docker endpoint first, then pulls that exact image before starting the runner.

```sh
SUPABASE_BIN=/opt/homebrew/bin/supabase \
SUPABASE_TELEMETRY_DISABLED=1 \
bash supabase/ci/run.sh --artifacts-dir /tmp/token-planet-migration-results
```

The artifact directory must be an empty absolute path outside the repository. The runner keeps the manifest and sanitized summaries there; raw Docker inspect responses, SQL output, and CLI output stay in the temporary work directory and are removed during cleanup.


## Shop revamp transition policy (implementation in progress)

Shop revamp migrations are reviewed in a local worktree. Hosted application, deployment, and publishing are outside this implementation authorization.

- Canonical contribution upload must not mint wallet credits from `p_state.wallet_credits`. The legacy call receives only the authenticated account's already-stored server rows. New credits belong to validated reset/import transactions. Existing `private.planet_wallet_credits` values and timestamps are preserved byte-for-byte; these are **legacy carry-forward rows with unproven provenance** because the earlier writer accepted client amounts. This transition does not retrospectively establish provenance, recompute them, or authorize a wallet reset.
- Natural-object transition is per account/cycle at the first valid revamp upload. It ignores legacy object arrays and client growth fields, generates from server raw/canonical daily contributions, and records a private generation marker in the same transaction. Failed uploads roll back raw state, contributions, projection, and marker. This is not a blanket world-object reset. Later server-generated identities/seeds remain stable across retries, devices, and lower corrected growth; unverified legacy historical maximum counts are not guaranteed.
- Effect timelines are personal authenticated data. Their revisions, UTC intervals, reward timezone, and wallet/reward history must not be added to `WorldPlanet` or group responses.

Validation uses a separate disposable project at `/private/tmp/token-planet-shop-revamp-test`: project ID `token-planet-shop-revamp-test`, DB port `55432`, container and volume `supabase_db_token-planet-shop-revamp-test`. Reset/migration/test commands must explicitly select this workdir and `--local`. Do not select, stop, reset, or clear the original `token-planet` container/volume as a fallback. The plan document records focused results; full reset/reward/import/UI integration is still pending.

- Natural removal migration 00207 adds the required charged `price` to the table introduced by 00206. The ordered revamp rollout assumes that this newly introduced table has no removal rows before 00207: no earlier migration/RPC writes to it. Do not apply this step to an environment with manually populated rows without a separately reviewed reconciliation policy; no historical price is inferred or reset. Local fixtures explicitly seed their test price. Canonical available balance includes server base wallet credits and the separate game reward ledger, less landscape/avatar purchases and natural removal charges.
- Private timeline synchronization uses server-confirmed cycle bounds separately from positive effect intervals. It must not infer missing historical bounds from device timestamps, wallet credits, or growth-journal estimates. Uncovered historical events remain in raw lifetime totals and do not create effect segments or activity reward claims. Known bounds and effect history are private account data, independent of shared-world publication.

### Verified local checkpoint (2026-10-02)

The authenticated effect worker now connects the personal timeline, validated local contribution rebuild, and the exact nine-field canonical aggregate payload. User consent permits echoing only the same project's `get_my_planet_state` response, with wallet balance/credits replaced by 0/[]; no local profile, objects, or wallet claims are uploaded. A missing server state holds synchronization rather than cold-bootstrapping it. A pending guest shop import blocks account transitions before ownership/snapshot changes, while scan-only restoration continues local collection for that explicit hold.

Local or server sharing pause holds the contribution upsert and public snapshot uploads, since the existing upsert also updates the public planet projection. Personal timeline reads and local rebuilds may continue. The typed held result is not a completed reset preupload. Independent local QA passed account-switch4/worker14/full Rust202 with the three known sandbox socket tests excluded; the reviewer cleared the first-login, sharing-pause, and startup-restore findings. Signed reset command/UI and durable reset-intent recovery remain unconnected. Complete guest import, revamped public sharing, and actual native/browser validation remain pending. No hosted migration, live user aggregate upload, live removal transaction, or deployment was performed.

## Group chat local verification

Approved target: `/tmp/token-planet-group-chat-supabase`, project `token-planet-group-chat-qa`, API `http://127.0.0.1:54321`, DB port `54322`, Supabase CLI **2.118.0**. All commands below run from the repository root. The scratch config links this checkout's migrations/tests; it is distinct from the default repository project's `token-planet` identity. Never link this harness to hosted/production, restart/reset the scratch stack, or remove Docker data. Current repository and scratch configs enable Realtime.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s supabase/tests -p test_group_chat_realtime.py
PYTHONDONTWRITEBYTECODE=1 python3 supabase/tests/test_group_chat_target_guard.py
npx --yes supabase@2.118.0 test db --local --workdir /tmp/token-planet-group-chat-supabase supabase/tests/group_chat_access.sql supabase/tests/group_chat_membership.sql supabase/tests/group_chat_rpc.sql
bash supabase/tests/group_chat_concurrency.sh
PYTHONDONTWRITEBYTECODE=1 python3 supabase/tests/group_chat_realtime.py --run
```

The pure harness tests perform no network or DB changes. The live harness requires explicit `--run`, accepts no target overrides, rejects connection/proxy environment overrides, verifies Docker Unix context plus immutable local DB container identity/project/workdir/ports and local API identity before fixture writes, and does not follow HTTP redirects. CLI status (including credentials) is captured only in memory; credentials never appear in argv or reports. HTTP/WebSocket operations have five-second deadlines, child commands have 20-second timeouts, sends have a five-minute scenario deadline, and WebSocket heartbeat is sent every 15 seconds. Its only output is an allowlisted aggregate of fixed check IDs/statuses and numeric latency metrics; arbitrary exception text, tokens, IDs, profiles, and bodies are withheld.

The live harness creates four independent anonymous users A/B/C/D in this same local instance, two worlds, and synthetic messages; it cleans only these recorded fixtures. Repeated runs may reach local Auth anonymous-signup limits. Do not reset the stack or switch projects to bypass that condition. It verifies REST/RPC privileges separately from SELECT RLS and Postgres Changes delivery, including new membership, departure/rejoin, private author/read/request mappings, server profile fallback/paused sharing, idempotency, and cascade. Publication includes the needed public message table; private chat tables are not published. Tombstones use **UPDATE**, `REPLICA IDENTITY DEFAULT`, and an old payload without the previous body. Hard DELETE events are not an authorized chat delivery path because Supabase does not filter them using RLS. No user objects are added to the locked `realtime` schema.

`R15` uses at least 100 online samples and reports `unmet` if server-to-WebSocket P95 exceeds 1,000 ms. Its parser receipt timing is transport evidence, not desktop rendering latency; it does not validate React/Tauri display P95. Disconnect recovery closes/rejoins a client socket and queries missing inserts/tombstones; it never restarts Supabase or the desktop application. The [15-criterion verification record](../docs/2026-10-10-group-chat-verification.md) distinguishes proven subchecks from remaining native/UI/platform/restart gates.

Only the authenticated sender's server `planet_member_state.nickname`/`avatar` are snapshotted. Paused sharing does not reveal token/planet statistics through chat. Public chat payloads use an opaque author key, not Auth user IDs. Member leave and Auth-row removal preserve allowed message bodies/snapshots; group deletion cascades chat state. Own-message deletion removes the active DB body and keeps a tombstone; it does not promise immediate backup/log erasure or invent a permanent-account deletion UI policy.
