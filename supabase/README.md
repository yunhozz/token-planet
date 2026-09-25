# Token World shared-world database

The versioned migrations and pgTAP tests run locally with Supabase CLI 2.118.0 and Docker. They have not been deployed to a hosted project.

```sh
npx --yes supabase@2.118.0 start --exclude gotrue,realtime,storage-api,imgproxy,kong,mailpit,postgrest,postgres-meta,studio,edge-runtime,logflare,vector,supavisor
npx --yes supabase@2.118.0 db reset
npx --yes supabase@2.118.0 test db
```

The world and membership tables enforce one shared world per account and at most 10 members. Authenticated members can read their world shell and their own usage snapshot rows. The `get_world_summary(uuid)` API checks membership, then returns only combined tokens, growth, stage, progress, member count, and group coverage. Raw logs, paths, prompts, session IDs, and member-level totals are absent from the database contract.

The database requires a recognized world timezone and keeps it fixed. A world owner's account cannot be deleted while the world still points to it; ownership must be transferred or the world dissolved first. Invite and snapshot upload operations are implemented in later migrations.
