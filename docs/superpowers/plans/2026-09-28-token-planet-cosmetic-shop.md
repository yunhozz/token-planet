# Token Planet Cosmetic Shop Implementation Plan

> **For agentic workers:** Implement this plan task-by-task in the current session. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a token shop and permanent cosmetic inventory that works for local planets and signed-in accounts, survives resets and account changes, and shows equipped cosmetics to group members.

**Architecture:** Keep stable slot/SKU drawing metadata in the desktop catalog and store local ownership/equipment by account and cycle in SQLite. Supabase owns authenticated purchase, balance, equipment, and guest-import decisions through atomic RPCs; Tauri commands expose canonical results to React. The same scene renderer draws local previews, the owner’s planet, and the equipped-only group view.

**Tech Stack:** React 19, TypeScript, Tauri 2, Rust, SQLite, Supabase PostgreSQL/RPC.

**Spec:** `docs/superpowers/specs/2026-09-28-token-planet-cosmetic-shop-design.md`

## Global Constraints

- Cosmetics do not affect growth credit, object creation, usage totals, usage accounting, or ranking.
- Use stable string `slot_id` and SKU keys; catalog and UI are lists with no fixed slot count or products-per-slot limit.
- Keep purchase history account-wide and equipment keyed by account, current cycle, and slot.
- Derive available balance as confirmed reset credits minus confirmed cosmetic purchases; use server catalog prices for signed-in purchases.
- Make guest balance-check-and-purchase one SQLite transaction; make signed-in purchase and import decisions one Supabase transaction.
- Preserve purchase idempotency by request UUID and unique account/SKU ownership; repeat requests return the original canonical result.
- Reset clears current equipment in the same transaction that confirms a new cycle; reject equipment updates from an older cycle.
- Expose only current equipped `slot_id` and SKU in group planet responses; keep balances, purchase history, and ownership private.
- Preserve unknown catalog/equipment keys from newer servers; unsupported clients neither render nor offer them.
- Keep the implementation changes uncommitted for the user.

## Review Focus

- Concurrent or repeated purchases cannot make the account balance negative or charge one SKU twice; pin in the Supabase purchase RPC tests.
- A local purchase import failure leaves guest rows intact and never marks them as account-owned; pin in the guest-import tests.
- An equipment update from an old cycle or stale slot version cannot restore a cleared cosmetic; pin in reset/equipment RPC tests.
- Switching between local use and two accounts never exposes one account’s purchases or equipment to another; pin in SQLite account-switch tests.
- Unknown future slots/SKUs survive a read-write cycle but stay invisible and unselectable in an older renderer; pin in storage and scene tests.

---

### Task 1: Local catalog, wallet, and account-scoped state

**Files:**
- Create: `apps/desktop/src-tauri/src/domain/cosmetic_shop.rs`
- Create: `apps/desktop/src-tauri/src/storage/cosmetic_shop.rs`
- Modify: `apps/desktop/src-tauri/src/domain/mod.rs`
- Modify: `apps/desktop/src-tauri/src/storage/mod.rs`
- Modify: `apps/desktop/src-tauri/src/storage/ledger.rs`
- Modify: `apps/desktop/src-tauri/src/storage/planet_accounts.rs`
- Test: `apps/desktop/src-tauri/src/storage/cosmetic_shop.rs`

**Interfaces:**
- Define local slot IDs `sky`, `ring`, and `surface`; define stable SKUs `star_cluster` (별무리), `aurora` (오로라), `thin_ring` (얇은 고리), `double_ring` (이중 고리), `flag` (깃발), and `crystal_tower` (수정탑), with basic items priced at 100,000 and advanced items at 500,000 tokens.
- Produce serializable `CosmeticSlot`, `CosmeticProduct`, `EquippedCosmetic`, and `CosmeticShopState` types. Catalog data is a list; state carries current cycle, available balance, owned SKU list, equipped list, and whether account actions require online confirmation.
- Produce `Ledger::cosmetic_shop_state()`, `Ledger::purchase_guest_cosmetic(purchase_id, sku)`, and `Ledger::equip_guest_cosmetic(slot_id, sku)`. Guest purchase returns a distinct insufficient-balance/already-owned/unknown-SKU error and changes no rows on failure.

- [x] Add SQLite purchase, purchase-request, and equipment tables keyed by account ID. Purchases have a unique account/SKU; request rows store the original result by account/request UUID, including terminal failures; equipment has one row per account/cycle/slot and stores an optimistic slot version. Clear equipment in the same SQLite transaction as `reset_planet` changes the cycle.
- [x] Implement catalog iteration and state calculation from existing reset credits minus purchase prices; keep SKU and slot values as strings so unknown server keys can be retained.
- [x] Test `guest_purchase_spends_100000_once_and_persists`: seed one 100,000 credit, purchase one basic item, assert zero balance after reopen, then assert retry by request ID and retry by SKU add no debit.
- [x] Test `guest_purchase_rejects_insufficient_balance_without_rows`: seed 499,999 credits, attempt a 500,000 item, assert insufficient-balance result and unchanged credit/purchase counts; replay the same request UUID after adding credits and assert it still returns the recorded original result.
- [x] Test `equipment_is_cycle_scoped_and_account_scoped`: equip one owned item, switch to another account and back, then reset the cycle; assert the other account is empty, the owner retains ownership, and reset equipment is empty.
- [x] Test `unknown_equipment_keys_survive_catalog_projection`: persist an unknown slot/SKU and assert it remains in stored state while the local supported-catalog projection omits it.

### Task 2: Private server catalog and atomic purchase/equipment RPCs

**Files:**
- Create: `supabase/migrations/202609280002_cosmetic_shop.sql`
- Create: `supabase/tests/cosmetic_shop.sql`
- Modify: `supabase/tests/fixtures/planet.inc` only if shared test helpers are needed.

**Interfaces:**
- Add private catalog slot/product tables seeded with the three slots and six SKUs; product rows carry stable SKU, slot ID, server price, catalog revision, sale visibility, and immutable purchase semantics.
- Add private account lock, purchase, purchase-request, and equipment tables. Purchases record SKU, server-confirmed price, and timestamp; request rows store the original result by account/request UUID, including terminal failures; equipment records account, cycle, slot, SKU, and version.
- Expose authenticated RPCs `get_my_cosmetic_state()`, `purchase_my_cosmetic(p_purchase_id uuid, p_sku text, p_catalog_revision integer)`, and `equip_my_cosmetic(p_cycle_id text, p_slot_id text, p_sku text, p_expected_version bigint)`. Derive owner only from `auth.uid()`; revoke direct table access.
- Purchase replies distinguish `purchased`, `already_owned`, `insufficient_balance`, and `catalog_mismatch`, with canonical state and price/balance values.

- [x] Add the private schema, RLS/revokes, indexes, stable catalog seeds, and constraints for positive prices, valid IDs, unique account/request ID, unique account/SKU, and one account/cycle/slot equipment row. Keep SKU prices immutable after registration; a price change is a new SKU.
- [x] Implement `get_my_cosmetic_state` using reset-credit sum minus purchase-price sum and return only the caller’s ownership/equipment plus the buyable catalog.
- [x] Implement `purchase_my_cosmetic` with an account lock row that exists even before a planet profile; validate the SKU/revision, return the original result for a matching request UUID (including terminal failures), reject request-UUID reuse for a different SKU, check ownership and balance, and insert the server catalog price atomically.
- [x] Implement `equip_my_cosmetic` to validate current planet cycle, owned SKU, SKU/slot match, and expected slot version; support free unequip and return the latest canonical slot on a version conflict. Keep sold-out SKUs equipable by owners who already own them.
- [x] Add a trigger on `planet_member_state.current_cycle_id` that clears equipment in the same database transaction as an accepted planet reset; test `cycle_change_clears_all_equipment`.
- [x] Add pgTAP tests `purchase_replay_and_duplicate_sku_do_not_charge_twice`, `purchase_failure_replay_returns_original_result`, `insufficient_balance_preserves_wallet_and_purchase_rows`, `catalog_revision_mismatch_is_distinct`, `purchase_lock_works_without_planet_profile`, `equipment_requires_owned_matching_sku_and_current_cycle`, `retired_sku_remains_equippable`, `cycle_change_clears_all_equipment`, and `private_cosmetic_tables_are_not_directly_readable`.
- [x] Add a two-session database check for simultaneous purchases against one remaining balance; assert one canonical purchase and a nonnegative balance.

### Task 3: Guest adoption, authenticated commands, and reset synchronization

**Files:**
- Create: `apps/desktop/src-tauri/src/commands/cosmetic_shop.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/sync/client.rs`
- Modify: `apps/desktop/src-tauri/src/sync/worker.rs`
- Modify: `apps/desktop/src-tauri/src/storage/planet_accounts.rs`
- Modify: `apps/desktop/src-tauri/src/storage/ledger.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `supabase/migrations/202609280002_cosmetic_shop.sql` for guest-import RPCs.

**Interfaces:**
- Register Tauri commands `get_shop_state`, `purchase_cosmetic`, and `equip_cosmetic`.
- Add `SupabaseSyncClient::cosmetic_shop_state`, `purchase_cosmetic`, `equip_cosmetic`, and `import_guest_cosmetics` using the RPCs from Task 2.
- Add authenticated guest import RPC `import_my_guest_cosmetics(p_import_id uuid, p_wallet_credits jsonb, p_purchases jsonb)`; validate credit keys and SKU prices, dedupe existing reset-credit and purchase keys, skip charges for SKUs the account already owns, and commit all new rows only if resulting balance remains nonnegative.

- [x] Route guest commands through the SQLite transaction from Task 1; route signed-in purchase/equip through online authenticated RPCs and only persist the canonical success response locally.
- [x] Keep signed-in inventory, last confirmed equipment, and preview readable while offline or sync-paused; return an explicit unavailable result for purchase/equip without locally assuming success.
- [x] Extend local account switching so each account’s purchase/equipment cache remains isolated and guest purchase rows remain available until the server confirms import.
- [x] On first account adoption, submit guest credits and purchases together once before general planet-state upload. Keep pending guest credits out of the ordinary wallet upload until this RPC succeeds, so rejection leaves both guest credit and purchase records local; retain the guest history, keep login active, and add no unconfirmed SKU to account inventory.
- [x] Persist one purchase UUID before the first network request and reuse it after timeout, restart, or response loss; never create a locally confirmed account purchase. Mirror canonical equipment and balance after sync.
- [x] Test `guest_import_failure_retains_guest_state_and_account_ownership`, `guest_import_replay_is_idempotent`, `guest_import_existing_sku_adds_no_charge`, and `account_switch_restores_only_matching_cosmetics` in Rust and pgTAP.
- [x] Test a delayed old-cycle equip after reset and assert the RPC returns the newest empty/current slot state without restoring the old SKU.

### Task 4: Owner shop, inventory, preview, and scene rendering

**Files:**
- Create: `apps/desktop/src/components/CosmeticShop.tsx`
- Create: `apps/desktop/src/components/__tests__/CosmeticShop.test.tsx`
- Modify: `apps/desktop/src/types/usage.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/PlanetScene.tsx`
- Modify: `apps/desktop/src/App.css`

**Interfaces:**
- `CosmeticShop` receives `CosmeticShopState`, active tab, and callbacks for purchase/equip/unequip; render the slot and product arrays without fixed counts.
- Extend `PlanetScene` with an optional `equippedCosmetics: EquippedCosmetic[]`; draw only recognized slot/SKU pairs in separate sky, ring, and surface layers placed outside existing growth objects.
- Preview renders the selected product over the current planet without changing inventory or equipment.

- [x] Add a detailed-view shop entry point and switchable shop/inventory lists; show current available balance, owned status, per-item price, and disabled choices for unsupported or unavailable entries.
- [x] Require confirmation before purchase and show the item price and resulting balance; show separate messages for insufficient balance, already-owned SKU, catalog mismatch, offline/sync-paused action, and pending network result.
- [x] Add equipped and free-unequip actions per slot; do not make purchase automatically equip the item.
- [x] Draw the six initial cosmetics in `PlanetScene` at stable slot locations across all planet stages without changing object or progress rendering.
- [x] Test `shop_lists_catalog_and_preview_does_not_equip`, `purchase_confirmation_shows_price_and_resulting_balance`, `inventory_equips_owned_item_and_allows_free_unequip`, `additional_slot_and_product_render_without_schema_changes`, and `unknown_cosmetic_is_preserved_but_not_rendered`.
- [x] Run the desktop typecheck/build and component tests; manually inspect detail and compact scene layouts.

### Task 5: Equipped-only group rendering and release checks

**Files:**
- Modify: `apps/desktop/src-tauri/src/domain/planet.rs`
- Modify: `apps/desktop/src-tauri/src/sync/client.rs`
- Modify: `apps/desktop/src-tauri/src/commands/sharing.rs`
- Modify: `supabase/migrations/202609280002_cosmetic_shop.sql` to replace `get_world_planets` without rewriting an earlier migration.
- Modify: `supabase/tests/world_summary.sql` or create `supabase/tests/world_cosmetics.sql`
- Modify: `apps/desktop/src/components/WorldCommunity.tsx`
- Modify: `apps/desktop/src/components/__tests__/SharingPanels.test.tsx`

**Interfaces:**
- Add `equipped_cosmetics: Vec<EquippedCosmetic>` to `WorldPlanet`; the group RPC returns only current-cycle `slot_id` and SKU for each visible member.
- Reuse the Task 4 scene renderer for compact cards and selected member detail.

- [x] Add a replacement `get_world_planets` definition to `supabase/migrations/202609280002_cosmetic_shop.sql` with current-cycle equipped keys only; do not rewrite earlier migrations or serialize purchase, balance, wallet-credit, or purchase-ID data.
- [x] Verify hidden/shared-invisible planets return no rows and that the group RPC rejects or omits all private purchase fields.
- [x] Render equipped cosmetics on member cards and detail view; preserve unsupported values in transport while rendering only locally supported pairs.
- [x] Test `group_payload_exposes_equipped_keys_only`, `hidden_planet_hides_equipped_cosmetics`, and `group_scene_renders_equipped_cosmetics_without_wallet_data`.
- [x] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`, `npm test --prefix apps/desktop`, `npm run build --prefix apps/desktop`, and the Supabase pgTAP suite.
- [x] Record macOS and Windows interactive checks as outstanding if those actual app environments are unavailable; leave all source changes uncommitted.
