# 상점 시스템 개편 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Harness precedence:** 위 일반 실행 지침보다 Yunho Harness의 실행 경로를 우선한다. 사용자 계획 승인 후 Coder가 모든 제품 코드·테스트를 수정하고, 개발 팀장이 통합하며 QA와 Reviewer가 검증한다. 팀장이 제품 코드를 직접 수정하거나 실행 방식을 다시 선택받지 않는다.

**Goal:** 풍경 32종의 반복 구매·개별 배치·7종 효과, 아바타 16종의 착용, 자연 개체 유료 제거를 게스트와 로그인 계정에 일관되게 제공한다.

**Architecture:** 기존 cosmetic_shop을 개체 중심 계약으로 전환하고 효과 계산·기여·보상 원장을 분리한다. 게스트는 SQLite 트랜잭션, 로그인 계정은 Supabase RPC가 권위 저장소이며 UI에는 확정 상태를 반환한다. 미리보기와 드래그 좌표는 임시 상태다.

**Tech Stack:** React 19, TypeScript 6, Tauri 2, Rust, SQLite, PostgreSQL/Supabase, Vitest, pgTAP.

**Spec:** [승인된 상세 설계](../specs/2026-10-01-shop-system-revamp.md)

**상태:** 상세 설계 승인 완료 / 이 구현 계획 승인 대기. 문서 작성만 수행했으며 아래 체크와 명령은 아직 실행하지 않았다.

## Global Constraints

- 풍경 32종: 성장 8종, 나머지 6효과 각각 4종. SKU당 총 5개, 변형 0~4와 문자열 seed를 영구 저장한다.
- 아바타 16종: head/outfit/face/back 부위별 하나 착용, SKU당 한 번 구매, 게임 효과 없음.
- 풍경 기본가 5,000,000 / 15,000,000 / 40,000,000 / 100,000,000. 아바타 기본가 100,000,000 / 200,000,000 / 350,000,000 / 500,000,000.
- 효과는 적립·성장·상점 할인·대기 단축·제거 할인·시대 보상·연속 활동 보상의 7종이다. 자연 개체 이동·이동권·별도 자연 생성 증가 효과는 제외한다.
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
LandscapeInstance: instance_id, sku, variation_index, seed:string, variation_version
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
- [ ] 구현: 견적·소유·잔액·주기·버전·좌표를 검증하고 결제·개체·결과를 원자 저장한다. 배치/회수로 활성 구성이 바뀔 때만 새 효과 구간을 연다. 단순 위치 이동은 효과를 다시 지급하지 않는다. 아바타 네 부위 버전은 별도 저장한다.
- [ ] 통과 확인: 같은 필터; 아바타 1억+할인15%→8,500만, 자기 구매 할인 제외, stale quote 재확인, 잔액 부족/잘못된 소유/버전·주기 충돌 무차감을 추가 단언한다.
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
- 승인 전 코드/테스트/QA/DB 적용을 실행하지 않는다. 현재 계획과 수정 설계는 커밋하지 않은 검토용 workspace diff다.

## 팀장 자체 검토

- spec §1–3 → Tasks1/2/3/5/11: 32+16 상품·가격·소유 한도·변형·아바타 할인.
- spec §4 → Tasks4/6: 7효과·상한·prospective 구간·보상 키·시간대·초기화 마감.
- spec §5 → Tasks1/10: geometry/통로/드래그/키보드/변형/미리보기 경계.
- spec §6 → Tasks7/12: 제거 고정가·원자 차감·tombstone·현재 주기 부활 방지.
- spec §7–9 → Tasks2/3/5/6/8/9: 공통 타입·권한·버전·주기·replay·오프라인·계정 경계.
- spec §10–11 → Tasks2/5/11/12: UI·아바타·출시 전 정리 범위.
- spec §12 → Task12: 독립 QA/Reviewer와 명령별 검증 증거.
- active_instances에 배치 개체만 넘기는 계약, 일별 성장 계산, 문자열 seed, 정상 초기화와 개발 정리의 분리를 대조했다.
- Review Focus 5개 모두 소유 Task의 명시적 검증에 연결했다. 지시하지 않은 자연 이동 기능이나 별도 생성 효과를 넣지 않았다.
- 12단계는 각기 의미 있는 도메인/저장/서버/UI 결과를 가지며 파일 경로와 인터페이스를 선행 단계에 정의했다. 제품 코드와 전체 함수 본문을 계획에 복제하지 않았다.
