# Token World MVP acceptance

This checklist separates verified local behavior from work that needs a hosted Supabase project and a native Windows machine. Do not publish a cross-platform MVP until the pending checks pass.

## Verified in the macOS development environment

- [x] Solo mode launches without an account. Compact and detailed windows render a planet, confirmed token subtotal, and separate unknown-source state.
- [x] The updated macOS `.app` bundle builds. A Token World app window opens; compact and detailed layouts render and switch in the macOS UI.
- [x] Rust tests cover creator-timezone day buckets, changed historical revisions, SQLite restart persistence, account-scoped queues, scan-health coverage, deletion cutoff, and capped retry timing.
- [x] Local Supabase pgTAP tests cover world membership/RLS, invite validity, same-device retries, two-device addition, aggregate summaries, owner transfer, leave/rejoin cutoff, and account-wide usage deletion.
- [x] Local Supabase Auth + Mailpit delivered a six-digit `{{ .Token }}` message; request, verification, and session refresh succeeded.
- [x] The aggregate RPC contract contains only world ID, installation ID, day, agent, nullable token categories, coverage, revision, and payload hash. Group summary responses contain no member-level totals.
- [x] React receives sharing status and group summary, while Rust stores Supabase sessions through OS credential APIs. Raw logs, prompts, paths, and response IDs are not uploaded.

## Pending before a shared-world release

- [ ] Configure a hosted Supabase free project, custom SMTP, Auth email templates, migrations, and public client URL/key. Verify actual email delivery, expiry, resend throttling, and refresh there.
- [ ] Complete an end-to-end sign-in, invite, upload, offline restart/retry, pause/resume, leave, and deletion pass in the packaged macOS app against the hosted project.
- [ ] Confirm the updated release bundle launches by its exact path, then check macOS menu-bar icon click, keyboard activation, close/reopen, and launch after copying the `.app` into Applications.
- [ ] Build the native Windows app and check tray click/keyboard behavior, secure credential storage, Windows account refresh, network payloads, and a real native Windows Claude Code transcript with usage fields. WSL is outside MVP scope.
- [ ] Repeat the hosted end-to-end sharing checks on native Windows, including a two-device same-account deletion/rejoin case and a two-member world.

The app accepts self-reported client aggregates. A modified client can submit fabricated totals. Device-scoped deduplication does not detect a log copied to a second installation.
