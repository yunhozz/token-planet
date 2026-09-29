# Token Planet shared-world database

The earlier migration chain and 121 pgTAP assertions were verified locally with Supabase CLI 2.118.0 and Docker (PostgreSQL 17). New anonymous-auth and personal-code changes require fresh verification before release.

```sh
npx --yes supabase@2.118.0 start --exclude gotrue,realtime,storage-api,imgproxy,kong,mailpit,postgrest,postgres-meta,studio,edge-runtime,logflare,vector,supavisor
npx --yes supabase@2.118.0 db reset
npx --yes supabase@2.118.0 test db
```

For local Auth and REST development, start Supabase with Auth, Kong, and PostgREST enabled:

```sh
npx --yes supabase@2.118.0 start --exclude realtime,storage-api,imgproxy,postgres-meta,studio,edge-runtime,logflare,vector,supavisor,mailpit
```

If the DB-only stack above is already running, stop this local project first with `npx --yes supabase@2.118.0 stop --project-id token-planet`, then run the Auth start command. Stopping without `--no-backup` preserves its local database data.

The desktop creates a Supabase anonymous user when someone first starts sharing. Its session is stored in the operating system credential store, and its user ID identifies the member; nicknames may repeat. Losing that session loses access to the same member identity. Each Mac is a separate member, even when both use the same hosted project. Email, password, OTP, SMTP, and Mailpit are not part of this flow.

The world and membership tables enforce one shared world per account and at most 10 members. Authenticated members can read their world shell and their own usage snapshot rows. The `get_world_planets(uuid)` API replaces the retired `get_world_summary(uuid)` and returns member profiles, derived planet state, token totals, growth, coverage, and ranks after checking membership. `get_my_planet_state()` and `upsert_my_planet_state(jsonb,jsonb)` isolate private planet and wallet state by `auth.uid()`. Raw logs, paths, prompts, session IDs, and wallet credits are excluded from group responses. Tests cover account isolation, device retries and addition, canonical reset timestamp round trips, wallet deduplication, cooldown, coverage, and access control.

The database requires a recognized world timezone and keeps it fixed. A world owner's account cannot be deleted while the world still points to it; ownership must be transferred or the world dissolved first. Earlier migrations implemented single-use invitations; the current flow gives each user a unique personal code that they can view or rotate. Only the current world owner's code admits new members, up to the world capacity. Other migrations implement idempotent per-device uploads, ownership transfer, leave, and deletion of a member's synced aggregates.

For hosted use, apply the latest migrations and enable **Anonymous Sign-Ins** in the project's Authentication → Sign In / Providers settings. The local equivalent is `auth.enable_anonymous_sign_ins` in `supabase/config.toml`. A publishable key remains in the desktop configuration; never put a secret or service-role key there. The hosted setting and an actual cross-device invitation must be checked before calling hosted sharing ready. See the [Supabase anonymous sign-ins guide](https://supabase.com/docs/guides/auth/auth-anonymous).
