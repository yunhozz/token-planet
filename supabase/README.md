# Token World shared-world database

The full migration chain and 121 pgTAP assertions are verified locally with Supabase CLI 2.118.0 and Docker (PostgreSQL 17). The personal-planet migration corrections have not been applied or tested on the hosted project.

```sh
npx --yes supabase@2.118.0 start --exclude gotrue,realtime,storage-api,imgproxy,kong,mailpit,postgrest,postgres-meta,studio,edge-runtime,logflare,vector,supavisor
npx --yes supabase@2.118.0 db reset
npx --yes supabase@2.118.0 test db
```

For local Auth and REST verification, start Supabase with Auth, Kong, Mailpit, and PostgREST enabled:

```sh
npx --yes supabase@2.118.0 start --exclude realtime,storage-api,imgproxy,postgres-meta,studio,edge-runtime,logflare,vector,supavisor
```

The local `confirmation` and `magic_link` templates use `{{ .Token }}` so the six-digit code can be entered inside the desktop window. Test mail is visible in the local Mailpit service. OTP expiry is configured at one hour; hosted SMTP must be verified for delivery and resend limits before release.

The world and membership tables enforce one shared world per account and at most 10 members. Authenticated members can read their world shell and their own usage snapshot rows. The `get_world_planets(uuid)` API replaces the retired `get_world_summary(uuid)` and returns member profiles, derived planet state, token totals, growth, coverage, and ranks after checking membership. `get_my_planet_state()` and `upsert_my_planet_state(jsonb,jsonb)` isolate private planet and wallet state by `auth.uid()`. Raw logs, paths, prompts, session IDs, and wallet credits are excluded from group responses. Tests cover account isolation, device retries and addition, canonical reset timestamp round trips, wallet deduplication, cooldown, coverage, and access control.

The database requires a recognized world timezone and keeps it fixed. A world owner's account cannot be deleted while the world still points to it; ownership must be transferred or the world dissolved first. Later migrations implement single-use hashed invitations, idempotent per-device uploads, ownership transfer, leave, and deletion of a member's synced aggregates.

The hosted Token World project and database migrations are ready, but a custom SMTP provider and verified sender domain are not configured. Hosted OTP delivery and its email templates therefore remain deferred. After SMTP is configured, apply both OTP templates and verify code delivery and session refresh before inviting other users. Local Auth and OTP development can use the Mailpit setup above. See the [Supabase free-tier template change](https://supabase.com/changelog/46599-changes-to-email-template-customisation-on-free-tier).
