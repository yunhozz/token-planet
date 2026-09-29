# Token Planet Shop Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The user's lean-work-harness instruction routes all code and test-code edits through a GPT-6 Luna Max `lean_coder`; the team lead owns final verification.

**Goal:** Expand the shop with eight new decorations and a fourth simultaneous slot, while raising the sale prices of six existing looks without changing prior purchases.

**Architecture:** Keep the six original SKUs immutable and owned/equippable, retire them from new sale, and register six new-price SKUs for the same looks plus eight unique SKUs. A stable old→new style map prevents charging twice for the same appearance in guest and signed-in purchases. Server purchase remains authoritative for accounts; Rust handles offline guest purchases; React displays inventory and previews and draws all supported cosmetics.

**Tech Stack:** Tauri 2/Rust/SQLite, React 19/TypeScript/SVG, Supabase PostgreSQL RPC and pgTAP, Cargo and Vitest.

**Spec:** `docs/superpowers/specs/2026-09-29-token-planet-interactions-and-shop-expansion-design.md`

**Execution order:** Complete the companion avatar-interactions plan first; this plan then builds on its `PlanetScene.tsx`, CSS and scene tests. Each plan remains independently testable.

## Global Constraints

- Existing SKU, slot, original price, purchase amount and purchase ID stay immutable. Old purchases and queued guest imports keep their historical price.
- Fixed new prices: replacement basic looks 500,000 each; replacement premium looks 2,000,000 each; meteor shower 1,000,000; moonlets 3,000,000; flower garden 1,000,000; observatory 5,000,000; pond 750,000; lantern 1,500,000; rover 3,000,000; greenhouse 5,000,000.
- Four slots (`sky`, `ring`, `surface`, `forecourt`) may show one cosmetic each. Ownership persists through reset; equipment clears with the cycle.
- Keep raw logs, paths, prompts, wallet, ownership and purchase history out of group payloads. Group members see equipped keys only.
- Server catalog/RPC deployment precedes app deployment. Unknown SKUs remain preserved but hidden on old clients.
- Follow the Supabase skill before SQL implementation: inspect relevant changelog/docs and discover CLI command flags with `--help`; create a new migration using `supabase migration new cosmetic_catalog_expansion`, then use its generated path. Never edit an applied migration.
- Route code and test edits to `lean_coder`; review and run final checks in the team lead.

## Review Focus

1. A legacy owner buying its replacement SKU after a retry or on another device pays nothing and retains the original purchase amount. Pin in Task 1.
2. A queued old-price guest purchase imports after the price expansion without repricing; an equivalent owned style is not charged twice. Pin in Task 1 and Task 2.
3. Server-first deployment with an old app leaves unknown new SKUs and the new slot untouched rather than deleting equipment. Pin in Task 3.
4. New `forecourt` equipment remains separate from `surface` through reset and stale-cycle requests. Pin in Task 1 and Task 3.
5. A long four-slot scene at every era and in compact/group layouts keeps avatar, generated objects and bubble legible. Pin in Task 3 visual check.

---

## File Structure

- Create one CLI-generated `supabase/migrations/*_cosmetic_catalog_expansion.sql`: new slot/SKUs, retired legacy sale flags, style equivalence and replacement RPC definitions.
- Modify `supabase/tests/cosmetic_shop.sql`: expanded catalog, legacy/new-equivalent purchase, price, reset, import and private table assertions. Modify `supabase/tests/cosmetic_guest_import.sql` for old-price queued import and equivalence.
- Modify `apps/desktop/src-tauri/src/domain/cosmetic_shop.rs`: exact local catalog and old→new style mapping.
- Modify `apps/desktop/src-tauri/src/storage/cosmetic_shop.rs`: local guest duplicate-style purchase handling and expanded catalog tests.
- Create `apps/desktop/src/components/cosmeticStyles.ts`: frontend supported SKU set and old/new style equivalence for display and drawing.
- Modify `apps/desktop/src/components/CosmeticShop.tsx`: show one sale row per appearance, legacy owned rows in inventory, fourth slot and expanded product list.
- Modify `apps/desktop/src/components/PlanetScene.tsx`: draw eight new decorations and accept old/new equivalent SKUs for existing art.
- Modify `apps/desktop/src/App.css`: expanded shop layout and any scene spacing needed for four slots.
- Modify `apps/desktop/src/components/__tests__/CosmeticShop.test.tsx`, `apps/desktop/src/components/__tests__/PlanetScene.test.tsx`, and `apps/desktop/src/components/__tests__/SharingPanels.test.tsx`: frontend purchase visibility, preview, slot and group drawing tests.

## Catalog Contract

Use these IDs consistently in SQL, Rust, TS, tests and the SVG renderer. Old IDs remain supported but `purchasable=false` on the server.

| Slot | New SKU | Display name | Price | Legacy equivalent |
| --- | --- | --- | ---: | --- |
| sky | `star_cluster_v2` | 별무리 | 500,000 | `star_cluster` |
| sky | `aurora_v2` | 오로라 | 2,000,000 | `aurora` |
| ring | `thin_ring_v2` | 얇은 고리 | 500,000 | `thin_ring` |
| ring | `double_ring_v2` | 이중 고리 | 2,000,000 | `double_ring` |
| surface | `flag_v2` | 깃발 | 500,000 | `flag` |
| surface | `crystal_tower_v2` | 수정탑 | 2,000,000 | `crystal_tower` |
| sky | `meteor_shower` | 유성우 | 1,000,000 | — |
| ring | `moonlets` | 작은 위성들 | 3,000,000 | — |
| surface | `flower_garden` | 꽃 정원 | 1,000,000 | — |
| surface | `observatory` | 천문대 | 5,000,000 | — |
| forecourt | `pond` | 연못 | 750,000 | — |
| forecourt | `lantern` | 등불 | 1,500,000 | — |
| forecourt | `rover` | 탐사 로버 | 3,000,000 | — |
| forecourt | `greenhouse` | 온실 | 5,000,000 | — |

### Task 1: Server catalog and atomic ownership rules

**Files:** Create the CLI-generated migration; modify `supabase/tests/cosmetic_shop.sql` and `supabase/tests/cosmetic_guest_import.sql`.

**Interfaces:** Keep `public.get_my_cosmetic_state()`, `public.purchase_my_cosmetic(uuid,text,integer)`, `public.equip_my_cosmetic(text,text,text,bigint)` and `public.import_my_guest_cosmetics(uuid,jsonb,jsonb)` signatures and response shapes. Add `private.cosmetic_style_equivalence(new_sku text, legacy_sku text)` with six pairs. `purchase_my_cosmetic` returns `already_owned` without a charge if the account already owns the legacy SKU for a requested v2 SKU. The existing request-ID result lookup still runs first. Guest import accepts the original legacy SKU and its 100,000/500,000 price.

- [ ] **Step 1: Write failing pgTAP assertions** for 4 slots/20 catalog rows (6 legacy + 14 new), six disabled legacy sales, v2 server prices, old owner→new `already_owned`, same request replay, same appearance with two devices, old-price guest import, four-slot equipment and private equivalence table access. Update the existing `select plan(53)` and guest-import `select plan(20)` counts to match the assertions actually present; retain both `finish()` calls.

```sql
select is(jsonb_array_length(public.get_my_cosmetic_state()->'slots'), 4, 'four slots');
select is(jsonb_array_length(public.get_my_cosmetic_state()->'products'), 20, 'legacy and new catalog rows');
select is((public.purchase_my_cosmetic('66666666-6666-4666-8666-666666666666',
  'star_cluster_v2', 1)->>'status'), 'already_owned', 'legacy owner is not charged again');
```

- [ ] **Step 2: Run** `npx --yes supabase@2.118.0 test db` from repository root; expect the new assertions to fail. If the local Docker stack is unavailable, record the exact condition and continue with SQL review; do not claim pgTAP passed.
- [ ] **Step 3: Inspect** `https://supabase.com/changelog.md` and the relevant RPC/RLS documentation, run `npx --yes supabase@2.118.0 migration new --help`, then create the migration with `npx --yes supabase@2.118.0 migration new cosmetic_catalog_expansion`. In that new file insert `forecourt`, the 14 new products above at catalog revision 1, six immutable equivalence pairs, and mark the six legacy rows `purchasable=false`.

```sql
insert into private.cosmetic_slots(slot_id, display_name) values ('forecourt', '앞마당');
update private.cosmetic_products set purchasable = false
where sku in ('star_cluster','aurora','thin_ring','double_ring','flag','crystal_tower');
create table private.cosmetic_style_equivalence (
  new_sku text primary key references private.cosmetic_products(sku),
  legacy_sku text not null unique references private.cosmetic_products(sku)
);
insert into private.cosmetic_products(sku,slot_id,display_name,price,catalog_revision) values
  ('star_cluster_v2','sky','별무리',500000,1),
  ('aurora_v2','sky','오로라',2000000,1),
  ('thin_ring_v2','ring','얇은 고리',500000,1),
  ('double_ring_v2','ring','이중 고리',2000000,1),
  ('flag_v2','surface','깃발',500000,1),
  ('crystal_tower_v2','surface','수정탑',2000000,1),
  ('meteor_shower','sky','유성우',1000000,1),
  ('moonlets','ring','작은 위성들',3000000,1),
  ('flower_garden','surface','꽃 정원',1000000,1),
  ('observatory','surface','천문대',5000000,1),
  ('pond','forecourt','연못',750000,1),
  ('lantern','forecourt','등불',1500000,1),
  ('rover','forecourt','탐사 로버',3000000,1),
  ('greenhouse','forecourt','온실',5000000,1);
insert into private.cosmetic_style_equivalence(new_sku,legacy_sku) values
  ('star_cluster_v2','star_cluster'), ('aurora_v2','aurora'),
  ('thin_ring_v2','thin_ring'), ('double_ring_v2','double_ring'),
  ('flag_v2','flag'), ('crystal_tower_v2','crystal_tower');
```

- [ ] **Step 4: Replace** the purchase and guest-import RPC bodies in the new migration by copying their current definitions and adding the six-pair equivalence check under the existing account lock. Preserve authentication checks, explicit `search_path`, idempotent request results, balance check and least privilege. The old-price import compares each purchase with its unchanged SKU catalog price. If an imported legacy SKU is equivalent to one already owned, apply the existing duplicate-owned import behavior without a second debit. Do not grant direct table access.

```sql
-- Run only after the request-ID replay lookup and product lookup.
if exists (
  select 1 from private.cosmetic_style_equivalence e
  join private.cosmetic_purchase owned on owned.sku = e.legacy_sku
  where owned.user_id = v_user_id and e.new_sku = p_sku
) then
  v_result := jsonb_build_object('purchase_id',p_purchase_id,'sku',p_sku,
    'status','already_owned','price',v_product.price,'available_balance',v_balance);
end if;
```

- [ ] **Step 5: Run** `npx --yes supabase@2.118.0 test db` and the migration listing command discovered through `--help`; expect pgTAP success and the new migration listed. Review function execute grants, the private table's RLS/access model, and the exact JSON returned to members.
- [ ] **Step 6: Commit** Task 1 files with `feat(shop): 기존 구매를 보존하며 상점 상품과 가격 확장`.

### Task 2: Guest catalog and purchase equivalence

**Files:** Modify `apps/desktop/src-tauri/src/domain/cosmetic_shop.rs` and `apps/desktop/src-tauri/src/storage/cosmetic_shop.rs`.

**Interfaces:** `cosmetic_slots()` returns four slots and `cosmetic_products()` returns 20 products, including non-purchasable legacy SKUs. Add `legacy_equivalent(new_sku: &str) -> Option<&'static str>` for the six v2 SKUs. The local guest purchase path returns `AlreadyOwned` with no debit if that account owns the equivalent legacy SKU, while a pending request UUID still replays its original result before catalog checks.

- [ ] **Step 1: Write failing Rust tests** for the exact 14 new products/prices, four slots, guest `star_cluster` ownership followed by `star_cluster_v2` purchase at no charge, old 100,000 purchase price after the catalog change, new guest purchase at 500,000, and reset clearing `forecourt` equipment without losing ownership.

```rust
assert_eq!(legacy_equivalent("star_cluster_v2"), Some("star_cluster"));
assert_eq!(legacy_equivalent("meteor_shower"), None);
assert_eq!(cosmetic_products().iter().find(|p| p.sku == "star_cluster").unwrap().price, 100_000);
```

- [ ] **Step 2: Run** `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml cosmetic_shop`; expect new tests to fail.
- [ ] **Step 3: Add** all catalog rows and the pure equivalence function. In the guest purchase transaction, check both the requested SKU and its legacy equivalent in `cosmetic_purchase` before subtracting price; reject non-purchasable legacy SKUs for new requests but still return `AlreadyOwned` for a prior owner/replayed request. Preserve the original `cosmetic_purchase.price` row.

```rust
let legacy_owned = match legacy_equivalent(sku) {
    Some(old) => transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM cosmetic_purchase WHERE account_id=?1 AND sku=?2)",
        params![account_id, old],
        |row| row.get::<_, bool>(0),
    )?,
    None => false,
};
if owned || legacy_owned {
    let result = CosmeticPurchaseResult {
        purchase_id: purchase_id.into(), sku: sku.into(),
        status: CosmeticPurchaseStatus::AlreadyOwned,
        price, available_balance: balance,
    };
    store_purchase_request(transaction, &account_id, purchase_id, sku, &result)?;
    return Ok(result);
}
```

- [ ] **Step 4: Run** targeted Cargo tests and `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml`; expect success.
- [ ] **Step 5: Commit** Task 2 files with `feat(shop): 게스트 신규 상품과 동일 외형 구매 방지`.

### Task 3: Shop UI and scene artwork

**Files:** Create `apps/desktop/src/components/cosmeticStyles.ts`; modify `apps/desktop/src/components/CosmeticShop.tsx`, `apps/desktop/src/components/PlanetScene.tsx`, `apps/desktop/src/App.css`, and the three component test files named in File Structure.

**Interfaces:** `cosmeticStyles.ts` exports `SUPPORTED_SLOTS`, `SUPPORTED_SKUS`, `styleIdForSku(sku: string): string`, and `legacyEquivalent(newSku: string): string | null`. It maps each v2 SKU to its original art key; every new SKU maps to itself. `CosmeticShop` consumes the supported lists, filters a purchasable v2 row if its legacy equivalent is owned, and shows the legacy SKU in inventory. `PlanetScene` draws by style ID while leaving equipped SKU keys untouched in storage/sync.

- [ ] **Step 1: Write failing Vitest cases** for four slots, 14 purchasable sale rows for a new user, an old owner seeing exactly one owned legacy look and no duplicate sale row, all eight new preview SKUs, four simultaneous drawn cosmetics, unknown SKU preserved but hidden, and selected-group scene showing only equipped art. Assert price and post-purchase balance in the confirmation dialog for `observatory` at 5,000,000.

```tsx
expect(styleIdForSku("star_cluster_v2")).toBe("star_cluster");
expect(styleIdForSku("pond")).toBe("pond");
expect(screen.queryByRole("button", { name: "별무리 구매" })).not.toBeInTheDocument();
```

- [ ] **Step 2: Run** targeted CosmeticShop, PlanetScene and SharingPanels tests; expect new cases to fail.
- [ ] **Step 3: Implement** style mapping and shop filtering. Draw `meteor_shower`, `moonlets`, `flower_garden`, `observatory`, `pond`, `lantern`, `rover`, and `greenhouse` with the existing pixel SVG palette and static positions. Reserve a distinct lower area for `forecourt`; do not obscure the avatar's walk path, `surface` item, generated-object tiles or speech bubble. Keep preview non-persistent; equipment still stores the actual SKU. Keep the slot/product list data-driven and retain unknown IDs from server state without rendering them.

```ts
const LEGACY_BY_NEW: Record<string, string> = {
  star_cluster_v2: "star_cluster", aurora_v2: "aurora",
  thin_ring_v2: "thin_ring", double_ring_v2: "double_ring",
  flag_v2: "flag", crystal_tower_v2: "crystal_tower",
};
export const legacyEquivalent = (sku: string) => LEGACY_BY_NEW[sku] ?? null;
export const styleIdForSku = (sku: string) => LEGACY_BY_NEW[sku] ?? sku;
const saleRows = products.filter((product) => product.purchasable
  && !owned.has(product.sku)
  && !(legacyEquivalent(product.sku) && owned.has(legacyEquivalent(product.sku)!)));
```
- [ ] **Step 4: Run** targeted Vitest files, `npm --prefix apps/desktop test`, `npm --prefix apps/desktop run build`, and `git diff --check`; expect success. Visually inspect every era, compact personal view, selected group view and shop preview at the new four-slot maximum.
- [ ] **Step 5: Commit** Task 3 files with `feat(shop): 확장 장식과 앞마당 슬롯 표시`.

## Final Verification and Rollout

- Run all relevant Cargo, Vitest, frontend build and local pgTAP checks after integration. Use focused checks first, then broader checks only for concrete regression risk.
- Check purchase replay, insufficient balance, guest import, old-SKU equipment, four-slot equipment and reset against a local Supabase stack. Verify server catalog migration before the new app build connects; do not deploy to a hosted project without the user's separate deployment approval.
- Inspect actual packaged macOS app layouts if available. Report native Windows and hosted two-device purchase checks as unverified until run on those environments. Preserve the release gates in `docs/release-acceptance.md`.
- Confirm group RPC payloads still exclude price, wallet, ownership and purchase history; check focused commit scope and `git status --short` for pre-existing unrelated changes.
