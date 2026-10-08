# Token Planet MVP acceptance

This checklist separates verified local behavior from work that needs a hosted Supabase project and a native Windows machine. Do not publish a cross-platform MVP until the pending checks pass.

## Current multiplayer baseline (2026-10-08)

Use the [approved policy](superpowers/specs/2026-10-08-token-planet-multiplayer-policy-design.md): independent personal planets; exact individual current/lifetime tokens, growth credits and ranks visible within private groups of at most 10. Current device-bound anonymous Auth and reusable owner codes differ from desired 7-day, one-successful-use, owner-revocable invitations. Rotation invalidates the previous personal code but does not implement that desired invitation policy. Session loss cannot recover the same member; recovery, new read-only visits and joint goals remain deferred.

Raw logs, prompts, original paths and original agent session identifiers stay local. Group responses exclude Auth tokens, wallet balances and credit history. The [current verification record](2026-10-08-multiplayer-verification.md) contains P01–P05 document review and V01–V13 runtime status. The 2026-10-08 QA attempt observed only local onboarding before A became inaccessible through CUA: V01 remains partially observed/unverified and V02–V13 remain not run/unverified. Subsequent A-only restart and Launch Services activation did not restore CUA access by exact path or bundle ID. Hidden Popup behavior is a source-based hypothesis, not a confirmed cause; an explicitly approved alternative access path or fix is needed. Document review does not complete a release gate; two-user verification does not prove the 10-member boundary, all native platform checks, or production readiness.

## Verified in the macOS development environment

The checked items below preserve their historical scope. In particular, the OTP and group-summary-without-member-totals checks are evidence for the earlier email-auth/shared-planet flow. Current shared-world sign-in uses anonymous Auth and personal invitation codes; verify that flow separately before release.

- [x] Solo mode launches without an account. Compact and detailed windows render a planet, confirmed token subtotal, and separate unknown-source state.
- [x] The updated macOS `.app` bundle builds. A Token Planet app window opens; compact and detailed layouts render and switch in the macOS UI.
- [x] Rust tests cover creator-timezone day buckets, changed historical revisions, SQLite restart persistence, account-scoped queues, historical scan-health coverage, deletion cutoff, and capped retry timing.
- [x] React tests cover an owner roster refresh when a member is replaced without changing the member count.
- [x] Local Supabase pgTAP tests cover world membership/RLS, invite validity, same-device retries, two-device addition, aggregate summaries, owner transfer, leave/rejoin cutoff, and account-wide usage deletion.
- [x] Local Supabase Auth + Mailpit delivered a six-digit `{{ .Token }}` message; request, verification, and session refresh succeeded.
- [x] The aggregate RPC contract contains only world ID, installation ID, day, agent, nullable token categories, coverage, revision, and payload hash. Group summary responses contain no member-level totals.
- [x] React receives sharing status and group summary, while Rust stores Supabase sessions through OS credential APIs. Raw logs, prompts, paths, and response IDs are not uploaded.

## Pending before a shared-world release

- [ ] Configure a hosted Supabase project, apply migrations, enable Anonymous Sign-Ins, and verify the packaged app can create and refresh its anonymous session.
- [ ] Complete an end-to-end sign-in, invite, upload, offline restart/retry, pause/resume, leave, and deletion pass in the packaged macOS app against the hosted project.
- [ ] Confirm the updated release bundle launches by its exact path, then check macOS menu-bar icon click, keyboard activation, close/reopen, and launch after copying the `.app` into Applications.
- [ ] Build the native Windows app and check tray click/keyboard behavior, secure credential storage, Windows account refresh, network payloads, and a real native Windows Claude Code transcript with usage fields. WSL is outside MVP scope.
- [ ] Repeat hosted end-to-end sharing checks on native Windows with two independent anonymous users. The historical two-device same-account deletion/rejoin expectation depends on future account/device linking and is deferred; it is not a supported current anonymous-auth release check.

## Automated release and production migration gates

- [ ] Configure the GitHub Actions variables and the `supabase-production` environment protection and secrets described in [`docs/deployment.md`](deployment.md).
- [ ] For a release candidate, verify the matching `vX.Y.Z` tag and review both platform artifacts in the generated draft Release. Do not publish it until the shared-world and native-platform checks above pass.
- [ ] Before each production migration run, review the migrations on `master`, use the explicit confirmation input, and obtain the required `supabase-production` environment approval. The workflow performs a dry-run and then applies migrations in the same approved job.

The app accepts self-reported client aggregates. A modified client can submit fabricated totals. Device-scoped deduplication does not detect a log copied to a second installation.
