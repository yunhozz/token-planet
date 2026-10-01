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
- Hosted DB 적용·실제 사용자 집계 전송·실제 원격 제거 거래·push/배포는 실행하지 않았다. 이 기준점은 검증된 로컬 구현 결과이며 전체 개편 완료 또는 출시 가능 판정이 아니다.
