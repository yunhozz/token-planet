# Token World shared-world database

The versioned migrations and pgTAP tests run locally with Supabase CLI 2.118.0 and Docker. They have not been deployed to a hosted project.

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

The world and membership tables enforce one shared world per account and at most 10 members. Authenticated members can read their world shell and their own usage snapshot rows. The `get_world_summary(uuid)` API checks membership, then returns only combined tokens, growth, stage, progress, member count, and group coverage. Raw logs, paths, prompts, session IDs, and member-level totals are absent from the database contract.

The database requires a recognized world timezone and keeps it fixed. A world owner's account cannot be deleted while the world still points to it; ownership must be transferred or the world dissolved first. Later migrations implement single-use hashed invitations, idempotent per-device uploads, ownership transfer, leave, and deletion of a member's synced aggregates.

For a newly created free hosted Supabase project, Token World needs a separately configured SMTP provider before the OTP email templates can be customized. This is the selected deployment path; no hosted SMTP provider or project is configured yet. Apply both OTP templates, verify code delivery and session refresh, then deploy migrations and rerun policy tests before inviting other users. See the [Supabase free-tier template change](https://supabase.com/changelog/46599-changes-to-email-template-customisation-on-free-tier).
