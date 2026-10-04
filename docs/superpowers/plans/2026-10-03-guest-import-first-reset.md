# 일반 게스트 첫 reset 가져오기 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Harness:** 실행 방식은 기존 Lead → Coder → 독립 QA → 독립 Reviewer다. 호출한 스킬이 추가 executor/agent를 만들지 않는다.

**Goal:** 새 provenance가 있는 제한된 ordinary 첫 reset 게스트 상태를 fresh 계정에 원자적으로 가져오고 local public RPC → worker/cache → 공개 장면 성공을 증명한다.

**Architecture:** 신규 lineage의 occurrence sequence, cycle baseline 및 reset 증거를 같은 로컬 transaction에 기록한다. schema 2 서버 validator가 canonical contribution·journal·wallet을 재구성하고 writer가 공통 account lock과 immutable receipt 아래 저장한다. 로컬은 prefix ACK와 ownership/cache를 원자 완료한 뒤 append delta를 처리한다.

**Tech Stack:** Rust/rusqlite/SQLite, 기존 chrono/chrono-tz·serde·sha2·uuid, PostgreSQL/Supabase RPC, pgTAP, bash, 기존 localhost HTTP mocks.

**Spec:** [승인된 written spec](../specs/2026-10-03-guest-import-first-reset-addendum.md).

**현재 실행 상태 (2026-10-04):** Tasks1–10 PASS. Task9 actual loopback public RPC→native worker/cache→explicit sharing/public scene E2E는 native1/1, denial3/viewer read3/DML0으로 통과했다. 독립 cleanup은 schema/history/48-table digest와 auth0/world0/shim0/API·proxy·run-file 부재를 확인했고 새 exact PostgREST 이미지만 제거했다. Task10 최신 독립 Rust380 PASS/0 FAIL/actual ignored1, Desktop327/build, 전체 SQL40 suites1103/1103 및 race4/4는 PASS다. 아래 최종 증거가 이전 PARTIAL/403 checkpoint를 대체하며 최종 full-scope Reviewer CLEAR 및 acceptance 완료. 의도한 Task9/10 파일만 local conventional commit으로 확정하며 push 없음.

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
- hosted/live 데이터·배포·merge를 실행하지 않는다. 후속 사용자 지시에 따라 완료된 Task의 QA/Reviewer gate 후 commit·non-force push를 수행한다. 계획 작성 단계의 code/DB/probe/migration/commit 금지는 당시 단계에 적용된다.
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

**당시 범위 상태 (Task4 checkpoint):** Tasks1–4 gate 완료 후 중단했던 기록이다. 후속 사용자 재개 지시와 현재 실행 상태가 이를 대체한다.

## Task 3: immutable capture·source relation·raw를 유지하는 game gate

**소유:** Coder. **의존:** Tasks1–2.

**Files:** Create `storage/guest_shop_import_v2.rs`; Modify `storage/mod.rs`, `storage/shop_import.rs`(version dispatch만), `storage/planet_accounts.rs`, `lib.rs`, `storage/cosmetic_shop.rs`, `storage/shop_effects.rs`, `commands/cosmetic_shop.rs`; Create positive JSON/inc; Test capture/game gate 모듈.

**Interfaces:** `PendingGuestShopImport::{Legacy(GuestShopImportStatus),V2(GuestShopImportV2Status)}`; `capture_guest_shop_import_request(&mut self,target:&str) -> Result<PendingGuestShopImport,ScanError>`; `pending_guest_shop_import_request(&self,target:&str) -> Result<Option<PendingGuestShopImport>,ScanError>`; `guest_import_source_relation(&self,Uuid) -> Result<GuestImportSourceRelation,ScanError>`; `guest_import_game_mutations_allowed(&self) -> Result<bool,ScanError>`.

**실행 경계 기록:** 기존 public `shop_device_contribution`은 자체 transaction을 열므로 capture transaction에서 중첩 호출하지 않는다. `shop_effects.rs`의 기존 canonical SQL/build 부분만 connection-scoped helper로 추출하고 public wrapper의 transaction·raw 조회·commit 순서를 유지한다. 같은 connection의 실제 raw 및 canonical builder를 capture transaction에서 사용하며 기존 출력·hash 동등성 회귀를 확인한다. `shop_import.rs::capture_data`는 visibility만 최소 노출해 재사용한다. 기존 collector는 confirmed bounds와 effect history에 의존하므로 v2에서는 실제 저장된 guest provenance bounds/reset receipt를 읽는 source collector를 사용한다. v1 collector·validator 호출·byte·hash는 보존하고 신규 bounds/reset proof 누락 RED 및 기존 경로 동등성 control을 먼저 확인한다. v2 전용 integrity 검증은 실제 provenance로 확인한 guest bounds를 지원하되 wire의 `effect_cycle_bounds_authoritative=false`, `effect_timeline_state=null`, `effect_history=[]`를 유지한다. 임시 authority=true 또는 flag를 변경한 clone 검증은 수용하지 않으며 위조 proof 거부와 wire flag 회귀를 확인한다. legacy integrity가 불확실하면 `Unverifiable`로 처리한다. core RED→GREEN·API 동결 후 계정/scan/game gate 및 fixture를 연결한다.

- [x] **RED:** `captures_actual_builder_payload_and_prefix`, `recapture_keeps_id_after_append`, `existing_v1_capture_is_not_upgraded`, `scan_collects_raw_while_games_frozen`.

```rust
assert_eq!(captured.canonical_payload, actual_builder_payload);
assert_eq!(recaptured.request, captured.request);
assert_eq!(append_relation, GuestImportSourceRelation::AppendOnly);
assert_eq!(corrected_relation, GuestImportSourceRelation::CapturedPrefixChanged);
assert!(raw_after_scan > raw_before_scan);
assert!(!ledger.guest_import_game_mutations_allowed().unwrap());
```

- [x] 실제 검증 filter `guest_import_v2` 85/85 및 전체 `--lib` 329/329 실행으로 capture/game gate를 포함해 확인했다.
- [x] 실제 canonical builder/journal/provenance/data/ID/target/hash/phase를 같은 SQLite transaction에서 capture한다.
- [x] persisted seq/key/version/content로 exact/append/correction/unverifiable 판정. 전체 fingerprint 불일치만으로 판단하지 않는다.
- [x] storage-level game/reward gate와 native pending/error 표시 연결. `AppState::scan` raw 수집 유지.
- [x] ownership 이동 전에 capture를 얻고 v2 lineage의 legacy cosmetic import 생성·ordinary upload를 선행하지 않는다.
- [x] 실제 normal native flow에서 JSON/inc 생성. serialization 후 수선 금지, 기존 raw1m fixture hash 보존.
- [x] **GREEN:** current append에도 snapshot0, prefix correction hold, switch/reopen 보존, 모든 game 경로 거부 중 raw 증가, schema1 byte 불변.
- [x] QA는 fixture provenance/도달성, Reviewer는 mutation/reward/account 순서를 확인한다.

## Task 4: pure SQL validator와 pinned runner

**소유:** Coder. **의존:** Task3 fixture gate.

**Files:** CLI 생성 `M_VALIDATION`; Create `shop_guest_import_v2_validation.sql`, `run_guest_import_v2.sh`.

**Interface:** `private.shop_guest_import_v2_normalize(p_request jsonb,p_now timestamptz) -> jsonb`. invalid/unsupported는 NULL. 정상 결과는 source/prefix identity, bounds/baseline, 독립 canonical/journal/reset/raw credit/ACK/source metadata다. pure/read-only, fixed search_path, client EXECUTE 없음.

- [x] 실행 전에 Supabase current official changelog/docs와 installed CLI help 확인. 자동 upgrade/install/reset 없음.
- [x] `supabase migration new guest_import_first_reset_validation`으로 실제 path 생성·기록.
- [x] 기존 race pinning을 재사용해 runner 생성. 새 copy 경로/hash parity/TAP plan·not-ok·cleanup/전후 digest/ROLLBACK 검사.
- [x] **RED:** native fixture identity/schema1 hold controls를 먼저 통과시키고 v2 helper 부재의 positive 실패를 별도 확인한다.

```sql
select ok(private.shop_guest_import_v2_normalize(v2_request, trusted_now) is not null,
          'native first-reset source is independently reconstructed');
```

- [x] `bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_validation.sql` 실행. 승인된 local staging만 사용.
- [x] shape/range/hash와 occurrence→cycle/date/activity/canonical/journal/reset/wallet 재구성 구현. growth 양쪽 below5/era empty, journal encoding/schema1 유지.
- [x] 의도한 RED와 구현 파일이 준비되면 위 local staging 계약에 따라 `M_VALIDATION`의 실제 local-only 적용 command를 확인·기록·실행한다. signature/ACL/RLS/search_path·schema1 경계·기존 행/receipt 보존을 확인한다. 안전한 incremental 적용을 확정하지 못하면 중단한다.
- [x] **GREEN:** 원본 raw1m과 명세17.2의 모든 독립 mutation. 마지막 proof만 invalid이며 앞선 control은 정상일 때 게임 DML0 확인.
- [x] QA는 purity/determinism/의도한 실패, Reviewer는 authority/overflow/timezone/hash/permission 확인.

**Acceptance:** pure v2 positive와 invalid hold. public은 아직 held-only.

## Task 5: private atomic bootstrap writer

**소유:** Coder. **의존:** Task4.

**Files:** CLI 생성 `M_WRITER`; Create `shop_guest_import_v2_writer.sql`.

**Interface:** `private.shop_guest_import_v2_bootstrap(p_import_id uuid,p_request jsonb) -> jsonb`. 기존 account lock/public receipt/private empty receipt를 확인한다. 별도 normalized proof/ACK storage가 필요하면 private에 두고 receipt(user_id,import_id)에 FK 연결한다. client EXECUTE/public dispatch 없음.

- [x] `supabase migration new guest_import_first_reset_writer`로 path 생성.
- [x] **RED:** validator positive 후 writer 부재의 expected imported 실패. runner writer suite 실행.
- [x] auth/envelope→lock→receipt→fresh→전체 normalize→FK writes→persisted DTO equality→success receipt 마지막 저장 구현.
- [x] 명세12의 planet/shop/device/contribution/baseline/activity/reset/settlement/wallet/journal/source ACK를 정확히 저장. reward/item/world/membership 없음, shared_visible=false.
- [x] 사용자 승인 null profile 보완: 검증된 null만 nickname `행성 동기화 대기`/avatar `masculine`로 canonical 초기화, valid non-null 보존, malformed fallback 없음. auth metadata·immutable payload 수정 없음, 저장값/PlanetState/receipt 동일·replay 불변·hidden 초기화 검사. 기존 계획의 의존 순서와 검증 경로는 동일하다.
- [x] 기존 fresh predicate를 보존하고 새 orphan success proof도 fresh에서 제외.
- [x] 의도한 RED와 구현 파일이 준비되면 위 local staging 계약에 따라 `M_WRITER`의 실제 local-only 적용 command를 확인·기록·실행한다. signature/ACL/RLS/search_path·schema1 경계·기존 행/receipt 보존을 확인한다. 안전한 incremental 적용을 확정하지 못하면 중단한다.
- [x] **GREEN:** replay/conflict/new-ID active, held/private empty 호환, orphan receipt, 마지막 proof DML0.

```sql
select is(imported_result->>'status', 'imported', 'whole bootstrap succeeds');
select is(replayed_result, imported_result, 'same payload replays immutable receipt');
select is(conflict_result->>'status', 'request_conflict', 'changed payload cannot overwrite');
```

- [x] journal/settlement/final receipt의 fixture-local trigger 실패로 game/success receipt/new lock 전체 rollback 검사. production test mode 금지.
- [x] QA는 정확한 저장값/digest, Reviewer는 FK/snapshot/ledger/receipt/ACL 확인.

## Task 6: public schema2 dispatch·호환성·경쟁

**소유:** Coder. **의존:** Task5.

**Files:** CLI 생성 `M_PUBLIC`; Create `shop_guest_import_v2_public.sql`, `shop_guest_import_v2_race.sh`. 기존 writer에 common lock 누락이 증명되면 편집 전에 실행 brief에 정확한 파일 scope를 기록한다.

**Interfaces:** public `import_guest_shop(uuid,jsonb)`의 schema1 held 유지/schema2 dispatch. 같은 ID의 payload equality는 fresh/validation보다 먼저다. 응답은 명세13을 따른다.

- [x] `supabase migration new guest_import_first_reset_public`으로 path 생성.
- [x] **RED:** old wrapper에서 native v2는 held/reject, schema1/anon/private controls는 정상. public suite 실행.
- [x] version dispatch/auth/grant/revoke/RLS/search_path 구현. schema1 완화 없음.
- [x] 실제 usage/journal/game writer의 account lock→game lock을 확인하고 누락만 수정한다. race는 실제 initial usage RPC를 호출한다.
- [x] pinning/소유 fixture cleanup을 갖춘 race harness 작성.
- [x] 의도한 RED와 구현 파일이 준비되면 위 local staging 계약에 따라 `M_PUBLIC`의 실제 local-only 적용 command를 확인·기록·실행한다. signature/ACL/RLS/search_path·schema1 경계·기존 행/receipt 보존을 확인한다. 안전한 incremental 적용을 확정하지 못하면 중단한다.
- [x] **GREEN:** 같은 ID·같은 payload, 같은 ID·다른 payload, 서로 다른 ID, import×initial usage. bootstrap은 한 번, replay/conflict/active 결과, overwrite 없음.

```bash
bash supabase/tests/shop_guest_import_v2_race.sh
bash supabase/tests/run_guest_import_v2.sh shop_guest_import_native_first_reset_hold.sql
bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_public.sql
```

- [x] QA는 race/cleanup/ACL/digest, Reviewer는 dispatch/lock/receipt 호환을 확인한다.

**Rollout/rollback:** `M_VALIDATION→M_WRITER→M_PUBLIC`을 native 송신보다 먼저 준비한다. 구 backend에서는 같은 ID/payload를 pending 호환 hold로 보존하고 legacy fallback/new ID 없음. 새 capability API 없음. 새 schema2 dispatch 중단은 가능하나 이미 imported된 data/receipt를 downgrade/delete하지 않는다. hosted 적용 없음.

## Task 7: native RPC와 atomic local completion

**소유:** Coder. **의존:** Tasks3/6.

**Files:** Modify `sync/client.rs`, `storage/guest_shop_import_v2.rs`, `storage/planet_accounts.rs`, `storage/growth_journal.rs`, `storage/shop_effects.rs`, `storage/cosmetic_shop.rs`; Test transport/completion 모듈.

**Interfaces:** `SupabaseSyncClient::import_guest_shop(&self,access_token:&str,request:&GuestShopImportV2Request) -> Result<GuestShopImportV2Result,SyncError>`; `mark_guest_shop_import_attempt_started(&mut self,target:&str,import_id:Uuid) -> Result<(),ScanError>`; `complete_guest_shop_import(&mut self,target:&str,result:&GuestShopImportV2Result) -> Result<GuestImportCompletion,ScanError>`; `GuestImportCompletion::{Imported,ImportedWithCorrectionHold,Held}`. cache helpers는 같은 transaction을 사용한다.

- [x] **RED transport:** 정확한 rest RPC path/bearer/p_import_id/p_request, 불변 retry, typed correlation. filter `guest_import_v2_transport` 실행.
- [x] strict response 구현. old backend/401/truncated/wrong account/import/hash/cycle/version/missing ACK/held의 success state는 pending 유지, fallback 없음.
- [x] **RED completion:** valid receipt의 ownership/cache/ACK 미완료와 마지막 marker 쓰기 실패 시 모든 local 상태 불변.

```rust
assert_eq!(result.import_id, pending.request.snapshot.import_id);
assert_eq!(result.ack.prefix_fingerprint, pending.request.snapshot.provenance.prefix_fingerprint);
assert_eq!(ledger_state_after_failed_commit, ledger_state_before);
```

- [x] caches/ownership/marker/ACK/phase/correction hold를 하나의 SQLite transaction으로 완료한다. selected account 재확인, captured journal revision/hash만 ACK하고 새 revision은 pending 보존.
- [x] **GREEN:** local failure/reopen/retry, duplicate completion, append delta/raw rows 보존, imported+correction hold 원자 저장. filter `guest_import_v2_completion` 실행.
- [x] QA transport/failure, Reviewer correlation/transaction/account isolation.

## Task 8: worker·account guards·delta/복구

**소유:** Coder. **의존:** Task7.

**Files:** Create `sync/guest_shop_import.rs`; Modify `sync/mod.rs`, `sync/worker.rs`, `lib.rs`, 최종 routing만 `storage/planet_accounts.rs`, `commands/cosmetic_shop.rs`.

**Interfaces:** `GuestShopImportTransport: Send + Sync`는 `import_guest_shop(&self, access_token:&str, request:&GuestShopImportV2Request) -> impl Future<Output=Result<GuestShopImportV2Result,SyncError>> + Send`를 제공한다; `sync_pending_guest_shop_import(state:&AppState,client:&impl GuestShopImportTransport,access_token:&str,target:&str) -> Result<GuestImportSyncOutcome,String>`; outcome::{NoPending,Held,Imported,ImportedWithCorrectionHold}. 순서는 pending import→completion→delta rebuild→기존 signed sync다.

- [x] **RED:** `pending_import_precedes_all_uploads`, `response_loss_recovers_same_receipt`, `append_ack_only_prefix`, `late_closed_usage_no_new_credit`, `account_switch_cannot_apply_response`.

```rust
assert_eq!(mock.first_remote_call(), "import_guest_shop");
assert_eq!(retry.request, first.request);
assert_eq!(late_old.wallet_balance, imported.wallet_balance);
assert_eq!(other_account.cache, original_other_account_cache);
```

- [x] filter `guest_import_v2_worker` 실행.
- [x] durable attempt-before-await/target/prefix/same-ID 복구 구현. network await 중 ledger mutex를 유지하지 않는다.
- [x] sync_once의 legacy cosmetic/journal/usage보다 먼저 연결한다. selected account와 ledger ownership은 completion까지 구분한다.
- [x] completion 후에만 delta rebuild. delta 없으면 captured upload 재예약0, 있으면 새 canonical version/current 반영, 늦은 old 기록은 lifetime만/credit0.
- [x] attempt 전 correction은 hold, 이후에는 receipt 복구→imported hold/held 처리. raw scan/private read 유지.
- [x] **GREEN:** await 중 switch/append/correction, reopen, backend mismatch, server success/local failure, sharing pause, refresh failure의 marker 보존.
- [x] QA focused+기존 signed reset/contribution/legacy 회귀, Reviewer order/mutex/account/pending/double credit.

## Task 9: 실제 local API→native worker/cache→sharing→공개 scene

**소유:** Coder. **의존:** Tasks6/8.

**Files:** Create `shop_guest_import_v2_public_scene.sql`, `shop_guest_import_v2_api.sh`, `shop_guest_import_v2_e2e.sh`, `shop_guest_import_v2_native.py`; test-only local orchestration in `sync/guest_shop_import.rs`. Python helper는 동일 승인 범위의 보호된 env/Cargo 호출과 public privacy 검사를 별도 소유 파일로 분리한다. 기존 sharing/WorldPlanet을 사용하고 새 private 공개 필드는 추가하지 않는다.

**Interfaces/environment:** native fixture, actual public DB result, native client/worker, explicit sharing을 연결한다. 현재 DB-only 프로젝트에 API가 있다고 가정하지 않는다. api.sh는 pinned 환경만 사용해 loopback 전용 isolated test API와 synthetic JWT를 준비한다. 기존 hosted/primary API·실제 credentials/session을 읽지 않는다.

- [x] **RED:** imported hidden placeholder, explicit sharing 후 profile/current0/lifetime/empty scene. mock-only imported 응답은 실제 DB 성공 증거가 될 수 없다.
- [x] installed/cached stack의 PostgREST version/image를 확인해 고정한다. pinned disposable DB 전용 run-specific service/config/port/test credentials와 loopback binding을 준비하고 새 service/fixture만 cleanup한다. runtime/image/port를 확인할 수 없으면 환경 blocker로 중단하며 primary/hosted 또는 미승인 전역 설치로 대체하지 않는다.
- [x] 이 환경에서만 synthetic auth fixture/JWT를 만들고 auth.uid/RPC grants를 검사한다. DB identity/copy hash/소유 cleanup을 runner와 공유한다. native에 service_role을 전달하지 않는다.
- [x] 실제 public RPC로 native exact request를 송신하고 DB result를 actual client/worker/completion에 적용한다. canonical payload/ACK equality 확인.
- [x] 성공 후 기존 explicit sharing interface로 viewer를 구성해 공개 scene 조회.
- [x] **GREEN:** recursive private-field deny, 반복 조회 DML0, hidden/outsider/other world/anon, initial0, current append 증가, late-old wallet 불변.

```bash
bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_public_scene.sql
bash supabase/tests/shop_guest_import_v2_e2e.sh --run
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib guest_import_v2 -- --nocapture
```

- [x] QA는 실제 DB/RPC와 mock/native 증거를 구분·연결한다. Reviewer는 privacy/visibility/isolation/과도한 완료 주장을 검토한다.
- [x] 실제 Tauri 화면은 disposable app-data/source가 별도 확인됐을 때만 검사한다. 기능 E2E가 rendering PASS를 의미하지 않는다. 조건 검토 완료: 별도 disposable UI app-data가 확인되지 않아 화면 검사는 미실행이며 rendering PASS를 주장하지 않는다.

## Task 10: 통합 회귀와 완료 gate

**소유:** 수정 Coder, acceptance Lead, QA/Reviewer 독립. **의존:** Tasks1–9.

**Files:** 앞 Task의 test scope. Lead가 이 plan/spec의 evidence/status를 기록하고 Coder가 제품 범위를 바꾸지 않는다.

- [x] 최종 안정 상태에서 전체 Rust/format 실행.

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
rustup run stable cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all -- --check
```

- [x] 기존 SQL+새 v2 전체 suite 및 영향을 받은 race 실행. proofless fixture hash/schema1 hold 보존.
- [x] native command/account routing/pending output 변경이면 Desktop 회귀/build.

```bash
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

- [x] QA는 counts/pinning/fixture hash/digest/auth cleanup/fault/race/limitations를 보고한다.
- [x] Reviewer는 final diff의 spec/authority/compatibility/atomicity/prefix ACK/cache account/privacy를 확인한다. Findings→Coder→영향받은 QA, 필요한 범위의 재검토만 수행.
- [x] 변경 없는 evidence는 재사용한다. 변경된 code에 과거 PASS counts를 붙이지 않는다.
- [x] Lead는 제한된 ordinary local public 성공과 미지원 domain·hosted 미실행을 기록한다. 미해결 failure가 있으면 해당 완료 판정을 보류한다.

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

Task3 gate 당시의 기록: local storage/capture/scan/command acceptance만 완료했고 Task4–10 및 public RPC/native completion/worker/public scene은 미검증이었다. 당시 hosted/live·deployment·commit·push는 수행하지 않았다. 이후 실행과 사용자 지시 변경은 아래 Task4 기록에 따른다.


## Task 4 execution evidence (2026-10-03)

Task4 PASS: pure schema2 validator와 pinned runner의 독립 QA 및 Reviewer 재검토 CLEAR. 사용자의 최신 지시에 따라 Task4 완료 후 중단한다. Tasks5–10의 writer/public RPC/native completion/worker/public scene은 수행하지 않았으며 승인된 설계의 후속 미구현 범위로 남는다. 완료된 Task마다 commit·non-force push하는 후속 사용자 지시가 기존 push 금지 실행 제약을 대체한다. Tasks1–3 checkpoint는 `63dff1054874a3c7fb64e44eb4c1ca098f37c8b8`으로 기존 `origin/feat/shop-system-revamp`에 push됐다.

CLI 2.118.0이 생성한 migration은 `supabase/migrations/20261003061830_guest_import_first_reset_validation.sql`이다. 설치·upgrade·reset 없이 지정된 local project `token-planet-shop-revamp-test`, Docker `desktop-linux`의 로컬 Unix endpoint, 고정 container/volume 및 host port 55432, `postgres|postgres|5432`를 확인했다. 해당 파일만 `psql -X -v ON_ERROR_STOP=1 --single-transaction`으로 적용했다. 기존 010004 helper의 본문·권한 동등성을 확인했으며 누락된 history 행을 repair하거나 다른 pending migration을 일괄 적용하지 않았다.

정상 native fixture와 schema1 held control을 유지한다. occurrence로 raw aggregate/canonical segment/activity/journal/reset/wallet을 독립 재구성한다. 과거 cycle의 end는 실제 bonus settlement 시각과 일치하며 activation..reset 안에 있어야 한다. 실제 bounds/baseline/journal reset 경계는 그대로 일치시킨다. 0-token segment를 보존하고 activity의 최초 시각은 양수 occurrence에서만 계산한다. 미래 capture와 inner prefix hash mutation은 선행 형식·outer source hash 오류에 가려지지 않도록 수정했다. Reviewer의 P2 세 건을 해소했다.

독립 QA 명령·결과:

- `bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_validation.sql`: exit0, 53/53, 명시적 ROLLBACK 및 probe cleanup.
- `cargo test --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --lib post_reset_zero_occurrence_is_captured_as_valid_v2_with_empty_daily_totals`: exit0, 1/1 actual native builder/capture parity.
- `bash -n supabase/tests/run_guest_import_v2.sh`, `git diff --check`: exit0.

QA 증거: `/private/tmp/shop-import-v2-task4-qa-review-{summary,sql,native,before,after}.log`. 수정 전 RED는 `/private/tmp/shop-import-v2-task4-reviewer-p2-red-confirmed.log`의 53개 중 51 PASS/정확한 두 정상 source 실패다. 변경 후 전체 GREEN과 적용 기록은 `/private/tmp/shop-import-v2-task4-reviewer-p2-{green,apply,poststate}.log`다.

Migration SHA256 `d62e939da7a5898b5b584ad48a27edbdbb084a024f54deb5e10142a86714708a`; suite `bfc77efc75acd42552ebd7e7b2cf7b2c6cf6f253364bb442358664670efb6bc2`; runner `79ede74a3ddb3bc57e9cb7cbf6a7c817f4dc3dd5eea0303463e982473e0addd9`. 이전 Task3의 v1/v2 fixture 네 SHA는 불변이다. 설치된 다섯 private 함수는 postgres owner/SECURITY INVOKER/빈 search_path/postgres-only EXECUTE이고 신규 public 함수는 0개다. Normalizer body MD5 `7ec86be52da19ee502d7ecb936374434`. Migration history 26행/max `20261001000208`/digest `953f6733f0fa190faefec5ff83ff022b`와 public/private/auth 보호 데이터 digest `dce33ee61f853986a32dd5fc84600182`가 QA 전후 동일하다. Hosted/live 작업·배포·merge는 수행하지 않았다.

## Tasks 5–10 재개와 새 disposable baseline (2026-10-03)

사용자는 Task5부터 후속 작업 진행, 환경 복구, 기존 환경이 없음을 확인한 뒤 **새 isolated disposable DB 구축**을 명시 승인했다. 위 Task4 중단 기록은 당시 상태이며 재개 지시가 이를 대체한다. 기존 고정 container·volume·config와 해당 백업이 없었으므로 기존 DB의 digest 연속성은 검증하거나 주장하지 않는다.

설치된 CLI 2.118.0과 cached PostgreSQL `17.6.1.171`만 사용했다. 새 workdir/config/project는 본문의 고정 값, Docker는 `desktop-linux`의 로컬 Unix socket, DB container/volume은 `supabase_db_token-planet-shop-revamp-test`, host port는 `55432`다. 포트·container·volume 부재를 확인한 뒤 DB만 시작했으며 ancillary services·seed·자동 migration 적용은 비활성화했다. Identity `postgres|postgres|5432`, 적용 전 public/private 테이블 0·auth 사용자 0을 확인했다. 다른 프로젝트·volume, hosted/live, reset·설치·upgrade는 수행하지 않았다.

Committed HEAD의 migration 28개를 temp copy/hash 확인 후 실제 의존 순서(기존 202609 baseline → shop001/actions → effects002 → 00200–00208 → import004 → v2 validator)로 개별 `psql -X -v ON_ERROR_STOP=1 --single-transaction` 적용했다. 새 history는 실제 적용 파일만 28행/max `20261003061830`/version-name digest `446e2f5d8d72d262fbde82641b1f2f5c`다. 새 public/private 테이블 43, auth 사용자 0, private v2 validator 함수 5를 확인했다.

Validator suite 53/53·exit0·명시적 ROLLBACK/cleanup, 새 보호 데이터 digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba` 전후 동일. 기존 schema1 native held suite는 15/15 assertion과 SQL ROLLBACK이나 v2 runner 완료 marker 누락으로 exit1이며, 성공 실행으로 계산하지 않는다. 이 runner 연결은 Task6에서 보완한다. Task5 writer는 아직 적용하지 않았다.

증거: `/private/tmp/shop-import-v2-new-environment-{start,baseline-apply,baseline-state,validation,schema1-hold}.log`; temp committed copy와 hash/order manifest는 `/private/tmp/token-planet-shop-revamp-test/baseline-{committed,manifest.sha256,order.txt}`. 공통 실행 brief는 `/private/tmp/shop-import-v2-tasks5-10-brief.md`다.


## Task 5 execution evidence (2026-10-03)

Task5 PASS: private atomic writer의 독립 QA 및 Reviewer CLEAR. CLI 생성 `supabase/migrations/20261003122029_guest_import_first_reset_writer.sql`을 신규 승인된 disposable baseline에만 단일 transaction으로 적용했다. 적용 당시 container `5c438ca666762433ccc04c237fd803f268953ac7d608b97394c790ad09651e33`, project/volume/55432/local Unix endpoint/config/identity를 재검증했다. 다른 pending migration이나 hosted/live 적용은 없다. Migration history는 새 baseline 28행/max `20261003061830` 그대로이며 직접 적용 writer의 history repair는 하지 않았다.

Writer는 기존 receipt에 nullable object-checked `normalized_proof`를 추가하고 postgres-only EXECUTE/SECURITY INVOKER/빈 search_path의 private bootstrap을 구현한다. 초기화 side effect가 있는 lock helper 대신 공통 account lock 행을 직접 잠그며, 기존 held/private-empty replay와 conflict를 freshness/정규화보다 먼저 처리한다. 전체 proof 정규화 후 FK 순서 저장·read-only DTO 대조·정확한 ACK·success receipt 마지막 기록을 수행한다. 승인된 null profile 보완·hidden 초기화, reward/item/world/membership 미생성을 확인했다.

RED: `/private/tmp/shop-import-v2-task5-writer-red-final-proof.log`에서 47개 중 validator/control 10 PASS, writer 부재 37개 의도한 실패, 명시적 ROLLBACK 및 digest 동일. GREEN 및 독립 QA: writer 47/47, validator 53/53, 각 exit0/ROLLBACK/cleanup 및 보호 데이터 digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba` 전후 동일/auth 사용자 0. replay/conflict/new-ID active·held/private-empty/orphan success·최종 canonical proof 보류/game DML0·journal/settlement/final receipt 주입 실패의 game/success receipt/new lock 전체 rollback을 확인했다. 기존 journal/device/wallet 테이블은 baseline RLS 미설정이나 anon/authenticated 접근 권한은 차단되어 있으며 이 단계에서 권한을 확대하지 않았다.

명령: `bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_writer.sql`, `bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_validation.sql`, `git diff --check`. 증거: `/private/tmp/shop-import-v2-task5-writer-{apply,green}.log`, `/private/tmp/shop-import-v2-task5-qa-{writer,validator}.log`. Migration SHA256 `d71501c699ff2c09e74843aca3a75bf94efdd546bb88b8de28878268265f1142`. Public dispatch/race/native completion/worker/API/public scene 및 Tasks6–10은 아직 완료하지 않았다.


## Task 6 execution evidence (2026-10-03)

Task6 PASS: public schema2 dispatch·schema1 held 호환·공통 lock 보완·경쟁의 독립 QA 및 Reviewer CLEAR. CLI 생성 `supabase/migrations/20261003134201_guest_import_first_reset_public.sql`만 고정 새 disposable DB에 단일 transaction으로 적용했다. 기존 usage contribution writer는 account→planet lock을 이미 사용하며, 누락을 확인한 private journal get/upsert/delete 세 함수만 새 migration에서 account lock 우선으로 보완했다. 적용 전후 owner/security/search_path/ACL이 정확히 같다. 기존 public RPC service_role EXECUTE와 journal RPC 권한 패턴을 유지하고 private writer 접근을 확대하지 않았다.

Expanded RED: public 27개 중 17 PASS/dispatch·common-lock·journal→active 미구현 10개 의도한 실패, explicit rollback/digest 동일. Final GREEN 및 독립 QA: public 27/27, 기존 schema1 native hold 15/15, 실제 public RPC 경쟁 4/4, 모두 exit0. Hold suite의 완료 marker만 추가해 이전 runner 연결 문제를 해결했다. Journal read/upsert/invalid rollback/delete/tombstone의 기존 DTO 동작을 검사했다.

경쟁은 same-ID equal imported replay·다른 유효 payload conflict·다른 ID에서 imported 하나/active 하나·실제 `public.upsert_my_planet_state` 초기 usage가 먼저 성공한 경우 import active를 검증했다. winner receipt/payload 불변, usage profile·1-token current/lifetime 및 행수 보존, bootstrap 한 번만 저장을 확인했다. 실행별 synthetic UUID/application name·fixture SHA·정확한 local context/config/container/volume/port/identity를 확인하고 owned auth/receipt/game/session만 정리했다. SQL rollback 및 race cleanup 뒤 보호 데이터 digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba` 동일/auth 0.

명령: `bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_public.sql`, `bash supabase/tests/run_guest_import_v2.sh shop_guest_import_native_first_reset_hold.sql`, `bash supabase/tests/shop_guest_import_v2_race.sh`, `bash -n supabase/tests/shop_guest_import_v2_race.sh`, `git diff --check`. 증거: `/private/tmp/shop-import-v2-task6-public-{red-final,apply,green}.log`, `/private/tmp/shop-import-v2-task6-race-green.log`, `/private/tmp/shop-import-v2-task6-legacy-hold-green.log`, `/private/tmp/shop-import-v2-task6-qa-{public,hold,race}.log`. M_PUBLIC SHA256 `6bbece7deb9455ead9ebd25b329a5f6a0a89665e4d11c69f1b4ef39aa4810269`. 새 baseline history 28행은 유지하며 direct writer/public staging의 history repair는 하지 않았다. Task5 checkpoint `41dc8c0b1d942cc1778f1ddcf1dcdf2888eb2d3b`은 non-force push 완료다. Tasks7–10 native completion/worker/실제 API·공개 scene/통합 gate는 아직 완료하지 않았다.


## Task 7 execution evidence (2026-10-04)

Task7 PASS: typed native RPC와 atomic SQLite completion의 독립 QA 및 Reviewer CLEAR. 변경은 `sync/client.rs`, `storage/guest_shop_import_v2.rs`, `storage/planet_accounts.rs`, `storage/shop_effects.rs`다. Immutable request/same ID, strict raw JSON parser, account/import/source/cycle/version/prefix 및 정확한 journal ACK 집합을 검증한다. Journal generation/deletion/old-new cycle metadata는 captured state와 대조하며 timestamp는 UTC instant 동등성을 사용한다.

Immediate transaction 안에서 selected auth account 재확인, cache/ownership/captured revision ACK/confirmed canonical baseline/phase/result를 저장하고 마지막 marker 뒤 단일 commit한다. Exact capture version을 유지하고 content-changing append는 그 이후 version으로 rebuild한다. Append raw/new revision은 보존하고 prefix만 ACK한다. Correction은 imported-with-hold로 저장하며 held response는 business state/ownership을 바꾸지 않는다. 실제 session selection routing과 worker는 Task8 범위다.

TDD: timeline bare UUID 대조, append-only hold 분류, newer revision merge의 ACK 보존, captured version3 exact 실패/append version1 회귀, nested duplicate canonical_version 응답 허용, journal generation/deletion/cycle mismatch 허용을 각각 RED로 재현한 뒤 최소 수정했다. 실제 final-marker trigger 실패는 seed 3테이블을 포함한 전체 ordered row/schema snapshot 불변과 trigger만 제거한 동일 ID/result retry로 확인했다. Reopen/duplicate full state equality도 통과했다.

최종 독립 QA: `CARGO_INCREMENTAL=0 cargo test --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --lib guest_import_v2 -- --nocapture` 102/102, exit0. Snapshot 보강 affected completion 9/9도 독립 QA 통과했다. Stable rustfmt check와 git diff check 통과. 증거: `/private/tmp/shop-import-v2-task7-qa-journal-metadata.log`, `shop-import-v2-task7-qa-expanded-snapshot.log`, `shop-import-v2-task7-canonical-{v2-runtime-red,append-red,baseline-green}.log`, `shop-import-v2-task7-duplicate-canonical-{red,green}.log`, `shop-import-v2-task7-journal-{generation-transport-red,generation-completion-red,metadata-matrix-red,metadata-combined}.log`. Reviewer의 canonical version seed/strict parser/journal metadata findings는 모두 해결됐고 최종 재검토 CLEAR다.

반복 ENOSPC 동안 승인된 이 checkout Cargo generated target/incremental만 정리했고 자동 승인 writer-lock 실패를 우회하지 않았다. 이후 공간 복구 후 offline cached rebuild로 검증했다. DB reset/recreation/migration, hosted/live/deploy/merge, 설치/upgrade는 수행하지 않았다. 사용자 .DS_Store 삭제와 untracked landscape plan은 보존한다. Tasks8–10 worker/실제 API·공개 scene/통합 gate는 아직 완료하지 않았다. Task6 checkpoint `861c5a99474029517780a5d779d4e49e5270e5bb`은 non-force push 완료다.


## Task 8/9/10 현재 검증 checkpoint (2026-10-04)

Task8 구현은 저장됐지만 독립 Reviewer P2가 남아 완료 gate는 열려 있다. 실제 인증 후 dispatch는 durable V2 attempt를 먼저 저장하고 mutex를 해제한 뒤 RPC를 호출하며, 정상 imported/no-pending 때만 기존 signed reset recovery 및 legacy sync를 이어간다. Selected auth와 active local ownership을 구분하고 exact pending selection outcome만 재개한다. Response-loss 동일 request retry, await account switch, append exact-prefix ACK/new revision 보존, late closed-cycle lifetime 증가/추가 credit0, correction, sharing pause, refresh/backend/local-marker 실패 복구를 테스트했다.

독립 QA checkpoint: `/private/tmp/task8-qa-fixture-fix-repeat1.log`, `repeat2.log`, `repeat3.log`은 각각 focused 1/1; `/private/tmp/task8-qa-fixture-fix-full.log`은 serialized 전체 Rust 364/364, exit0. Format/diff check exit0. 이전 flaky canonical capture는 fixture가 ingestion보다 미래인 base+1/+2ms occurrence를 만들며 `occurrence_after_ingestion`으로 lineage를 부적격 처리한 것이 원인이었다. Fixture만 occurrence 경과 후 ingest하고 모든 setup 이후 reset 시각을 정하도록 수정했다. Production provenance guard는 유지했다.

Fresh Reviewer P2: `ImportedWithCorrectionHold`에서 확정 서버 상태 조회까지 중단된다. Spec §11에 따라 guarded `get_my_planet_state` read는 계속되어야 하며 uploads/game mutations는 계속 막아야 한다. Timeline/journal getter는 행 초기화 side effect가 있어 무조건 read-only로 간주하지 않는다. 이 finding의 TDD 수정·영향 QA·targeted 재검토가 남아 있다. 위 364 PASS는 수정 전 checkpoint이며 아직 저장되지 않은 수정의 성공 증거로 사용하지 않는다.

Task9 환경 blocker: Supabase CLI 부재, 고정 API 서비스/port 비활성, Docker socket 접근 permission denied; escalated read-only docker ps도 응답 없이 멈춰 중단했다. Runtime 설치·대체 프로젝트/API·hosted credentials 사용은 하지 않았다. 실제 local RPC→native worker/cache→explicit sharing→public scene E2E 및 rendering은 미검증이다.

Task10 가능한 frontend gate는 실행했지만 dependency 환경으로 시작 단계에서 막혔다. `npm --prefix apps/desktop test` exit1: Rolldown WASI/native binding 누락, 테스트 count 없음. `npm --prefix apps/desktop run build` exit1: `typescript/lib/_tsc.js` 누락으로 TypeScript 시작 실패, Vite 미실행. Active Node22.18.0은 package engine22.21.0과 다르다. Logs `/private/tmp/task10-qa-desktop-test.log`, `/private/tmp/task10-qa-desktop-build.log`. 설치/upgrade는 수행하지 않았다.

SQL/schema 변경은 Task7/8에 없다. 변경 없는 prior SQL evidence는 validator53/writer47/public27/schema1 hold15/race4 PASS 및 owned auth cleanup/protected digest 동일이라는 Tasks5/6 기록을 재사용한다. 현재 Docker 접근 제한으로 최종 SQL/race rerun과 현재 DB digest 재확인은 수행하지 못했으며 이전 digest를 현재값으로 주장하지 않는다. Task9 및 Task10 전체 완료 판정은 보류한다. Task7 local commit `25eb0fa`의 push는 자동 승인 검토에서 destination authorization 증거 부족으로 거절됐으며 재시도하지 않았다. Task8 commit은 P2 gate가 해결된 뒤에만 로컬로 수행한다. 기존 사용자 .DS_Store 삭제와 untracked landscape plan은 보존한다.


## Task 8 최종 gate와 Task 10 가능한 검증 (2026-10-04)

Task8 PASS: 위 checkpoint의 P2는 별도 behavioral RED (`[]` vs `[planet_state]`) 후 guarded read callback으로 해결했다. ImportedWithCorrectionHold에서 실제 `my_planet_state`를 조회하되 selected auth/active ownership/cycle을 await 전후 확인하고 cache merge·upload·reset·receipt/ACK/hold 변경은 하지 않는다. Held는 이 경로를 실행하지 않는다. 조회 중 account switch의 non-selection DB 전체 불변도 통과했다. Fresh Reviewer는 lookup-only/no-merge가 §11을 충족하며 Task8 전체 diff에 남은 substantive finding이 없다고 CLEAR 판정했다.

최종 독립 QA: `CARGO_INCREMENTAL=0 cargo test --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --lib -- --test-threads=1` 365/365 exit0; `--lib authenticated_sync_ -- --test-threads=1` 12/12 exit0. Test-only wrapping 수정 후 독립 format/diff 재검사 exit0. Logs `/private/tmp/task8-correction-read-red.log`, `/private/tmp/task8-qa-correction-read-full.log`, `/private/tmp/task8-qa-correction-read-focused.log`, `/private/tmp/task8-qa-correction-read-fmt-final.log`. 이전 364 count는 수정 전 checkpoint이며 최종 code에는 365 count를 사용한다.

Task10 Rust/full-scope correctness review/format은 위 최종 상태 증거로 충족했다. SQL 및 affected race는 변경 없는 Tasks5/6 evidence를 재사용하지만 현재 Docker 환경 때문에 최종 rerun은 불가다. Task9 실제 local API/E2E 및 Task10 Desktop test/build는 위 환경 blocker 때문에 미완료이며 전체 Tasks5–10 완료는 주장하지 않는다. Dependency/runtime 설치나 다른 DB/API 사용 없이 중단 조건을 지켰다. Task8은 로컬 conventional commit만 수행하며 거절된 push를 재시도하지 않는다.


## Task 9/10 환경 복구 후 checkpoint (2026-10-04)

사용자는 Docker 복구·CLI 설치 후 Task9/10 진행과 Desktop 환경 수선을 승인했다. Docker Engine29.8.1은 elevated local read에서 정상 응답한다. 기본 sandbox의 socket permission denied는 daemon 장애 증거가 아니며 elevated local-only runner를 사용한다. CLI2.119.0이 설치됐고 pinned config/project/container/volume은 기존 값, PG17.6.1.171/auth0/history28/max20261003061830을 유지한다. DB host binding0.0.0.0/::55432는 기존 상태로 보존하며 Task9 API는 별도 loopback-only가 요구된다. 다른 DB/volume을 수정하거나 초기화하지 않았다.

기존 `/usr/local/bin/node`22.21.0 및 npm10.9.4를 명시 PATH로 사용하고 승인된 `npm ci`로 lockfile 의존성만 복원했다. Node/npm 설치는 추가로 필요하지 않았다. Package-lock SHA256 `8e93c5d0d30963e7d670f0ee7f1d2ec98a0bfac285efdc4b35140956ed7926cb` 불변, package/source 수정 없음. 이전 missing TS/Rolldown startup blocker는 해결됐다.

최신 독립 QA: validation53/53, writer47/47, public27/27, schema1 hold15/15, affected race4/4 모두 exit0. 각 SQL rollback 및 owned race cleanup 후 보호 digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba` 동일, test-owned auth/receipt/game 행0. Logs `/private/tmp/task10-qa-sql-{validation,writer,public,hold,race}.log`. Desktop tests327/327(18files) 및 TypeScript/Vite build47modules exit0: `/private/tmp/task10-qa-restored-desktop-{test,build}.log`. Task8 최종 Rust365/365와 fmt/diff 및 full-scope Reviewer CLEAR는 코드 불변으로 재사용한다.

Task9 actual API/E2E는 아직 미완료다. Cached image inventory에 PostgREST가 없으며 설치 CLI2.119.0의 bundled artifact manifest는 `ghcr.io/supabase/cli/postgrest:v16.4-r0@sha256:63a8d4acfdeb107b6568f4582759c78072100ef07951a7fbe58c9a51241138a7`(upstream `postgrest/postgrest:v16.4`)를 지정한다. Plan의 cache-miss 중단 조건에 따라 image pull/API start를 하지 않았다. 환경 의존 없는 planned public-scene/runner artifacts를 먼저 준비·검토하고 이 exact image의 별도 준비 승인이 필요하다. 전체 완료 gate는 Task9 실제 API/native/public scene 증거 후에만 닫는다. Push는 재시도하지 않는다.


## Task 10 전체 SQL 회귀 RED (2026-10-04)

위 선택 suite PASS 이후 전체 기존 SQL+v2를 독립 QA로 실행했다. 40 suites 중39 PASS/1 FAIL, 1102 assertions 중1101 PASS/1 FAIL이다. 유일한 실패는 `shop_guest_import_bootstrap.sql` TAP17의 schema1 public held-only privacy assertion이다. 원래 assertion은 반환 JSON을 출력하지 않으므로 실제 반환값 진단 전 원인을 확정하지 않는다. 이 회귀를 해결하고 Task5/6 public/schema1 및 race/전체40을 재검증하기 전 Task10 완료 판정은 보류한다.

모든 SQL/fixture staged hash가 일치하고 drift0, 각 suite 명시적 rollback, 보호 digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba` 동일/auth0/history28/max20261003061830/probe0. Summary `/private/tmp/task10-qa-full-sql-summary.json`, failure `/private/tmp/task10-qa-full-sql-shop_guest_import_bootstrap.log`, final state `/private/tmp/task10-qa-full-sql-final-state.log`.

Task9 준비물 public scene SQL은 8/8, readiness api/e2e scripts는 syntax PASS 및 exact uncached image gate에서 exit1로 중단됨을 독립 확인했다. Logs `/private/tmp/task9-qa-api-readiness.log`, `/private/tmp/task9-qa-e2e-failclosed.log`. 이는 fail-closed readiness 증거이며 실제 E2E 성공이 아니다. Public scene privacy/fingerprint assertion 범위도 추가 검토 중이다.


## Task 10 schema1 privacy 회귀 수정 최종 증거 (2026-10-04)

실제 diagnostic RED는 schema1 요청과 private 성공 receipt가 같을 때 public RPC가 `status=imported`, private `result` 및 `source_metadata`를 반환함을 확인했다. 원인은 M_PUBLIC의 schema 분기 이전 private receipt replay였다. CLI2.119.0 생성 `supabase/migrations/20261004013811_guest_import_schema1_private_receipt_guard.sql`(SHA256 `aaf60b3ba940a3e621325a87502335e3db35c04e0707ac453000da44e10a096b`)은 이 replay를 schema2에만 제한한다. 기존 schema1 strict validator/public held replay/freshness/형식은 유지했다. Applied 과거 migration은 수정하지 않았다.

Independent candidate QA는 동일 session BEGIN→candidate→original suite→ROLLBACK으로 bootstrap31/public27/hold15/scene9=82/82 통과하고 function definition/ACL/owner/security/search_path/history/auth/digest 전후 동일을 확인했다. 이후 Lead는 exact config/context/containerID/volume/55432/identity 및 source/container SHA를 재확인해 corrective migration 하나만 `psql -X -v ON_ERROR_STOP=1 --single-transaction` 적용했다. Owned temp copy 제거, 다른 migration/history repair/reset/API/hosted 작업 없음. Apply 기록 `/private/tmp/task10-schema1-guard-apply.log`.

최종 독립 QA 전체40 SQL suites **1103/1103 PASS**, 모든 runner exit0/명시적 rollback/hash drift0, affected race4/4 PASS 및 owned fixtures cleanup0. 실제 schema1 응답은 `active_account`, `has_result=false`, `has_source_metadata=false`; schema2 replay/public27 및 legacy hold15 유지. 보호 digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba`, auth0/history28/max20261003061830/probe0. Owner postgres/SECURITY DEFINER/빈 search_path/기존 ACL 동일. Logs `/private/tmp/task10-qa-schema1-final-summary.json`, `task10-qa-schema1-final-shop_guest_import_bootstrap.log`, `task10-qa-schema1-final-race.log`, `task10-qa-schema1-final-state.log` 및 같은 prefix의 per-suite logs. Rust365/fmt/diff, Desktop327/build PASS는 해당 source 불변으로 재사용한다.

Fresh independent Reviewer `reviewer_schema1_privacy_final`은 privacy 수정 및 prepared artifacts CLEAR 판정했다. Public scene9 assertion은 cycle ID 포함 recursive deny와 첫/반복 조회 전후 settlement/reset receipt/device canonical/ACK fingerprint를 보강했다. API/e2e 준비 검사는 syntax 및 exact missing-image fail-closed가 확인됐고 실제 E2E가 아님을 명시한다. Task9 actual RPC→native worker/cache→explicit sharing/public scene, current append/late-old API 통합 및 rendering은 미구현·미실행이다. 그 증거 전에는 Task9/전체5–10 완료를 주장하지 않는다.

남은 정확한 환경 승인 범위는 CLI2.119.0 bundled image `ghcr.io/supabase/cli/postgrest:v16.4-r0@sha256:63a8d4acfdeb107b6568f4582759c78072100ef07951a7fbe58c9a51241138a7` 다운로드와 run-specific loopback-only API 시작/owned cleanup이다. Plan cache-miss 중단 조건에 따라 pull/start는 아직 하지 않았다. 기존 DB/volume/port와 unrelated resources를 변경하지 않는다. Push도 재시도하지 않는다.

## Task 9 exact image 승인과 API lifecycle 증거 (2026-10-04)

Full shell runner는 fixture UUID 부재 proof/보호 manifest 저장 전 DML 금지, owner-scoped world→auth cleanup, digest 복원, pre-manifest abort/response-loss/unknown-file 보존을 구현했다. Frozen SHA `4aeb09b2e8b78c606a26cad2ebfa0a26b8696b1b00e946a75b6d1b1813f4f5de`의 독립 shell QA는 **11개 listed/discoverable mock harness**, 12 embedded Python compile, bash/diff PASS다(`/private/tmp/task9-shell-final-preexecution-qa.log`). Coder의 최초 12 harness 보고는 확인된 11개로 정정한다. Native helper 호출은 mocked이며 실제 fixture 실행은 helper QA와 joint Reviewer 후 진행한다.

추가 security pre-execution gate는 Python `ProxyHandler({})`/redirect 거절과 native cfg(test) no-proxy/no-redirect client, credential 읽기 전 소유 proxy.port와 canonical loopback origin/source/world-id 검증을 포함한다. 독립 QA는 5 pure harness, 7 Python compile, Rust 10/10(실제 E2E 1 ignored), fmt/diff PASS를 확인했다(`/private/tmp/task9-security-preexecution-qa.log`, `/private/tmp/task9-security-native-qa.log`). Fresh Reviewer는 lifecycle-only live 실행 CLEAR로 판정했다. Production client constructor는 변경하지 않았다.

현재 frozen Rust source의 Task10 독립 전체 serialized 회귀는 **375 passed/0 failed/1 ignored**, exit0, fmt PASS다(`/private/tmp/task10-task9-full-rust-qa.log`, `/private/tmp/task10-task9-fmt-qa.log`). Ignored 1개는 아직 실행하지 않은 실제 Task9 native E2E이며 이 전체 lib 성공에 포함하지 않는다. SQL40/1103·race4 및 Desktop327/build는 변경 없는 해당 source의 이전 독립 증거를 재사용한다. Task9 actual gate는 열린 상태다.

UID501 사용자 실행에서 짧은 PATH의 `/usr/sbin/lsof` 누락으로 첫 시도는 side effect 전에 중단했다. `/usr/sbin`만 추가한 재실행은 owned API `127.0.0.1:54480` → owned proxy → missing RPC 404/PGRST202 → cleanup을 exit0으로 검증했다(`/private/tmp/task9-proxy-live-lifecycle-path.log`). 소유 API label inventory와 run 경로는 비었고 protected digest는 `ce9ef4b37d29422d9d8cd8cb3c9d9bba`로 동일했다. 이 증거는 proxy lifecycle만 검증하며 fixture/native actual E2E 완료를 의미하지 않는다.

사용자가 `ghcr.io/supabase/cli/postgrest:v16.4-r0@sha256:63a8d4acfdeb107b6568f4582759c78072100ef07951a7fbe58c9a51241138a7`만 pull하고 기존 고정 disposable DB를 사용하는 loopback API와 실제 E2E를 실행한 뒤 소유 자원을 정리하도록 승인했다. 이 한 image를 pull해 digest와 PostgREST16.4를 확인했다. DB reset/rebind, hosted/primary, 다른 image·volume 정리는 범위 밖이며 push 차단은 유지한다.

API script SHA `eb65f8b65c76dff528a36bd257f3612ef2519cef2a8aa21acb0c71fe4c21ea46`은 static/pure QA PASS와 독립 시작 전 Reviewer CLEAR를 받았다. Exact desktop-linux socket, pinned DB/network/config/image, mode별 인자와 canonical 소유 경로, run label/ID/image/loopback 소유권을 검사한다. Mock absent-container 정리는 성공하고 mismatch/multiple/query failure는 파일을 보존하며 실패한다. `/private/tmp/task9-api-preflight-qa-final.log`.

실제 lifecycle QA는 `--start` → loopback HTTP200 → `--stop`을 exit0으로 검증했다. Run `9cb42bcb73a0ce81b93db74b`, API `127.0.0.1:50259`만 bind, 기존 고정 DB network 사용. 소유 API container와 runtime 파일은 제거됐으며 auth0, 보호 digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba`, DB container ID/identity/기존55432 바인딩이 불변이었다. `/private/tmp/task9-api-lifecycle-qa.log`.

이 증거는 lifecycle/health에 한정한다. Synthetic auth fixture와 실제 RPC/native worker/cache/public scene E2E는 아직 미실행이며 proxy/outer runner와 current append/late-old native acceptance를 작성 중이다. 전체 완료 및 rendering PASS는 주장하지 않는다.

### Actual E2E 진단 checkpoint (2026-10-04)

이후 joint pre-execution QA/Reviewer CLEAR를 받은 UID501 `--run`을 실행했다. 첫 실패는 native RPC 전에 pending import의 game-mutation guard가 `rebuild_shop_contributions`를 `InvalidShopState`로 거절한 테스트 순서 오류였다. 오류 문맥을 가리던 Python 요약기는 별도 RED→GREEN/독립14/14 QA/Reviewer로 수정했다. Production guard는 유지하고 immutable request/version/prefix/current0 및 raw17 행을 확인한 뒤, 재계산과 journal 준비를 Imported·초기 shared0 확인 뒤로 이동했다. 독립 focused6/6(실제 probe ignored1)·guard5/5 QA와 Reviewer CLEAR를 받았다.

다음 실제 실행은 첫 import RPC까지 진행했으나 HTTP403으로 실패했다. Safe cfg(test) transport 진단은 요청을 한 번만 전달하고 고정 오류 종류·HTTP 상태만 기록한다(독립7/7, 실제 probe ignored1, Reviewer CLEAR). `/private/tmp/task9-actual-e2e-safe-error.log`가 `HttpRejected(403)`을 확인하며 local completion에는 도달하지 않았다. `authenticated`의 public RPC EXECUTE/schema USAGE는 확인됐으므로 정확한 PostgREST 오류 코드 확인 전 권한이나 production 동작을 바꾸지 않는다.

각 실패 후 독립 read-only cleanup QA는 auth0/world0, protected digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba`, history28/max20261003061830, 기존 DB ID/volume/55432 binding 보존과 소유 API/proxy/run 파일 부재를 확인했다. 최신 `/private/tmp/task9-safe-error-cleanup-qa.log`. 승인 image는 재시도용으로만 cached 상태이며 referencing container는 없다. Task9 실제 E2E 및 Task10 최종 gate는 여전히 열려 있고 완료/rendering PASS를 주장하지 않는다.

### Task9 로컬 인증 호환 bounded 설계·계획 수정안

기존 API/E2E runner에 run 소유 임시 pre-request 함수를 연결합니다. DB 객체 생성·API 시작·실패 복구에 순서 의존성이 있어 계획이 필요합니다. 추가 호환 fixture 범위는 CEO 승인 사실을 반영하며, 최종 계획 gate는 Lead가 처리합니다.

**설계**

- 기존 `private` schema에 고유 `private.task9_claim_bridge_<24hex>() RETURNS void`를 생성합니다. `SECURITY INVOKER`, `SET search_path = ''`를 사용하고 API 노출 schema는 `public`을 유지합니다.
- 기존 schema owner/ACL과 authenticated USAGE를 기록·검증합니다. 기존 권한은 변경하지 않습니다. 새 함수의 `PUBLIC EXECUTE`를 생성 transaction 안에서 revoke하고 **authenticated만 EXECUTE**를 부여합니다.
- 함수는 `current_user='authenticated'`, JSON object claims, 문자열 `role='authenticated'`, 문자열 canonical UUID `sub`를 모두 검증합니다. UUID는 소문자 8-4-4-4-12 hex와 `uuid` cast를 통과해야 합니다. 누락·잘못된 타입·malformed JSON·role 불일치는 SQLSTATE42501로 거절합니다.
- 검증 후 상태 변경은 `pg_catalog.set_config('request.jwt.claim.sub', sub, true)` 하나입니다. `auth.uid()`, role defaults, 기존 함수·ACL·product migration은 변경하지 않습니다.
- owned API env에 정확한 `PGRST_DB_PRE_REQUEST` 함수명을 넣습니다. anon은 함수를 실행할 권한이 없어 기존 403 거절을 유지합니다. anon 권한을 추가하지 않습니다.
- hook 적용 후 API health는 짧은 수명의 synthetic authenticated JWT로 OpenAPI를 읽습니다. signed `role`과 canonical UUID `sub`를 사용하며 auth row는 생성하지 않습니다. JWT는 owned mode0600 manifest에서 읽고 argv·로그·출력에 노출하지 않습니다. 응답에 불필요한 DB 쓰기가 발생하지 않아야 합니다.

권장안은 이 임시 bridge입니다. legacy image 교체는 exact image 승인 경로를 바꾸며, `auth.uid()` 교체는 production 인증 동작을 바꾸므로 현재 범위에 맞지 않습니다.

**실행 계획**

수정 파일은 `/Users/yunho/Desktop/project/token-planet/supabase/tests/shop_guest_import_v2_api.sh`와 `/Users/yunho/Desktop/project/token-planet/supabase/tests/shop_guest_import_v2_e2e.sh`입니다.

1. **TDD mocks:** 함수 사전 존재, authenticated USAGE 부재, 다른 DB identity, manifest 손상·소유권 불일치, CREATE/OID 기록/COMMIT 응답 소실, health 실패, DROP 불일치·재정리 경로를 RED→GREEN으로 검증합니다. authenticated health JWT 보호, anon403, 정확한 hook env도 포함합니다.
2. **DDL 전 manifest:** pinned DB/container/image/volume/binding, 함수 signature 초기 부재, schema owner/ACL, role settings, 기존 함수 정의·권한, history·row digest·전체 schema fingerprint와 예정 정의 hash를 보호 manifest에 원자 저장·flush합니다.
3. **생성 transaction:** `CREATE OR REPLACE` 없이 함수와 소유 함수 ACL을 생성합니다. 열린 transaction에서 실제 OID·owner·signature·definition hash·ACL·security/search-path 설정을 읽어 manifest에 저장·flush한 뒤 COMMIT합니다. COMMIT 전 연결 소실은 rollback, COMMIT 응답 소실은 저장된 OID로 판정·복구합니다.
4. **rollback SQL:** 정상 authenticated UID 일치, 오류 claims42501, transaction 종료 후 legacy GUC 복원, `auth.uid()`·기존 ACL 불변을 검증합니다. anon 거절과 pooled 요청 간 UID 누출 부재는 실제 HTTP에서도 확인합니다.
5. **독립 gate:** mocks·syntax/embedded Python compile·rollback SQL을 QA가 확인하고 fresh Reviewer가 권한·manifest·실패 복구·JWT 보호를 검토합니다. CLEAR 후 UID501로 approved exact API/E2E만 실행합니다. authenticated health, anon403, 원래 import→worker/cache→sharing/public scene acceptance를 모두 유지합니다.
6. **정리·복구:** 신규 요청 유입과 owned proxy/API를 중단하고 기존 owner-scoped fixture cleanup을 수행합니다. shim manifest는 DB 정리 완료까지 보존합니다. pinned DB와 OID·owner·signature·정의 hash·ACL·설정이 모두 일치할 때만 `DROP FUNCTION`을 CASCADE 없이 실행합니다. 실패·불일치는 manifest를 보존하고 비정상 종료합니다. 함수가 이미 없어도 전체 baseline 검증 후에만 파일을 제거합니다.

수용 조건은 actual E2E 성공과 임시 함수/API/proxy/fixture 부재, schema·ACL·settings fingerprint, history28/max20261003061830, row digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba` 및 기존 bindings 복원입니다. 다른 DB의 결과는 증거에서 제외합니다.

자체 검토에서 anon 권한 확대와 hook 적용 후 anonymous health 의존성을 제거했습니다. 남은 실행 전 확인은 authenticated의 실제 schema USAGE, fingerprint coverage, health JWT 보호와 pre-COMMIT manifest protocol입니다. 경로 문제는 발견하지 않았으며 파일 수정·live 작업은 수행하지 않았습니다.

Lead 통합: `/private/tmp/task9-sqlstate-cleanup-and-claims-qa.log`의 rollback-only boolean proof는 JSON claims에서 UID NULL, legacy setting에서 UID 일치를 확인했다. Exact HTTP403/42501은 `/private/tmp/task9-actual-e2e-sqlstate.log`에 status/code만 기록됐다. CEO는 reversible compatibility fixture와 위 authenticated-only/health 수정안을 승인했다. Native execution mode, 현재 checkout, QA→fresh Reviewer→live 순서를 유지한다. Product migration/기존 ACL 변경 및 push는 금지한다.


## Task9 actual PASS와 Task10 최종 증거 (2026-10-04)

이 절이 앞선 PARTIAL/환경 blocker/HTTP403 checkpoint를 대체한다. 승인된 exact PostgREST16.4 image와 UID501·127.0.0.1 API만 사용했다. 제품 migration/auth.uid/기존 ACL·role settings를 변경하지 않고, private의 run-owned invoker/empty-search-path/authenticated-only pre-request 함수가 검증된 JSON UUID sub를 transaction-local legacy GUC에 연결했다. manifest/OID/정의 hash/권한·전체 보호 baseline 확인 후 exact DROP(no CASCADE) 및 복원을 수행했다.

TDD 및 독립 gate: initial process-substitution RED는 함수 로딩 자체가 실패한 무효 scaffold였다. Coder 자신의 미검증 body만 제거해 기존 snapshot을 복원한 뒤 실제 owned source-prefix와 기존 stop positive control로 true RED→GREEN을 다시 수행했다. 이후 OID·fingerprint mismatch, 출력 한도, coalesced marker, COMMIT 응답 소실, absent/idempotent cleanup, lifecycle/readiness RED→GREEN을 기록했다. 최종 API SHA `b973f95a9c81714867e458d7bea7c4d33727277332fb45e812706b9a2a6b5b6a`의 독립 QA는 lifecycle14/create7/baseline8/identifier1/fixture cleanup/Python17/syntax PASS이며 fresh Reviewer CLEAR다. 로그 `/private/tmp/task9-auth-shim-readiness-final-independent-qa.log`.

첫 shim 실제 실행은 import/cache/initial sharing까지 성공한 뒤 current17 worker guard에서 실패했다. native fixture가 로컬 `account:<UUID>`를 bare UUID 입력인 worker에 넘겨 `account:account:<UUID>`를 비교한 원인이다. 실제 첫 guard를 사용하는 집중 RED는 prefixed 입력/remote0을 재현했고, cfg(test) current/late 호출 두 곳만 bare UUID로 수정했다. prefixed 거절/remote0과 bare 허용/planet_state1을 보존한 focused QA1/1 및 fresh Reviewer CLEAR 후 재실행했다. 제품 guard는 변경하지 않았다. 실패 후에도 독립 cleanup PASS다(`/private/tmp/task9-auth-shim-failed-run-cleanup-qa.log`).

**실제 Task9 PASS:** `/private/tmp/task9-actual-e2e-bare-account.log`, runner exit0, native ignored probe를 명시 실행해 **1 passed**, public boundary **denials3/viewer_reads3/database_digest_unchanged=true**. 실제 RPC 결과와 immutable prefix ACK·로컬 ownership/cache, hidden initial0, explicit sharing, current17 증가, late closed-cycle7의 lifetime 증가/current17·growth·단일 wallet credit 불변, recursive private-field deny를 검증했다. run `7227c20cf62d32a45b5055b2`, API127.0.0.1:58582의 owned service만 사용했다. 별도 disposable Tauri UI 환경이 없어 rendering은 미실행이다.

**독립 성공 cleanup PASS:** `/private/tmp/task9-auth-shim-success-run-cleanup-qa.log`. DB identity `postgres|postgres|5432`, pinned container/image/volume/config/기존55432 binding 유지. schema fingerprint `9e6a0f732a1def6cb355d6f946773c20`, complete history hash `345a066ea3b807a4a54b0d7609e6200a`(28/max20261003061830), 48-table digest `ce9ef4b37d29422d9d8cd8cb3c9d9bba`, auth0/world0/shim0. API name/label inventory0, proxy PID23531/run directory 부재. 별도 refs/containers가 없는 exact 새 image만 제거하고 inspect 부재를 검증했다(`/private/tmp/task9-approved-image-final-cleanup.log`). DB/volume/기존 network/binding 및 다른 resources는 제거하지 않았다.

**Task10 회귀:** 최신 독립 serialized offline Rust **380 passed/0 failed/1 ignored**, fmt/diff/hash PASS(`/private/tmp/task10-task9-final-rust-bare-arg-qa.log`); ignored1은 위 실제 Task9 실행으로 별도 PASS가 확보됐다. Desktop327/327(18files)·TS/Vite build47modules의 `/private/tmp/task10-qa-restored-desktop-{test,build}.log`, 전체 SQL40/1103 assertions 및 affected race4/4의 `/private/tmp/task10-qa-schema1-final-summary.json`/`task10-qa-schema1-final-race.log`를 해당 코드/SQL 불변 및 실제 cleanup의 schema/history/digest 복원 증거와 함께 재사용한다. SQL40에는 validation53/writer47/public27/schema1 hold15/public scene9와 기존 회귀가 포함된다. API shim은 제품 migration을 바꾸지 않았고 cleanup 후 기존 schema로 돌아왔다. 전체 SQL/race를 이번 actual run 뒤 다시 실행했다고 주장하지 않는다.

제한된 ordinary first-reset zero-effect 도메인의 로컬 성공만 검증했다. hosted/primary/deploy/merge/push는 미실행이며 broader domain/correction 재정산 및 UI rendering PASS를 주장하지 않는다. 최종 full-scope Reviewer `reviewer_task9_task10_final_acceptance`는 substantive P1/P2 findings 없음으로 CLEAR 판정했다. 독립 final resource QA(`/private/tmp/task9-final-resource-absence-qa.log`)는 exact image ref/ID 부재, pinned DB healthy·config/volume/bindings 불변, schema/history/48-table digest 동일 및 auth0/world0/shim0/API·proxy·run 부재를 다시 확인했다. Tasks5–10 acceptance 완료. Local commit은 이 Task9/10의 의도한 9개 파일만 포함하며 commit ID는 git log와 최종 보고에 기록한다. Push는 하지 않는다.
