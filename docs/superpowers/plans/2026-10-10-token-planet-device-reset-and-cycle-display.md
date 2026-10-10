# 행성 이동 안내와 기기 데이터 전체 초기화 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.
>
> **Harness routing:** 이 저장소에서는 CEO가 승인·handoff·통합을 조정한다. Coder가 구현·테스트 파일을 편집하고 QA와 Reviewer는 읽기 전용으로 검증한다. 태스크별 agent·리뷰·커밋을 자동으로 추가하지 않는다.

**목표:** 기존 행성 이동을 유지하면서 현재 순서를 안내하고, 서버와 원본 로그를 보존하는 복구 가능한 기기 초기화와 로컬 로그아웃을 제공한다.

**구조:** SQLite 영속 phase와 트랜잭션, 현재 서비스 키링 항목 제거, 공통 lifecycle 경계로 초기화를 조정한다. 로컬 세대로 비동기 응답과 여러 창을 보호하고 엄격한 cutoff로 과거 기여를 차단한다. 행성 순서는 서버 모델과 분리된 로컬 snapshot에 전달한다.

**기술:** Rust, Tauri 2, rusqlite, tokio sync, keyring 3.6, React 19, TypeScript, Vitest.

**명세:** /Users/yunho/Desktop/project/token-planet/docs/superpowers/specs/2026-10-10-token-planet-device-reset-and-cycle-display-design.md

## 전역 제약

- 실행 workspace와 Git 준비는 CEO handoff에서 승인된 대상을 사용한다.
- 구현·설정·테스트 파일 편집은 Coder 소유다.
- 기존 테이블 52개를 분류한다. 사용자 테이블 50개는 삭제·재생성, setting은 world_timezone만 보존, shop_schema_version은 유지한다.
- guest_shop_import_v2_capture가 없으면 건너뛰고, 있으면 모든 행을 삭제한다.
- DB 파일을 삭제·교체하거나 foreign_keys를 해제하지 않는다.
- provenance 보호 트리거는 reset 트랜잭션 안에서만 처리하고, 보호 제약이 복구되지 않으면 commit하지 않는다.
- world_timezone, Ledger.timezone, SourceConfig.timezone을 보존·일치시킨다.
- 사용자 custom/active root를 제거하고 두 agent를 활성 기본값으로 복원한다.
- 기본 경로는 기존 RootOptions::from_env(None, None, timezone)와 resolve_roots를 사용한다.
- phase는 idle, pending, local_committed, completed다. 재시도는 같은 request, cutoff와 생성 ID를 사용한다.
- 잠금 순서는 lifecycle → sync_gate → SESSION_GATE → 짧은 로컬 mutex다.
- 복구는 계정 복원·세션 갱신·스캔·일지·보상·동기화보다 먼저 수행한다.
- 초기화 cutoff 이후의 기록만 게임·공유 기여에 허용한다: occurred_at_utc > cutoff_at_utc.
- 전체 초기화에서는 서버 파괴적 API, 서버 로그아웃, 새 익명 계정 생성을 호출하지 않는다.
- Codex·Claude 원본 로그, 외부 폴더, 다른 서비스의 키링 항목을 변경하지 않는다.
- 복구 시에는 pending 상태에 저장한 키링 식별자를 사용해 같은 항목을 찾는다. 현재 환경변수로 다른 서비스 항목을 추정하지 않는다.
- domain::planet::PlanetState의 필드·serde·서버 wire와 RPC 키를 변경하지 않는다.
- 기존 정산·쿨다운·요청 ID·재시도 계약을 유지하고 새 제품 의존성·서버 migration·배포를 추가하지 않는다.
- 검증은 격리 DB, 가짜 키링, 가짜 API를 사용한다. 환경 설정 실패는 의도된 RED로 취급하지 않는다.
- 최종 흐름은 Coder → QA → 전체 변경 Reviewer → CEO acceptance다.

## Review Focus

1. 지연 capture, 미등록 테이블, immutable provenance 트리거가 삭제 누락이나 보호 제약 상실을 만들지 않는가? Task 1에서 검증한다.
2. 키링 제거 뒤 DB 오류나 앱 종료가 나도 옛 계정을 복원하지 않고 같은 초기화를 이어가는가? Tasks 3·5에서 검증한다.
3. 폴더 선택창, 숨겨진 창, 늦은 응답이 옛 경로·견적·일지·오류를 되살리지 않는가? Tasks 4·7에서 검증한다.
4. cutoff와 같은 초의 로그 및 재작성 로그가 보상·import·새 기기 업로드를 부활시키지 않는가? Tasks 2·8에서 검증한다.
5. 빈 signed 캐시·legacy/import 이력은 unknown으로 처리하고 순번 필드는 서버 wire에 포함하지 않는가? Tasks 6·7에서 검증한다.

## 파일 책임과 공통 계약

| 파일 | 책임 |
|---|---|
| 신규 apps/desktop/src-tauri/src/domain/device_reset.rs | 초기화 phase·세대·context·결과 DTO |
| 신규 apps/desktop/src-tauri/src/domain/planet_ordinal.rs | 로컬 순서 DTO |
| 신규 apps/desktop/src-tauri/src/storage/device_reset.rs | 테이블 등록, 제어 상태, 트리거, 원자적 초기화 |
| 신규 apps/desktop/src-tauri/src/lifecycle.rs | 일반 작업·초기화 실행권과 복구 차단 |
| 신규 apps/desktop/src-tauri/src/commands/device_reset.rs | Tauri adapter, coordinator, startup 복구 |
| storage 모듈과 collectors/discovery.rs | 기본 guest 재생성, cutoff, canonical 이력 |
| sync/auth.rs | 정확한 키링 제거와 테스트 주입 경계 |
| lib.rs, commands/*.rs, sync/worker.rs | 데이터 진입점, startup과 worker 경계 |
| growth.rs | 로컬 snapshot의 세대·순서 |
| 신규 apps/desktop/src/lib/localLifecycle.ts | 로컬 command envelope와 세대 수락 |
| 신규 apps/desktop/src/components/DeviceResetPanel.tsx | 초기화 안내·확인·진행·복구·완료 UI |
| App.tsx, App.css, types/usage.ts, lib/sharing.ts, hooks/useShopActions.ts | 여러 창 상태와 행성 이동 안내 |

공통 백엔드 계약:

    enum DeviceResetPhase { Idle, Pending, LocalCommitted, Completed }

    struct DeviceResetState {
        request_id: Option<String>,
        generation: u64,
        phase: DeviceResetPhase,
        cutoff_at_utc: Option<String>,
        new_cycle_id: Option<String>,
        new_device_id: Option<String>,
        new_lineage_id: Option<String>,
        service_id: Option<String>,
    }

    struct LocalContext { generation: u64 }
    struct ExpectedPlanetContext { generation: u64, account_id: String, current_cycle_id: String }
    struct LocalEnvelope<T> { generation: u64, data: T }
    struct LocalCommandError { generation: u64, code: String, message: String, details: Option<Value> }
    struct DeviceResetView { state: DeviceResetState, actions_blocked: bool, storage_completed: bool }
    struct DeviceResetResult { state: DeviceResetState, storage_completed: bool }

새 DTO enum은 snake_case로 직렬화한다. 오류 코드는 stale_generation과 reset_recovery_required를 구분한다. 기존 서버 확정 이동 오류 payload는 details에 보존한다.

## 작업 1: 영속 초기화와 provenance 트리거 보호

**파일:** 신규 domain/device_reset.rs, storage/device_reset.rs; 수정 domain/mod.rs, storage/mod.rs, storage/ledger.rs, storage/guest_provenance.rs와 기본 guest 재생성에 필요한 저장 모듈; 테스트는 storage/device_reset.rs 및 guest_provenance.rs.

**인터페이스**

    impl Ledger {
        fn device_reset_state(&self) -> Result<DeviceResetState, ScanError>;
        fn prepare_device_reset(&mut self, expected_generation: u64, service_id: &str,
                                cutoff: DateTime<Utc>) -> Result<DeviceResetState, ScanError>;
        fn commit_device_reset(&mut self, request_id: &str) -> Result<DeviceResetState, ScanError>;
        fn complete_device_reset(&mut self, request_id: &str) -> Result<DeviceResetState, ScanError>;
        fn local_generation(&self) -> Result<u64, ScanError>;
    }

제어 테이블은 device_reset_control이다. singleton 행, phase별 필수 필드와 generation overflow를 검증한다.

초기화 트랜잭션 안에서 기존 SQL 정의를 확인한 뒤 다음 세 trigger만 일시 제거한다: guest_provenance_occurrence_version_no_delete, guest_provenance_mutation_no_delete, guest_provenance_usage_delete. no_update 두 개, usage_update, response_replaces_snapshot은 유지한다. 행 삭제와 새 guest 생성 뒤 원래 정의를 복구하고 전체 trigger inventory와 foreign_key_check를 통과한 뒤 local_committed와 함께 commit한다. 정의가 예상과 다르거나 복구가 실패하면 transaction 전체를 rollback한다.

- [ ] immutable provenance 행이 있는 실패 테스트를 작성한다. local_reset_with_immutable_provenance_rows_succeeds는 성공, local_reset_protection_active_after_success는 UPDATE/DELETE 차단을 단정한다.
- [ ] local_reset_unknown_table_or_trigger_rejected_before_delete, local_reset_optional_capture_all_phases, local_reset_inventory_covers_schema를 추가한다. 미등록 테이블·알 수 없는 보호 trigger면 기존 행과 phase가 유지되고, 지연 생성 capture는 absent 또는 각 phase에서 안전하게 초기화되어야 한다.
- [ ] drop 직후·행 삭제 후·guest seed 중·trigger 복구 중 오류 주입 테스트를 추가한다. local_reset_rollback_restores_data_and_schema는 데이터와 trigger 정의가 모두 원래 상태인지 확인한다.
- [ ] local_reset_usage_delete_still_records_mutation_after_success와 local_reset_preserves_timezone_schema_version을 작성한다. 일반 삭제의 provenance 동작은 보존하고 환경 시간대와 shop_schema_version만 유지한다.
- [ ] RED 확인: cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml local_reset_
- [ ] 등록된 테이블만 FK 순서에 따라 삭제하고, 없는 등록 테이블은 건너뛰며, 저장된 새 ID·cutoff로 같은 transaction 안에서 guest 기본 상태를 생성한다. 기존 Ledger::open 재호출만으로 lineage 생성을 대신하지 않는다.
- [ ] GREEN에서 신규 reset 테스트 및 기존 guest provenance·account storage 테스트를 실행한다. 성공과 rollback 양쪽에서 trigger 정의·동작을 확인한 뒤 CEO에게 inventory와 fault-injection 결과를 보고한다. 이 단계에서는 UI를 노출하지 않는다.

## 작업 2: 엄격한 cutoff와 소스 기본값

**의존:** 작업 1. **파일:** storage/device_reset.rs, ledger.rs, growth_journal.rs, shop_effects.rs, guest_provenance.rs, guest_shop_import_v2.rs, outbox.rs, collectors/discovery.rs 및 관련 테스트.

**인터페이스**

    fn device_reset_cutoff(&self) -> Result<Option<DateTime<Utc>>, ScanError>;
    fn record_after_reset_cutoff(occurred_at: DateTime<Utc>, cutoff: Option<DateTime<Utc>>) -> bool;
    fn prepare_default_source_config(timezone: Tz) -> Result<SourceConfig, String>;

- [ ] local_reset_cutoff_exact_microseconds, local_reset_cutoff_same_second_not_inclusive, local_reset_cutoff_preserves_raw_usage, local_reset_cutoff_blocks_rewards_journal_import_upload, local_reset_cutoff_survives_reopen_source_rewrite_account_switch를 작성한다. 이전·동일 시각은 제외, cutoff 1µs 이후는 포함, 과거 raw usage는 표시되지만 게임 기여·보상·일지·provenance/import·outbox에는 포함되지 않고 경계가 재시작·경로 변경·계정 변경 뒤에도 유지되어야 한다.
- [ ] local_reset_default_roots_and_enabled로 custom/active root 제거, 두 agent 활성, 환경변수별 기본 경로, 원장 시간대 보존을 고정한다. RED 확인 명령:

    cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml local_reset_cutoff
    cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml local_reset_default

- [ ] 공통 cutoff predicate를 게임·공유 적격성 경로에 적용하고 cumulative fallback도 과거 누적값을 되살리지 않게 한다. cutoff 부재 시 기존 적격성은 유지한다. 기본 경로에는 기존 RootOptions와 resolve_roots를 재사용한다.
- [ ] GREEN에서 위 테스트와 기존 guest·outbox 테스트를 실행한다. 환경변수 테스트는 주입된 RootOptions 또는 직렬화로 병렬 간섭을 막는다. 일반 다음 행성 이동 경계와 기존 import 동작을 회귀 확인한다.

## 작업 3: 현재 서비스의 로컬 키링 제거

**의존:** 작업 1의 service 식별 계약. **파일·테스트:** apps/desktop/src-tauri/src/sync/auth.rs.

    trait LocalSessionRemover: Send + Sync {
        fn service_id(&self) -> Result<String, AuthError>;
        fn remove_local_session(&self) -> Result<(), AuthError>;
    }
    impl SessionStore {
        fn remove_local_session(&self) -> Result<(), AuthError>;
        fn from_service_id(service_id: &str) -> Result<Self, AuthError>;
    }

- [ ] service_id가 현재 Token Planet 키링 entry의 URL hash username임을 고정한다. 새 요청은 현재 AuthConfig에서 계산한 값과 일치해야 하며, 재시작 복구는 pending에 저장된 service_id로 동일 entry를 구성한다.
- [ ] local_reset_session_missing_is_success, local_reset_session_corrupt_json_can_be_removed, local_reset_session_targets_only_current_service, local_reset_session_delete_failure_is_error, local_reset_session_removal_has_no_auth_network_calls 테스트를 작성한다.
- [ ] RED 확인: cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml local_reset_session
- [ ] keyring 3.6 삭제 API로 현재 URL에 해당하는 항목만 제거한다. JSON parse·refresh·server signout 없이 수행하고 오류에 credential을 포함하지 않는다.
- [ ] GREEN에서 가짜 저장소를 사용해 멱등성·항목 격리·오류 동작을 검증한다. 실제 제품 키링은 사용하지 않는다.

## 작업 4: lifecycle과 로컬 세대

**의존:** 작업 1; 작업 5보다 먼저 완료. **파일:** 신규 src-tauri/src/lifecycle.rs, 수정 lib.rs, growth.rs, commands/cosmetic_shop.rs, commands/growth_journal.rs, commands/sharing.rs, sync/worker.rs 및 해당 테스트.

    impl LocalLifecycle {
        async fn enter(&self, expected_generation: u64)
            -> Result<OperationPermit<'_>, LocalCommandError>;
        async fn enter_reset(&self, expected_generation: u64)
            -> Result<ResetPermit<'_>, LocalCommandError>;
        async fn enter_recovery(&self) -> Result<ResetPermit<'_>, LocalCommandError>;
    }
    impl OperationPermit<'_> { fn generation(&self) -> u64; }

OperationPermit은 일반 read 실행권, ResetPermit은 배타적 write 실행권이다. startup recovery만 예상 세대 없이 진입한다. permit을 가진 내부 함수는 lifecycle을 재획득하지 않는다.

- [ ] 기존 작업 대기, 오래된 queued generation 거부, pending 복구 중 정상 command 차단, 백그라운드 restore·scan 보호, 폴더 선택 후 세대 재검증, nested acquire 방지, State<AppState>가 없던 invite/member session command 보호를 local_reset_lifecycle_* 테스트로 고정한다.
- [ ] RED 확인: cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml local_reset_lifecycle
- [ ] 모든 데이터·세션 command와 worker restore·scan·sync를 lifecycle 경계에 둔다. lock 순서는 lifecycle → sync_gate → SESSION_GATE → 짧은 로컬 mutex로 통일한다. dialog 응답을 기다릴 때 실행권을 놓고 실제 저장 직전에 세대를 재검증한다.
- [ ] 데이터 command는 LocalContext를 입력받고 LocalEnvelope 또는 LocalCommandError를 반환한다. 데이터 이벤트에도 generation을 붙인다. 기존 confirmed reset 오류 details는 보존한다. set_detail_view와 hide_popover는 데이터 command가 아니므로 제외한다.
- [ ] GREEN 및 오류 회귀: cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml local_reset_lifecycle, 이어서 reset_planet_response. 적용된 command·worker 목록과 잠금 순서를 CEO에게 보고한다.

## 작업 5: 초기화 coordinator와 startup 복구

**의존:** 작업 1~4. **파일:** 신규 src-tauri/src/commands/device_reset.rs, 수정 commands/mod.rs, lib.rs, storage/device_reset.rs 및 테스트.

    #[tauri::command]
    async fn get_device_reset_state(state: State<'_, AppState>)
        -> Result<LocalEnvelope<DeviceResetView>, LocalCommandError>;

    #[tauri::command]
    async fn reset_device_data(state: State<'_, AppState>, app: AppHandle, context: LocalContext)
        -> Result<LocalEnvelope<DeviceResetResult>, LocalCommandError>;

    #[tauri::command]
    async fn retry_device_reset(state: State<'_, AppState>, app: AppHandle,
                                request_id: String, context: LocalContext)
        -> Result<LocalEnvelope<DeviceResetResult>, LocalCommandError>;

    async fn recover_device_reset_before_startup(state: &AppState, app: &AppHandle)
        -> Result<DeviceResetView, LocalCommandError>;

    async fn run_device_reset(state: &AppState, session_remover: &dyn LocalSessionRemover,
                              action: DeviceResetAction, prepared_config: SourceConfig)
        -> Result<DeviceResetResult, LocalCommandError>;

상태 조회는 차단 중에도 허용한다. reset_device_data는 새 명시 요청, retry_device_reset은 저장된 request 재개, startup 함수는 정상 worker 전에 recovery를 실행한다. retry와 startup recovery는 pending에 저장된 service_id로 같은 SessionStore 항목을 구성한다. 내부 coordinator는 Tauri emit/tray와 분리해 fake remover로 테스트한다.

- [ ] happy path guest/signed, preflight 실패 무변경, 키링 실패 후 pending, 로그아웃 뒤 DB 오류 재시도, 각 phase 재시작, local_committed에서 재삭제 금지, config 실패 시 옛 config scan 차단, source folder 부재, 원격 파괴 API·signup 없음, double click 단일 request 테스트를 작성한다.
- [ ] RED 확인: cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml local_reset_coordinator
- [ ] spec의 prepare→pending→keyring→SQLite→memory→completed 순서를 구현한다. startup은 AppState 등록 후 recovery를 실행하고 성공했을 때만 정상 worker·계정 복원·scan을 시작한다. 기본 config·시간대 검증은 pending 전에 끝낸다.
- [ ] DB commit 뒤 config·latest·실패 상태·파생 cache 반영 전에는 일반 작업을 허용하지 않는다. 초기 guest snapshot은 외부 로그 scan 없이 만든다. shell과 recovery UI는 남겨도 복구 실패 중 정상 worker는 시작하지 않는다. event/UI 실패로 저장 초기화를 다시 실행하지 않는다.
- [ ] GREEN과 기존 account switch 회귀를 실행한다: local_reset_coordinator, account_switch. phase별 fault-injection 결과와 server 미호출 증거를 CEO에게 보고한다.

## 작업 6: 서버 모델과 분리된 행성 순서

**의존:** 작업 1·4. **파일:** 신규 domain/planet_ordinal.rs, 수정 domain/mod.rs, growth.rs, storage/ledger.rs, storage/planet_accounts.rs, sync/worker.rs, canonical 반영 경로, lib.rs; wire 테스트는 sync/client.rs.

    enum PlanetOrdinalStatus { Verified, Unknown }
    struct PlanetOrdinal { status: PlanetOrdinalStatus, current: Option<u64> }
    impl Ledger {
        fn planet_ordinal(&self) -> Result<PlanetOrdinal, ScanError>;
        fn record_canonical_planet_history(
            &mut self, account_id: &str, state: &PlanetState
        ) -> Result<(), ScanError>;
    }

WorldSnapshot에만 generation과 planet_ordinal을 추가한다. nested PlanetState와 서버 serde 계약은 변경하지 않는다. canonical 완료-cycle 집합과 완전성 근거는 계정·주기별 setting에 저장하고 별도 경제 순번 counter를 만들지 않는다.

- [ ] fresh guest·완전한 빈 remote history는 1, zero-credit 정산은 count, 정확한 중복은 dedupe, 충돌·현재 cycle 완료·알려진 누락은 unknown, signed merge cache는 canonical 전 unknown, legacy와 완료 import만 count, pending import 제외, 계정 변경·일지 삭제·잔액 소비에 불변, usage incomplete와 순서 독립, retry에서 한 번만 증가, stale context 거부 테스트를 추가한다.
- [ ] 로컬 snapshot에만 ordinal/generation이 있고 nested planet에는 없는지, PlanetState key set·upload p_state·server response 계약이 그대로인지, reset RPC body가 기존 p_request_id와 p_cycle_id뿐인지 고정하는 wire 테스트를 추가한다.
- [ ] RED 확인: cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml cycle_ordinal. 신규 snapshot 테스트는 실패하고 이미 성립한 기존 wire 불변 테스트는 통과할 수 있다.
- [ ] canonical response만 완전성 근거로 저장하고 유효한 고유 previous_cycle_id 수+1을 계산한다. known missing/mismatch는 unknown으로 둔다. reset_planet은 예상 generation·account·cycle을 서버 확인 전후에 검증하고, 변경됐으면 이동을 중단해 재확인을 요구한다.
- [ ] GREEN·이동 회귀: cycle_ordinal, reset. 서버 RPC body와 PlanetState serializer가 바뀌지 않았음을 보고한다.

## 작업 7: 프런트엔드 세대와 UI

**의존:** 작업 4~6. **파일:** 신규 apps/desktop/src/lib/localLifecycle.ts, components/DeviceResetPanel.tsx와 각 테스트; 수정 App.tsx, App.css, types/usage.ts, lib/sharing.ts, hooks/useShopActions.ts 및 기존 App·hook 테스트.

    type LocalEnvelope<T> = { generation: number; data: T };
    type LocalContext = { generation: number };
    type PlanetOrdinal = { status: "verified" | "unknown"; current: number | null };
    function invokeLocal<T>(command: string, args?: Record<string, unknown>): Promise<T>;
    function acceptLocalGeneration(generation: number): boolean;

WorldSnapshot 타입에 generation·planet_ordinal만 추가한다. ShopContext.local_generation을 써서 같은 계정·cycle에서도 초기화 후 hook 상태를 지운다.

- [ ] 늦은 성공·오류 무시, focus 시 missed reset event 재조회, cancel 시 invoke 없음, profile 전·guest·signed·usage error 화면에서 초기화 진입, 부분 실패의 동일 request 재시도, 완료 뒤 화면 실패에서 재삭제 없음, N/N+1·unknown 문구, 예상 context 전달, 캐시 초기화, confirmed reset 오류 판별을 테스트한다.
- [ ] RED 확인:

    npm --prefix apps/desktop test -- src/lib/__tests__/localLifecycle.test.ts src/components/__tests__/DeviceResetPanel.test.tsx
    npm --prefix apps/desktop test -- src/__tests__/App.test.tsx src/hooks/__tests__/useShopActions.test.tsx

- [ ] 모든 데이터 invoke와 공유 invite/member 호출을 공통 transport로 연결한다. 초기 bootstrap/recovery 조회 전에는 변경 작업을 차단한다. 초기 return 화면에도 공통 설정 진입을 제공한다.
- [ ] 승인된 한국어 문구를 사용하고 확인·진행·부분 실패·retry·저장 완료·화면 재조회 상태를 구분한다. 세대 변경 시 context epoch, request ID, 선택·견적·상점·일지·공유·미리보기 상태를 무효화한다. hidden window는 focus 때 재확인한다.
- [ ] GREEN 및 build: 위 테스트와 npm --prefix apps/desktop run build를 실행한다. UI 상태별 결과, 늦은 응답, 순번 문구와 기존 오류 보존을 CEO에게 보고한다.

## 작업 8: 통합·QA·최종 검토

**의존:** 작업 1~7. 제품 수정은 발견한 결함에 한정하며 담당 작업의 failing test부터 고친다.

- [ ] 격리 DB에서 다중 계정·구매·immutable provenance·v2 capture·custom/disabled source와 지연 요청을 준비한다. 키링 제거 뒤 DB 실패→재시작→동일 request 복구→trigger 복구·commit→기본 config 반영→과거 로그 재스캔을 연속 검증한다.
- [ ] rollback 시 데이터/schema 유지, 성공 후 trigger 보호, 과거 기여·import·outbox 부재, guest 1번째, 외부 fixture 불변, 모든 창의 늦은 응답 폐기, cutoff 뒤 새 기록만 적격, 서버 wire 불변을 단정한다.
- [ ] 전체 검증을 실행한다:

    cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
    npm --prefix apps/desktop test
    npm --prefix apps/desktop run build
    cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --check
    git diff --check

  모두 exit 0이어야 한다. native·환경 문제와 미실행 검증은 제품 실패와 구분해 보고한다.
- [ ] QA는 spec 수용 기준 1~16 각각에 테스트·명령·결과·증거·미확인 범위를 연결한다. 실제 키링·다중 창·트레이 검증은 CEO가 지정한 격리 환경에서만 수행한다.
- [ ] QA 후 Reviewer가 전체 diff에서 table/setting inventory, trigger 복구, startup 순서, lock/generation, 부분 실패, cutoff/import, server wire, canonical 순서, 외부·서버 보존을 독립 검토한다. 수정이 생기면 영향 QA와 최종 review를 반복한다.

## 실행 순서와 명세 coverage

    1 → 2
    1 → 3
    1 → 4
    1~4 → 5
    1·4 → 6
    4~6 → 7
    1~7 → 8 → QA → Reviewer → CEO acceptance

| 명세 | 작업 |
|---|---|
| §1·§11 이동·UI | 6·7·8 |
| §3 inventory·제약 | 1·8 |
| §4 시간대·소스·dialog | 1·2·4·5·7 |
| §5 새 guest·삭제 제외 | 1·3·5·8 |
| §6 phase·startup | 1·5·8 |
| §7 lifecycle·다중 창 | 4·7·8 |
| §8 인증·server 보존 | 3·4·5·6·8 |
| §9 cutoff | 2·8 |
| §10 순서·context | 6·7·8 |
| §12 실패·retry | 1·3·5·7·8 |
| §14 acceptance | 각 작업·8 |

공유 파일이 많으므로 한 Coder가 작업 순서대로 편집한다. 신규 초기화 UI는 저장·인증·lifecycle·복구가 완료되기 전에 노출하지 않는다.

## 자기 검토와 실행 전 확인

- 기존 52개 테이블, 지연 capture, 신규 제어 상태를 초기화 inventory에 포함했다.
- immutable trigger의 기존 데이터 삭제, 정상 이후 보호, 실패 rollback을 별도로 검증한다.
- 행성 순서는 로컬 snapshot에만 두며 PlanetState와 server RPC wire에 필드를 추가하지 않는다.
- 공통 인터페이스의 생산·소비 단계와 모든 태스크 의존 관계를 명시했다.
- Review Focus 다섯 항목과 spec 수용 기준을 테스트 작업에 연결했다.
- 이 계획은 execution target·테스트 실행 권한·검증 환경을 자동 승인하지 않는다. CEO는 문서 승인 후 별도 handoff에서 target과 권한을 확인한다.
- 루트 package.json은 없으므로 모든 npm 명령은 apps/desktop을 대상으로 한다.
- Planner는 계획 초안 작성을 위해 테스트·코드·데이터를 변경하거나 실행하지 않았다.
