# 일반 게스트 첫 reset 가져오기 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Harness:** 실행 방식은 기존 Lead → Coder → 독립 QA → 독립 Reviewer다. 호출한 스킬이 추가 executor/agent를 만들지 않는다.

**Goal:** 새 provenance가 있는 제한된 ordinary 첫 reset 게스트 상태를 fresh 계정에 원자적으로 가져오고 local public RPC → worker/cache → 공개 장면 성공을 증명한다.

**Architecture:** 신규 lineage의 occurrence sequence, cycle baseline 및 reset 증거를 같은 로컬 transaction에 기록한다. schema 2 서버 validator가 canonical contribution·journal·wallet을 재구성하고 writer가 공통 account lock과 immutable receipt 아래 저장한다. 로컬은 prefix ACK와 ownership/cache를 원자 완료한 뒤 append delta를 처리한다.

**Tech Stack:** Rust/rusqlite/SQLite, 기존 chrono/chrono-tz·serde·sha2·uuid, PostgreSQL/Supabase RPC, pgTAP, bash, 기존 localhost HTTP mocks.

**Spec:** [승인된 written spec](../specs/2026-10-03-guest-import-first-reset-addendum.md).

## Global Constraints

- 계획 실행은 **사용자의 계획 승인 이후**다. 아래 명령/검사는 향후 실행 항목이며 실행 결과가 아니다.
- A 정책: 사용량은 자기 신고 입력, 지급은 서버 독립 재계산·확정이다. issuer/attestation을 추가하지 않는다.
- `schema_version=2`, `provenance.version=1`, domain=`ordinary_first_reset_zero_effect_v1`.
- 기존 schema 1 capture/fixture/strict helper/held receipt는 수정·승격하지 않는다.
- 새 단일 lineage/device, complete coverage, ordinary 첫 reset 정확히1회, 양수 raw credit, capture current/growth/stage/progress0만 지원한다.
- 모든 효과0, 구매·아이템·제거·보상·era progress·legacy partial import 없음. old growth는 native/서버 모두 threshold5 미만, 불확실하면 전체 보류다.
- normal scan/reset의 journal 준비·reward settlement를 생략하지 않는다.
- pending 중 게임 변경·guest reward 확정은 제한하되 raw 수집·저장은 계속한다.
- 동일 capture ID/payload, captured prefix/version/journal ACK를 보존한다. 새 ID retry·부분 bootstrap·active overwrite·held 승격 없음.
- correction은 명세의 보류와 same-ID receipt 복구다. 자동 재정산 없음.
- hosted/live 데이터·배포·push/merge를 실행하지 않는다. 계획 작성 단계의 code/DB/probe/migration/commit도 금지다.
- 새 migration은 승인 후 설치된 CLI의 `supabase migration new <semantic_name>`으로 생성한다. timestamp 파일명을 미리 만들지 않는다.
- 편집은 Task별 Coder 소유다. Lead는 통합/검사/승인, QA와 Reviewer는 독립 읽기·검사만 한다. 공유 파일의 작업은 gate 이후 순차 실행한다.
- 이 작업은 데이터·지급·복구를 바꾸는 높은 위험의 작업이다. 변경된 경계의 QA/Reviewer를 단계별로 수용하며, 변경 없는 검증/리뷰를 반복하지 않는다.

## Review Focus

1. 같은 key 재수집과 내용 correction의 구분: duplicate는 sequence를 늘리지 않음 — Tasks1/3.
2. pending freeze 중 scan 지속: raw는 저장하고 game/reset/reward 확정만 제한 — Tasks2/3.
3. 서버 성공 후 응답 유실·local commit 실패·계정 변경: same-ID 복구와 cache isolation — Tasks7/8.
4. 늦은 old-cycle delta: lifetime만 증가하고 closed credit·ACK 범위 증가 없음 — Tasks6/8.
5. visibility/privacy: shared_visible=false, 명시적 sharing 후 공개 확인, private 증거 노출 없음 — Tasks6/9.

## 깊이 평가·공유 상태·의존성

파일 수/LOC와 무관하게 계획이 필수다. occurrence/prefix/canonical/journal/receipt를 공유하고 capture→server validation→writer→receipt→local ownership/cache→delta의 lifecycle과 복구가 의존한다. SQLite와 DB의 원자성, Rust/SQL/RPC/race/privacy의 서로 다른 검증 경로 및 server-before-client migration 순서도 필요하다.

```text
1 타입/provenance → 2 baseline/reset → 3 capture/freeze/fixture
→ 4 pure validator → 5 private atomic writer → 6 public dispatch/race
→ 7 native RPC/local completion → 8 worker/delta/recovery
→ 9 local public scene E2E → 10 통합 QA/Reviewer/Lead acceptance
```

이 단계들은 별도 기능 출시가 아니라 한 제한된 성공 계약을 완성하는 순차 gate다. 후속 gate 승인/수용 전에 public 동작을 먼저 활성화하지 않는다.

## 파일 구조·정확한 경로

Rust 상대 경로의 root는 `apps/desktop/src-tauri/src/`다.

| 경로 | 책임 |
|---|---|
| 새 domain/guest_shop_import.rs | schema2 DTO·result/status/ACK·source relation·canonical encoding |
| 새 storage/guest_provenance.rs | lineage/sequence/key mapping/mutation/baseline/reset 증거 |
| 새 storage/guest_shop_import_v2.rs | v2 capture·durable phase·prefix 검사·atomic completion |
| 기존 domain/mod.rs, storage/mod.rs | module 등록 |
| 기존 storage/ledger.rs | 실제 수집 transaction·reset UUID 연결 |
| 기존 storage/shop_effects.rs | transaction-scoped rebuild/settlement와 canonical 기여 |
| 기존 storage/cosmetic_shop.rs | ordinary reset·credit/provenance 원자 확정·game gate |
| 기존 storage/growth_journal.rs | exact revision/hash ACK |
| 기존 storage/planet_accounts.rs, lib.rs | ownership 이동 방지·account/cache guard·scan 지속 |
| 새 sync/guest_shop_import.rs, 기존 sync/mod.rs | import orchestration와 transport abstraction |
| 기존 sync/client.rs, sync/worker.rs | typed RPC·sync 순서 연결 |
| 기존 commands/cosmetic_shop.rs | native mutation pending/correction gate |
| CLI 생성 migration3개 | validator → private writer → public dispatch |
| 새 supabase/tests/shop_guest_import_v2_validation.sql | pure validator |
| 새 supabase/tests/shop_guest_import_v2_writer.sql | atomic writer/fault injection |
| 새 supabase/tests/shop_guest_import_v2_public.sql | public/schema compatibility |
| 새 supabase/tests/shop_guest_import_v2_public_scene.sql | 공유 DTO/visibility/privacy |
| 새 supabase/tests/shop_guest_import_v2_race.sh | pinned concurrency |
| 새 supabase/tests/run_guest_import_v2.sh | pinned SQL/TAP/copy/hash/보존 검사 |
| 새 supabase/tests/fixtures/shop_guest_import_v2_first_reset.json 및 .inc | untouched native positive fixture |
| 새 supabase/tests/shop_guest_import_v2_api.sh 및 shop_guest_import_v2_e2e.sh | 격리 local API 준비·RPC/native/public evidence |

새 파일은 **향후 만들 경로**다. 지금 존재하거나 실행 가능하다고 주장하지 않는다. 기존 큰 파일은 임의 전면 재구조화하지 않고 새 책임만 focused module로 분리한다.

## 공통 실행·환경·증거 계약

명령은 repository root 기준이다.

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib guest_import_v2 -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
rustup run stable cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all -- --check
bash supabase/tests/run_guest_import_v2.sh <suite.sql>
```

현재 pinned Rust1.98.1 minimal에는 rustfmt가 없고 installed stable에는 있다. formatting은 process-scoped stable을 사용하며 전역 toolchain을 바꾸거나 새 component를 설치하지 않는다.

SQL runner는 다음을 쓰기 전 검증한다.

```text
project_id=token-planet-shop-revamp-test
workdir=/private/tmp/token-planet-shop-revamp-test
config=/private/tmp/token-planet-shop-revamp-test/supabase/config.toml
host port=55432
container=volume=supabase_db_token-planet-shop-revamp-test
server identity=postgres|postgres|5432
Docker endpoint=local unix socket
```

실제 container ID/running 상태·mount destination/name·port mapping·server identity를 대조한다. 불일치면 종료하고 primary token-planet/hosted DSN으로 대체하거나 프로젝트를 자동 start/reset하지 않는다.

실행별 새 container temp 경로에 suite/fixture를 복사하고 host/container hashes를 확인한다. 기존 목적지에 docker cp하여 stale 파일을 실행하지 않는다. psql exit0뿐 아니라 TAP not-ok/plan count/finish/cleanup 오류가 없어야 한다. SQL은 최종 ROLLBACK, race commit은 실행별 UUID/application name으로 소유한 fixture만 cleanup한다. 기존 session/data를 삭제·종료하지 않는다.

DB 전후 public/private count/digest와 auth residue, 기존 proofless fixture hash를 기록한다. migration 경로는 CLI 생성 후 아래 역할에 기록하여 동일 경로를 인계한다.

```text
M_VALIDATION = 생성된 *_guest_import_first_reset_validation.sql
M_WRITER = 생성된 *_guest_import_first_reset_writer.sql
M_PUBLIC = 생성된 *_guest_import_first_reset_public.sql
```

## Local migration 적용·staging 계약

Task4/5/6에서 CLI가 만든 파일은 생성만으로 DB에 적용됐다고 판단하지 않는다. 각 migration의 의도한 RED를 먼저 기록하고, 설치된 CLI help와 현재 pinned local config로 **정확한 local-only incremental apply 절차**를 확인·기록한 뒤 적용한다. 확정한 command·실제 migration path·project/container/port·적용 순서와 전후 schema 검증을 실행 증거에 남긴다.

primary/hosted 사용과 자동 reset은 금지한다. 다른 pending migration까지 적용되는지, 기존 행/receipt를 파괴하는지, 부분 적용이나 rollback 불가능 상태를 만드는지 확인한다. 해당 migration만 순수 incremental 적용하는 절차가 안전하지 않으면 **중단하고 Lead에게 blocker를 보고**한다. destructive reset이나 대체 project를 추정해서 사용하지 않는다.

적용 직전 pinned 환경을 다시 검증한다. 적용 후 해당 helper/table/function signature·owner/ACL/RLS/search_path, schema1 held 경계, 기존 데이터/receipt 보존을 확인하고 다음 GREEN suite를 실행한다. M_VALIDATION → M_WRITER → M_PUBLIC 순서를 유지하며 앞 단계의 staging·QA/Reviewer gate를 통과하기 전 다음 migration을 적용하지 않는다.

## Task 1: v2 타입과 신규 lineage·occurrence

**소유:** Coder. **의존:** 없음.

**Files:** Create `domain/guest_shop_import.rs`, `storage/guest_provenance.rs`; Modify `domain/mod.rs`, `storage/mod.rs`, `storage/ledger.rs`; Test 새 모듈의 `guest_import_v2_provenance_tests`.

**Interfaces:**

- `GuestShopImportV2Request`, `GuestShopImportV2Snapshot`, `GuestProvenanceV1`, `GuestOccurrence`, `GuestFirstResetReceipt`, `GuestImportAck`, `GuestShopImportV2Result`: 명세의 정확한 wire 필드.
- `GuestShopImportV2Status`: request, source_relation, phase, correction_hold를 갖는 durable status.
- `GuestImportSourceRelation::{Exact,AppendOnly,CapturedPrefixChanged,Unverifiable}`; `GuestImportStatus::{Imported,ActiveAccount,SourceUnverifiable,RequestConflict}`.
- `GuestImportPhase::{Captured,AttemptStarted,Held,Imported}` 및 `GuestImportEncodingError`.
- `canonical_json_bytes<T: Serialize>(&T) -> Result<Vec<u8>, GuestImportEncodingError>`.
- `prefix_fingerprint(&GuestProvenanceV1)`, `source_fingerprint(&GuestShopImportV2Snapshot) -> Result<String, GuestImportEncodingError>`.
- `initialize_guest_provenance_in_transaction(&Transaction, DateTime<Utc>) -> Result<(), ScanError>`.
- `record_guest_occurrence_in_transaction(&Transaction, &ParsedRecord, DateTime<Utc>) -> Result<(), ScanError>`.

- [x] **RED:** `new_empty_lineage_records_version_one_occurrence`, `duplicate_scan_keeps_uuid_and_watermark`, `changed_key_records_correction`, `legacy_database_never_becomes_new_lineage` 작성. 실제 저장 후 reopen에도 UUID 유지, 첫 seq/version1, duplicate 증분0, correction 증거, legacy 비적격을 확인한다.

```rust
assert_eq!(first.ingest_seq, 1);
assert_eq!(first.record_version, 1);
assert_eq!(after_duplicate.ingest_watermark, before.ingest_watermark);
assert_eq!(after_duplicate.occurrence_id, before.occurrence_id);
assert!(!legacy_lineage.eligible);
```

- [x] cargo test filter `guest_import_v2_provenance` 실행. 필요한 기록/동작 부재의 RED를 setup 장애와 구분한다.
- [x] additive SQLite lineage/key mapping/immutable version/mutation journal을 기존 source insertion transaction에 연결한다. raw와 provenance의 별도 transaction 쓰기는 금지한다. 원래 key는 local에, wire에는 opaque UUID를 저장한다. effective-record replacement는 중복 계산 대신 correction으로 분류한다.
- [x] strict keys/bigint bounds/UTC 정밀도/sorted encoding 구현. 기존 journal hash 변경 없음.
- [x] **GREEN:** overflow, UUID/seq 중복, correction/delete, mixed device, clock regression, invalid time, sensitive key 제외, new/legacy reopen 검사.
- [x] QA가 additive 호환성과 일반 duplicate를 확인한다. Reviewer는 eligibility·transaction·privacy를 읽고 Lead가 gate를 수용한다.

**Acceptance:** 오래된 source에 합성 증거 없음, duplicate 안정성, 변경 검출, wire에 path/prompt/log 없음.

**Gate 결과:** PASS — 독립 QA의 focused 36/36 및 전체 `--lib` 274/274 통과; Reviewer CLEAR. 증거: `/private/tmp/shop-import-v2-task1-qa-p2-focused.log`, `/private/tmp/shop-import-v2-task1-qa-p2-lib.log`, `/private/tmp/shop-import-v2-task1-qa-p2-fmt.log`, `/private/tmp/shop-import-v2-task1-qa-p2-diff.log`. 독립 Reviewer 보고의 Lead 기록: `/private/tmp/shop-import-v2-task1-reviewer-clear.md`.

## Task 2: 실제 baseline과 ordinary reset 원자 기록

**소유:** Coder. **의존:** Task1 gate.

**Files:** Modify `storage/guest_provenance.rs`, `storage/shop_effects.rs`, `storage/cosmetic_shop.rs`, `storage/ledger.rs`; Test `guest_import_v2_reset_tests`.

**Interfaces:** `record_guest_cycle_baseline_in_transaction(&Transaction, cycle_id:&str, started_at:DateTime<Utc>) -> Result<(),ScanError>`; `record_guest_first_reset_in_transaction(&Transaction,&GuestFirstResetReceipt) -> Result<(),ScanError>`. 기존 `reset_guest_planet`/`reset_planet` signature는 호환 유지한다. 새 `prepare_guest_import_source_in_transaction(&Transaction, now:DateTime<Utc>) -> Result<(),ScanError>`는 rebuild/journal/reward 준비를 같은 transaction handle에서 수행한다.

- [x] **RED:** `ordinary_scan_settle_reset_records_zero_baselines`, `reset_uuid_links_actual_request_and_credit`, `reset_provenance_failure_rolls_back_credit`. normal journal/reward 준비를 포함한 one-date raw1m을 사용한다.

```rust
assert_eq!(receipt.result.raw_tokens, 1_000_000);
assert_eq!(receipt.result.credited_tokens, 1_000_000);
assert_eq!(receipt.result.final_effect_revision, 0);
assert_eq!(receipt.result.frozen_deadline_before_reset_utc, None);
assert_eq!(receipt.result.reset_available_at_utc, reset_at + chrono::Duration::seconds(86400));
assert_eq!(receipt.request.request_id, receipt.result.request_id);
```

- [x] cargo test filter `guest_import_v2_reset`에서 의도한 RED 확인.
- [x] preparation/receipt/provenance/old close/new baseline/credit을 하나의 논리적 게임 transaction으로 묶고 nested commit을 피한다.
- [x] eligible new lineage의 `reset_planet`에서 legacy-reset 문자열 대신 실제 call 전에 UUID를 생성·영속화한다. legacy/replay/caller UUID는 보존한다.
- [x] 실제 cycle 생성 시 baseline, reset 시 old 종료 기록. positive history/authoritative=true 제조 없음.
- [x] **GREEN:** raw1m 시대/보상 없음, 제외 threshold, reset 경계의 new cycle, reopen 후 deadline 불변, 주입 실패 시 credit/reset/bounds 전체 rollback.
- [x] QA는 normal/reset 회귀, Reviewer는 UUID·transaction·rollback을 확인한다.

**Acceptance:** provenance/credit 불일치 없음. production 준비를 생략하지 않음.

**Gate 결과:** PASS — Task 2 focused 55/55, 전체 Rust `--lib` 295/295, 기존 reset 회귀 1/1, stable rustfmt, `git diff --check` 통과. Reviewer 재검토 CLEAR. Clock regression과 `occurred_at == reset_at`의 old-cycle credit 경계는 각각 RED→GREEN으로 검증했다. 증거: `/private/tmp/shop-import-v2-task2-qa-final-{focused,lib,reset,fmt,diff}.log`, `/private/tmp/shop-import-v2-task2-clock-{red,green2}.log`, `/private/tmp/shop-import-v2-task2-reset-boundary-{red,green}.log`. 독립 Reviewer 보고의 Lead 기록: `/private/tmp/shop-import-v2-task2-reviewer-clear.md`.

**범위 상태:** Tasks 1–2 완료. Task 3 로컬 구현 진행 중이며 해당 체크리스트는 아직 미완료다. Task 4 이후, public RPC 성공 및 공개 장면 E2E는 미착수.

## Task 3: immutable capture·source relation·raw를 유지하는 game gate

**소유:** Coder. **의존:** Tasks1–2.

**Files:** Create `storage/guest_shop_import_v2.rs`; Modify `storage/mod.rs`, `storage/shop_import.rs`(version dispatch만), `storage/planet_accounts.rs`, `lib.rs`, `storage/cosmetic_shop.rs`, `storage/shop_effects.rs`, `commands/cosmetic_shop.rs`; Create positive JSON/inc; Test capture/game gate 모듈.

**Interfaces:** `PendingGuestShopImport::{Legacy(GuestShopImportStatus),V2(GuestShopImportV2Status)}`; `capture_guest_shop_import_request(&mut self,target:&str) -> Result<PendingGuestShopImport,ScanError>`; `pending_guest_shop_import_request(&self,target:&str) -> Result<Option<PendingGuestShopImport>,ScanError>`; `guest_import_source_relation(&self,Uuid) -> Result<GuestImportSourceRelation,ScanError>`; `guest_import_game_mutations_allowed(&self) -> Result<bool,ScanError>`.

**실행 경계 기록:** 기존 public `shop_device_contribution`은 자체 transaction을 열므로 capture transaction에서 중첩 호출하지 않는다. `shop_effects.rs`의 기존 canonical SQL/build 부분만 connection-scoped helper로 추출하고 public wrapper의 transaction·raw 조회·commit 순서를 유지한다. 같은 connection의 실제 raw 및 canonical builder를 capture transaction에서 사용하며 기존 출력·hash 동등성 회귀를 확인한다. `shop_import.rs::capture_data`는 visibility만 최소 노출해 재사용한다. 기존 collector는 confirmed bounds와 effect history에 의존하므로 v2에서는 실제 저장된 guest provenance bounds/reset receipt를 읽는 source collector를 사용한다. v1 collector·validator 호출·byte·hash는 보존하고 신규 bounds/reset proof 누락 RED 및 기존 경로 동등성 control을 먼저 확인한다. v2 전용 integrity 검증은 실제 provenance로 확인한 guest bounds를 지원하되 wire의 `effect_cycle_bounds_authoritative=false`, `effect_timeline_state=null`, `effect_history=[]`를 유지한다. 임시 authority=true 또는 flag를 변경한 clone 검증은 수용하지 않으며 위조 proof 거부와 wire flag 회귀를 확인한다. legacy integrity가 불확실하면 `Unverifiable`로 처리한다. core RED→GREEN·API 동결 후 계정/scan/game gate 및 fixture를 연결한다.

- [ ] **RED:** `captures_actual_builder_payload_and_prefix`, `recapture_keeps_id_after_append`, `existing_v1_capture_is_not_upgraded`, `scan_collects_raw_while_games_frozen`.

```rust
assert_eq!(captured.canonical_payload, actual_builder_payload);
assert_eq!(recaptured.request, captured.request);
assert_eq!(append_relation, GuestImportSourceRelation::AppendOnly);
assert_eq!(corrected_relation, GuestImportSourceRelation::CapturedPrefixChanged);
assert!(raw_after_scan > raw_before_scan);
assert!(!ledger.guest_import_game_mutations_allowed().unwrap());
```

- [ ] cargo test filters `guest_import_v2_capture`, `guest_import_v2_game_gate` 실행.
- [ ] 실제 canonical builder/journal/provenance/data/ID/target/hash/phase를 같은 SQLite transaction에서 capture한다.
- [ ] persisted seq/key/version/content로 exact/append/correction/unverifiable 판정. 전체 fingerprint 불일치만으로 판단하지 않는다.
- [ ] storage-level game/reward gate와 native pending/error 표시 연결. `AppState::scan` raw 수집 유지.
- [ ] ownership 이동 전에 capture를 얻고 v2 lineage의 legacy cosmetic import 생성·ordinary upload를 선행하지 않는다.
- [ ] 실제 normal native flow에서 JSON/inc 생성. serialization 후 수선 금지, 기존 raw1m fixture hash 보존.
- [ ] **GREEN:** current append에도 snapshot0, prefix correction hold, switch/reopen 보존, 모든 game 경로 거부 중 raw 증가, schema1 byte 불변.
- [ ] QA는 fixture provenance/도달성, Reviewer는 mutation/reward/account 순서를 확인한다.

## Task 4: pure SQL validator와 pinned runner

**소유:** Coder. **의존:** Task3 fixture gate.

**Files:** CLI 생성 `M_VALIDATION`; Create `shop_guest_import_v2_validation.sql`, `run_guest_import_v2.sh`.

**Interface:** `private.shop_guest_import_v2_normalize(p_request jsonb,p_now timestamptz) -> jsonb`. invalid/unsupported는 NULL. 정상 결과는 source/prefix identity, bounds/baseline, 독립 canonical/journal/reset/raw credit/ACK/source metadata다. pure/read-only, fixed search_path, client EXECUTE 없음.

- [ ] 실행 전에 Supabase current official changelog/docs와 installed CLI help 확인. 자동 upgrade/install/reset 없음.
- [ ] `supabase migration new guest_import_first_reset_validation`으로 실제 path 생성·기록.
- [ ] 기존 race pinning을 재사용해 runner 생성. 새 copy 경로/hash parity/TAP plan·not-ok·cleanup/전후 digest/ROLLBACK 검사.
- [ ] **RED:** native fixture identity/schema1 hold controls를 먼저 통과시키고 v2 helper 부재의 positive 실패를 별도 확인한다.

```sql
select ok(private.shop_guest_import_v2_normalize(v2_request, trusted_now) is not null,
          'native first-reset source is independently reconstructed');
```

- [ ] `bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_validation.sql` 실행. 승인된 local staging만 사용.
- [ ] shape/range/hash와 occurrence→cycle/date/activity/canonical/journal/reset/wallet 재구성 구현. growth 양쪽 below5/era empty, journal encoding/schema1 유지.
- [ ] 의도한 RED와 구현 파일이 준비되면 위 local staging 계약에 따라 `M_VALIDATION`의 실제 local-only 적용 command를 확인·기록·실행한다. signature/ACL/RLS/search_path·schema1 경계·기존 행/receipt 보존을 확인한다. 안전한 incremental 적용을 확정하지 못하면 중단한다.
- [ ] **GREEN:** 원본 raw1m과 명세17.2의 모든 독립 mutation. 마지막 proof만 invalid이며 앞선 control은 정상일 때 게임 DML0 확인.
- [ ] QA는 purity/determinism/의도한 실패, Reviewer는 authority/overflow/timezone/hash/permission 확인.

**Acceptance:** pure v2 positive와 invalid hold. public은 아직 held-only.

## Task 5: private atomic bootstrap writer

**소유:** Coder. **의존:** Task4.

**Files:** CLI 생성 `M_WRITER`; Create `shop_guest_import_v2_writer.sql`.

**Interface:** `private.shop_guest_import_v2_bootstrap(p_import_id uuid,p_request jsonb) -> jsonb`. 기존 account lock/public receipt/private empty receipt를 확인한다. 별도 normalized proof/ACK storage가 필요하면 private에 두고 receipt(user_id,import_id)에 FK 연결한다. client EXECUTE/public dispatch 없음.

- [ ] `supabase migration new guest_import_first_reset_writer`로 path 생성.
- [ ] **RED:** validator positive 후 writer 부재의 expected imported 실패. runner writer suite 실행.
- [ ] auth/envelope→lock→receipt→fresh→전체 normalize→FK writes→persisted DTO equality→success receipt 마지막 저장 구현.
- [ ] 명세12의 planet/shop/device/contribution/baseline/activity/reset/settlement/wallet/journal/source ACK를 정확히 저장. reward/item/world/membership 없음, shared_visible=false.
- [ ] 기존 fresh predicate를 보존하고 새 orphan success proof도 fresh에서 제외.
- [ ] 의도한 RED와 구현 파일이 준비되면 위 local staging 계약에 따라 `M_WRITER`의 실제 local-only 적용 command를 확인·기록·실행한다. signature/ACL/RLS/search_path·schema1 경계·기존 행/receipt 보존을 확인한다. 안전한 incremental 적용을 확정하지 못하면 중단한다.
- [ ] **GREEN:** replay/conflict/new-ID active, held/private empty 호환, orphan receipt, 마지막 proof DML0.

```sql
select is(imported_result->>'status', 'imported', 'whole bootstrap succeeds');
select is(replayed_result, imported_result, 'same payload replays immutable receipt');
select is(conflict_result->>'status', 'request_conflict', 'changed payload cannot overwrite');
```

- [ ] journal/settlement/final receipt의 fixture-local trigger 실패로 game/success receipt/new lock 전체 rollback 검사. production test mode 금지.
- [ ] QA는 정확한 저장값/digest, Reviewer는 FK/snapshot/ledger/receipt/ACL 확인.

## Task 6: public schema2 dispatch·호환성·경쟁

**소유:** Coder. **의존:** Task5.

**Files:** CLI 생성 `M_PUBLIC`; Create `shop_guest_import_v2_public.sql`, `shop_guest_import_v2_race.sh`. 기존 writer에 common lock 누락이 증명되면 편집 전에 실행 brief에 정확한 파일 scope를 기록한다.

**Interfaces:** public `import_guest_shop(uuid,jsonb)`의 schema1 held 유지/schema2 dispatch. 같은 ID의 payload equality는 fresh/validation보다 먼저다. 응답은 명세13을 따른다.

- [ ] `supabase migration new guest_import_first_reset_public`으로 path 생성.
- [ ] **RED:** old wrapper에서 native v2는 held/reject, schema1/anon/private controls는 정상. public suite 실행.
- [ ] version dispatch/auth/grant/revoke/RLS/search_path 구현. schema1 완화 없음.
- [ ] 실제 usage/journal/game writer의 account lock→game lock을 확인하고 누락만 수정한다. race는 실제 initial usage RPC를 호출한다.
- [ ] pinning/소유 fixture cleanup을 갖춘 race harness 작성.
- [ ] 의도한 RED와 구현 파일이 준비되면 위 local staging 계약에 따라 `M_PUBLIC`의 실제 local-only 적용 command를 확인·기록·실행한다. signature/ACL/RLS/search_path·schema1 경계·기존 행/receipt 보존을 확인한다. 안전한 incremental 적용을 확정하지 못하면 중단한다.
- [ ] **GREEN:** 같은 ID·같은 payload, 같은 ID·다른 payload, 서로 다른 ID, import×initial usage. bootstrap은 한 번, replay/conflict/active 결과, overwrite 없음.

```bash
bash supabase/tests/shop_guest_import_v2_race.sh
bash supabase/tests/run_guest_import_v2.sh shop_guest_import_native_first_reset_hold.sql
bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_public.sql
```

- [ ] QA는 race/cleanup/ACL/digest, Reviewer는 dispatch/lock/receipt 호환을 확인한다.

**Rollout/rollback:** `M_VALIDATION→M_WRITER→M_PUBLIC`을 native 송신보다 먼저 준비한다. 구 backend에서는 같은 ID/payload를 pending 호환 hold로 보존하고 legacy fallback/new ID 없음. 새 capability API 없음. 새 schema2 dispatch 중단은 가능하나 이미 imported된 data/receipt를 downgrade/delete하지 않는다. hosted 적용 없음.

## Task 7: native RPC와 atomic local completion

**소유:** Coder. **의존:** Tasks3/6.

**Files:** Modify `sync/client.rs`, `storage/guest_shop_import_v2.rs`, `storage/planet_accounts.rs`, `storage/growth_journal.rs`, `storage/shop_effects.rs`, `storage/cosmetic_shop.rs`; Test transport/completion 모듈.

**Interfaces:** `SupabaseSyncClient::import_guest_shop(&self,access_token:&str,request:&GuestShopImportV2Request) -> Result<GuestShopImportV2Result,SyncError>`; `mark_guest_shop_import_attempt_started(&mut self,target:&str,import_id:Uuid) -> Result<(),ScanError>`; `complete_guest_shop_import(&mut self,target:&str,result:&GuestShopImportV2Result) -> Result<GuestImportCompletion,ScanError>`; `GuestImportCompletion::{Imported,ImportedWithCorrectionHold,Held}`. cache helpers는 같은 transaction을 사용한다.

- [ ] **RED transport:** 정확한 rest RPC path/bearer/p_import_id/p_request, 불변 retry, typed correlation. filter `guest_import_v2_transport` 실행.
- [ ] strict response 구현. old backend/401/truncated/wrong account/import/hash/cycle/version/missing ACK/held의 success state는 pending 유지, fallback 없음.
- [ ] **RED completion:** valid receipt의 ownership/cache/ACK 미완료와 마지막 marker 쓰기 실패 시 모든 local 상태 불변.

```rust
assert_eq!(result.import_id, pending.request.snapshot.import_id);
assert_eq!(result.ack.prefix_fingerprint, pending.request.snapshot.provenance.prefix_fingerprint);
assert_eq!(ledger_state_after_failed_commit, ledger_state_before);
```

- [ ] caches/ownership/marker/ACK/phase/correction hold를 하나의 SQLite transaction으로 완료한다. selected account 재확인, captured journal revision/hash만 ACK하고 새 revision은 pending 보존.
- [ ] **GREEN:** local failure/reopen/retry, duplicate completion, append delta/raw rows 보존, imported+correction hold 원자 저장. filter `guest_import_v2_completion` 실행.
- [ ] QA transport/failure, Reviewer correlation/transaction/account isolation.

## Task 8: worker·account guards·delta/복구

**소유:** Coder. **의존:** Task7.

**Files:** Create `sync/guest_shop_import.rs`; Modify `sync/mod.rs`, `sync/worker.rs`, `lib.rs`, 최종 routing만 `storage/planet_accounts.rs`, `commands/cosmetic_shop.rs`.

**Interfaces:** `GuestShopImportTransport: Send + Sync`는 `import_guest_shop(&self, access_token:&str, request:&GuestShopImportV2Request) -> impl Future<Output=Result<GuestShopImportV2Result,SyncError>> + Send`를 제공한다; `sync_pending_guest_shop_import(state:&AppState,client:&impl GuestShopImportTransport,access_token:&str,target:&str) -> Result<GuestImportSyncOutcome,String>`; outcome::{NoPending,Held,Imported,ImportedWithCorrectionHold}. 순서는 pending import→completion→delta rebuild→기존 signed sync다.

- [ ] **RED:** `pending_import_precedes_all_uploads`, `response_loss_recovers_same_receipt`, `append_ack_only_prefix`, `late_closed_usage_no_new_credit`, `account_switch_cannot_apply_response`.

```rust
assert_eq!(mock.first_remote_call(), "import_guest_shop");
assert_eq!(retry.request, first.request);
assert_eq!(late_old.wallet_balance, imported.wallet_balance);
assert_eq!(other_account.cache, original_other_account_cache);
```

- [ ] filter `guest_import_v2_worker` 실행.
- [ ] durable attempt-before-await/target/prefix/same-ID 복구 구현. network await 중 ledger mutex를 유지하지 않는다.
- [ ] sync_once의 legacy cosmetic/journal/usage보다 먼저 연결한다. selected account와 ledger ownership은 completion까지 구분한다.
- [ ] completion 후에만 delta rebuild. delta 없으면 captured upload 재예약0, 있으면 새 canonical version/current 반영, 늦은 old 기록은 lifetime만/credit0.
- [ ] attempt 전 correction은 hold, 이후에는 receipt 복구→imported hold/held 처리. raw scan/private read 유지.
- [ ] **GREEN:** await 중 switch/append/correction, reopen, backend mismatch, server success/local failure, sharing pause, refresh failure의 marker 보존.
- [ ] QA focused+기존 signed reset/contribution/legacy 회귀, Reviewer order/mutex/account/pending/double credit.

## Task 9: 실제 local API→native worker/cache→sharing→공개 scene

**소유:** Coder. **의존:** Tasks6/8.

**Files:** Create `shop_guest_import_v2_public_scene.sql`, `shop_guest_import_v2_api.sh`, `shop_guest_import_v2_e2e.sh`; test-only local orchestration in `sync/guest_shop_import.rs`. 기존 sharing/WorldPlanet을 사용하고 새 private 공개 필드는 추가하지 않는다.

**Interfaces/environment:** native fixture, actual public DB result, native client/worker, explicit sharing을 연결한다. 현재 DB-only 프로젝트에 API가 있다고 가정하지 않는다. api.sh는 pinned 환경만 사용해 loopback 전용 isolated test API와 synthetic JWT를 준비한다. 기존 hosted/primary API·실제 credentials/session을 읽지 않는다.

- [ ] **RED:** imported hidden placeholder, explicit sharing 후 profile/current0/lifetime/empty scene. mock-only imported 응답은 실제 DB 성공 증거가 될 수 없다.
- [ ] installed/cached stack의 PostgREST version/image를 확인해 고정한다. pinned disposable DB 전용 run-specific service/config/port/test credentials와 loopback binding을 준비하고 새 service/fixture만 cleanup한다. runtime/image/port를 확인할 수 없으면 환경 blocker로 중단하며 primary/hosted 또는 미승인 전역 설치로 대체하지 않는다.
- [ ] 이 환경에서만 synthetic auth fixture/JWT를 만들고 auth.uid/RPC grants를 검사한다. DB identity/copy hash/소유 cleanup을 runner와 공유한다. native에 service_role을 전달하지 않는다.
- [ ] 실제 public RPC로 native exact request를 송신하고 DB result를 actual client/worker/completion에 적용한다. canonical payload/ACK equality 확인.
- [ ] 성공 후 기존 explicit sharing interface로 viewer를 구성해 공개 scene 조회.
- [ ] **GREEN:** recursive private-field deny, 반복 조회 DML0, hidden/outsider/other world/anon, initial0, current append 증가, late-old wallet 불변.

```bash
bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_public_scene.sql
bash supabase/tests/shop_guest_import_v2_e2e.sh
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib guest_import_v2 -- --nocapture
```

- [ ] QA는 실제 DB/RPC와 mock/native 증거를 구분·연결한다. Reviewer는 privacy/visibility/isolation/과도한 완료 주장을 검토한다.
- [ ] 실제 Tauri 화면은 disposable app-data/source가 별도 확인됐을 때만 검사한다. 기능 E2E가 rendering PASS를 의미하지 않는다.

## Task 10: 통합 회귀와 완료 gate

**소유:** 수정 Coder, acceptance Lead, QA/Reviewer 독립. **의존:** Tasks1–9.

**Files:** 앞 Task의 test scope. Lead가 이 plan/spec의 evidence/status를 기록하고 Coder가 제품 범위를 바꾸지 않는다.

- [ ] 최종 안정 상태에서 전체 Rust/format 실행.

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
rustup run stable cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all -- --check
```

- [ ] 기존 SQL+새 v2 전체 suite 및 영향을 받은 race 실행. proofless fixture hash/schema1 hold 보존.
- [ ] native command/account routing/pending output 변경이면 Desktop 회귀/build.

```bash
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

- [ ] QA는 counts/pinning/fixture hash/digest/auth cleanup/fault/race/limitations를 보고한다.
- [ ] Reviewer는 final diff의 spec/authority/compatibility/atomicity/prefix ACK/cache account/privacy를 확인한다. Findings→Coder→영향받은 QA, 필요한 범위의 재검토만 수행.
- [ ] 변경 없는 evidence는 재사용한다. 변경된 code에 과거 PASS counts를 붙이지 않는다.
- [ ] Lead는 제한된 ordinary local public 성공과 미지원 domain·hosted 미실행을 기록한다. 미해결 failure가 있으면 해당 완료 판정을 보류한다.

## 가정·열린 항목의 처분

- referenced written spec은 사용자 승인 완료다. 문서의 기존 ‘승인 대기’ 머리말은 초안 시점의 상태이며 재승인 요구가 아니다.
- existing managed worktree를 사용한다. 새 worktree/runtime/dependency 설치를 전제로 하지 않는다.
- CLI/DB/API readiness는 계획 승인 후 안전한 실행 단계에서 확인한다. 없으면 그 단계의 환경 blocker를 보고하며 임의 환경으로 대체하지 않는다.
- migration 실제 path는 CLI 생성 후 기록하고 semantic name/dependency는 고정한다.
- actual canonical builder/journal hash가 명세와 모순되면 fixture를 수선하지 않고 Lead에게 구체적 모순을 반환한다.
- 이 도메인 안의 제품 미결정은 없다. 더 넓은 성공 범위/correction 재정산/hosted rollout은 명시적인 범위 밖이다.

## Lead self-review — writing-plans

| 점검 | 결과 |
|---|---|
| Spec coverage | §1–6→Tasks1/3/4; §7→2/4/5; §8–9→3–7; §10–11→3/7/8; §12–13→4–7; §14→7/8; §15→9; §16–18→4/6/10 |
| Step scan | named RED/구체 assertion/run/minimal implementation/GREEN/독립 gate. 본문을 대신 구현하지 않고 결정·인터페이스를 고정 |
| Type consistency | request/result/status/phase/source relation/ACK/error는 Task1. completion/outcome은 소비 전에 정의, legacy variant는 기존 GuestShopImportStatus |
| Review Focus | 다섯 입력/실패 class 모두 소유 Task와 named test 연결 |
| Proportion | 명세와 비슷한 규모. 전체 함수 body·재구성 알고리즘·행렬 복제 대신 인터페이스/검증을 명시 |
| Compatibility | new eligibility/schema1 불변/held 무승격/server-before-client/dispatch rollback 분리 |
| Lifecycle | prefix/append/correction/precise ACK/server-success-local-fail/account switch/late-old 포함 |
| Security/data | auth/grants/RLS/search_path/pinned hash copy/소유 cleanup/public privacy 포함 |
| Lead 보완 | installed stable format, draft에서 누락된 result/status/phase 타입, DB-only→격리 API readiness 단계를 추가. 환경 준비·구현 완료로 주장하지 않음 |
| Execution gate | code/test/DB probe/migration/commit 미실행. 다음은 사용자의 계획 승인 |


## Task 3 execution evidence (2026-10-03)

Task3 PASS after independent QA and Reviewer targeted re-review CLEAR. Exact QA commands: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib guest_import_v2 -- --nocapture` 85/85; full `--lib` 329/329, including account switch 6 and cosmetic command 26. Stable cargo format and git diff checks passed. Logs: `/private/tmp/shop-import-v2-task3-qa-review-{focused,lib,fmt,diff,fixture}.log`.

Reviewer confirmed immutable capture, raw-preserving gates, ownership ordering and compatibility; two P2 fixes retain complete zero-token post-reset occurrences/current0/actual canonical builder parity and preserve AttemptStarted independently of correction_hold. V2 occurrence cycle aggregates are checked against raw day-agent count/sum/coverage before grouping. Positive new usage remains a durable hold. No unresolved P1/P2.

Actual native fixture is generated by `exports_native_v2_first_reset_fixture_from_durable_capture` without serialized repair. JSON/inc bytes match. Retained raw claim and one reset proof, zero effects/bonus/current, deadline 86400s, authoritative=false. V2 JSON SHA256 `c5bbc955848e86d389c4d4071026b940f1f8837617cb9ed11f138a610d4e3a62`; inc `c6280034c20667e5d9d06ca2746ed7c398bb5ba988aa10c4d69bf80fa00bac5f`. V1 JSON SHA256 `8655804f5f7b4bb4761e3616362cbf519816cfcff3875b5bbe87dea60e42bf0c`; inc `9261df576689250909dac48b4095f5cb2032ca1b00cc60110301fffe5c3077cb`, unchanged.

This is local storage/capture/scan/command acceptance. Task4 pure SQL validation is next; Tasks4–10, public RPC success, native completion/worker and public scene have not been accepted. No hosted/live, deployment, commit or push activity.
