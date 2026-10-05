# Supabase Migration Order Fix Implementation Plan

> **For agentic workers:** Follow the Yunho Harness: CEO coordinates; Coder implements; QA verifies; Reviewer reviews. Do not perform remote database operations or push.

**Goal:** Make migration replay and CI staging apply Supabase migrations in the same dependency-safe filename order.

**Architecture:** Give the two prerequisite migrations unique 14-digit versions that sort before their dependent migrations. Make the CI staging manifest preserve Supabase CLI's UTF-8 filename byte order while leaving the manifest format and SQL bytes unchanged.

**Tech Stack:** Supabase SQL migrations, Python 3 `unittest`, Git.

**Spec:** Bounded design approved in this chat on 2026-10-06; no standalone spec file.

## Global Constraints

- Work only in `/Users/yunho/Desktop/project/token-planet` on the existing `master` checkout.
- Preserve the contents of all migration SQL files; only rename the two approved files.
- Preserve the staging manifest format and synthetic version namespace.
- Production migration history currently ends at `20260929112729`; no October migration versions are recorded there.
- Do not reset, delete, or write to Supabase databases or preview branches; do not run Docker/Supabase runtime, commit, or push.
- Run focused offline verification only; report that hosted Preview recovery still requires a subsequent push by the user.

## Review Focus

- Mixed 12-digit and 14-digit versions sort by complete filename: pin this in a synthetic fixture test.
- The shop-system prerequisite precedes versions `20261001000101` through `20261001000103`: pin the resulting staged manifest order in a repository migration test.
- The effects/reset prerequisite follows `20261001000103` and precedes `20261001000200` through `20261001000208`: pin the full sequence in the same repository migration test.
- UTF-8 filename byte order matches the Supabase CLI: include non-ASCII suffixes in the synthetic fixture.
- Ordinals, synthetic versions, hashes, and SQL bytes remain aligned: assert these in the synthetic fixture and compare pre/post migration file hashes.

---

### Task 1: Align Supabase and CI migration order

**Files:**

- Rename `supabase/migrations/202610010001_shop_system_revamp.sql` to `supabase/migrations/20261001000100_shop_system_revamp.sql`.
- Rename `supabase/migrations/202610010002_shop_effects_and_reset.sql` to `supabase/migrations/20261001000104_shop_effects_and_reset.sql`.
- Modify `supabase/ci/prepare_migrations.py`.
- Test `supabase/ci/test_prepare_migrations.py`.

**Interfaces:** Keep `_source_snapshot(source)` record fields and the `prepare`/`verify` CLI unchanged. Only the returned ordering changes: compare `source_filename.encode("utf-8")` bytes, matching Supabase CLI ordering.

- [x] **Step 1: Record migration SQL hashes and source inventory.** Capture SHA-256 for both prerequisite files and the current migration file count before edits.
- [x] **Step 2: Write the red regression tests.** Update the mixed-length fixture to expect full filename byte order; assert manifest ordinals, synthetic versions, source versions, hashes, and staged SQL bytes. Add non-ASCII suffixes and a repository-level assertion for the approved `00100`–`00104`–`00200` dependency sequence.
- [x] **Step 3: Run focused tests and confirm expected failures.**

  Run: `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s supabase/ci -p 'test_prepare_migrations.py' -v`

  Expected: failures only for the current version-string order and missing renamed migration versions; import or environment errors do not count as valid red results.
- [x] **Step 4: Implement the approved renames and ordering change.** Rename the two SQL files without changing their bytes; change `_source_snapshot` to sort by each complete `source_filename` encoded as UTF-8. Update comments/test names that still describe version-string ordering. Do not change the manifest format or synthetic namespace.
- [x] **Step 5: Run focused tests and confirm green.** Re-run the command in Step 3; all tests must pass.
- [x] **Step 6: Verify file preservation and final diff.** Confirm both SQL hashes match Step 1, the migration file count is unchanged, the repository-level manifest has prerequisites before dependents, `git diff --check` passes, and no unrelated files changed.
- [x] **Step 7: Independent acceptance.** QA independently runs the focused offline suite and confirms the manifest sequence. After QA, Reviewer checks the final diff for migration identity/order, byte preservation, and scope. CEO resolves findings and accepts the change.

## Self-Review

- **Coverage:** Both filename-order inversions, UTF-8 byte ordering, manifest metadata, SQL-byte preservation, and focused verification are covered.
- **Step clarity:** Each step has a single checkable result; RED precedes the implementation and GREEN follows it.
- **Interface consistency:** The staging CLI, record shape, manifest format, and namespace remain unchanged.
- **Review focus:** Each listed failure mode is pinned by the synthetic or repository-level regression assertion, with SQL-byte preservation checked by recorded hashes.
- **Proportion:** One bounded task covers the coupled migration rename and staging-order fix without unrelated refactoring.
