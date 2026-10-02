# 상점 시스템 개편 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Harness precedence:** 위 일반 실행 지침보다 Yunho Harness의 실행 경로를 우선한다. 사용자 계획 승인 후 Coder가 모든 제품 코드·테스트를 수정하고, 개발 팀장이 통합하며 QA와 Reviewer가 검증한다. 팀장이 제품 코드를 직접 수정하거나 실행 방식을 다시 선택받지 않는다.

**Goal:** 풍경 32종의 반복 구매·개별 배치·7종 효과, 아바타 16종의 착용, 자연 개체 유료 제거를 게스트와 로그인 계정에 일관되게 제공한다.

**Architecture:** 기존 cosmetic_shop을 개체 중심 계약으로 전환하고 효과 계산·기여·보상 원장을 분리한다. 게스트는 SQLite 트랜잭션, 로그인 계정은 Supabase RPC가 권위 저장소이며 UI에는 확정 상태를 반환한다. 미리보기와 드래그 좌표는 임시 상태다.

**Tech Stack:** React 19, TypeScript 6, Tauri 2, Rust, SQLite, PostgreSQL/Supabase, Vitest, pgTAP.

**Spec:** [승인된 상세 설계](../specs/2026-10-01-shop-system-revamp.md)

**상태:** 상세 설계·구현 계획 승인 완료 / 구현 진행 중. 개별 단계 체크는 완료 증거를 확인한 뒤 갱신한다.

## Global Constraints

- 풍경 32종: 성장 8종, 나머지 6효과 각각 4종. SKU당 총 5개, 변형 0~4와 문자열 seed를 영구 저장한다.
- 아바타 16종: head/outfit/face/back 부위별 하나 착용, SKU당 한 번 구매, 게임 효과 없음.
- 풍경 기본가 5,000,000 / 15,000,000 / 40,000,000 / 100,000,000. 아바타 기본가 100,000,000 / 200,000,000 / 350,000,000 / 500,000,000.
- 효과는 적립·성장·상점 할인·대기 단축·제거 할인·시대 보상·연속 활동 보상의 7종이다. 자연 개체에는 유료 제거와 제거 기록 저장을 적용하며, 별도 자연 생성 증가 효과는 포함하지 않는다.
- 퍼센트 효과량은 가격 열별 1 / 1.5 / 2 / 3%. 상한은 적립 30%, 성장 20%, 상점 할인 15%, 대기 단축 25%, 제거 할인 30%.
- 시대 보상은 가격 열별 500,000 / 1,500,000 / 4,000,000 / 10,000,000, 시대당 상한 10,000,000. 활동 보상은 10,000 / 30,000 / 80,000 / 200,000, 날짜당 상한 500,000.
- 제거 기본가는 생성 시대 0~4에 100,000 / 250,000 / 500,000 / 1,000,000 / 2,000,000. 할인은 정수 bps와 올림, 적립은 주기 합계의 마지막 내림.
- 상점 할인은 아바타에도 적용하며 새 할인 개체는 자신의 결제에 적용하지 않는다. 견적 변경은 재확인한다.
- 실제 사용량·토큰 순위에 보너스를 섞지 않는다. 문명 순위는 보너스 성장 점수를 사용한다.
- 발생 시각의 효과 이력을 사용하고 활성 전 활동에 소급 적용하지 않는다. 시대별/날짜별 지급 키를 영구 보존한다.
- 일반 초기화는 풍경 소유·아바타 착용·활동 지급 이력을 유지하고 풍경 배치를 회수한다. 다음 대기 시각은 초기화 시 확정하며 최소 18시간이다.
- 로그인 계정의 구매·편집·초기화는 온라인 확정이 필요하다. 오프라인 원본 수집은 계속한다.
- 기존 적용 migration을 수정하지 않는다. 새 migration 작성과 로컬 검증만 포함하며 원격 적용은 별도다.
- 전환 시 계정·인증·원본 사용량·기본 적립을 보존하고 구형 상점 데이터를 정리한다.
- 이 계획은 단일 Coder의 순차 실행 기준이다. 공유 파일에 여러 Coder를 동시에 배정하지 않는다. 커밋은 한국어 Conventional Commits를 따른다.

## Review Focus

- 결과가 불명확한 재시도와 같은 ID의 다른 payload: 한 번 차감 또는 명시적 충돌 — Tasks 3/5/8.
- 두 기기의 마지막 구매 경쟁, stale 버전/주기: 5개 한도와 무차감 거절 — Tasks 5/7.
- 늦은 활동·집계 수정·날짜 경계: 발생 시각 효과와 동일 지급 키 사용 — Tasks 4/6.
- 제거 후 오래된 원격 objects: 현재 주기 부활 방지, 새 주기 정상 생성 — Tasks 7/8.
- 계정 전환 중 늦은 응답·드래그 취소·미리보기: 다른 계정 오염과 임시 효과 없음 — Tasks 9/10/11.

---

## 공통 경로·계약·순서

이 문서의 경로 접두사는 `R = apps/desktop/src-tauri/src/`, `F = apps/desktop/src/`이다. `R/domain/shop_effects.rs`는 `apps/desktop/src-tauri/src/domain/shop_effects.rs`를 뜻한다. SQL 경로는 그대로 사용한다. 같은 Files 줄에서 접두사 없이 나열한 파일은 그 줄의 첫 파일과 같은 디렉터리다. Test의 Rust 항목은 해당 파일의 inline test module이다.

| 파일 | 책임 |
| --- | --- |
| R/domain/cosmetic_shop.rs | 상품·소유·요청·결과 계약 |
| 신규 R/domain/shop_effects.rs | 활성 합산·가격·성장·적립 순수 계산 |
| 신규 R/domain/landscape_geometry.rs | 논리 좌표·배치 경계·통로 검증 |
| R/storage/cosmetic_shop.rs | SQLite 소유·배치·착용·결제 |
| 신규 R/storage/shop_effects.rs | 효과 구간·기여 대체·보상 원장 |
| R/storage/ledger.rs, R/growth.rs | 원본 활동·자연 생성·일반 초기화 연계 |
| R/commands/cosmetic_shop.rs, R/sync/* | 게스트/서버 라우팅·가져오기·공유 |
| 신규 F/hooks/useShopActions.ts | UI 확정 상태·견적·pending·계정 경계 |
| 신규 F/components/landscapeEditing.ts | 화면↔풍경 좌표·편집 상태 |

공통 타입은 R/domain/cosmetic_shop.rs와 F/types/usage.ts에 대응 정의한다.

```text
ShopState: account_id, current_cycle_id, catalog_revision, state_revision,
  available_balance, products, landscape_instances, placements,
  avatar_owned_skus, avatar_equipment, effects, reward_state,
  action_unavailable_reason, guest_import_pending, guest_import_error
LandscapeInstance: instance_id, sku, variation_index, seed:string, variation_version, placement_version
LandscapePlacement: instance_id, cycle_id, x, y, version
AvatarEquipment: head/outfit/face/back -> { sku:null|string, version }
ShopQuote: target, catalog_revision, effect_revision, price
ShopActionResult: status, request_id, state:ShopState
NaturalObjectKey: cycle_id, stage, ordinal
EffectContribution: device_id, cycle_id, date, effect_revision, tokens, growth_bps, wallet_bps
ActivityDay: reward_date, first_occurred_at_utc, canonical_version
```

`ShopRequest`는 purchase/place/retrieve/equip_avatar/remove_natural tagged enum이며 요청 ID, 관련 cycle ID·대상 버전·견적을 포함한다. 아바타 착용은 cycle에 종속시키지 않는다. `QuoteTarget`은 purchase SKU 또는 natural key다. `ShopProduct`는 spec의 category/price/zone 또는 slot/effect type-value/revision을 가진다. `ActiveEffects`는 7종의 상한 적용 수치다. `ShopError`와 결과 status는 spec §9에 placed/retrieved/equipped/unequipped/removed를 추가한다.

EffectContribution의 growth_bps/wallet_bps는 저장된 효과 revision을 도메인에서 조회해 채운 계산용 필드다. RPC에는 해당 revision과 토큰 합을 전송하고 서버가 비율을 다시 조회하므로 클라이언트 비율을 채택하지 않는다.

Rust와 SQL은 checked integer/numeric 중간 연산을 사용한다. JS number에 전달하는 가격·잔액·revision은 안전 정수 범위를 넘으면 명시적으로 거절한다. seed는 항상 문자열이다. 정규화한 요청 payload를 저장하여 동일 ID의 다른 내용은 충돌로 판정한다.

의존 순서: 1→2→3→4→5→6→7→8→9→10→11→12. 중간 단계의 계약은 연결 단계에서 함께 교체한다. 서버 migration은 번호 순으로 적용하며 마지막 migration 이후를 새 앱 계약의 최소 서버 버전으로 선언한다. 전환 중 구형 앱의 쓰기는 중단한다.

### Task 1: 상품·효과·배치 계약

**Files:** Modify R/domain/cosmetic_shop.rs, R/domain/mod.rs, R/domain/planet.rs, F/types/usage.ts; Create R/domain/shop_effects.rs, R/domain/landscape_geometry.rs, F/components/landscapeEditing.ts; Test 새 Rust 모듈 및 F/components/__tests__/landscapeEditing.test.ts.

**Interfaces:** `shop_products() -> Vec<ShopProduct>`; `capped_effects(products: &[ShopProduct], active_instances: &[LandscapeInstance]) -> Result<ActiveEffects, ShopError>`; `discounted_price(base:u64, discount_bps:u16) -> Result<u64, ShopError>`; `validate_placement(product:&ShopProduct, point:LandscapePoint, terrain:LandscapeBounds) -> Result<(), ShopError>`; TS `validatePlacement(product:ShopProduct, point:LandscapePoint, terrain:LandscapeBounds):boolean`.

- [ ] 작성: `shop_catalog` 테스트가 48개 unique SKU, 성장 8개, 하늘 6개, spec의 모든 SKU/가격/효과를 대조한다. `shop_price`는 `discounted_price(5_000_001,1500)==4_250_001`과 overflow 거절을 단언한다.
- [ ] 실패 확인: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml shop_catalog` 및 `shop_price` 필터.
- [ ] 구현: 공통 타입·카탈로그·정수 계산을 작성한다. 호출부를 전환하는 Task9까지 구형 타입/메서드에 읽기 변환 adapter를 유지하여 중간 단계에서도 Rust/TS 전체가 컴파일되게 하고 Task9에서 제거한다. 구형 쓰기로 새 상태를 수정하지 않는다. 효과 합산에 보관함 개체를 넘기지 않는다. 배치된 instance ID와 일치하는 개체만 active_instances로 만든다.
- [ ] 구현: 구매 스프라이트 footprint는 지상 64×64, 하늘 96×64 논리 단위로 통일한다. x/y는 footprint 좌상단이다. terrain은 기존 layoutLandscape의 전체 생성 자연 ID에서 계산하고 제거된 ID도 포함해 경계가 줄지 않게 한다. 하늘은 terrain.y−220부터 terrain.y까지다. 기존 walkway rows/connector columns를 금지 셀로 사용하고 footprint 교차를 거절한다. Rust/TS/SQL에 같은 fixture를 사용한다.
- [ ] 통과 확인: 위 Rust 필터와 `npm --prefix apps/desktop test -- src/components/__tests__/landscapeEditing.test.ts`; NaN/Infinity·영역 밖 전체 footprint·통로 교차 거절을 단언한다.
- [ ] 커밋: `feat(shop): 상점 카탈로그와 개체 계약 정의`

### Task 2: SQLite 전환·계정 저장

**Files:** Modify R/storage/cosmetic_shop.rs, R/storage/ledger.rs, R/storage/mod.rs, R/storage/planet_accounts.rs; Create R/storage/shop_effects.rs; Test R/storage/cosmetic_shop.rs.

**Interfaces:** `Ledger::shop_state(&self)->Result<ShopState,ScanError>`; `Ledger::store_confirmed_shop_state(&mut self,state:&ShopState)->Result<(),ScanError>`.

- [ ] 작성: `shop_schema_transition`에서 구형 구매/장착/캐시/대기 요청 DB를 두 번 재개방하고 계정·원본·기본 적립 보존, 구형 지출 제외 잔액, 중복 초기화 없음, A/B 계정 분리를 단언한다.
- [ ] 실패 확인: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml shop_schema_transition`.
- [ ] 구현: schema version migration으로 개체/배치/아바타/요청/효과 이력/기여/보상/tombstone/reward timezone을 추가한다. 계정별 키와 canonical 캐시 교체를 한 트랜잭션으로 묶는다. 구형 상점 상태만 정리한다.
- [ ] 통과 확인: 동일 필터.
- [ ] 커밋: `feat(shop): SQLite 상점 상태 전환`

### Task 3: 게스트 구매·배치·착용

**Files:** Modify R/storage/cosmetic_shop.rs, R/storage/shop_effects.rs; Test R/storage/cosmetic_shop.rs.

**Interfaces:** `Ledger::quote_shop(&self,target:&QuoteTarget)->Result<ShopQuote,ScanError>`; `Ledger::apply_guest_shop_request(&mut self,request:&ShopRequest,now:DateTime<Utc>)->Result<ShopActionResult,ScanError>`.

- [ ] 작성: `guest_shop`에서 5회 구매 variation={0,1,2,3,4}, 6번째 limit_reached/무차감, 동일 요청 단일 개체·단일 차감, 다른 payload request_conflict를 단언한다.
- [ ] 실패 확인: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml guest_shop`.
- [ ] 구현: 견적·소유·잔액·주기·버전·좌표를 검증하고 결제·개체·결과를 원자 저장한다. 배치/회수로 활성 구성이 바뀔 때만 새 효과 구간을 연다. 단순 위치 이동은 효과를 다시 지급하지 않는다. placement_version은 활성 배치 행과 별도로 보유 개체에 유지하고 회수→재배치에도 증가시켜 오래된 버전 0 요청을 거절한다. 아바타 네 부위 버전은 별도 저장한다.
- [ ] 통과 확인: 같은 필터; 아바타 1억+할인15%→8,500만, 자기 구매 할인 제외, 회수→재배치 버전 증가와 회수 전 요청 거절, stale quote 재확인, 잔액 부족/잘못된 소유/버전·주기 충돌 무차감을 추가 단언한다.
- [ ] 커밋: `feat(shop): 게스트 개체 구매와 편집 구현`

### Task 4: 발생 시각 기여·게임 보상

**Files:** Modify R/domain/shop_effects.rs, R/storage/shop_effects.rs, R/storage/ledger.rs, R/growth.rs, R/domain/planet.rs, R/lib.rs; Test R/domain/shop_effects.rs, R/storage/shop_effects.rs, R/growth.rs.

**Interfaces:** `weighted_growth(total_tokens:u64,segments:&[EffectContribution])->Result<f64,ShopError>`; `cycle_token_bonus(segments:&[EffectContribution])->Result<u64,ShopError>`; `Ledger::rebuild_shop_contributions(&mut self)->Result<(),ScanError>`; `Ledger::settle_guest_rewards(&mut self,now:DateTime<Utc>)->Result<(),ScanError>`.

- [ ] 작성: `shop_effect`에서 일일10만 중5만에 성장20%→1.1, 역순 동일, T=0→0, 1토큰×100·적립1%→최종1을 단언한다.
- [ ] 실패 확인: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml shop_effect`.
- [ ] 구현: canonical 원본을 발생 시각으로 효과 구간에 배정하고 집계 수정·소스 비활성화는 기여를 대체한다. world_snapshot은 읽기 조합을 유지하고 R/lib.rs의 호출 직전에 mutable Ledger로 기여/게스트 보상을 확정한다. read-only snapshot마다 보상을 지급하지 않는다.
- [ ] 구현: 시대 키 account/cycle/stage와 활동 키 account/reward-date를 사용한다. 활동 첫 발생 시각·reward_timezone을 저장한다. 시대 다중 통과는 각 단계 처리하며 보상은 spec 상한을 적용한다. 이미 확정 보상은 회수하지 않는다.
- [ ] 통과 확인: 같은 필터; 활성 전 무보너스, 늦은 활성 후 기록 적용, 일별 재집계 무중복, 시간대 변경/초기화 후 활동 키 유지, 비연속 날 무보상, 시대 재통과 무보상을 단언한다.
- [ ] 커밋: `feat(shop): 효과 구간과 게임 보상 집계`

### Task 5: 서버 상점·권한·원자성

**Files:** Create supabase/migrations/202610010001_shop_system_revamp.sql, supabase/tests/shop_revamp.sql, supabase/tests/shop_concurrency.sh; Modify supabase/tests/cosmetic_shop.sql, supabase/tests/cosmetic_guest_import.sql.

**Interfaces:** RPC `get_my_shop_state()->jsonb`, `quote_shop_action(p_target jsonb)->jsonb`, `apply_shop_action(p_request jsonb)->jsonb`.

- [ ] 작성: pgTAP는 48 SKU, 타인 접근 차단, 5번째/6번째, replay/payload 충돌, stale quote, 아바타 할인·단일 소유를 단언한다.
- [ ] 실패 확인: 로컬 `npx --yes supabase@2.118.0 test db`.
- [ ] 구현: 신규 카탈로그/테이블/RLS/RPC와 계정 지갑 잠금. 잠금 후 소유 수·견적·버전 검증. 클라이언트 비율/금액을 신뢰하지 않는다. DB 함수가 SQL geometry fixture와 같은 경계를 검증한다. 구형 상품 쓰기 계약을 종료하고 구형 상점 지출을 정리한다.
- [ ] 통과 확인: 로컬 `npx --yes supabase@2.118.0 db reset` 뒤 `test db`. 기존 pgTAP를 새 계약에 맞춰 유지하고 보호 기능을 삭제하지 않는다.
- [ ] 작성·검증: supabase/tests/shop_concurrency.sh에서 두 psql 세션의 5번째 구매 경쟁과 stale 배치 경쟁을 실행해 1개 구매 성공/최종5개/중복차감 없음, 배치 한 번 확정을 단언한다. 스크립트는 SHOP_TEST_DATABASE_URL의 호스트가 localhost/127.0.0.1인지 확인하고 그 외는 실행을 거절하며 접속 정보를 출력하지 않는다. Task7 이후 동시 제거 사례를 확장한다.
- [ ] 커밋: `feat(shop): 서버 상점 트랜잭션 추가`

### Task 6: 서버 성장·보상·초기화

**Files:** Create supabase/migrations/202610010002_shop_effects_and_reset.sql, supabase/tests/shop_effects.sql; Modify supabase/tests/personal_planets.sql.

**Interfaces:** 기존 `upsert_my_planet_state(jsonb,jsonb)`는 기여 revision·효과별 daily segments·ActivityDay를 받는다. RPC `reset_my_planet(p_request_id uuid,p_cycle_id text)->jsonb`; 상태에 reward_timezone/frozen reset_available_at_utc 포함.

- [ ] 작성: Task4 숫자 fixture와 segment 합 불일치/미등록 effect revision/역행 canonical revision 거절, replay 무중복, 실제 토큰 순위 불변을 단언한다.
- [ ] 실패 확인: 로컬 `npx --yes supabase@2.118.0 test db`.
- [ ] 구현: 서버 효과 구간·기기별 canonical 대체 집계로 계산한다. 기여의 active revision과 날짜·주기 유효성을 검증한다. reward_timezone은 최초 계정 설정으로 고정하고 동일 날짜 키를 사용한다.
- [ ] 통과 확인: 초기화 기본 적립+별도 보너스 한 번, 25% 단축→18시간, 회수/다른 기기 갱신 후 deadline 불변, 다른 기기의 마감 주기 지연분은 lifetime만 갱신, 게임 지갑 무증가를 단언한다.
- [ ] 커밋: `feat(shop): 서버 효과 계산과 초기화 확정`

### Task 7: 자연 제거·게스트 일반 초기화

**Files:** Modify R/storage/cosmetic_shop.rs, R/storage/ledger.rs, R/growth.rs, R/domain/planet.rs; Create supabase/migrations/202610010003_natural_object_state.sql, supabase/tests/natural_object_removal.sql; Extend supabase/tests/shop_concurrency.sh.

**Interfaces:** remove_natural 요청은 NaturalObjectKey/버전/견적을 사용한다. `Ledger::reset_guest_planet(&mut self,request_id:&str,cycle_id:&str,now:DateTime<Utc>)->Result<ShopActionResult,ScanError>`로 기존 reset 처리에 idempotency를 부여한다.

- [ ] 작성: `natural_removal` Rust/SQL fixture에 시대별 고정가·30%할인·현재 시대와 가격 독립·제거 후 원본/성장 불변을 단언한다.
- [ ] 실패 확인: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml natural_removal` 및 로컬 `test db`.
- [ ] 구현: 차감+tombstone+결과를 원자 저장하고 전체 생성 ID 수에 tombstone을 포함한다. 공유/화면만 제거 개체를 제외한다. 단순 행 삭제나 위치 override는 넣지 않는다.
- [ ] 구현: 게스트 초기화도 주기별 단일 적립·고정 deadline·배치 회수·소유/아바타/활동 이력 보존으로 전환한다.
- [ ] 통과 확인: 같은 필터와 shop_concurrency.sh; replay/동시 제거 한 번 차감, 이미 제거 무차감, stale 주기 무차감, 성장 갱신 후 부활 없음, 새 주기 정상 자연 생성, 18시간 deadline 불변.
- [ ] 커밋: `feat(shop): 자연 제거와 주기 상태 전환`

### Task 8: RPC·가져오기·공유 동기화

**Files:** Modify R/sync/client.rs, R/sync/worker.rs, R/sync/aggregate.rs, R/storage/cosmetic_shop.rs, R/storage/ledger.rs, R/storage/planet_accounts.rs, R/domain/planet.rs; Create supabase/migrations/202610010004_shop_import_and_sharing.sql, supabase/tests/shop_guest_import.sql; Modify supabase/tests/world_summary.sql, supabase/tests/world_access.sql.

**Interfaces:** SupabaseSyncClient methods `shop_state/quote_shop_action/apply_shop_action/reset_my_planet/import_guest_shop` consume matching RPC bodies and return ShopState/ShopActionResult. `Ledger::pending_guest_shop_import()->Result<Option<GuestShopImport>,ScanError>`; `mark_guest_shop_imported(&mut self,import_id:&str,state:&ShopState)->Result<(),ScanError>`. GuestShopImport contains spec §8.2 ownership/placements/effects/tombstones/rewards/wallet/cycle. WorldPlanet shares placements/variants/avatar slots/tombstones, not private balance/history.

**개인 효과 이력 계약:** 인증된 본인만 조회하는 RPC `get_my_shop_effect_timeline()` / typed `ShopEffectTimeline`에 account/cycle, account-global effect revision, 서버 clock, 확정 UTC 구간·활성 ID·효과를 포함한다. 로컬 이력 검증·원자 교체 뒤 기여를 재구축하며 stale/동일 revision 다른 내용/계정 전환 late 응답을 거절한다. 개인 이력을 WorldPlanet에 넣지 않는다. baseline/closed cycle·여러 기기·늦은 활동·재조회·교체 실패 rollback을 검증한다.

- [ ] 작성: `shop_sync` round trip/replay/transport failure 동일 ID、전체 guest import 단일 처리, private 공유 필드 없음, 다른 계정 late 응답 무시를 단언한다.
- [ ] 실패 확인: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml shop_sync` 및 로컬 `test db`.
- [ ] 구현: 서버 효과 이력으로 미전송 기여를 재배정하고 확인된 보상만 캐시한다. 초기화 요청 전에 해당 기기 기여 업로드를 완료한다. 가져오기는 ID/주기/5개 한도/원장 합을 전체 검증하고 한 트랜잭션으로 가져온다.
- [ ] 통과 확인: 활성 서버 주기 불일치/한도 초과 시 부분 복사 없음, 동일 import 재시도 잔액 불변, 오래된 원격 objects가 tombstone을 덮지 않음, 미전송 다른 기기 주기 마감 정책 확인.
- [ ] 커밋: `feat(sync): 신규 상점과 효과 상태 동기화`

### Task 9: Tauri 명령·UI 확정 상태

**Files:** Modify R/commands/cosmetic_shop.rs, R/commands/mod.rs, R/commands/sharing.rs, R/lib.rs, F/App.tsx, F/types/usage.ts; Create F/hooks/useShopActions.ts; Test F/__tests__/App.test.tsx.

**Interfaces:** commands `get_shop_state`, `quote_shop_action(target)`, `apply_shop_action(request)` and existing reset command route guest/server. `useShopActions(context:ShopContext):ShopActions` exposes state/quote/pending/refresh/apply; ShopContext identifies account/cycle/connectivity, ShopActions uses ShopRequest/ShopActionResult.

- [ ] 작성: 온라인 정책·원본 오프라인 수집 유지·불명확한 요청ID 보존·계정 전환 뒤 늦은 응답 무시를 단언한다.
- [ ] 실패 확인: `npm --prefix apps/desktop test -- src/__tests__/App.test.tsx`.
- [ ] 구현: 기존 sync gate에 새 명령을 연결하고 account/cycle context token으로 stale 응답을 버린다. quote_changed는 재확인을 요구한다. 실패 시 임시 좌표/착용을 복구한다.
- [ ] 통과 확인: 같은 테스트와 `npm --prefix apps/desktop run build`.
- [ ] 커밋: `feat(shop): canonical 상점 동작 연결`

### Task 10: 풍경 편집·개체 아트

**Files:** Modify F/components/PlanetLandscape.tsx, planetLandscapeCamera.ts, planetLandscapeLayout.ts, PlanetLandscapeDecorations.tsx, PlanetCosmeticOrganizer.tsx, CosmeticThumbnail.tsx, F/App.css; Create F/components/LandscapeObjectSprite.tsx; Test F/components/__tests__/PlanetLandscape.test.tsx, planetLandscapeCamera.test.ts, landscapeEditing.test.ts.

**Interfaces:** `screenToLandscape(point:ScreenPoint,camera:LandscapeCamera,viewport:LandscapeViewport):LandscapePoint`; `LandscapeObjectSprite({instance,product,selected})`; onPlace/onRetrieve create Task9 ShopRequests with captured versions.

- [ ] 작성: 줌/팬 역변환 저장값 일치, pointercancel/범위 밖 복귀, pending 중 중복 제출 없음, 키보드 시작/방향키/확정/취소를 단언한다.
- [ ] 실패 확인: `npm --prefix apps/desktop test -- src/components/__tests__/PlanetLandscape.test.tsx src/components/__tests__/planetLandscapeCamera.test.ts src/components/__tests__/landscapeEditing.test.ts`.
- [ ] 구현: 지상26/하늘6 상품에 5변형 아트를 만들고 seed/version에 맞춰 표시한다. y+ID 안정 순서로 선택하고 보관함은 개체 ID로 구분한다. Task1 footprint·통로 계약을 따른다. 제거 개체를 포함한 terrain 경계로 카메라와 배치를 계산한다.
- [ ] 통과 확인: 같은 테스트; 재실행/동기화 동일 외형, 다섯 개체 구별, 빈 공간 팬과 개체 드래그 분리, reduced motion. 실제 화면 QA는 Task12에서 수행한다.
- [ ] 커밋: `feat(landscape): 개별 오브젝트 배치 편집`

### Task 11: 아바타 레이어·상점 UI

**Files:** Modify F/components/AvatarSprite.tsx, PlanetScene.tsx, PlanetLandscape.tsx, CosmeticShop.tsx, CosmeticThumbnail.tsx, PlanetProfileSetup.tsx, cosmeticStyles.ts, F/App.css; Create F/components/AvatarEquipmentLayers.tsx; Test F/components/__tests__/CosmeticShop.test.tsx, PlanetScene.test.tsx, F/__tests__/App.test.tsx.

**Interfaces:** AvatarSprite adds `equipment:AvatarEquipment`; AvatarEquipmentLayers receives avatar/equipment/facing. CosmeticShop category/tab/callbacks use ShopState and Task9 actions.

- [ ] 작성: 풍경 n/5·배치 수, avatar 할인 표시/단일 소유/부위 착용, 미리보기 효과 불변, 카테고리/닫기/cycle 변경 시 미리보기 해제를 단언한다.
- [ ] 실패 확인: `npm --prefix apps/desktop test -- src/components/__tests__/CosmeticShop.test.tsx src/components/__tests__/PlanetScene.test.tsx`.
- [ ] 구현: 머리/의상/얼굴/등의 16 상품을 두 기본 모습에 조합하고 기존 걷기·좌우 전환·눈 깜빡임을 유지한다. 메인/팝오버/프로필/공유에 같은 착용 데이터를 전달한다. 활성 효과·보상 timezone·초기화 시각을 표시한다.
- [ ] 통과 확인: 같은 테스트; 16상품×2 기본 모습, 헬멧+얼굴, SVG 경계, 재실행과 일반 초기화 후 착용 유지. 현재 확정 견적만 결제 확인에 표시한다.
- [ ] 커밋: `feat(shop): 아바타 착용과 상품 화면 개편`

### Task 12: 제거 UI·통합·독립 검증

**Files:** Modify F/components/PlanetLandscape.tsx, PlanetCosmeticOrganizer.tsx, F/App.tsx, F/App.css, supabase/README.md, docs/release-acceptance.md; Create F/components/NaturalRemovalDialog.tsx, F/components/__tests__/NaturalRemovalDialog.test.tsx.

**Interfaces:** NaturalRemovalDialog target=NaturalObjectKey, quote=ShopQuote, pending:boolean, onConfirm/onCancel callbacks. 자연 개체에는 제거 견적만, 구매 개체에는 배치/회수만 제공한다.

- [ ] 작성: 대상/기본가/할인/최종가 표시, 취소 무요청, 잔액 부족 무제거, 실패 복구, 타인 편집 불가를 단언한다.
- [ ] 실패 확인: `npm --prefix apps/desktop test -- src/components/__tests__/NaturalRemovalDialog.test.tsx`.
- [ ] 구현: 확인창·초기화 결과를 연결하고 local migration/정리 범위/최소 서버 계약/원격 적용 별도를 README와 release acceptance에 기록한다.
- [ ] Coder/팀장 통합 검증: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`, `npm --prefix apps/desktop test`, `npm --prefix apps/desktop run build`, 로컬 `npx --yes supabase@2.118.0 db reset`와 `test db`, `bash supabase/tests/shop_concurrency.sh`. README의 DB-only/Auth 구성을 사용하고 hosted DB에 reset을 실행하지 않는다.
- [ ] QA: 게스트 구매→5개 배치→새 활동→제거→재실행→일반 초기화, 로그인 두 기기 경쟁/오프라인/가져오기/공유를 확인한다. 줌·드래그·키보드·32상품×5변형·아바타 조합을 실제 화면에서 확인한다. QA는 수정하지 않고 결과를 팀장에게 반환한다.
- [ ] Reviewer: 권한/지갑 원장 합/기여 대체/효과 구간/주기 마감/tombstone/개인 정보 노출을 읽기 전용 검토한다. 발견 사항은 팀장을 통해 Coder에 전달하고 영향 받은 QA를 재실행한다.
- [ ] 완료 판정: spec §12의 10개 조건에 체크 증거를 연결한다. SQL 환경 부재나 미해결 발견은 완료로 처리하지 않는다.
- [ ] 커밋: `feat(shop): 자연 제거 흐름과 개편 통합`

## 가정·열린 항목·승인 경계

- 하나의 계획으로 유지한다. 카탈로그/소유/효과/기여/초기화가 같은 데이터 계약에 의존하므로 단계별 독립 출시보다 마지막 통합 검증 후 출시한다.
- Task1의 고정 footprint와 terrain 산정은 설계의 배치 영역을 구체화한 구현 제안이며 이 계획 승인으로 확정한다. 자연 개체 위치를 편집하는 기능을 추가하지 않는다.
- 기여에는 날짜별 최초 발생 시각과 canonical revision을 포함한다. 효과별 토큰 합만으로 활동 지급 시점을 추측하지 않는다.
- 기존 서버의 기기별 사용량 합산을 유지한다. 같은 원본 파일의 기기 간 복제까지 전역 중복 제거된다고 주장하지 않는다.
- Docker/Supabase 로컬 환경이 필요하다. 없으면 SQL/경쟁 검증 미완료를 표시하고 준비한 로컬 환경에서 끝낸다. hosted DB로 대신 테스트하지 않는다.
- 이 문서에는 Coder 실행 순서가 확정되어 있다. 별도 기능 선택을 구현자에게 남기지 않았으며 미확정 제품 결정은 없다. 실제 원격 적용 위치/시점은 이후 배포 단계다.
- 승인 전 코드/테스트/QA/DB 적용을 실행하지 않는다. 승인본 설계와 계획은 분리 작업을 위해 문서 전용 커밋 f941a9f에 기록했다. 구현은 연결된 managed worktree에서 진행한다.

## 팀장 자체 검토

- spec §1–3 → Tasks1/2/3/5/11: 32+16 상품·가격·소유 한도·변형·아바타 할인.
- spec §4 → Tasks4/6: 7효과·상한·prospective 구간·보상 키·시간대·초기화 마감.
- spec §5 → Tasks1/10: geometry/통로/드래그/키보드/변형/미리보기 경계.
- spec §6 → Tasks7/12: 제거 고정가·원자 차감·tombstone·현재 주기 부활 방지.
- spec §7–9 → Tasks2/3/5/6/8/9: 공통 타입·권한·버전·주기·replay·오프라인·계정 경계.
- spec §10–11 → Tasks2/5/11/12: UI·아바타·출시 전 정리 범위.
- spec §12 → Task12: 독립 QA/Reviewer와 명령별 검증 증거.
- active_instances에 배치 개체만 넘기는 계약, 일별 성장 계산, 문자열 seed, 정상 초기화와 개발 정리의 분리를 대조했다.
- Review Focus 5개 모두 소유 Task의 명시적 검증에 연결했다. 자연 개체에는 승인된 유료 제거 및 제거 기록 저장만 적용하고 별도 생성 효과를 추가하지 않았다.
- 12단계는 각기 의미 있는 도메인/저장/서버/UI 결과를 가지며 파일 경로와 인터페이스를 선행 단계에 정의했다. 제품 코드와 전체 함수 본문을 계획에 복제하지 않았다.

## 구현 진행 기록

- 2026-10-01 Tasks 1–3: 카탈로그·효과 순수 계산·배치 계약, SQLite 전환, 게스트 구매/배치/회수/아바타 착용 구현. Coder 보고: Rust 전체 110/110, `guest_shop` 3/3, TS 배치 검사 1/1, `npm run build`(tsc + Vite) 성공.
- 독립 QA: `guest_shop` 3/3, `schema_transition` 1/1, `domain::` 10/10, `local_catalog` 1/1, TS 배치 검사 1/1 성공. 아바타 중복 구매와 실제 타인 ID 조작 검사 보강을 후속 Coder에 전달했다.
- Task 4: 발생 시각 기반 기여·교체·성장 스냅샷·게스트 보상 구현. 최종 Coder Rust 118/118, `shop_effect` 10/10, 성장 스냅샷 1/1. 독립 QA와 수정 재검사 `guest_shop` 4/4·`shop_effect` 10/10 성공. 시대 감소 후 재도달 무중복과 타인 실제 개체 거절도 확인했다. 기반 Tasks 1–4 커밋은 `8829dcb`다.
- Task 5a: 서버 카탈로그·저장 모델·상태/견적 RPC·접근 권한 구현. 전용 로컬 프로젝트에서 lead와 독립 QA 모두 pgTAP 22/22 성공. 실제 구매/배치/장착·15% 할인 상한·geometry·동시 구매는 Task 5b 후속이다.
- 로컬 검증 환경: `/private/tmp/token-planet-shop-revamp-test`, project ID `token-planet-shop-revamp-test`, DB 컨테이너·볼륨 `supabase_db_token-planet-shop-revamp-test`, 포트 55432. 기존 프로젝트 reset은 자동 승인 검토가 데이터 폐기 승인 불충분 사유로 거절했다. 전용 컨테이너/볼륨을 확인한 뒤 이 폐기 가능한 환경만 reset했고 13개 마이그레이션 적용이 성공했다. 기존 DB에는 그 전에 신규 additive migration 3개가 적용됐으며 데이터 삭제/reset은 하지 않았다.
- Task 5b는 기존 5a 마이그레이션을 수정하지 않고 신규 `20261001000101_shop_actions.sql`로 분리한다. Rust 게스트 자연 제거/초기화 Task 7과 파일 소유를 분리해 진행한다. 전체 완료 판정은 후속 동기화/UI/통합 QA와 Reviewer 이후다.
- 모든 구현은 로컬 worktree에서 진행하며 원격 DB 적용·배포·push·merge는 수행하지 않는다.

- Task 5b 구매: 독립 QA pgTAP 31/31 성공, 신규 구매 트랜잭션 커밋 `0a224eb`. 배치/회수/장착은 후속 immutable migration `20261001000102_shop_placement_and_equipment.sql`에서 진행하며 전용 DB 최초 배치 4/4 성공. 확장 검증과 장착은 진행 중이다.
- Tasks 10/11 독립 아트 단위: 32풍경×5변형, 16장비×2기본 모습×2방향의 primitive/stroke/transform 경계와 기존 PlanetScene을 독립 QA가 재검사하여 77/77 성공했다. 아바타 5개 도형 inset 수정으로 이전 경계 초과 finding을 해결했고 `bdb5f1a`로 커밋했다. 브라우저 실제 래스터화와 헬멧+얼굴 식별성은 Task12 통합 화면 QA에서 확인한다.
- Task 7 게스트: Coder 전체 Rust 126/126, 데스크톱 tsc/Vite 빌드 성공. 자연 제거 3/3, 초기화 3/3, closed-cycle streak/초기화 전 견적 각각 1/1 성공. 로그인 계정의 로컬 초기화는 거부하며 서버 경로 연결은 Task9에 남는다. 독립 QA 9/9 성공 후 `e8bbdfd`로 해당 범위를 커밋했다. 정산 성공 직후 초기화 저장 실패의 안전 재시도와 자연 제거→성장 갱신→재실행→새 주기 연속 검사는 후속 검증으로 남겨 두었다.
- 기반 커밋의 독립 Reviewer 검사에서 같은 날 초기화 전후의 성장 기여를 현행 cycle로 제한하지 않은 P1 1건을 발견했다. 수정과 같은 날 초기화→새 활동 RED/GREEN, 정산 후 초기화 실패→안전 재시도 검증을 Task8 시작 조건에 추가했다. 그 외 해당 커밋 범위의 확정 finding은 없다.
- SQL 배치/회수/아바타 착용 단위: 전용 DB에서 55개 중 미구현 장착 7개 RED를 확인한 뒤 구현하여 lead와 독립 QA 모두 55/55 성공했다. `a5f4290`으로 커밋했다. 기존 writer 폐기·이전 소비 잔액 제외·두 세션 경쟁은 Task5 후속이다.
- Reviewer P1 수정: 같은 날 초기화→새 활동 검사에서 `InvalidShopState` RED를 재현하고 현행 cycle 조건으로 GREEN을 확인했다. 독립 QA 전체 Rust 127/127와 집중 2개 각각 1/1 성공 후 `104179b`로 커밋했고 Reviewer가 P1 해결을 재확인했다. 실패 주입 검사는 보너스 정산 후 기본 적립 저장 실패 시 열린 주기·효과 구간·10개 배치 유지, deadline/초기화 receipt 부재, 보너스 1회 유지와 같은 요청 ID 재시도 성공을 확인하여 해당 공백을 해소했다.
- Task8은 준비된 상태/견적/동작 RPC transport만 `sync/client.rs`에서 시작한다. 서버 효과/초기화/제거/가져오기 계약 완료 전에는 로그인 로컬 초기화나 UI 서버 경로를 완료로 판정하지 않는다.
- Task8 전송 단위: 상태/견적/동작의 실제 mock HTTP 경로·본문·인증·canonical DTO와 401/JSON/연결 종료 후 같은 요청 ID 재호출을 확인했다. Coder 전체 Rust130/130, 독립 QA 집중3/3 성공 후 `9ed07ae`로 커밋했다. reset/import/cache/공유는 아직 후속이다.
- Task5 경쟁: 호스트 psql 없이 전용 Docker 컨테이너만 사용하는 고정 대상 스크립트를 구현했다. 첫 직접 blocker 검사 실패를 간접 대기 체인 검사로 수정한 후 lead와 독립 QA 모두 실제 두 세션 경쟁을 통과했다. 두 root PID가 계정 blocker에 도달, 구매1/한도거절1, 최종 소유5/원장5/1억→7,500만을 확인했고 임시 계정·blocker 잔존0을 확인했다. `9fe9e15`로 커밋했다. 배치·제거 경쟁은 후속이다.
- 구형 경로 폐기: 전용 DB에서 28개 중 15개 RED를 확인한 뒤 신규 migration 적용으로 lead와 독립 QA 모두28/28 성공, `19debad`로 커밋했다. 구형 3개 쓰기 RPC 권한 제거·owner 55000 종료, 장착 삭제 trigger 폐기, 데이터 보존과 canonical 잔액 200만−새 지출30만=170만을 확인했다. 기존 구형 쓰기 성공 fixture는 Task12에서 새 계약에 맞춰 유지한다.
- Task10 순수 좌표 변환: 실제 SVG의 xMidYMid meet 및 viewBox/DOMRect를 반영했다. 새15개 RED→기존7개 포함22개 GREEN·독립 QA 성공 후 `d3a96c7`, 유한 입력의 파생 overflow 검사를 보강해 독립23/23 성공 후 `f64fb66`으로 커밋했다. tsc 성공, 기본 Vite dist 정리 권한 오류는 임시 outDir 생산 빌드 성공으로 검증했다. App/드래그 저장 연결은 아직 후속이다.
- Task6a 기여 검증: 139줄 RED fixture를 저장하고 첫3개 실패 후 새 contribution 테이블 부재로 중단됨을 확인했다. 실제 완전 실행/atomic replacement/성장 계산 GREEN과 독립 QA는 신규 서버 구현 후 확인한다. 가져오기 DTO 제안은 SQL 효과/초기화/자연 상태 계약 완료 전까지 초안으로 유지한다.
- Task12 독립 제거 확인창: missing component 및 개체 번호 표시 RED 후24/24를 확인했다. 독립 QA가 제거 할인30% 상한 불일치를 발견해 다섯 시대 하한−1 RED→수정→독립28/28 성공으로 해결했다. `c1d6f6f`로 커밋했다. 실제 actions 중복 클릭·브라우저 focus·메인 제거 연결은 후속이다.
- Task6a 내부 검증/원자 대체: 직접 helper 부재 RED 후 전용 DB 적용·lead/독립 QA14/14 성공, `932f60a`로 커밋했다. 이후 독립 Reviewer가 성장 날짜에 보상 시간대를 사용한 문제와 원본 없는 활동일 허용 P2 두 건을 발견했다. 기존 migration을 수정하지 않고 별도 검증 수정 migration의 RED/GREEN·QA·리뷰 재확인 후 공개 업로드를 적용한다.
- 공개 기여 업로드 RED는 helper 적용 후 완전 실행26개 중17개 실패로 확인했고, raw-only 두 번째 기기 검사도 실패 단계에 도달했다. 후속 upload migration에 weighted 자연 생성·전체 rollback 검사를 추가한다. 보상/초기화는 해당 수정과 공개 업로드 GREEN 다음 단계다.

- Task6a 검증 수정: 성장일은 mutable raw timezone, 보상일은 고정 reward timezone을 적용한다. 원본 없는 활동·최초시각 주기 경계·겹치는 보상일 원본 재사용·같은 보상일 초기화 전후 합산을 독립 fixture로 확인했다. 후속 두 aggregate RED 후 numeric 전체 합/모든 주기 날짜 합으로 수정하여 격리 DB lead와 독립 QA 모두22/22 성공, Reviewer 미해결 P2 없음 확인 후 `cdef694`로 커밋했다. 공개 업로드/보상/초기화 transaction은 별도 후속 검증이다.
- 상품 썸네일 추가 회귀: 미등록 아바타/잘못된 zone·slot/instance SKU 불일치 fallback과 frozen 입력 불변성을 독립 QA13/13으로 확인하고 `8c0f32b`로 커밋했다.
- 공개 upload draft는 아직 적용하지 않았다. legacy upsert의 client objects 병합이 server 자연 생성에 영향을 줄 수 있는 경계를 발견해 forged·duplicate natural identity RED 및 trusted 기존 ID/seed 보존, weighted stage threshold/전체 rollback을 다음 검증 단위에 포함한다.
- 최신 검증 수정 적용 후 기존 state/quote22·purchase31·placement/equipment55·legacy retirement28을 독립 QA가 재실행하여136/136 성공했다. 공개 wrapper 리뷰에서 client objects가 생성 판단에 들어가는 P1을 발견했고, 정상 서버 생성 기록의 유일성/해시·업로드 위조 배제를 후속 migration의 acceptance로 확인한다. 기존 적용 migration은 수정하지 않는다.

- Task8 기여 DTO/읽기 단위: missing API E0599 RED→단일 SQLite read transaction에서 account/device/version/all-cycle 기여와 날짜별 활동 조회 구현. Coder 및 독립 QA focused1/1·shop_effects13/13·전체 Rust131/131 성공 후 `1ec0cc5`로 커밋했다. 최초 loopback bind 권한 실패를 명시하고 권한 허용 재실행 결과를 확인했다. accessor 자체는 재구축하지 않으며 worker는 업로드 전 명시적으로 재구축해야 한다.
- Task8 후속 공백: 기존 ShopState/cache는 서버 효과 구간을 갖지 않으므로 worker를 먼저 연결하지 않는다. 개인 effect timeline RPC/DTO와 서버 이력 원자 교체를 위 계약에 명시했다. SQL 업로드/marker·보상/초기화와 해당 개인 조회 계약을 갖춘 뒤 worker를 연결한다.

- 공개 upload 검증: canonical public RPC에서 zero-growth client 위조/중복 개체가 저장되는 P1 RED(28개 중1실패)를 직접 확인했다. 계정/주기별 private 첫 생성 marker로 legacy 배열을 최초 새 upsert에서 제외하고 서버 raw/canonical 성장으로 생성한다. 전역 자연 개체 reset은 하지 않으며 marker는 성공 후만 저장한다. Rust/SQL seed 좌표의 numeric 나눗셈 반올림도 별도 RED로 확인하고 exact div로 수정했다. 중간 독립 QA73/73, 후속 재시도/두 기기/두 차례 성장 감소 ID·seed 보존을 포함한 lead85/85 성공이다. owner/cycle matrix·기존 상점136 회귀·최종 QA/Reviewer 이후 해당 단위를 커밋한다.

- 공개 upload 최종 gate: 클라이언트 wallet_credits 신규 삽입·기존 금액 변경 RED를 확인하고 서버 auth.uid() 원장만 입력에 재구성했다. 기존 금액/시각은 변경하지 않으며 역사적 출처는 미검증 carry-forward로 기록한다. 기존 회귀 자금 fixture는 서버 원장에 같은 ID/금액/시각을 seed하고, 자연 배치 fixture는 실제 양수 canonical 사용량의 서버 생성 stage0:0으로 보강했다. lead 및 독립 QA 전체250/250, Reviewer 추가 차단 결함 없음 후 `036be08`로 공개 SQL 단위를 커밋했다. 원격 DB는 적용하지 않았다.
- 다음 작업: Task6 서버 게임 보상 원장/정산과 Task8 개인 timeline DTO·검증·원자 캐시는 분리된 파일에서 TDD로 진행한다. 서버 reset/tombstone/import/shared 계약, worker/명령/UI 실제 연결과 최종 통합 검증이 아직 남아 있으므로 전체 개편은 미완료다.

- Task6 보너스 helper: 100개 1토큰·1% 구간의 최종 합산 내림 missing-function RED 후 전용 DB migration 적용으로1/1 GREEN. 계정/주기 격리·소수 누적·numeric overflow·null scope·API 권한을 보강해 lead 및 독립 QA11/11, 기존 회귀 포함261/261 성공 후 `ecdb7b8`로 커밋했다. 이 helper는 읽기 계산만 하며 실제 지급/초기화 완료를 뜻하지 않는다. 신규 보상 원장/정산은 별도00203 migration에서 이어간다.
- Task8 timeline DTO/API missing-type/API RED, Task9 hook missing-module RED를 저장했다. 개인 이력 원자 교체 및 context/same-ID 재시도 구현을 각자의 별도 파일에서 진행하고, 서버 계약 완료 후 worker/App에 연결한다.

- Task9 준비 훅: 첫13개 GREEN 뒤 QA의 foreign 응답/pending 소유권 2건과 Reviewer의 refresh/quote 동시성 P2 2건을 각각 RED로 재현해 수정했다. 최종 독립 QA22/22·tsc, 임시 outDir Vite 성공, Reviewer 재확인 후 `9f0222b`로 커밋했다. 실제 Tauri/App 연결 완료를 뜻하지 않는다.
- Task8 개인 timeline/cache: typed DTO, 캡처한 계정·주기 검증, 서버 보상 timezone 최초 교체/이후 고정, 연속 구간 검증 및 이력·메타·기여 원자 교체를 구현했다. 독립 QA Rust138/138(기존 localhost mock3개 제외)·Reviewer 차단 결함 없음 후 `908feb2`로 커밋했다. rustfmt component는 환경에 없어 포맷 도구 실행은 미완료다. 서버 RPC/worker, 로그인 다기기 확정 성장 표시와 수정 집계 반영은 후속이다.
- Task6 시대 원장: missing helper RED→6/6→보강17/17, 독립 전체278/278 GREEN. Reviewer가 잠금 전에 clock을 캡처해 다른 거래의 효과 변경 후 이전 스냅샷으로 지급할 수 있는 P2를 발견해 커밋을 보류했다. 실제 두 세션 lock 대기 RED/GREEN 및 QA/Reviewer 재확인 뒤 진행한다. streak 50만 상한·잔액/public upload 연결·secure reset은 후속이다.

- Task6 시대 원장 P2: 두 세션에서 실제 account lock 대기를 관찰한 RED 뒤 두 잠금·stage 확인 후 server clock을 캡처하도록 수정했다. 독립 QA race GREEN·17/17·전체278/278, Reviewer 해결 확인 후 `5e40326`으로 커밋했다. 격리 fixture/session 잔존은 0이다.
- Tasks9/11 패널 단위: 독립 QA50/50·TypeScript 성공, Reviewer 구매/재시도 차단 결함 없음. 아바타 외형 전용 문구 RED/GREEN 후 lead15/15 재검사와 함께 `967a39d`로 커밋했다. 실제 App/Tauri 연결과 브라우저 화면 검증은 후속이다.
- Task6 공개 보상 지급: 신규 `shop_reward_delivery.sql`은 immutable planet timezone Asia/Seoul과 frozen reward timezone UTC를 분리한 fixture로 공개 upsert 경로를 검증한다. 최초14개 중 잔액·streak key·financial revision의8개 의도된 RED를 확인했다. 신규00204 구현과 독립 QA/Reviewer는 아직 진행 중이다.

- Task6 공개 보상 최종 gate: 늦은 전일 활동 재평가, reset 당일 계정 최초 활동 주기, 실제 정산/현재 주기 시대 보상 요약의 공개 경로 RED를 각각 확인했다. 00204 수정 후 lead와 독립 QA 전체307/307, Reviewer 세 finding 해결 확인 후 `f95457d`로 커밋했다. secure reset의 실제 정산 원장은 후속이다.
- Task10 풍경 상호작용: 드래그 시작 version 고정과 동시 갱신 취소, 설치된 개체 키보드 이동, 확정 아바타 레이어 회귀를 보강했다. 독립 QA75/75·Reviewer 해결 확인 후 `189861b`로 커밋했다. 실제 브라우저 pointer capture/시각 검사는 남아 있다.
- Task9 App 연결 진행: 관리 워크트리에서 canonical ShopPanel을 실제 마운트하고 구매→보관함→풍경 드래그 배치→회수 및 아바타 미리보기→확정 착용 통합 테스트를 개별 통과했다. 전체 App 회귀는 구형 cosmetic fixture를 canonical 계약으로 교체 중이며 독립 QA/완료 판정 전이다. 원본 체크아웃은 clean으로 확인했다.
- Task6 secure reset 00205: 누락 RPC 및 클라이언트 주기 임의 변경의 4개 테스트 중3개 RED를 확인했다. DDL/receipt/정산 helper 초안만 저장했다. public reset 구현 패치는 자동 검토가 명시 승인 부족 사유로 거절하여 적용되지 않았고, 사용자 확인 전 해당 차단 부분을 수정하거나 재시도하지 않는다.
- Task8 timeline 경계: sparse 첫 revision1 이전의 현재 주기 활동은 revision0/기본 효과로 매핑된다. 효과 구간이 없는 과거 주기 활동은 baseline으로 분류되어 주기 ID가 보존되지 않는 공백이 있다. 순수 저장소 회귀 검증을 진행하며 서버의 실제 reset 경계 이력 계약과 개인 RPC/worker 연결 전에는 여러 주기 동기화 완료로 판정하지 않는다.

### 현재 인수 체크리스트와 다음 담당

- [x] Native 상점 명령 브리지: 독립 QA 집중17/17·전체154/154(환경상 loopback3개 제외), Reviewer 통과, 로컬 커밋 `d96391a`.
- [x] sparse 효과 시작 경계의 양수 사용량 검증: 독립 QA19/19, 테스트 전용 커밋 `830c6db`. 과거 주기 복원을 완료한 것은 아니다.
- [x] App canonical 구매·보관함·배치/회수·아바타: 독립 QA/Reviewer 통과, `13fac2a`. App44/44·전체 frontend291/291. 실제 브라우저/네이티브 시각 검증은 마지막 gate에 남는다.
- [x] 자연 제거 견적: 5시대 가격·할인/올림·잘못된 키·권한 거절54개와 기존 회귀361/361, 독립 QA/Reviewer 통과, `ac94395`.
- [x] 서버 초기화00205: 사용자의 `RPC 승인` 조건으로 구현하고 독립 QA 집중159/159 및 전체477/477, Reviewer 통과 후 `5b9f992`로 커밋했다. 실제 동시 reset/cooldown 대기 검사와 signed native/UI 연결은 별도 gate다.
- [x] 자연 제거 실행00207: 보상 포함 잔액·차감/replay/conflict·tombstone·account/cycle·rollback·no-regen·전체 DTO 입력 경계를 구현했다. 독립 QA47/47 및 전체480/480, Reviewer P2 해결 확인 후 `0e1f7ee`. 실제 두 세션 제거 경쟁 및 native/UI 연결은 별도 gate다.
- [x] 실제 자연 제거 UI: guest617e32c, signednative843b8f0, signedUI16ecb30. 독립 QA 관련146/146·전체frontend315/315와 Reviewer 통과. 모의 명령/JSDOM이며 실제 Tauri/브라우저/hosted 거래는 미실행이다.
- [ ] 로그인 초기화 연결: worker canonical preupload 및 account/cycle/atomic cache 계약을 검증하기 전까지 unavailable 상태다.
- [x] 개인 effect timeline RPC/캐시: 008 `c752630`과 Rust `1539e86`/`707a3d0`, 독립 SQL43/523 및 Rust27+2/168, Reviewer 통과. 확인된 경계와 positive history를 보존하고 미확인 과거 원본은 lifetime에만 남긴다.
- [ ] 개인 effect timeline worker 연결: typed contribution fetch/apply/rebuild/upload, cold bootstrap·계정 교체·sharing 독립 경로는 미완료다. 사용자의 추가 명시 승인 후, 정해진 집계 schema의 인증 기여 전송 구현과 mock 검증을 진행한다. 실제 사용자 데이터를 hosted로 보내는 실행은 하지 않는다.
- [ ] 게스트 원자 가져오기·공유: 보유/변형/장착·배치·제거 기록을 포함하는 import 및 공유 장면 계약의 실제 연결과 계정 격리 검증이 남아 있다.
- [ ] 최종 전체 frontend/Rust/SQL 회귀, 브라우저 실제 배치·아바타·제거/초기화 화면, 운영 문서와 마지막 Reviewer 인수 판단. 원격 DB 적용·호출·배포·push·merge는 이 로컬 구현 범위에 포함하지 않는다.

### 2026-10-02 통합 체크포인트

- App 실제 canonical 구매·보관함·메인 배치/회수·아바타 연결은 독립 QA/Reviewer를 통과해 `13fac2a`로 커밋했다. App44/44, 관련132/132, frontend 전체291/291, TypeScript 및 임시 outDir Vite 빌드를 확인했다. 실제 브라우저/네이티브 화면 검증은 남아 있다.
- 자연 제거 견적은 54개 행렬과 기존 회귀 포함361/361, Reviewer 통과 후 `ac94395`로 커밋했다. 견적은 제거 차감 완료를 뜻하지 않는다.
- 서버 reset DTO/전송은 `a720f55`(집중9/9, Rust157/157·loopback3개 제외), 등록 local reset의 로그인 거절은 `c434a95`(집중3/3, Rust159/159·loopback3개 제외)로 커밋했다. 직접 Tauri invoke가 아닌 같은 guard 경계와 명령 호출 순서 검증이다. 로그인 server reset의 account/cache/preupload/UI 연결은 남아 있다.
- 00205 reset과 00207 자연 제거를 함께 적용할 때 제거 키가 내부 legacy upload 필드 검사를 깨는 실제 RED를 확인했다. 00205가 내부 입력에서만 제거 키를 제외하도록 수정한 뒤 disposable project reset/reapply와 reset48+remove21=69/69를 확인했다.
- 전체14파일 실행은 429개에서 실패했다. 00207 잔액 helper가 기존 게임 보상 원장 합산을 빠뜨린 6개 회귀와, 기존 제거 fixture의 새 필수 price 누락을 발견했다. 해당 수정과 전체 GREEN, 독립 QA/Reviewer 전에는 00205/00207을 확정하지 않는다. 초기화 전체 rollback·request conflict·metadata guard와 제거 계정/주기·rollback·재생성 방지 검증도 계속 보강한다.
- 개인 타임라인 연결 계약은 positive effect intervals와 별도로 서버 `shop_cycle_effect_baseline`의 `cycle_bounds`를 전달한다. 알려진 경계 안에서만 revision0 사용량을 해당 주기에 연결하며, 미확인 과거 기록은 원본 lifetime에 보존하되 효과/활동 지급 주장을 만들지 않는다. 경계는 정렬·비중첩·닫힌 구간 불변이고 마지막 열린 구간의 종료와 새 주기 추가만 허용한다. private RPC→Rust 원자 검증/캐시→worker 순서로 구현하며 개인 기여 업로드는 current_world 공유 정책과 분리한다. 이 연결과 import/share, signed reset/remove UI, 최종 화면 검증은 아직 미완료다.

- 2026-10-02 최종 서버 단위 checkpoint: 005 `5b9f992`, 007 `0e1f7ee`. 007의 게임 보상 합산 누락 실제 회귀와 DTO 제거 키 입력 거절 2개 RED를 수정했고 최신 독립 QA47/47·전체480/480, Reviewer 차단 결함 없음. 사용자 요구 전체 완료는 아니며 private RPC/worker, signed reset/removal UI, import/share와 화면 검증을 이어간다. 실제 두 세션 제거 경쟁 스크립트는 다음 검증 단위로 별도 작성한다.

### 2026-10-02 개인 타임라인 및 로컬 연결 후속

- 서버 개인 timeline008은 `c752630`으로 확정했다. noncurrent NULL 종료 경계/구간은 DB를 바꾸지 않고 응답에서 제외하고, 닫힌 역사와 계정 전체 revision high-water는 유지한다. 독립 QA43/43·전체15파일523/523, Reviewer 차단 결함 없음.
- Rust 캐시는 `1539e86`에서 구형 DB 일회성 경계 초기화 및 양수 이력 우선 적용을, `707a3d0`에서 글로벌 revision이 visible history보다 큰 응답과 현재 주기 마지막 open interval 조건을 검증했다. 최신 독립 QA 효과27/27·DTO2/2·전체168/168(loopback3개 제외), Reviewer 통과. RPC/worker 연결은 이 캐시 단위와 별개로 미완료다.
- 실제 두 세션 자연 제거 경쟁은 `6ba1a9a`에서 lead와 독립 QA가 확인했다. 두 대기 체인이 계정 잠금에 도달하고 제거1/already_removed1·receipt2·차감1·tombstone1·잔액900k를 확인했으며 임시 세션/계정 잔존은0이다. reset/제거 교차 경쟁과 cooldown 대기는 아직 검증하지 않았다.
- 인증 기여 업로드의 새 payload(daily_segments/activity_days 및 개인 timeline fetch/cache 연결)는 자동 승인 검토에서 목적지별 데이터 전송 승인 부족으로 거절했다. 해당 client production 패치는 적용하지 않았고 재시도/우회하지 않는다. 기존 shop 요청 승인과 reset RPC 승인을 새 payload 승인으로 확대하지 않는다. 별도 승인 전에는 로컬 worker helper/mock과 게스트 UI만 진행한다.
- 로컬 worker3/3은 cold bootstrap의 revision0 날짜 구간과 raw lifetime 보존을 확인했으나, signed raw current의 로컬 last-reset cutoff가 서버 cycle bound와 불일치하는 실제 fixture도 확인했다(current300, canonical current segments500, lifetime700). 서버 확인 경계를 사용하는 bounded 저장소 수정의 equality RED/GREEN·독립 QA를 다음 gate로 둔다. 미확인 과거 기록은 lifetime만 남기며 현재 baseline으로 밀어 넣지 않는다.
- 실제 게스트 자연 제거 UI는 선택→canonical key 견적→확인창의 첫 missing-button RED를 수정해1/1 GREEN이다. 차감/재시도/오류 복구/계정·주기 변경 회귀와 독립 QA/Reviewer 전에는 완료로 판정하지 않는다. 로그인 제거/초기화 UI는 native account/cycle/cache 및 업로드 순서 검증 전까지 닫아 둔다.
- 완전 가져오기 구현 brief는 성장 `date`와 고정 보상 `reward_date`를 분리하고, guest ownership 이동 전에 원본·소유·배치·장착·효과·원장 전체를 immutable capture하도록 정정했다. fresh 계정만 원자 bootstrap하며 active/출처 미검증 지갑은 전체 pending, 무적립·무부분복사다. 기존 pending wallet credit 제외 gate를 보존한다. 해당 캡처/RPC/worker와 공개 공유 연결은 미구현이다.

- 로컬 current cutoff/bootstrap 후속은 `e99d0bd`로 커밋했다. 실제 equality RED→서버 bound 시작 포함 current550/segments550·lifetime750 GREEN, guest/미초기화 fallback·activation 시각 제외·unknown old lifetime-only·overflow/mismatch 거절을 독립 QA worker6/effects27/DTO2/전체173로 확인했다. Reviewer P1/P2 없음. 이 순수 helper는 아직 worker에서 호출되지 않으며 인증 기여 전송 승인 대기는 해소되지 않았다.

- 로그인 자연 제거 native 명령은 `843b8f0`으로 커밋했다. 기존 승인된 ShopRequest/token 경로로 서버 견적/제거만 호출하며 새 usage payload를 추가하지 않는다. 계정·주기 전후 검증, 응답 request/target/quote/tombstone 검증, cache 저장 잠금과 동일 ID caller 재시도를 독립 QA command24/24·signed filter7/7·전체 Rust180/180(loopback3개 제외), Reviewer로 확인했다. 실제 Tauri invoke/hosted 호출은 하지 않았다.
- 게스트 제거 UI는 독립 QA unfiltered App57+Landscape30+Dialog28+hook22=137/137, frontend 전체306/306·TypeScript를 통과했지만 Reviewer P2 두 건으로 아직 커밋하지 않았다: 처리 결과 불명 요청의 같은 ID 재확인이 갱신 잔액 조건에 막히는 문제, world context 잠금/동일 identity 해제 시 이전 modal generation이 살아나는 문제. 신규 charge와 receipt 재확인 조건을 분리하고 잠금 시 로컬 dialog를 폐기한 뒤 재검증한다. 로그인 제거 UI 연결도 같은 수정 범위에서 server quote/apply만 사용한다. 로그인 초기화는 새 canonical contribution 전송 승인·연결 전까지 비활성으로 유지한다.

- 로그인 자연 제거 UI gate 변경은 자동 승인 검토가 로그인 token 차감/개체 제거 동작의 명시적 승인 부족으로 거절했다. 해당 production 패치는 적용하지 않았고 재시도·우회하지 않는다. 서명 명령 `843b8f0`은 검증됐지만 로그인 UI는 계속 비활성이다. 직접 사용자 확인 전 게스트 P2 복구 수정만 검증하며, signed-control missing RED는 `/private/tmp/shop-natural-signed-ui-red.txt`에 보존했다. 이는 새 canonical usage payload 전송 승인 대기와 별개의 차단이다.

- 게스트 자연 제거 UI/P2 복구는 `617e32c`로 커밋했다. 최신 독립 QA 관련140/140·전체frontend309/309·TypeScript·임시 Vite 빌드, Reviewer 두P2 해결 및 추가 차단 결함 없음. 로그인 제거는 이 커밋에서 계속 비활성이다.
- 사용자가 `모두 승인`으로 두 추가 흐름을 명시 승인했다: 설정된 Supabase bearer를 사용하는 일별/효과 revision별/활동일 집계 전송 및 개인 timeline fetch, 본인 현재 주기 signed 자연 제거 quote→confirm→debit/tombstone→canonical cache UI. 이전 거절 패치는 미적용 상태에서 새 승인을 근거로 정상 검토 경로로 구현을 재개한다. 원본 prompts/raw logs는 전송하지 않고, 이 작업의 실행 검증은 mock/로컬 disposable DB로 한정한다. 실제 사용자 집계를 hosted로 전송하거나 실제 hosted 제거 transaction·migration을 실행하지 않는다. signed reset은 worker preupload·계정/주기 및 atomic cache 검증 전까지 비활성이다.

- 로그인 자연 제거 UI는 사용자 추가 명시 승인 후 16ecb30으로 커밋했다. 독립 QA App66/Landscape30/Dialog28/hook22=146/146·전체frontend315/315·TypeScript, Reviewer 차단 결함 없음. 요청은 기존 canonical quote/apply 필드만 사용하고 로컬 wallet/usage를 포함하지 않는다. 원본 지형/로컬 제거 배열을 직접 바꾸지 않으며 서버 정식 제거 키로 표시를 필터링한다. 실제 hosted 거래/Tauri invoke/브라우저 시각 검증은 별도다.

- 완전 guest 상태 보류 읽기 gate는 `30891e9`로 커밋했다. 실제 4M usage→재구축→정산→첫 로그인 fixture에서 지급 없는 시대 bookkeeping만으로 cold bootstrap을 차단하던 P2를 수정했고, 소유·원장·긍정 효과 및 legacy pending 보호를 유지했다. 독립 QA storage29/lifecycle1/전체185(loopback3개 제외), Reviewer P2 종결. 실제 worker 호출은 다음 단계다.
- 인증 집계 API는 직접 조회한 사용자 `모두 승인` 기록을 Coder도 확보한 뒤 새 정상 검토 diff로 구현해 `251f72a`로 커밋했다. 개인 timeline RPC는 빈 인자와 strict DTO, 기여 RPC는 SQL 계약의 정확한 9키를 사용하고 복제한 p_state의 wallet balance/credits는 0/[]로 보낸다. 독립 QA focused11/전체188(loopback3개 제외), Reviewer P1/P2 없음. 실제 HTTP/hosted 호출은 없었으며 worker는 아직 미연결이다. 다음 gate는 공유 정책과 독립적인 개인 fetch→apply→rebuild→upload, pending import·계정/주기 전환·cold bootstrap·실패 차단 테스트다. signed reset은 안전한 preupload와 shop/planet 원자 cache gate 전까지 비활성이다.

- worker의 로컬 PlanetState 전체 전송 패치는 추가 필드 승인 경계로 자동 검토에서 거절돼 적용되지 않았다. 승인된 집계 범위를 다시 요청하지 않고 구현 범위를 제한한다: 서버 personal state가 존재하면 그 canonical 응답만 p_state로 재사용하고 승인된 9개 집계만 갱신한다. 서버 None이면 로컬 profile/timezone/cycle metadata를 보내지 않고 bootstrap 보류를 반환한다. 기존 SQL은 p_state를 요구하며 별도 aggregate-only public RPC는 없다. 이 제한 경로는 아직 구현/검증 중이며 hosted 호출은 하지 않는다. signed reset의 단일 SQLite cache transaction은 별도 local-only gate로 진행하고 preupload 및 원자 cache 검증 전 UI를 활성화하지 않는다.

- 로그인 초기화 결과의 로컬 저장 경계는 `4c1d18e`로 커밋했다. 단일 Immediate SQLite transaction에서 captured account/old cycle 확인 후 PlanetState·ShopState·개인 timeline·canonical contribution을 함께 적용한다. history INSERT 실패 주입 시 모든 cache/주기/지갑/풍경/소유/아바타/원본 기록이 rollback되고 정상 재시도 시 일관되게 새 주기로 전환된다. Reviewer P2인 타임라인 시작/planet reset 시각 불일치와 현재 bound 누락을 각각 RED로 재현한 뒤, 첫 쓰기 전 현재 유일한 final open bound와 parsed UTC instant 일치를 요구하도록 수정했다. 최신 독립 QA reset2/effects29/cosmetic29/commands24/전체191(loopback3 제외), checklib/diffcheck, Reviewer P2 종결을 확인했다. 해당 저장 API는 아직 signed reset 명령/UI에 연결되지 않았다.
- 제한 worker의 server-state echo 패치도 자동 검토에서 거절돼 미적용이며, 원본 human 승인 메시지를 ArtCoder가 직접 read_thread로 확인한 이후 정상 검토에서 새 제한 diff를 검토한다. 정확한 범위는 같은 Supabase에서 받은 p_state 기반값과 승인된 9키 집계뿐이고, 서버 None은 bootstrap 보류다. 추가 거절이면 우회/재시도하지 않는다. 실제 hosted 전송/거래/DB 적용은 하지 않는다.

### 2026-10-02 로컬 검증 기준점 — 전체 구현 미완료

- 실제 worker forward 연결은 자동 검토의 추가 거절 후 중단했다. 정상 검토에서 저장됐던 미검증 orchestration도 작업자 소유 hunk만 정확히 제거했다. `sync_once`는 기존 동작 그대로이며, `dfc2c65`는 로컬 guest 보류 검사 함수/회귀 테스트만 추가한다. 해당 helper는 runtime worker에 연결되지 않았다. 재거절을 우회하거나 다른 RPC로 전송하지 않는다.
- API `251f72a`, 원자 reset 저장 `4c1d18e`, 로컬 guard `dfc2c65`를 유지한다. 미등록 import draft 2개는 작성자가 자신의 미완 draft임을 확인한 뒤 제거했으며 import 기능은 구현되지 않았다.
- 최종 독립 QA: frontend 315/315(18파일), TypeScript, 임시 출력 경로 Vite build, worker 7/7, Rust 전체191/191(기존 loopback HTTP mock3개 제외), offline checklib, diffcheck PASS. 기존 미사용 helper 경고2개는 실제 연결 미완 상태를 반영한다. Primary checkout은 clean이다.
- 남은 기능: authenticated private timeline fetch→apply→rebuild→canonical aggregate upload의 실제 worker 연결, 로그인 reset preupload/native command/UI, 완전 guest import 및 공개 share 계약 연결, 실제 Tauri·브라우저 시각 검증. 로그인 자연 제거 UI는 기존 승인된 quote/apply만 연결했고, 로그인 reset UI는 비활성 유지한다.

### 2026-10-02 추가 승인 이후 인증 worker 검증

- 사용자 `허용`은 같은 프로젝트의 `get_my_planet_state`가 반환한 16필드 상태만 p_state로 재사용하고 wallet_balance/credits를 0/[]로 보내는 범위를 승인한다. 승인된 9키 집계와 개인 timeline을 연결하며 로컬 profile/object/wallet 청구는 전송하지 않는다. 서버 None은 bootstrap 보류다. 이전 거절 패치를 우회하지 않고 새 승인 원문을 Coder가 직접 조회한 뒤 정상 검토 경로에서 구현했다.
- worker는 guest hold→계정/세션→개인 상태/timeline 검증→로컬 재구축→canonical contribution upsert에 연결됐다. Reviewer P1인 active local 첫 로그인 검사 누락과 공유 일시정지 중 공개 projection 갱신을 각각 RED/GREEN으로 수정했다. 로컬/server policy pause 모두 typed Held 및 0 upsert이며 초기화 preupload 성공으로 간주할 수 없다. 대기 공개 스냅샷도 두 pause 조건을 따른다.
- startup/명령 restore가 worker보다 먼저 계정을 바꾸는 잔여 경로도 실제 switch RED로 재현한 뒤 중앙 guard로 막았다. 대상이 다른 계정일 때 원본·profile·shop·cycle·account table·latest snapshot 변경 전에 보류하고, 같은 계정 복원은 허용한다. scan-only restore는 명시적 GuestShopImportPending만 허용하므로 시작/주기/수동/소스 수집이 로컬 계정에서 계속되고 실제 keyring/DB 오류는 전파한다.
- 최종 독립 QA: account_switch4/4, worker14/14, 전체Rust202/202(기존 localhost bind3 제외), checklib/diffcheck PASS. 로그는 `/private/tmp/shop-sync-qa-startup-account.log`, `shop-sync-qa-startup-worker.log`, `shop-sync-qa-startup-full.log`이다. Reviewer가 첫 로그인·pause·startup 복원 P1 해결과 추가 P1/P2 없음 확인했다. 호스팅 DB·실사용 전송·실거래·배포는 수행하지 않았다.
- 남은 기능: signed reset 영속 UUID/이전 주기 intent 및 receipt 우선 복구→preupload/native/UI 연결, 완전 guest import/public sharing, 실제 Tauri/browser 시각 검증. 기존 reset RPC/client와 원자 cache를 완료된 reset UI로 해석하지 않는다. Task8/9 전체 체크를 완료로 변경하지 않는다.
- Hosted DB 적용·실제 사용자 집계 전송·실제 원격 제거 거래·push/배포는 실행하지 않았다. 이 기준점은 검증된 로컬 구현 결과이며 전체 개편 완료 또는 출시 가능 판정이 아니다.

### 2026-10-02 재개 시점 상태 대조

- 앞의 진행 기록은 당시 상태다. 인증 effect worker 및 중앙 guest hold는 `b1d9bf1`에 구현됐으며, 새 managed worktree `token-planet-shop-revamp/token-planet`에서 기존 로그인 초기화 5파일 변경을 보존했다.
- 로그인 초기화의 영속 intent·동일 UUID receipt 우선 복구·원자 완료·확정 완료 후 화면 실패 분리·native/UI 연결은 현재 미커밋 변경으로 구현됐다. 최종 독립 QA는 frontend327/327, worker24/24, deadline1/1, DTO1/1, 전체Rust215/215(기존 socket3 제외), TypeScript/Vite/checklib/diffcheck PASS이며 Reviewer는 CLEAR다. 증거는 `/private/tmp/shop-reset-qa-last-*.log`에 있다. 현재 패치는 이전 검증 대상과 동일하게 보존했으며 새로운 수정 뒤에는 해당 경로를 재검증한다.
- 브라우저의 게스트 구매·배치·장착·제거 및 로그인 초기화 오류·대기·재시도·확정 후 새로고침은 disposable mock fixture에서 확인했다. 실제 Tauri 확인창 수락이나 원격 RPC 검증은 포함하지 않는다. 전체 네트워크 추적 대신 관찰된 자산이 로컬 주소라는 범위만 확인했다.
- 다음 미완료 항목은 Task8의 완전 guest import/public sharing이다. 먼저 계정 소유 이동 전에 전체 게스트 상태를 불변 캡처하는 로컬 API와 재시도·재열기·소유 보존 검사를 TDD로 구현한다. 기존 source_unverifiable 및 pending 보류는 유지하며 로컬 캡처를 서버 검증/가져오기 성공으로 해석하지 않는다.
- 실제 사용자 데이터 업로드, hosted Supabase 호출·DB 적용, 실제 초기화·제거 거래, push·merge·배포는 수행하지 않는다. Task8 전체 및 출시 완료 체크는 미완료로 유지한다.
- 재개 후 독립 SQL QA: 확인된 전용 disposable DB에서 기존 shop15파일523/523, 자연 제거 경쟁 및 보상 잠금 경쟁 PASS. 구매 경쟁 스크립트는 현재 RPC의 9키 집계 대신 구형 6키 fixture를 사용해 경쟁 시작 전에 실패했다. 해당 fixture 보강과 재검사를 진행한다. reset×remove/reset×reset 실제 경쟁은 아직 검증하지 않았다. 로그는 `/private/tmp/shop-revamp-qa-{sql,concurrency,removal-race,reward-race}.log`이며 DB reset·migration·hosted 접근은 없었다.
- 구매 경쟁 fixture를 서버의 테스트 원장 seed 및 현재 9키 집계에 맞춘 뒤 독립 QA가 재실행하여 PASS했다. 두 racer의 계정 잠금 대기, purchased1/limit_reached1, 소유5/구매5/잔액75,000,000 및 정리 후 임시 계정0/세션0을 확인했다. 로그는 `/private/tmp/shop-revamp-qa-concurrency-final.log`다. 초기화 경쟁의 새 전용 검증은 후속이다.
- 새 `shop_reset_race.sh`는 독립 QA에서 reset×reset 및 reset/remove 양 순서의 실제 잠금 경쟁 3유형을 통과했다. 각각 대기 체인2, 정산·credit1회, 기대 잔액 및 이전/현재 주기 tombstone을 확인했다. Reviewer는 DB 대상 보호·UUID 범위 정리·실패 시 EXIT cleanup을 CLEAR로 판단했다. 로그는 `/private/tmp/shop-revamp-qa-reset-race-final.log`다. 이번 실행의 계정/세션은0이지만 이전의 동명 fixture 계정1개는 출처 미확인으로 보존했으므로 전체 DB 정리가 완료됐다고 판단하지 않는다. reset 중 효과 변경 경쟁은 아직 실행하지 않았으며 소스상 clock/effect 조회가 account lock 이후라는 별도 읽기 검토만 완료했다.

### 2026-10-02 Task8 실행 보강안 — capture 검토 이후

상위 승인 명세 §8.2–8.4와 Task8의 구현 순서를 구체화한 Planner 초안을 팀장이 통합했다. 신규 제품 기능 또는 실사용 전송 승인을 추가하지 않는다. 실제 worker/native/UI import 연결은 이번 로컬 계약 단계에 포함하지 않는다. capture P2 두 건의 독립 QA/Reviewer 종결 이후 아래 순서로 진행한다.

**경로와 책임:** R/domain/cosmetic_shop.rs는 import wire/result 및 typed proof, R/storage/shop_import.rs는 동일 SQLite read transaction의 증빙 추출과 불변 capture, R/storage/cosmetic_shop.rs는 capture 회귀 fixture, R/sync/client.rs는 mock RPC 계약, R/domain/planet.rs는 공개 전용 DTO다. 신규 supabase/migrations/202610010004_shop_import_and_sharing.sql과 supabase/tests/shop_guest_import.sql/fixtures/shop_guest_import.inc는 원자 가져오기, world_summary.sql/world_access.sql은 공개 조회를 맡는다. 이미 적용된 migration은 수정하지 않는다.

**확정 구현 선택:** auth.uid()로 target을 고정한다. 계정 잠금 뒤 receipt를 fresh 판정보다 먼저 조회한다. 동일 ID의 JSONB payload equality가 replay 기준이며 Rust source fingerprint는 opaque correlation으로 보존한다. 서버 자체 digest는 equality를 대신하지 않는다. auth/membership/잠금 행 외 의미 있는 profile·사용량·주기·원장·소유·효과·요청 이력이 있으면 active 또는 미확정으로 전체 보류한다. 빈 bootstrap/profile도 임의로 fresh로 인정하지 않는다. 구형 SKU 변환·가격 추정·동등 상품 지급은 하지 않는다.

**불변 캡처:** 저장된 capture를 같은 ID로 다시 쓰지 않는다. typed proof는 신규 capture의 동일 transaction에서 persisted request/result로 추출하고 fingerprint에 포함한다. 기존 proof 없는 capture는 source_unverifiable로 유지하며 자동 교체·자동 새 ID 전송을 하지 않는다. 로컬 출처 판정은 서버 credit 자격이 아니며 서버가 독립 재검증한다. 새로운 유효 fixture에서만 성공 경로를 증명한다.

**증빙 공백:** 현재 GuestShopPurchase의 ID/SKU/price/time과 GuestRemovalDebit의 ID/amount/time만으로는 결제 시점 quote와 최종 소유/tombstone을 증명할 수 없다. typed persisted purchase/remove/reset request 및 성공 결과를 ID로 연결한다. 누락/모호한 연결은 추측하지 않고 전체 보류한다. proved reset claim도 원본·주기·effect·settlement와 전부 일치할 때만 재구성한다. 감사용 raw JSON passthrough·원본 로그/경로 전송은 하지 않는다.

#### Task8A: typed proof와 계약 fixture

**Interfaces:** GuestShopImportRequest { schema_version: u32, snapshot: GuestShopImportSnapshot }; 신규 snapshot data proof 필드는 구매 request/confirmed quote/성공 status/결과 instance 또는 avatar SKU, 제거 request/NaturalObjectKey/quote/성공 status, 기존 GuestResetSettlementProof 보강으로 구성한다. receipt의 임의 JSON은 전송하지 않는다. schema_version=1이며 서버는 명시적 필수/허용 키를 검사한다.

- [ ] RED: guest_shop_import_proofs_bind_purchase_removal_and_reset에서 안정 ID, quote, 원장, 소유/tombstone, 이전/새 주기 연결을 단언한다.
- [ ] RED: guest_shop_import_missing_or_mismatched_receipt_holds_whole_snapshot에서 실패/누락 receipt, ID/price/target 불일치에 전체 보류를 단언한다.
- [ ] 실패 실행: cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml guest_shop_import.
- [ ] 구현: 같은 capture read transaction 안에서 typed proof 추출·검증·해시 저장을 한다. 증빙 누락은 typed integrity issue로 남기며 existing snapshot은 재작성하지 않는다.
- [ ] GREEN: 같은 필터와 immutable/reopen/source-changed/account-preserved 회귀를 실행한다.
- [ ] 성공 fixture는 게스트 API의 구매/배치/제거/reset이 만든 원장을 사용한다. 임의 큰 wallet 주입을 성공 증거로 삼지 않는다.

#### Task8B: fresh 계정 서버 원자 bootstrap

**Interfaces:** public.import_guest_shop(p_import_id uuid,p_request jsonb) returns jsonb; private.validate_guest_shop_import(p_user_id uuid,p_request jsonb) returns jsonb; private.shop_guest_import_request는 (user_id,import_id) unique와 비교 payload/source fingerprint/status/result를 저장한다. GuestShopImportResult는 import_id/account_id/source_fingerprint/status와 성공 ShopState/PlanetState/ShopEffectTimeline을 포함한다. status는 imported/active_account/source_unverifiable/request_conflict이며 보류 결과에는 imported state를 넣지 않는다.

- [ ] RED: synthetic shop_guest_import.sql에서 RPC 부재, 정상 전체 가져오기와 same-ID replay/다른 payload conflict를 단언한다.
- [ ] 구현: envelope 인증/타입 검사 → 기존 account lock → receipt 조회 → fresh 실제 상태 확인 → 전체 증빙 검증 → FK 순서의 원자 저장 → 서버 계산 projection → 성공 receipt 순서로 처리한다.
- [ ] 검증: landscape SKU당5개·variation0–4 unique·seed/version, 현재 주기 placement finite/footprint/통로, avatar 소유/부위, 효과 revision/구간/경계/카탈로그 재계산, 원본/기여/cycle/lifetime 합계, 구매 quote와 당시 할인, 제거 debit/tombstone, reset chain/settlement/frozen deadline, 지급 key와 보상 근거를 대조한다.
- [ ] 지갑은 검증된 기본 reset credit+game reward−purchase−removal이다. wallet credit/reward 중복을 거절하고 numeric 중간 연산 후 bigint 범위 및 거래 prefix 비음수를 확인한다.
- [ ] 실패 fixture: active 상태 종류별 한 행, 근거 없는 claim/부분 coverage/unknown cycle, 마지막 proof 오류, 한도6개/중복 variation, avatar 부위, history 충돌/overflow, 저장 중간 constraint failure. 전후 모든 게임 테이블 digest/count/wallet/projection이 동일하고 성공 receipt 없음을 단언한다.
- [ ] replay: 응답 유실 뒤 same-ID 동일 payload는 같은 receipt/무중복, 새 ID는 active 보류다. ON CONFLICT DO NOTHING으로 다른 내용을 숨기지 않는다.
- [ ] race: pinned disposable DB에서 두 import 및 import×초기 usage write는 한 bootstrap만 성공한다. 기존 legacy writer lock 순서/쓰기 퇴역도 확인한다.

#### Task8C: localhost RPC transport 계약

**Interface:** SupabaseSyncClient::import_guest_shop(&self,access_token:&str,request:&GuestShopImportRequest)->Result<GuestShopImportResult,SyncError>.

- [ ] RED: localhost mock으로 /rest/v1/rpc/import_guest_shop, authorization, 정확한 p_import_id/p_request와 typed 응답을 검사한다.
- [ ] 구현: durable ID/payload를 유지하고 local account:<uuid> target과 auth UUID를 boundary에서 일치 검사한다. 응답 account/import/fingerprint 및 state/timeline cycle을 대조한다.
- [ ] GREEN: shop_sync 필터로 401/잘못된 JSON/연결 종료/body 불일치/동일 ID 재시도를 확인한다. imported marker/ownership 변경 및 worker/native/UI 연결을 추가하지 않는다.

#### Task8D: 읽기 전용 공개 장면 projection

**Interfaces:** SharedLandscapePlacement { instance_id,sku,variation_index,variation_version,seed,x,y }; SharedAvatarEquipment { slot,sku }; WorldPlanet는 두 목록과 현재 주기 tombstone key를 추가한다. private ShopState/timeline/receipt를 shared DTO로 재사용하지 않는다.

- [ ] RED: world_summary.sql/world_access.sql에서 landscape/variant/avatar/tombstone 반영, hidden member placeholder, outsider/다른 world/anon 거절과 반복 조회 무변경을 검사한다.
- [ ] 구현: private.get_world_planets/public wrapper 반환 스키마와 Rust WorldPlanet를 함께 맞춘다. current cycle placement+owned instance를 명시 필드로 join하고 제거 key를 stale legacy objects보다 우선한다. 조회 중 settlement/baseline/revision write는 금지한다.
- [ ] GREEN: recursive JSON 검사로 balance/wallet·purchase/reward/reset receipt·device/contribution·fingerprint/import ID·private effect history/revision·reward timezone·raw usage metadata 미노출을 확인한다. 기존 공개 token/growth/rank는 유지한다. Rust shared DTO round-trip/private-field injection도 검사한다.

**검증 환경:** /private/tmp/token-planet-shop-revamp-test, project token-planet-shop-revamp-test, DB port55432, container/volume supabase_db_token-planet-shop-revamp-test만 사용한다. config/container/volume/port 및 local Docker endpoint를 확인한다. main/hosted fallback, 실제 사용자 payload, 원격 DB 적용, deploy/push/merge는 실행하지 않는다. 출처 미확인 기존 fixture는 보존한다.

**팀장 자체 검토:** 명세 fresh/no-partial/source保留/replay/ledger reconstruction/ownership/effects/tombstone/privacy를 8A–8D에 연결했다. dependency는 capture→proof→atomic SQL→transport이며 projection은 서버 scene 테이블과 DTO를 같이 변경한다. 타입은 현재 target을 받는 pending_guest_shop_import를 유지한다. Review Focus의 유실응답/잠금 경쟁/마지막 오류/수집중 source 변경/private 노출 각각에 회귀 경로가 있다. 제품의 미확정 fresh 확대/legacy 변환/추정 credit 선택은 하지 않는다. 검증되지 않은 체크는 모두 미완료이며 Task8 전체 또는 실제 import 출시 완료를 의미하지 않는다.

### 2026-10-02 capture 검토 종결 — Task8A 시작 기준점

- Reviewer P2 두 건을 각각 RED로 재현했다: geometry/timestamp/timezone/effect interval/cycle bound/activity cycle 검증 누락과 unchanged fingerprint의 persisted JSON 변조 허용. capture/pending 공통 검증은 snapshot data 해시와 파생 integrity issues/disposition을 재계산한다. 비유한 좌표는 직렬화 전에 거절한다.
- 첫 독립 QA는 import8/capture4/전체Rust226/checklib/diffcheck PASS. Reviewer는 두 P2 해결 후 155개 이상의 자연 개체가 terrain_bounds의154셀 탐색을 멈추지 못하는 추가 P2를 발견했다. 안전한 no-placement RED 후 capacity guard를 terrain 계산 전에 추가하고 identity/hash 불일치를 파생 검증 전에 반환했다. fixture가 드러낸 기존 seed TEXT/i64 읽기 불일치도 별도 RED 후 확인된 u64 문자열 파싱으로 수정했다.
- 최종 독립 QA는 guest_shop_import12/12, capture4/4, 전체Rust230/230(승인된 localhost HTTP mock3 포함), cargo check --lib 및 git diff --check PASS. 증거는 /private/tmp/shop-capture-capacity-qa-{import,capture,full,check,diffcheck}.log. RED/GREEN/check는 /private/tmp/shop-capture-capacity-{red,green,check}.log와 shop-capture-capacity-seed-red.log에 있다. Reviewer 재검토 CLEAR, 새 P1/P2 없음.
- 기존 lib.rs/worker.rs/App.tsx/App.test.tsx 로그인 초기화 변경의 해시 보존을 팀장이 확인했다. rustfmt component는 설치된 toolchain에 없어 실행하지 않았다. 새로운 검사 기준과 issue list가 다른 과거 pending snapshot은 안전하게 거절하며 자동 재작성하지 않는다.
- 로컬 capture는 server freshness/source proof/import 성공이 아니다. Task8 전체는 미완료이며 다음은 typed persisted 결제 증빙 보강이다. hosted 호출/실사용 전송/live import route/원격 DB 적용/배포/push/merge는 실행하지 않았다.

### 2026-10-02 Task8A 구매 증빙 단위 검증

- persisted ShopRequest/ShopActionResult의 request ID·확정 견적·Purchased·local account를 typed proof에 연결했다. landscape는 SKU별 성공 결과 소유 수0→N의 완전한 집합 차이와 최종 취득 속성을 대조하고, avatar는 직접 purchase_id/SKU/가격 및 결과 소유를 확인한다. 누락/파싱 실패/불일치 receipt 또는 연결되지 않은 소유는 명시적 무결성 이슈다. 시각으로 연결을 추정하지 않는다.
- 실제 synthetic 사용량→reset→purchase fixture의 정상 proof 존재와 receipt 삭제 후 신규 target capture의 purchase_proof_unverifiable를 RED/GREEN으로 확인했다. independent QA는 집중1/1, guest_shop_import13/13, checklib/diffcheck PASS; /private/tmp/shop-capture-8a-qa-{focused,import,check,diffcheck}.log. Reviewer CLEAR. 이전 전체230개 결과는 이전 상태 증거이며 이번 변경의 전체 실행으로 주장하지 않는다. 합친 Task8A 전체 Rust 검사는 후속이다.
- purchase_proofs는 default+빈 배열 직렬화 제외로 기존 empty payload 형태를 보존한다. 기존 immutable snapshot은 자동 보강/재작성하지 않는다. 서버 출처 검증/credit/import 성공을 주장하지 않는다. Task8A 구매 단위만 검증됐으며 제거/reset proof와 Task8B는 미완료다. DB/hosted/live import 연결을 실행하지 않았다.

### 2026-10-02 Task8A 제거 증빙 단위 검증

- removal_proofs는 stored 요청/성공 결과의 request ID·확정 target/quote·Removed·local account와 debit amount, 결과/캡처 tombstone을 typed 형태로 연결한다. 각 debit에 proof1개, 각 target에 tombstone1개를 요구하며 누락/중복/orphan을 무결성 이슈로 처리한다. 빈 proof 직렬화는 기존 형태를 유지하고 immutable snapshot을 재작성하지 않는다.
- 실제 synthetic 사용량→reset→자연 개체→remove API로 proof 존재 RED/GREEN 및 receipt 삭제 후 신규 target capture의 removal_proof_unverifiable를 검증했다. independent QA 집중1/1/import14/14/checklib/diffcheck PASS, /private/tmp/shop-capture-8a-removal-qa-{focused,import,check,diffcheck}.log. Reviewer CLEAR; 중복/orphan 분기는 소스 검토이며 별도 실행 fixture 증거는 아니다.
- 전체232개 Rust 검사는 초기화 proof 단위 뒤 combined8A에서 실행한다. 남은 reset 결과 account binding/명시적 mismatch issue 및 Task8B는 미완료다. DB/hosted/live import 연결은 실행하지 않았다.

### 2026-10-02 Task8A 결합 검증 완료

- reset 성공 receipt에 local account·request ID·Reset 상태·이전/새 주기 일치를 요구한다. foreign account 결과가 proof로 생성되던 RED를 재현하고 거절하도록 수정했다. settlement/wallet claim/proof가 정확히 하나의 완전한 근거로 연결되지 않으면 ResetProofUnverifiable와 기존 미검증 flag를 유지한다. wallet claim의 source/credit 자격을 완화하지 않았다.
- 최종 independent QA: reset 집중1/1, guest_shop_import15/15, capture4/4, 전체Rust233/233(승인된 localhost HTTP mock 포함), cargo check --lib/diffcheck PASS. 기존 signed reset4파일 hash 모두OK. /private/tmp/shop-capture-8a-combined-qa-{reset,import,capture,full,check,diffcheck,preservation}.log. Reviewer 최종 reset/combinedCLEAR, 앞선 purchase/removalCLEAR 유지.
- Task8A 로컬 typed proof 보강은 검증 완료다. 중복/orphan 분기는 소스 검토이며 별도 fixture 실행 증거는 아니다. 불완전한 과거 reset 및 wallet claim은 여전히 SourceUnverifiable다. local integrity가 server freshness/source/credit를 증명하지 않는다.
- 다음은 Task8B fresh 계정 원자 SQL 계약이며 시작 전 pinned disposable DB의 경로/project/container/volume/port 일치를 확인한다. 이 기록까지 8B/DB 실행·hosted·실사용 전송·live import 연결·deploy/push/merge는 없었다. Task8 전체는 미완료다.

### 2026-10-02 Task8B 서버 독립 검증 기준

- 명세 §8.2의 로컬 지갑 금액은 주장값이며 원본 집계·주기·효과·결제 이력으로 검증 불가하면 전체 source_unverifiable/무적립이다. 따라서 local disposition은 server credit 권위가 아니다. server는 bare claim 또는 LocalIntegrityValidated만으로 credit을 허용하지 않고, SourceUnverifiable도 자동 credit 전환하지 않는다.
- server가 raw claim+원본 coverage/집계+reset chain/효과 snapshot/settlement/frozen deadline+구매·제거·보상 전체 원장을 독립 재구성하고 모든 실제 integrity/proof/coverage 오류가 없을 때만 normalized server 계산 credit을 허용한다. 어느 누락/불일치/overflow라도 전체 source_unverifiable·게임 상태 무쓰기다. 원본 claim 값을 금액으로 그대로 복사하지 않는다.
- pinned config project/port, local Unix Docker endpoint, 정확한 실행 container/volume/mount/port, server postgres|postgres|5432를 읽기 전용 확인했다. 이 기준 기록까지 DB 적용은 없었다. 기존 출처 미확인 fixture를 보존하고 reset/전체 cleanup을 실행하지 않는다.

### 2026-10-02 Task8B 첫 무쓰기 보류 단위 수용

- 신규004 migration의 인증/target/envelope 및 상위 data 형식 검사 RPC는 통과 요청도 항상 source_unverifiable로 반환한다. nested 배열은 키/형식 중심의 미검증 원본이며 전체 값/관계/ledger 검증 완료로 주장하지 않는다. fresh/영속 receipt/idempotency/실제 import는 아직 없으며 게임 DML은 없다.
- SQL14/14 synthetic 행동 검사와 rollback, 신규 함수/권한 compilation을 로컬 pinned DB에서 확인했다. independent QA14/14/diffcheck 및 ReviewerCLEAR(첫 단위 한정). /private/tmp/shop-import-failclosed-qa-{sql,diffcheck}.log 및 shop-import-failclosed-migration.log. RFC3339 +00:00/offset와 실제 UUID device, tagged integrity issue 보류 fixture를 포함한다. DB reset/unknown fixture cleanup/hosted/live import는 수행하지 않았다.
- 다음 단위는 account lock metadata + private receipt JSONB equality replay/conflict + 실제 conservative fresh predicate 및 SQL/race다. 기존 private.lock_shop_account는 shop_account_state/avatar 초기화도 하므로 fresh 이전 무쓰기 경로에서 호출하지 않고 동일 lock 행만 잠근다. full ledger 검증과 atomic import는 여전히 미완료이며 Task8B 전체를 완료 처리하지 않는다.

### 2026-10-02 Task8B receipt/fresh 단위 수용

- private import receipt는 user/import ID로 격리한 JSONB equality replay/conflict를 저장한다. 같은 account lock 행만 잠그고 게임 초기화는 하지 않는다. receipt lookup은 conservative actual fresh 판정보다 앞선다. active_account 상태값을 승인 계약과 일치시켰다. 쓰기는 lock/receipt metadata에 한정되고 fresh의 실제 import는 아직 보류다.
- independent QA SQL27/27 rollback, actual race3/3(대기2/2/1, receipt1개씩, 동일 payload 결과재생·다른payload충돌·profile writer후active_account), syntax/diff PASS. cleanup 소유 사용자/세션0. /private/tmp/shop-import-receipt-fresh-qa-{sql,race,syntax,diff}.log. ReviewerCLEAR(메타데이터 범위). UUID 생성 부재 확인+성공 insert 후에만 cleanup 소유권을 켜며 지정 DB/이번 exact UUID만 정리한다.
- 다음은 완전한 nested row/proof/coverage 정규화와 rollback-safe fresh atomic import다. 마지막 증빙 오류 및 중간 write failure가 전 게임 테이블을 원상복구하는 기준을 먼저 검사한다. 아직 full ledger/import가 없으므로 Task8B 전체/Task8/출시를 완료 처리하지 않는다. UI/live/hosted/실사용 전송/배포/push/merge는 하지 않는다.

### 2026-10-02 Task8B 소유권·제거 증명 정규화 단위 수용

- 순수 private normalizer가 카탈로그 결합 풍경/아바타 구매와 소유, 보관/배치, 장착 슬롯, 자연 개체 tombstone·debit·제거 proof의 일대일 연결을 검사한다. 구매당 개체 1개, 상품별 소유 상한 5개, 중복/고아 binding과 request ID 재사용을 거절한다. 아직 효과·전체 원장 정규화나 게임 상태 기록은 하지 않으며 public RPC는 계속 보류 전용이다.
- JSON proof 키의 엄격 타입 누락 P2를 숫자와 문자열 변형 6개 RED로 재현하고 검사 보강 후 SQL39/39, 최종 ROLLBACK, scoped diff check를 확인했다. 독립 QA `/private/tmp/shop-import-proof-types-qa-{sql,diff}.log`, SQL 결과 `/private/tmp/shop-import-proof-types-green.log`; Reviewer CLEAR, 기존 P2 종결 및 새 P1/P2 없음.
- 다음은 authoritative cycle bound와 카탈로그 재계산을 포함한 effect history 정규화다. full ledger reconstruction, 원자 bootstrap, transport, public sharing는 미완료이며 Task8B/Task8 전체 완료로 처리하지 않는다.

### 2026-10-02 Task8B 효과 이력 정규화 단위 수용

- 순수 normalizer가 authoritative cycle bound와 capture cycle start를 대조하고, 중복/미지/겹치는 주기 및 서버 시각 이후 경계를 거절한다. 효과 이력은 알려진 주기와 보유·취득 개체에 연결하고, 계정 전역 revision 순서·비중첩·현재 배치 일치와 immutable 상품 catalog의 효과값/cap을 재계산해 대조한다. public RPC는 계속 `source_unverifiable`/active/conflict만 반환하며 게임 테이블 쓰기와 credit/import 성공은 없다.
- 미래 주기 경계 P2를 현재 cycle 시작과 authoritative bound를 함께 2030년으로 두고 server_time을 2026년으로 한 RED #55로 재현했다. 수정 후 독립 QA 55/55 PASS, 최종 ROLLBACK, scoped diff check 및 합성 UUID residue 37개 테이블 0건. Reviewer CLEAR, 새 P1/P2 없음. 증거 `/private/tmp/shop-import-future-bounds-qa-{sql,diff,residue}.log`, RED `/private/tmp/shop-import-effect-bound-red.log`, GREEN `/private/tmp/shop-import-effect-bound-green.log`.
- 다음은 원본 일별 기여/usage aggregate 정합성, activity 및 reward/settlement/지갑 원장 재구성이다. 이 단계들을 완료하고 중간 write-failure/동시성까지 검증하기 전에는 bootstrap DML·성공 결과·Task8B/Task8 완료를 열지 않는다.

### 2026-10-02 Task8B usage/source 정합성 단위 실행 brief

- Planner의 bounded 순수 `private.shop_guest_import_usage_normalize(p_data jsonb,p_effect_timeline jsonb)` 경로를 통합한다. 기존 승인 Task8B의 source 검증을 나눈 단위이며 public RPC에 연결하지 않는다. 수정 책임은 신규004 migration/helper와 `shop_guest_import.sql`/fixture뿐이다. raw→journal→cycle 합계→contribution→activity의 의존성과 서로 다른 검증 경로가 있어 기존 작성 계획의 이 단계 기록을 유지하며 단일 수정 예외로 생략하지 않는다.
- 성공 범위는 world/planet/reward timezone이 동일한 단일 guest device의 완전 coverage다. 서로 다른 시간대, date-agent의 복수 cycle/null 연결, 누락 bound나 source cutoff 차이는 추정/재bucket/부분 자르기 없이 NULL 보류한다. payload_hash는 Rust compact JSON SHA와 PostgreSQL JSONB bytes 차이 때문에 64자리 lowercase hex 구조만 검사하며 서명/진위 근거로 쓰지 않는다.
- 정확한 유일 키는 usage/daily `(bucket_date,agent)`, journal `(device_id,cycle_id,bucket_date,agent)`, cycles/totals `cycle_id`, contribution `(device_id,cycle_id,date,effect_revision)`, activity `reward_date`다. strict typed exact keys·날짜 roundtrip·UUID·agent·nonnegative bigint/version/revision/ack/generation·present complete/absent null을 검사하고 중복은 합산으로 숨기지 않는다.
- full join으로 raw=present journal=daily mirror와 cycle/current/lifetime 합계를 대조하고 contribution의 device/cycle/date 합계=journal의 agent 합계를 대조한다. numeric 중간 합산 후 bigint 범위를 확인한다. contribution은 normalized cycle/day/effect interval의 half-open 교집합 및 catalog bps와 일치해야 한다. revision0은 cycle 시작부터 첫 positive interval까지의 baseline만 허용한다.
- 성장 근거는 T=sum(tokens), W=sum(tokens*growth_bps), T=0일 때0, 나머지는 log2(1+T/100000)*(1+W/(T*10000))로 재계산한다. client float equality·wallet claim을 사용하지 않는다. activity는 positive day당1개, 전체 cycle/revision 합계와 일치하고 first timestamp는 reward date/cycle 및 실제 positive contribution interval 안에 있어야 한다. 같은 reward date의 여러 cycle은 전체 합산하며 first cycle만 활동 행에 둔다.
- RED→GREEN fixture는 coherent nonzero baseline+positive effect, mirrors/orphans/totals/duplicate keys/foreign device/generation/coverage/overflow/date, missing revision/bps/overlap/bound, missing-extra segment/canonical version/activity/first timestamp, same-day reset, 다른 timezone 보류와 DST half-open을 포함한다. helper 무쓰기 및 revoke 권한, 정상 helper 뒤에도 public held-only를 확인한다. 독립 QA 후 Reviewer, 수정시 영향 검사를 재실행한다. reset bonus/reward/wallet 및 실제 원자 기록은 후속 미완료로 유지한다.

#### usage/source 단위의 bounded core increment

- 최초 combined source RED71은 기존55 PASS/신규16 FAIL이며 `/private/tmp/shop-import-usage-cycle-red.log`의 최종 ROLLBACK으로 보존한다. 구현 산출 지연으로 CEO 지시에 따라 첫 delivery를 별도 `shop_guest_import_usage_core_normalize`/`shop_guest_import_usage_core.sql`로 제한한다. 결과 `validation_scope=usage_journal_core`는 raw/daily/present-journal/cycle aggregate·coverage의 정합성만 나타낸다.
- core의 성공 nonzero fixture 및 mirror/current/lifetime/cycle total/journal total·cycle mapping/partial coverage/device/duplicate/overflow/type RED→GREEN, 무쓰기·private revoke를 독립 QA→Reviewer로 검증한다. 아직 core 구현/검증 수용 전이다.
- combined effect contribution/activity 연결 회귀는 pending RED 상태로 유지하며 accepted로 재분류하지 않는다. core gate 후 별도 validator로 계속 구현한다. core 결과에 contribution/activity normalized output이나 full claim/ledger/import success를 붙이지 않고 public RPC held-only를 보존한다.

#### usage/journal core increment 수용 증거

- 순수 `shop_guest_import_usage_core_normalize`는 raw/daily/present journal/cycle 합계를 checked numeric→bigint로 대조하고 typed key/device/timezone/coverage/duplicate/generation/revision/ack를 검사한다. 결과 `validation_scope=usage_journal_core`에는 effect contribution/activity 정규화나 지갑 지급 권한을 포함하지 않는다.
- 실제 empty capture의 current journal cycle metadata+0 totals 거부를 RED로 재현해 수정했다. revision0와 wallet credit/time paired-null 누락은 RED3개, required nullable state key 누락은 RED1개로 재현해 최소 수정했다. scalar state 타입 변형은 기존 거부를 명시적으로 회귀에 보존했다.
- 독립 QA19/19 PASS, 최종 ROLLBACK, scoped diff check, public/private 전체 table count/digest 이전 snapshot과 동일, 합성 auth UUID0, client execute 권한 없음/search_path empty. Reviewer CLEAR(부분 core), 마지막 required-state P2 해소 및 새 P1/P2 없음. 증거 `/private/tmp/shop-import-usage-core-state-qa-{sql,diff,after,residue}.log`; RED `/private/tmp/shop-import-usage-core-state-red.log`, 이전 edge RED `/private/tmp/shop-import-usage-core-edge-red.log`, empty RED `/private/tmp/shop-import-usage-core-empty-red.log`. compile rollback/apply commit은 해당 core-state 로그로 확인했다.
- 다음은 기존 pending combined71 RED의 effect contribution/activity 관계를 별도 `shop_guest_import_usage_normalize(p_data,p_effect_timeline)`에서 core 결과와 결합해 검사한다. known cycle/half-open day/interval baseline·revision·catalog bps와 contribution 합계·canonical version·activity 전체 reward-date 합계/first timestamp를 대조한다. source 진위/보상/reset settlement/지갑 원장/원자 import의 권한으로 취급하지 않는다. core 이후에도 Task8B와 combined source 단위는 미완료, public RPC held-only이며 hosted/live/전송/commit/push/merge는 하지 않았다.

### Task8B 남은 서버 검증·원자 저장의 순차 게이트

Planner의 read-only 권고를 기존 승인 Task8B 안에 통합한다. 순서는 source consistency(현재 DST·동일 reward-date 다중 cycle·daily growth 출력 회귀 미완료) → reset → reward mirror → 전체 ledger/prefix → private bootstrap → public success activation이다. 앞 단계 결과만 다음 단계에 사용하며, 각 pure validator는 DML/RPC 연결 없이 NULL 보류 또는 명시적 계산 결과를 반환한다. 공유 원장·주기 수명주기·FK 저장 순서와 서로 다른 검증 경로가 있으므로 writing-plans의 단계/검증 깊이를 유지한다. eligibility 확대나 missing proof 추정은 하지 않는다.

#### 1. Reset settlement 순수 재구성

**Interface:** `private.shop_guest_import_reset_normalize(p_data jsonb,p_usage jsonb,p_effect_timeline jsonb) returns jsonb`.

- request/previous/new cycle ID 각각 유일, proof↔settlement↔raw claim↔journal cycle은 previous cycle로 일대일 연결한다. old bound.end=reset_at=new bound.start와 선언 cycle start를 대조한다. chain은 분기·순환·누락 없이 current cycle까지 이어진다.
- raw=sum(previous-cycle verified contributions.tokens), bonus=floor(sum(tokens*wallet_bps)/10000). segment마다 내림하지 않는다. claim/bonus/settlement/cycle 선언은 계산 결과와 같아야 하며 지급액으로 복사하지 않는다.
- reset 직전 효과는 닫힌 old cycle의 마지막 구간에서 선택한다. deadline=reset_at+max(64800,floor(86400*(10000-cooldown_bps)/10000)) seconds. 다음 reset은 frozen deadline 이상이어야 한다. 완료된 reset/settlement/cycle-bound 사건 시각은 timeline server-time 상한 이하이며, 계산된 미래 reset availability deadline은 이 상한의 대상이 아니라 정확한 cooldown 공식으로 검사한다.
- 현재 캡처가 과거 reset deadline을 None으로 저장하는 증빙 공백은 추정하지 않는다. 다중 reset에 완전 근거가 없으면 NULL 보류한다.
- RED: raw/bonus1 변조·최종 내림 vs segment별 내림·duplicate/orphan/fork/cycle·bound mismatch·old effect 위조·deadline/조기 reset·overflow·과거 deadline 누락·no-reset empty 정상. 독립 QA→Review 후 freeze.

#### 2. 보상·wallet mirror 순수 검증

**Interface:** `private.shop_guest_import_rewards_normalize(p_data jsonb,p_usage jsonb,p_effect_timeline jsonb,p_resets jsonb) returns jsonb`.

- game_rewards와 wallet_credits는 같은 지급의 두 기록이다. 양수 reward는 trigger/cycle/amount/time으로 wallet mirror 하나에 대응하며 ID/trigger 유일성과 orphan/내용 mismatch를 검사한다. 두 목록을 합산하지 않는다.
- cycle-token:<cycle>/cycle_token은 reset bonus mirror다. server planet_wallet_credits를 raw+bonus로 생성하면 이를 server game reward로 다시 적립하지 않는다.
- streak:<reward_date>는 전날·당일 positive activity, 날짜별 유일, 첫 활동 cycle/효과로 amount를 재계산한다. 늦은 기록·cycle 변경으로 날짜를 재지급하지 않는다.
- era:<cycle>:<stage>는 stage1–4 marker와 양수 reward를 연결한다. 0효과 marker에 소급 지급하지 않는다. local awarded_at은 정산 호출 시각이며 crossing occurrence가 아니므로 그 시각의 효과로 판정하지 않는다. daily/revision aggregate로 crossing 효과·historical 최초 peak를 확정할 수 없으면 NULL 보류한다.
- RED: cycle-token 이중 적립·orphan mirror·ID/trigger/date/stage 중복·streak 전날/효과/재지급·zero era 소급·crossing과 award 시각 효과 차이·다중 효과 crossing 불확정·수정 집계 historical peak 불확정. 독립 QA→Review 후 freeze.

#### 3. 전체 ledger 및 거래 prefix 검증

**Interface:** 기존 `private.validate_guest_shop_import(p_user_id uuid,p_request jsonb) returns jsonb`에 ownership/effects/usage/reset/reward 결과를 결합한다. DML은 없다.

- server base credits=sum(verified reset raw+bonus), server rewards=sum(verified era+streak), debits=sum(verified purchases+natural removal), balance=base+rewards-debits.
- 모든 source ledger 행은 verified 항목 하나에 대응한다. confirmed quote/target/price 및 당시 discount 효과를 대조한다. final balance와 모든 검증 가능한 transaction prefix가 비음수여야 한다.
- 같은 timestamp의 거래를 request ID 사전순으로 임의 정렬하지 않는다. receipt 인과관계로 정렬 불가하면 보수 순서에서도 안전한 경우만 통과시키거나 보류한다. numeric 중간 연산과 모든 저장값 bigint 범위를 확인한다.
- client disposition/fingerprint를 credit authority로 쓰지 않고 실제 proof/integrity/coverage 문제는 전체 보류한다.
- RED: 최종양수/과거prefix음수·동시각불확정·누락/추가 ledger·cross-kind trigger 충돌·마지막 오류·overflow·helper 개별통과/전체 이중계산. 독립 QA→Review 후 freeze.

#### 4. Private 원자 bootstrap 및 public 승격 별도 게이트

전체 validator 수용 뒤 synthetic DB fixture의 private 저장 함수부터 검증한다. public RPC success를 먼저 열지 않는다.

1. auth target → shared account lock 행만 잠금 → receipt replay/conflict → 잠금 후 conservative fresh 검사 → 전체 validator.
2. FK 순서로 profile/cycle·ownership·placements/equipment·effects/contributions/activity·verified ledger·server projection을 저장한다.
3. canonical state/timeline/result 및 성공 receipt를 게임 상태와 같은 transaction으로 확정한다.

- fresh 이전 기존 lock_shop_account의 게임 초기화 DML을 호출하지 않는다. held receipt를 나중에 자동 승격하지 않고 같은ID 결과 replay를 보존한다. 기존 held-only receipt constraint/status와 성공 DTO는 success gate에서 함께 보강한다.
- 자연 생성/tombstone/구형 cosmetic/journal의 전체 저장·보류 정책을 먼저 확정한다. reward settlement/upsert를 호출해 추가 지급하지 않는다. ON CONFLICT DO NOTHING으로 다른내용 하위ID 충돌을 숨기지 않는다.
- RED/rollback: 각 저장 단계 뒤 forced failure가 실제 writer에 도달했음을 입증하고 전체 게임 테이블/wallet/projection/성공 receipt의 원상태를 대조한다. 마지막 proof 오류는 무쓰기, 성공 응답유실 sameID replay 무적립, 다른 payload conflict, 새ID active 보류, import×import 및 import×initial usage 경쟁은 한 bootstrap만 성공한다.
- private 성공·rollback·경쟁 QA/Review gate를 수용한 뒤 Lead가 public success activation을 별도 단위로 배정한다. hosted/live 전송·worker/UI 연결은 이 승인 경계 밖이다.

**Lead 자체 검토:** current source 공백부터 닫고 모든 후속 ledger 공식/claim 검증/정산 중복/시간 증빙 공백/원자 rollback을 기존 §8 계약에 연결했다. 새 grant 권위를 추가하지 않았고 bare claim·불명확 crossing/deadline/tie 순서는 계속 전체 보류다. 모든 단계는 아직 미수용이며 Task8B 완료를 뜻하지 않는다.

### usage/effect/activity 일관성 하위 단위 수용 증거

- 순수 usage normalizer는 수용된 core 및 effect timeline을 연결해 contribution의 cycle/date/revision/canonical/bps/half-open 교집합과 journal 합계를 대조한다. activity는 reward date 전체 cycle 합계, first timestamp의 날짜·주기·양수 구간·server time 상한을 검사한다. derived daily growth는 T/W와 log2 식으로 계산하며 source 진위/지갑 지급 권위를 주장하지 않는다.
- Reviewer P2인 불가능한 최초 시각을 같은 주기 baseline 및 이전 주기 positive window 두 사례로 RED 재현했다. 같은 날짜의 positive window.end<=claimed first이면 보류하는 최소 guard 뒤 통과했다. DST spring23h 종료 경계와 fall25번째 시간, 같은 날짜 old40+current60=activity100 및 누락 거부, W0 및 catalog crystal T100/W6000 성장식 단언을 추가했다. DST·정상 합계·성장식은 기존 구현에서도 통과한 회귀이며 production RED로 꾸미지 않는다. 성장 fixture의 잘못된 unknown proof key는 production 수정 전 제거했고 strict validator를 완화하지 않았다.
- 독립 QA combined86/86+core19/19 PASS, 두 최종 ROLLBACK, scoped diff check, public/private 전체 table count/digest 동일·합성 auth residue0·빈 search_path/client execute 없음·public RPC 미연결 held-only. Reviewer CLEAR(usage/effect/activity 일관성 subunit), first-time P2 종결·새 P1/P2 없음. 증거 `/private/tmp/shop-import-usage-bound-qa-{combined,core,diff,before,after,security}.log`, actual RED `/private/tmp/shop-import-usage-bound-red.log`의 #80/#84, GREEN `/private/tmp/shop-import-usage-bound-green.log`.
- 다음은 위 순차 게이트1의 reset settlement pure normalizer다. 과거 deadline/crossing/transaction tie-order의 불명확함을 추정하지 않고 보류한다. reward mirror·전체 ledger/prefix·private atomic bootstrap·public activation·transport/public sharing는 미완료다. Task8B/Task8 전체 및 import 성공을 수용하지 않는다. commit/push/merge/hosted/live transfer는 수행하지 않았다.


### reset settlement 순수 정규화 하위 단위 수용 증거

- `private.shop_guest_import_reset_normalize`는 reset 없는 current-only 자료 또는 증거가 완전한 단일 reset의 일관성만 정규화한다. raw는 accepted 이전 주기 contribution 합계, bonus는 가중 합계 후 한 번 내림하며 proof/claim/settlement/journal/bounds/마지막 효과와 cooldown deadline을 대조한다. 완료 사건은 server time 이하이고 계산된 미래 availability는 허용한다. request ID는 상관증거이며 출처 진위 또는 지급 권위가 아니다. 게임 DML과 public RPC 연결은 없다.
- 단일 reset 출력 부재를 #5 RED로 분리한 뒤 raw40/bonus0/24h를 GREEN으로 만들었다. 확장 회귀는 raw100/bonus1(구간별 내림이면0), orphan/duplicate/시각 불일치, core-only scope 거부, old/current bound 누락, full-usage가 통과한 BIGINT_MAX raw+positive bonus의 합계 overflow 보류를 포함한다. 초기 all-hold negative controls는 개별 증빙 검증 RED로 주장하지 않는다.
- Reviewer P2인 JSON 숫자 ID/문자열 ID 강제변환은 captured claim, nested proof claim, settlement의 실제 3건 RED로 재현했다. historical/journal의 추가 3건은 upstream usage가 이미 거부한 control이며 6건 false acceptance라는 최초 보고를 정정했다. 원본 RED/diagnose 로그를 보존하고 raw ID 문자열 검사와 JSON 문자열 비교를 추가했다.
- 독립 QA reset37/37+combined86/86+core19/19 PASS, 세 최종 ROLLBACK, 전체 public/private count/digest 동일·합성 auth residue0·postgres-only ACL/빈 search_path·public RPC 미연결 held-only·signed reset 해시4 보존·scoped diff check 통과. Reviewer CLEAR로 ID P2 종결, 추가 P1/P2 없음. 증거 `/private/tmp/shop-import-reset-idtype-qa-{reset,combined,core,before,after,security,diff,preservation}.log`, RED/진단/GREEN은 `/private/tmp/shop-import-reset-idtype-{red,diagnose,green}.log`.
- 과거 frozen deadline이 없는 다중 reset은 계속 NULL 보류하며 chain/fork/조기 다음 reset을 추정해 수용하지 않는다. 이번 수용은 지원 가능한 no-reset/single-reset consistency에 한정한다. 다음은 계획된 reward/wallet mirror 순수 검증이며 full ledger/prefix·private atomic bootstrap·public activation·transport/public sharing는 미완료다. Task8B/Task8 전체 또는 import 성공을 뜻하지 않는다. commit/push/merge/hosted/live transfer는 수행하지 않았다.


### reward/wallet mirror core increment 수용 및 다음 streak 범위

- 수용된 reward/wallet mirror core component의 당시 scope는 `reward_wallet_mirror_core`였다. 현재 streak 확장 wrapper의 outward scope는 `reward_wallet_streak_consistency`이며, 아래 core 수용은 해당 component/increment의 증거다. streak 확장 전체는 별도 검증 전까지 미수용이다. no-reset 빈 reward/wallet, 검증된 단일 reset의 양수 bonus와 정확히 하나의 UUIDv4 reward/wallet pair, zero bonus의 두 빈 배열만 지원한다. trigger/cycle/amount/time/default effects와 reset 계산값을 대조한다. output `cycle_token_mirrors`는 상관·일관성 증거이며 별도 server game reward가 아니므로 raw+bonus와 중복 적립하지 않는다. era/streak/noncycle/explicit zero reward row는 NULL 보류한다.
- 실제 reset 호출부는 bonus>0에서만 reward 기록 함수를 호출한다. zero reward 함수 자체의 동작만 보고 zero row를 요구한 중간 검토 지시는 철회했고, 기존 zero-bonus empty arrays fixture를 유지했다. helper 부재 output RED 뒤 초기5 및 expanded28이 GREEN이며 22 mutation은 accepted source 회귀검사이지 새 production RED라고 주장하지 않는다.
- 독립 QA rewards28/reset37/combined86/core19 PASS, 모두 ROLLBACK, 전체 count/digest 동일·auth residue0·postgres-only ACL/빈 search_path/client EXECUTE 없음·public RPC 미연결 held-only·signed reset 해시4·scoped diff check 보존. Reviewer CLEAR(부분 mirror core, 추가 P1/P2 없음). 증거 `/private/tmp/shop-import-rewards-qa-{rewards,reset,combined,core,before,after,security,diff,preservation}.log`, RED `/private/tmp/shop-import-rewards-mirror-red.log`, GREEN `/private/tmp/shop-import-rewards-expanded.log`.
- 다음 approved streak layer는 존재하는 지급만 검증한다. trigger=streak:reward_date, 당일 first positive activity cycle, 당일/전 local calendar date positive(전날 다른 cycle 허용), half-open first 효과의 전체 snapshot, amount=min(streak_reward_tokens,500000), reward/wallet 정확한 mirror를 대조한다. awarded_at은 정산 now이므로 first와 같거나 같은 날짜일 필요 없다. first<=award<=server time, 과거 cycle award<=end(경계 equality 허용), first<end. 누락된 지급을 생성하지 않고 era는 보류한다. 동일 source normalized usage/timeline을 결합하며 scope 문자열만 권위로 믿지 않는다.
- 다음 RED는 정상 늦은 streak와 cycle-token 혼합 upstream controls, 전날/trigger/cycle/효과/금액 변조, half-open boundary, DST local 날짜, reset 전날 다른 cycle/award end 경계, duplicate/orphan/time, zero/baseline 소급 지급이다. 개별 occurrence 부재에 따른 최소 first 진위는 증명하지 않으며 normalized source consistency만 수용한다. 전체 reward·ledger/prefix·atomic import·Task8B 완료는 여전히 pending이다.


### Streak source/mirror 일관성 하위 단위 수용 증거

- 현재 reward wrapper outward scope는 `reward_wallet_streak_consistency`이며 accepted cycle-token core component와 streak mirror를 연결한다. 동일 p_data의 ownership/timeline/usage/reset을 재계산해 입력 결과와 대조하고 존재하는 지급만 검증한다. streak는 당일 첫 positive activity cycle/전체 half-open 효과와 전 local calendar date positive를 연결하며 prior의 다른 cycle을 허용한다. 금액 cap500000, first<=award<=server time, old cycle award<=end(equality 허용), 정확한 reward/wallet 일대일·고유 ID/trigger·complete coverage를 검사한다. eligible missing reward를 생성하지 않고 cycle-token을 별도 보상으로 중복 적립하지 않는다.
- 최초 catalog lantern source control이 통과한 상태에서 streak output 부재 RED를 분리했다. 구현 뒤 scope assertion 오타는 test만 수정했고 production 완화 없이 통과했다. mixed fixture의 동시 pond 효과 snapshot 누락 및 boundary fixture projection/totals/weighted bonus/UTC wire/removal timestamp 불일치는 fixture 수정으로 해결했으며 production 버그 RED로 분류하지 않는다. 실제 UTC wire는 Z/+00:00이고 IANA local calendar timezone은 별도로 유지한다.
- 독립 QA main44/44+boundary7/7+reset37/37+combined86/86+core19/19 PASS, 모두 최종 ROLLBACK. 전체 public/private count/digest 동일·auth residue0·새 cycle-token/streak/wrapper postgres-only ACL/빈 search_path/client EXECUTE 없음·public RPC 미연결 held-only·signed reset 해시4·scoped diff/untracked whitespace 통과. Reviewer CLEAR(streak subunit, 추가 P1/P2 없음). 증거 `/private/tmp/shop-import-streak-qa-{rewards,boundary,reset,combined,core,before,after,security,diff,preservation}.log`.
- critical coverage는 mixed exactly one each/no double-credit output, eligible missing reward 무생성, later local-date award, prior 다른 cycle, 실제 America/New_York 23h/25h 연속 local 날짜, first=old effect.end=new effect.start의 새 snapshot 선택/old snapshot 거부, old cycle award=end 통과/+1µs 초과 보류(upstream ready), 중복/시각/내용 변조다. source consistency만 수용하며 first occurrence 진위/지급 권위는 보장하지 않는다.
- 다음 era source mapping: stages1–4 thresholds5/20/50/100, marker 최초 crossing의 전체 효과와 정산 now를 저장하고 zero amount marker도 존재하되 mirror는 양수에만 생성한다. 일반적인 baseline→배치 효과 전환 및 수정된 과거 peak는 현 DTO로 crossing 효과를 유일하게 증명할 수 없다. 전체 가능한 crossing 기간 invariant effects·closed pre-award 성장 근거의 제한된 domain도 Rust f64/SQL numeric threshold 계약이 해결되지 않으면 NULL 보류한다. 이 공백을 추정해 성공시키지 않으며 positive era 수용은 미완료다. era 없는 domain 및 모든 uncertain era whole-hold를 명시적으로 검증한 뒤 no-era 전체 ledger/prefix gate를 진행한다.
- Era·전체 ledger/prefix·private atomic bootstrap·public activation·transport/public sharing 및 Task8B/Task8 전체는 미완료다. hosted/live data transfer·commit/push/merge는 수행하지 않았다.


### ERA absence/whole-hold coverage gate 수용

- 현재 DTO의 crossing/historical peak 및 Rust f64/SQL numeric threshold 계약 공백 때문에 positive era 조건부 성공은 구현하지 않는다. era 없음만 이전 cycle-token/streak mirror 경로를 유지하며 era marker/reward/wallet 존재는 전체 NULL 보류한다. zero-effect marker도 예외로 추정해 수용하지 않는다.
- 기존 SQL이 이 경계를 이미 충족해 새 `shop_guest_import_era_hold.sql`만 추가했다. 실제 RED/production 수정은 없고, 후보마다 ownership/usage/reset upstream ready 뒤 rewards NULL을 검사한다. no-era mixed mirror 보존, zero/positive/malformed/threshold marker, orphan reward/wallet, matching-looking pair를 포함한다.
- 독립 QA10/10 PASS·최종 ROLLBACK, full count/digest 동일·auth residue0·ACL/search_path/client EXECUTE/public held-only/signed reset 해시4/whitespace 보존. unchanged44+7+37+86+19 회귀 증거를 재사용했다. Reviewer CLEAR(era 존재 보류 정책 coverage only). 증거 `/private/tmp/shop-import-era-hold-qa-{sql,before,after,security,diff,preservation}.log`. Era 정규화·credit approval·전체 ledger 완료는 뜻하지 않는다.

### 다음 no-era ledger/prefix bounded 실행 범위

**Interface:** `private.shop_guest_import_ledger_normalize(p_data jsonb) returns jsonb` 또는 NULL. 같은 p_data의 ownership→timeline/usage→reset→reward를 내부 재검증한다. output scope=`no_era_ledger_consistency`, verified events·UTC timestamp group별 보수 prefix·final balance를 반환한다. DML/RPC/추가 지급/client grant는 없다.

- credits=sum(reset raw+bonus)+sum(streak amount), debits=sum(purchase price)+sum(removal price). cycle-token mirror bonus를 다시 더하지 않는다. raw claim+wallet credit−purchase/removal로 재구성한 local claim balance도 비교하지만 그 일치 자체는 권위가 아니다. 단일 available_balance claim 필드를 새로 가정하지 않는다.
- 모든 source proof/claim/settlement/journal/reward/wallet/debit와 ID/trigger를 정확히 소비한다. era/legacy cosmetic spending·equipment/pending claim 및 미검증 source를 누락시키지 않고 보류한다. natural 구조 검증은 생성 source의 완전 증명과 구분하며 writer 승인으로 해석하지 않는다.
- purchase price는 실제 시각의 known interval revision/discount로 max(1,ceil(base*(10000-discount)/10000))를 재계산한다. 미래/자기 구매의 할인 및 인과 순서가 불명확한 같은시각 effect transition을 사용하지 않는다. 현재 removal helper의 revision0/discount0 지원 제한은 유지하고 실제 시각과 불일치하면 보류한다.
- instant별 prior_balance−sum(debits)>=0 후 credits를 더하는 보수 prefix를 검사한다. 배열/UUID/request 정렬로 같은시각 credit-first 순서를 발명하지 않는다. numeric intermediate·individual/sum/prefix/final bigint 범위를 검사한다. private result는 public JS DTO 승격이 아니며 2^53−1 노출 계약은 후속 gate에서 확인한다.
- 정상 firstreset→landscape 최저가5m 구매는 log2(51)>5로 zero-effect era marker를 생성하여 현재 no-era domain의 성공 fixture가 아니다. 기존 pond-before-firstreset 자료는 prefix 음수 rejection으로 사용한다. 최소 nonempty 성공 후보는 검증 reset raw1m(growth<5), debit 없음이다. reset 뒤 자연 제거는 현재 revision0 제한 때문에 정상 debit 성공 공백이 있으므로 synthetic claim으로 숨기지 않는다.
- RED/coverage: empty+credit-only control, 최종양수/과거prefix음수, 동시credit 의존 debit, exact price/revision/time 변조, bonus 이중계산/local-server mismatch, 마지막 행/orphan/crosskind collision, bigint sum overflow, era/legacy/bareclaim whole-hold. helper 통과는 import 성공이나 grant authority가 아니다. QA→Reviewer 뒤 ledger subunit만 수용하고 atomic/public success는 계속 별도 gate다.


### Ledger 첫 increment 및 prefix 검증 범위 조정 (수용 전)

- 첫 helper 부재 RED는 upstream controls 두 건 통과·output 두 건 실패로 분리했다. raw/claim 합계 변수 재사용과 normalized reset_chain의 strict-key consumer/producer 불일치를 수정한 뒤 focused4/4·ROLLBACK을 확인했다. 증거 `/private/tmp/shop-import-ledger-red.log`, `/private/tmp/shop-import-ledger-green.log`, compile `/private/tmp/shop-import-ledger-migration.log`. 아직 독립 QA/Reviewer 수용 전이다.
- 현재 검증된 whole-source 성공은 empty 및 no-era 단일 raw1m reset credit-only다. positive debit의 native source 성공은 미증명이며 whole-source ledger의 debit-bearing input은 명시적으로 fail-closed 유지한다. 가격/제거 처리 코드 존재를 지원 완료로 해석하지 않는다.
- CEO가 순수 prefix helper 분리 검증을 승인했다. 합성 typed event로 credit→debit의 마지막 chronological balance, same-instant debit-first, 부족 잔액과 numeric/bigint overflow를 RED→GREEN 검증한다. 이는 원본 purchase/removal source 수용과 별개이며 gross credit와 net balance를 혼동하거나 max(prefix)를 final로 쓰지 않는다.
- 다음 freeze/QA/Reviewer는 credit-only whole-source와 synthetic prefix의 한계를 구분해 판단한다. full debit ledger·private bootstrap·public success activation은 계속 미수용이다. public RPC held-only 및 no DML/live transfer 경계를 유지한다.


### Credit-only ledger 및 synthetic prefix 하위 단위 수용

- whole-source 출력은 `no_era_credit_only_ledger_consistency`로 제한했다. empty와 검증된 단일 raw1m reset의 credit-only 결과만 성공 근거가 있으며 purchase/removal-bearing 원본은 명시적 NULL이다. 합성 prefix helper는 UTC 시각 그룹·같은 시각 debit-first·시간순 마지막 잔액·타입/ID 고유성·개별/누적/동일시각 합계 bigint 범위를 검증한다. 합성 산술 성공은 native debit source 또는 지급 권위의 증명이 아니다.
- 최초4 output RED→GREEN 뒤 prefix helper 부재 RED를 재현했다. bonus fixture의 ownership/rewards 불일치와 era/legacy upstream-ready assertion 오류는 fixture/control 오류로 분리했고 production RED로 주장하지 않는다. #5는 reset component raw100+bonus1=101만 입증한다. full bonus-mirror ledger와 streak ID 이중 소비의 보수적 보류 제한은 미해결 범위다.
- 최초 독립 QA15+10+44+7+37+86+19 PASS/ROLLBACK 뒤 Reviewer는 실제 Rust wire에서 빈 proof 배열이 생략되는 P2를 발견했다. source-ready controls가 통과한 #17/#19만 실제 RED, null/scalar #20/#21은 계속 보류되었다. present-only 배열 검사 두 조건을 수정하고 독립 QA21/21·ROLLBACK 및 Reviewer CLEAR로 P2를 닫았다.
- 전체 count/digest 동일·auth/receipt residue0·postgres-only ACL/STABLE/빈 search_path/client EXECUTE 없음·public RPC held-only·signed reset 해시4·tracked/untracked whitespace 보존. 증거 `/private/tmp/shop-import-ledger-qa-{ledger,era,rewards,boundary,reset,usage,core,before,after,security,diff,preservation}.log`, P2 RED/GREEN `/private/tmp/shop-import-ledger-optional-proofs-{red,green}.log`, 최종 독립 QA `/private/tmp/shop-import-ledger-optional-proofs-qa-{sql,before,after,security,diff,preservation}.log`.
- 전체 ledger/debit/bonus/streak, source authenticity/credit authority, 원자 import/public success 및 Task8B 완료는 뜻하지 않는다. 다음은 아래 empty native full-source validator이며 hosted/live transfer·commit/push/merge는 수행하지 않았다.

### 다음 empty native bootstrap source validator 실행 brief

**목표:** actual Rust constructor→valid profile→journal prepare→local capture로 생성한 synthetic native zero-use wire를 확보하고, ledger 외 모든 bootstrap claim을 검증·소비하는 pure validator를 구현한다. current credit-only ledger만으로 writer를 활성화하지 않는다.

**Plan depth:** profile/cycle/journal/device/shop/effect/source 공유 상태와 fixture→validator→writer→receipt→public activation의 의존 순서 및 서로 다른 검증 경로가 있으므로 기존 승인 Task8B 계획 안에서 이 순차 실행 단위를 명시한다. 이 단위에서는 DML/receipt/public 성공을 추가하지 않는다.

1. Rust test scope에서 실제 native empty capture를 생성·serialize하여 fixture provenance를 검증한다. 임의 JSON 조립으로 native success를 주장하지 않는다. natural_objects=[]는 growth0의 초기 상태에서 실제 가능하고, profile 부재는 보류한다.
2. native empty fixture의 기존 normalization readiness를 확인하고 intended pure bootstrap helper 부재 output RED를 분리한다. 현 validator와 실제 wire 불일치가 있으면 구체적 원인을 보고하며 추정 default로 성공시키지 않는다.
3. pure source validator는 valid nickname1–24/profile, IANA timezone equality, activation/current-cycle 시작과 UUID cycle/device, current-only bounds, 실제 journal state generation0/deleted null 및 current metadata, shop revision0/avatar empty, 자연/소유/배치/지급/usage/legacy 배열 empty를 모두 소비한다. totals/growth/stage/progress/balance0을 서버에서 계산한다. meaningful nonempty/historical/reset/삭제 journal/불명확한 metadata는 whole NULL이다.
4. 검증 결과는 private empty-bootstrap consistency뿐이며 source authenticity/credit 권위를 뜻하지 않는다. trusted private bootstrap 정책은 explicit `shared_visible=false`이며 capture가 제공하지 않는 공유 flag를 사실로 주장하지 않는다. actual field/shape는 fixture와 source 대조로 확정한다.
5. focused RED/GREEN 후 독립 QA와 Reviewer. last source field 오류에도 DML0, public RPC held-only, 모든 DB digest/기존 signed reset 보존. 이 gate 후에만 private writer를 배정한다.

**다음 writer 경계(아직 구현/수용 전):** postgres-only `private.shop_guest_bootstrap_receipt`를 기존 public held receipt와 분리하고 fresh predicate에 포함한다. account lock→별도 receipt replay/conflict→fresh 재검사→all-source validator→FK/논리 순서의 atomic writes→canonical result/별도 receipt. native empty domain만, hidden visibility, fixture-local trigger로 중간 실패 전 테이블 rollback·동시 writer 하나만 성공·public RPC가 private imported result를 반환하지 않음을 검증한다. shared public success receipt는 activation gate 전 저장하지 않으며 기존 held receipt를 승격하지 않는다. raw1m single-reset writer는 non-ledger source 공백 해결 후 별도 단위다.


### Native empty wire에 따른 pure validator 계약 확정

- 실제 constructor/profile/prepare/capture 테스트1/1이 `/private/tmp/shop-import-native-empty-rust.log`에서 통과했다. unchanged wire는 `fixtures/shop_guest_import_native_empty.json` 및 동일 payload SQL `.inc`에 보존했다. native effect bounds/history는 [], authoritative=false, timeline/canonical version=null이므로 기존 strict ownership/credit ledger의 ready를 가정하지 않는다. strict hold control과 새 empty-only helper 부재 RED를 분리하며 fixture 보정·capture product 변경은 금지한다.
- interface는 `private.shop_guest_import_bootstrap_source_normalize(p_data jsonb,p_now timestamptz)`다. trusted finite/non-null p_now는 pure 검사의 명시 입력이며 후속 writer는 transaction_timestamp()만 주입한다. client timestamp를 authority로 사용하지 않는다.
- existing data_valid의 전체 키/형식 검사 후 profile nonnull·nickname Unicode1–24/Rust Unicode trim 불변/avatar enum, UUID cycle/device와 동일 valid timezone3, activation=current start<=p_now, current true/end·bonus null을 검증한다. revision/lifetime/current totals는 정확히0; reset times/timeline/canonical version은 null; authority/reset-unverifiable/legacy flag는 false다.
- journal state는 generation0/deleted null의 정확한 object, journal cycles는 current ID/start와 일치하는 정확히1행이고 end/credit/credit time null이다. 모든 나머지 source 배열은 []이며 optional purchase/removal proofs만 missing 또는 []를 허용한다. 의미 있는 행, 삭제/역사 metadata, profile missing, nonzero/문자열/null totals, unknown/추가/누락 키는 whole NULL이다.
- output scope `native_empty_source_consistency`, `source_metadata`는 profile/timezones/UUID/current/activation/journal 및 null/false effect metadata를 그대로 보존한다. `derived_zero_state`는 wallet/current/lifetime/raw/bonus/growth/stage/progress0과 empty objects/equipment를 분리한다. `server_policy.shared_visible=false`는 서버 정책이며 incoming source claim이 아니다. freshness/authenticity/grant/public import 성공은 증명하지 않는다.
- 집중 RED8그룹: keys/types; profile/Unicode trim; UUID/timezone; activation/start/future/null/infinite clock; 종료/reset metadata; journal generation/deleted/cardinality/credit; source nonempty/nonzero; manufactured timeline/bounds/history/canonical/authority. 실제 native fixture positive와 기존 strict hold를 먼저 고정하며 independent QA→Reviewer 후 source-validator 단위만 수용한다. DML/private receipt/public success는 계속 다음 gate다.


### Native-empty source consistency 하위 단위 수용 증거

- 실제 Rust constructor→profile→prepare→capture의 zero-use wire는 effect timeline/canonical version=null, bounds/history=[], authoritative=false다. 원본 JSON 및 SQL inc를 함께 보존했고 기존 strict chain이 reset_rejected/ledger NULL로 보류하는 control을 유지했다. 별도 `bootstrap_source_normalize(jsonb,timestamptz)`만 정확한 native empty 형태를 검증한다. 기존 capture product/strict helper를 완화하지 않았다.
- native shape와 strict hold의 controls3 PASS 뒤 helper 부재 positive#4만 실제 RED였다. helper 구현 후 focused12/12 GREEN과 최종 ROLLBACK이며, grouped negative mutations는 설치된 helper와 정상 base output을 함께 검증한 회귀 근거다. source_metadata의 null/false 보존, derived_zero_state, 별도 hidden server_policy를 검사한다.
- 독립 QA source12+ledger21 PASS·두 ROLLBACK, native Rust1 passed/233 filtered, fixture JSON/inc/원본 출력 의미 동일·inc 원본 byte 동일. full count/digest 동일·합성 auth residue0·invoker/STABLE/postgres-only/빈 search_path/client EXECUTE 없음·public RPC held-only·signed reset 해시4·tracked/untracked whitespace 통과. Reviewer CLEAR(새 P1/P2 없음). 증거 `/private/tmp/shop-import-bootstrap-source-qa-{source,ledger,rust,fixture,before,after,security,diff,preservation}.log`, actual RED `/private/tmp/shop-import-native-bootstrap-red.log`, GREEN `/private/tmp/shop-import-native-bootstrap-expanded.log`.
- 이 수용은 native-empty source consistency뿐이며 authenticity/credit 권한/freshness/전체 ledger/private writer/공개 import 성공을 뜻하지 않는다. 다음 private writer는 read-only mapping을 완료한 뒤 empty-only 범위와 별도 receipt·rollback·경쟁 게이트로 구현한다. raw1m writer·public activation·transport/sharing는 미완료다. hosted/live transfer·commit/push/merge는 수행하지 않았다.


### Private native-empty atomic bootstrap 실행 계획

**범위/결정:** source validator 수용 후 postgres-only private writer와 별도 receipt를 구현한다. shared public RPC/receipt는 그대로 held-only다. fresh에는 새 private receipt 중 imported만 추가하고 held는 동일 ID replay/conflict의 근거로만 사용한다. 첫 invalid 요청이 새 ID의 올바른 empty 요청을 영구 차단하지 않으며 imported receipt만 남은 불일치 상태는 계속 fresh가 아니다. raw1m/debit/reward/era writer는 이 단위 밖이다.

**Interface:** private `shop_guest_bootstrap(p_import_id uuid,p_request jsonb)` → private result JSON. 기존 envelope와 auth.uid/target 검사를 재사용한다. 클라이언트 EXECUTE/table 권한 및 public wrapper는 없다. source disposition/fingerprint는 지급 권위가 아니며 JSONB payload equality만 immutable replay/conflict를 판정한다.

1. **RED contract 먼저:** native fixture envelope가 기존 validator를 통과한다는 control, private helper 부재의 intended result RED를 고정한다. synthetic auth/user와 ROLLBACK만 사용하며 기존 행은 보존한다.
2. **Receipt/lock:** `private.shop_guest_bootstrap_receipt(user_id,import_id,payload,source_fingerprint,status,result,created_at)` PK/user FK cascade, object/hex/status 제약, RLS enabled·client/table/function revoke. auth/envelope/target 오류는 쓰기 전에 raise. 직접 account lock 행 INSERT ON CONFLICT DO NOTHING/FOR UPDATE만 사용하고 `lock_shop_account()` 초기화 함수는 사용하지 않는다.
3. **Replay/fresh:** lock 아래 private receipt를 먼저 조회(equal replay/different conflict), 다음 기존 public held 동일 ID(equal held replay/different conflict)로 자동 승격을 막는다. 실제 fresh 전체 게임 테이블+private imported receipt를 재검사한다. active/source_unverifiable는 private held receipt만 저장하며 게임 쓰기는0이다. 기존 receipt는 수정하지 않는다.
4. **Validation/write:** transaction_timestamp 한 번을 trusted clock으로 source validator 호출한다. 모든 native source 검증을 완료한 뒤 게임 INSERT를 시작한다. planet_member_state(state_version1, source profile/timezone/current cycle/start, 모든 사용량/성장/stage/progress0, incompletefalse, objects[], hiddenfalse, reset/deadline null/cooldown0), shop_account_state(revision0/rewardtimezone), journal state(generation0/deletednull/timezone), journal current cycle(end/credit/time null)의 네 테이블만 쓴다. 정확한 열은 existing schema에 맞춘다. generic upsert/settlement/reward 호출은 금지한다.
5. **Result/atomic receipt:** read-only private.shop_state_json으로 canonical zero ShopState를 만들고 private result+원본 payload+normalized source metadata를 마지막 receipt에 같은 transaction으로 저장한다. device/contribution/equipment/effect/baseline/wallet/reward/settlement/daily rows는 만들지 않는다. 저장할 열이 없는 device/activation/null effect metadata는 원본과 source metadata로 보존하고 서버 이력으로 제조하지 않는다. 예상치 않은 constraint/INSERT 오류는 raise하여 전체 statement를 rollback한다. public timeline RPC는 baseline DML 때문에 결과 생성에 호출하지 않는다.
6. **Behavior checks:** native success 네 게임 테이블/hiddenfalse/zero canonical state; 마지막 source 오류 무게임쓰기; fixture-local trigger가 journal cycle 또는 receipt 단계에서 실패할 때 전 게임 테이블/성공 receipt rollback; same-ID equal replay/no additionalwrites, diff conflict; publicheldsame-ID held 유지/private success 후 public는 active 또는 기존held만. production test_mode/failure 인자는 금지한다.
7. **Race/security gates:** isolated same-ID equal/different, different-ID writer×writer, writer×initialusage 경쟁을 account lock 경계에서 검증한다. cleanup은 생성 전 UUID 부재와 성공 INSERT 후 이 실행 소유 ID만 허용한다. table/function client 접근0, public RPC body unchanged, 모든 DB count/digest/residue/signed reset 보존. freeze→독립 QA→Reviewer 후 empty private writer 단위만 수용한다. publicsuccess activation은 별도다.

계획 깊이는 shared state/receipt lifecycle/validation-before-write/rollback/race/public gate의 의존 순서 때문에 유지한다. 첫 RED와 최소 writer가 저장되기 전 추가 planning matrix는 확대하지 않는다. hosted/live payload/main DB/commit/push/merge는 금지한다.


### Private native-empty atomic bootstrap 하위 단위 수용 증거

- 실제 native empty fixture의 envelope/fresh/prestate controls와 helper 부재 output RED를 분리한 뒤 private writer를 구현했다. 성공은 hidden=false의 planet, revision0/reward timezone initialized의 shop account, generation0 journal state, null end/credit의 current journal cycle 네 게임 행만 생성하며 canonical zero result와 원본 payload/source metadata를 별도 private receipt에 마지막으로 저장한다.
- same-ID immutable replay/conflict, 기존 public held receipt 무승격, imported receipt만 남은 상태의 fresh 차단, active/source held의 무게임쓰기 및 held 이후 다른 valid ID 허용을 검증했다. 같은 statement의 DML/조회 snapshot ordering과 trigger의 table selection 오류는 fixture 오류로 수정했으며 production RED로 주장하지 않는다. journal-cycle 및 final receipt의 실제 주입 오류는 모든 game4/receipt/lock을 rollback했다. SQL 최종31/31 PASS·ROLLBACK·exit0.
- 새 race script의 순차 checkpoint 후 네 경합이 모두 PASS·exit0: same-ID equal 결과/replay 불변, same-ID different valid payload의 imported/conflict, different-ID의 imported/active, 실제 usage RPC 선행 lock 후 bootstrap active 및 profile/token1/기존 game rows 보존. cleanup은 소유 users/receipts/game rows/sessions0을 검증했다.
- 독립 QA writer31/source12/ledger21 PASS 및 각 ROLLBACK, race4/4 PASS·exit0. pinned local container/volume/55432와 postgres|postgres|5432 확인, 전체 public/private count/digest 동일·기존 행 보존, client table/function 접근0·invoker/VOLATILE/빈 search_path·public RPC held-only, signed reset 해시4·tracked/untracked whitespace/shell syntax 통과. 독립 Reviewer CLEAR(새 P1/P2 없음).
- 증거: `/private/tmp/shop-import-bootstrap-writer-qa-{writer,source,ledger,race,before,after,security,syntax,diff,preservation}.log`; 구현 SQL `/private/tmp/shop-import-native-bootstrap-writer-regression.log`; concurrency `/private/tmp/shop-guest-bootstrap-race-green.log`.
- 수용은 native-empty 전용 비공개 atomic prototype이다. source authenticity/일반 ledger/raw1m/private writer 확장/public success/transport/sharing 및 전체 Task8B 완료를 뜻하지 않는다. public RPC는 계속 held-only이며 hosted/main/live transfer·commit/push/merge는 수행하지 않았다. 다음 raw1m native provenance/hold blockers를 먼저 확인한다.


### Ordinary native first-reset의 nonempty import 보류 경계

- read-only Planner는 ordinary 첫 reset이 old-cycle effect interval을 생성하지 않고 new-cycle만 기록하는 것을 확인했다. capture는 old final revision 부재를 reset_receipts_unverifiable=true로 표시하며, 별도 cached timeline/bounds가 없으면 authoritative=false/null이다. post-reset contribution의 baseline 귀속 가능성과 DTO의 canonical upload payload 부재도 해결되지 않았다. 기존 합성 strict reset/credit-only ledger 성공은 ordinary native source 성공 근거가 아니다.
- 다음 bounded 단위는 정상 local API로 실제 raw1m/bonus0/current0 first-reset capture를 serialize하고, 원본 metadata를 보정하지 않은 채 strict normalizer/private writer held 및 게임 DML0을 검증한다. 제품 capture/strict helper 완화, manufactured history/bounds/canonical, reset ID 치환이나 raw1m INSERT는 배정하지 않는다.
- 범위1은 native held/no-DML 회귀와 held-only mocked transport/readonly sharing 검증이다. positive nonempty import는 별도 source-proof 계약 없이는 미완료/보류다. 계약에는 authoritative historical/current bounds, final effect revision/snapshot, reset deadline/UUID receipt linkage, occurrence cycle coverage, 실제 canonical upload payload/equality, journal 및 wallet의 독립 재구성 근거가 필요하다. source 진위나 credit authority를 임의 claim으로 대체하지 않는다.
- 기존 전체5–8시간 ETA는 철회한다. 범위1은 대략3–5시간, positive nonempty 범위는 새 계약/설계 승인 후 최소1–2 개발일의 별도 추정이며 확정 완료시간이 아니다. public RPC는 계속 held-only다.
