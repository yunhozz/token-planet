# 행성 이동 안내와 기기 데이터 전체 초기화 명세

## 1. 목적과 승인된 범위

기존 “행성 초기화”를 “다음 행성으로”로 표시한다. 현재 주기의 토큰 정산, 지갑 적립, 쿨다운, 로그인 계정의 서버 확정과 재시도 규칙은 유지한다. 이동 확인창에는 현재 계정에 기록된 행성 순서 N과 이동할 N+1을 안내한다.

신규 “행성 데이터 전체 초기화”는 이 기기에 저장된 Token Planet 사용자 데이터와 소스 설정, 현재 서비스의 익명 키링 세션을 제거한다. 초기화 후 로그아웃된 새 guest 상태와 프로필 시작 화면으로 돌아간다.

사용자는 현재 익명 계정과 공동 세계에 다시 접근하지 못할 가능성을 수용했다.

서버 계정·행성·지갑·구매·일지·공동 세계·회원 관계, 다른 기기의 로그인, Codex·Claude 원본 로그는 변경하지 않는다. 서버 삭제·운영 DB 초기화·계정 복구·원본 로그 삭제·경제 규칙 변경·포렌식 보안 삭제는 범위에 포함하지 않는다.

## 2. 현재 구조와 근거

기준 workspace는 /Users/yunho/Desktop/project/token-planet이다.

| 근거 | 영향 |
|---|---|
| apps/desktop/src/App.tsx:448, :480 | 기존 이동의 계정·주기·세계·epoch 검증과 확인창을 유지·확장한다. |
| apps/desktop/src-tauri/src/lib.rs:1293 | reset_planet은 정산 command다. 전체 초기화와 분리한다. |
| apps/desktop/src-tauri/src/sync/worker.rs:369 | 로그인 이동은 영속 요청 ID로 재시도한다. |
| apps/desktop/src-tauri/src/lib.rs:1139, :1609 | 저장 계정 자동 복원과 백그라운드 스캔은 초기화 복구보다 먼저 실행하면 안 된다. |
| apps/desktop/src-tauri/src/storage/ledger.rs:193, :209 | world_timezone을 DB open 전에 읽고 Ledger.timezone으로 사용한다. |
| apps/desktop/src-tauri/src/collectors/discovery.rs:19, :47, :53 | 기본 소스 경로는 환경변수와 home을 사용하며 SourceConfig에는 경로·시간대가 있다. |
| apps/desktop/src-tauri/src/lib.rs:1399, :1420 | 소스 enable은 DB 설정이고 사용자 지정 경로는 DB와 메모리 SourceConfig에 반영한다. |
| apps/desktop/src-tauri/src/storage/guest_shop_import_v2.rs:470 | v2 가져오기 capture는 지연 생성되는 사용자 테이블이다. 존재할 때 반드시 삭제한다. |
| apps/desktop/src-tauri/src/sync/auth.rs:83 | 키링 세션의 정확한 항목 제거 기능이 필요하다. |
| apps/desktop/src-tauri/src/commands/sharing.rs:244 | 이후 사용자가 공유를 시작하면 새 익명 계정이 만들어진다. |

## 3. 저장 데이터 전체 매핑

### 3.1 기존 테이블 52개의 처리

storage 전체의 CREATE TABLE 선언을 대조한 결과다. 같은 테이블을 만드는 테스트 fixture 선언은 중복 집계하지 않았다.

**사용자 행 삭제·필요한 기본 행 재생성: 50개**

| 그룹 | 테이블 | 생성 근거 |
|---|---|---|
| 원장·전송 | source_checkpoint, usage_record, daily_agent_total, outbox_snapshot | storage/ledger.rs:223 |
| 행성·계정 | planet_object, planet_wallet_credit, planet_account_state, planet_usage_owner | storage/ledger.rs:249, storage/planet_accounts.rs:40 |
| 성장 일지 | growth_journal_state, growth_journal_cycle, growth_journal_entry, growth_journal_remote_cycle, growth_journal_remote_entry | storage/growth_journal.rs:21 |
| 기존 장식 | cosmetic_purchase, cosmetic_purchase_request, cosmetic_equipment, cosmetic_guest_import, cosmetic_shop_remote_state, cosmetic_pending_purchase | storage/cosmetic_shop.rs:153 |
| 상점·배치·장착 | shop_account_state, shop_purchase, shop_action_request, shop_landscape_instance, shop_landscape_placement, shop_landscape_edit_version, shop_avatar_owned, shop_avatar_equipment, shop_natural_removal, shop_natural_removal_debit, shop_remote_state, guest_shop_import_capture | storage/cosmetic_shop.rs:201 |
| 효과·기여·보상 | shop_effect_history, shop_effect_cycle_bound, shop_effect_cycle_bounds_state, shop_effect_timeline_state, shop_effect_contribution, shop_contribution_state, shop_activity_day, shop_game_reward, shop_wallet_credit, shop_cycle_settlement, shop_era_progress | storage/shop_effects.rs:967 |
| guest 계보 | guest_provenance_meta, guest_provenance_lineage, guest_provenance_occurrence_key, guest_provenance_occurrence_version, guest_provenance_mutation, guest_provenance_cycle, guest_provenance_reset_receipt | storage/guest_provenance.rs:36 |
| v2 가져오기 | guest_shop_import_v2_capture | storage/guest_shop_import_v2.rs:470 |

**개별 처리: setting**

기존 사용자 설정 행을 삭제하고 새 guest 기본 설정을 생성한다. 유일하게 기존 world_timezone을 기기 환경 설정으로 보존한다.

삭제에는 다음이 포함된다.

- 프로필·계정 선택·행성·기기 ID·쿨다운·정산 관련 설정.
- 공유 계정·세계·전송·캐시·중지 시점 설정.
- codex_custom_root, claude_code_custom_root.
- codex_active_root, claude_code_active_root.
- codex_enabled, claude_code_enabled.
- 대기 이동·구매·가져오기와 관련된 저장 상태.

일부 key prefix만 선택해서 지우지 않는다. world_timezone 외의 기존 사용자 설정을 모두 제거한다. 새 복구 제어 메타데이터는 별도 테이블에 둔다.

**유지: shop_schema_version**

사용자 상태가 아닌 스키마 관리 데이터로 유지한다. 스키마·인덱스·앱의 기본 카탈로그와 리소스도 유지한다.

### 3.2 누락 방지와 지연 생성 테이블

초기화는 등록된 테이블만 처리한다. 등록 항목이 아직 생성되지 않았으면 해당 삭제를 건너뛴다. guest_shop_import_v2_capture가 없는 것은 오류가 아니다. 존재하면 capture·attempt·held·imported 상태 전체를 삭제한다.

등록되지 않은 비내부 테이블을 발견하면 사용자 삭제 전에 실패시켜 누락을 알린다. 이름 패턴으로 임의 테이블을 삭제하지 않는다.

스키마 비교 검증은 모든 비내부 테이블이 다음 중 하나로 분류되는지 확인한다.

- 사용자 행 삭제·재생성.
- setting의 명시적인 환경 설정 보존.
- 스키마 관리 데이터 유지.
- 신규 초기화 제어 테이블 유지.

새 사용자 테이블을 추가할 때 초기화 등록도 함께 수정해야 한다.

## 4. 시간대와 소스 설정

### 4.1 시간대

world_timezone은 기기 환경 설정으로 보존한다. 전체 초기화에서 현재 OS 시간대로 변경하지 않는다.

초기화 전 저장된 world_timezone, 열린 Ledger.timezone, SourceConfig.timezone의 일치를 확인한다. 불일치·손상은 사용자 삭제 전에 오류로 처리한다. 정상 초기화는 이 시간대를 새 planet_timezone과 guest 보상 시간대에도 사용한다.

이 선택은 열린 Ledger의 원장 날짜 계산과 설정을 일치시키고, 재시작 시 Ledger::saved_timezone()이 같은 값을 읽게 한다. 초기화 안내는 소스·사용자 설정이 초기화되며 기기 시간대는 보존됨을 명시한다.

### 4.2 소스 기본값

DB의 custom root와 active root를 제거한다. 두 agent의 enable 설정은 최초 실행 기본값인 활성 상태로 복원한다. 현재 agent_enabled()는 설정 부재를 true로 해석하므로 부재 상태로 복원할 수 있다.

기본 경로는 다음과 같다.

- Codex: CODEX_HOME/sessions, 환경변수가 없으면 home의 .codex/sessions.
- Claude: CLAUDE_CONFIG_DIR/projects, 환경변수가 없으면 home의 .claude/projects.

환경변수 자체나 외부 폴더는 변경하지 않는다.

### 4.3 반영 순서

초기화 배타적 실행권을 얻은 뒤, 삭제 전에 RootOptions::from_env(None, None, 보존 시간대)와 resolve_roots로 새 SourceConfig를 준비한다. 경로가 없다는 것은 일반 source health 상태이며 초기화 실패 사유가 아니다. home 등 필수 환경을 해석할 수 없으면 삭제 전 실패한다.

DB 초기화 트랜잭션이 성공한 후, 정상 스캔을 허용하기 전에:

1. AppState.config를 새 SourceConfig로 교체한다.
2. latest와 source health 캐시·실패 상태를 무효화한다.
3. Ledger의 growth_journal_signature 등 메모리 파생 캐시를 초기화한다.
4. 새로운 세대의 guest 시작 상태를 구성한다.
5. 그 뒤에만 일반 스캔을 허용한다.

enable은 DB에서 읽는 값이며 SourceConfig의 필드가 아니다. DB enable 초기화와 메모리 root 초기화를 각각 검증한다.

폴더 선택창이 초기화보다 먼저 열리고 나중에 닫히는 경우, 선택 결과를 저장하기 전 세대를 재검증해 옛 사용자 경로를 폐기한다. OS 폴더 선택창을 기다리는 동안 lifecycle 실행권을 계속 잡아 초기화를 막지 않는다.

## 5. 초기 상태와 삭제 제외

완료 상태는 다음과 같다.

- 계정 local, 프로필 없음.
- 새 cycle·device·guest lineage ID.
- 새 activation·초기화 경계.
- 지갑·구매·보상·일지·장착·배치 없음.
- 이전 쿨다운·요청·가져오기·공유 캐시 없음.
- 소스 기본 경로, 두 agent 활성, 기존 원장 시간대.
- 로컬 키링 세션 없음.
- 익명 계정을 자동 생성하지 않음.

모든 계정의 로컬 캐시를 삭제한다. 서버 계정·세계와 다른 기기 상태는 보존한다.

삭제는 앱이 기존 사용자 상태를 조회·복원하지 못하게 하는 기능이다. 원본 로그나 디스크 포렌식 수준의 삭제는 제공하지 않는다.

## 6. 저장계층과 복구 상태

열린 DB 파일을 삭제·교체하지 않는다. 기존 SQLite 연결에서 사용자 데이터 삭제와 새 guest 구성을 하나의 트랜잭션으로 수행한다. 외래키 순서를 준수하며 foreign_keys를 해제하지 않는다.

기존 생성 로직을 재사용할 수 있는 트랜잭션 내부 경로를 제공한다. Ledger::open 재호출만으로 초기 guest lineage 생성 조건을 만족한다고 가정하지 않는다.

별도 제어 레코드에는 다음만 저장한다.

- 초기화 request_id.
- 로컬 generation.
- phase: idle, pending, local_committed, completed.
- cutoff_at_utc.
- 새 cycle·device·lineage ID.
- 대상 키링을 특정하는 서비스 식별자.

토큰·기존 프로필·사용량을 제어 레코드에 저장하지 않는다.

실행 순서:

1. 예상 세대 검증 및 배타적 실행권 획득.
2. 진행 중인 데이터·동기화·세션 작업 종료 대기.
3. 시간대 일치 확인과 기본 SourceConfig 준비.
4. 새 세대·경계·식별자와 pending 영속 기록.
5. 대상 키링 세션 제거.
6. SQLite 트랜잭션에서 사용자 데이터 삭제·기본 guest 생성·local_committed 기록.
7. 메모리 설정과 snapshot 복구.
8. completed 기록, 결과·이벤트 전달, 정상 작업 허용.

재시도는 같은 식별자와 경계를 사용한다. local_committed 이후에는 삭제를 반복하지 않는다. pending 기록 이후 취소는 제공하지 않는다.

시작 시 제어 상태를 계정 복원·세션 갱신·스캔·일지·보상·동기화보다 먼저 확인한다.

- pending: 키링 제거와 로컬 초기화를 이어간다.
- local_committed: 새 설정과 메모리를 복구한다.
- completed 또는 초기화 이력 없는 idle: 정상 시작한다.
- 제어 상태 접근·복구 실패: 정상 작업을 차단하고 오류를 표시한다.

## 7. 동시 실행과 다중 창

일반 데이터 작업과 초기화 사이에 공통 lifecycle 실행 경계를 둔다. 초기화는 배타적으로 실행하고 일반 작업은 현재 세대에 귀속된다.

잠금 순서는 lifecycle → sync_gate → SESSION_GATE → 짧은 로컬 mutex로 통일한다. 내부 함수는 보유한 lifecycle 잠금을 다시 얻지 않는다. 네트워크 대기 동안 SQLite mutex를 유지하지 않는다.

백그라운드 계정 복원·스캔은 기존 sync_gate 밖에서도 실행되므로 별도로 lifecycle 경계에 포함한다.

진입 때 받은 세대와 실행 시 세대가 다른 대기 작업은 종료한다. 파일 선택처럼 사용자 응답을 기다리는 단계는 실행권을 놓고, 실제 변경 직전에 세대를 다시 검증한다.

로컬 응답과 데이터 이벤트에는 세대를 연결한다. 서버 RPC에는 보내지 않는다.

모든 창은 세대 변경 시:

- context epoch·요청 ID를 무효화한다.
- snapshot·상점·일지·공유·견적·선택·미리보기·pending 상태를 비운다.
- 옛 세대의 성공·실패·이벤트를 무시한다.
- 새 세대로 상태를 재조회한다.

숨겨진 창도 활성화될 때 백엔드 세대를 확인한다. 이벤트 누락을 안전성의 전제로 삼지 않는다.

초기화 전 이미 실행 중인 서버 작업은 안전하게 끝나도록 기다린다. 서버에 확정된 작업을 취소했다고 안내하지 않는다. 이전 대기 작업은 초기화 뒤 재전송하지 않는다.

## 8. 인증과 서버 API

SessionStore에 정확한 현재 서비스 항목 제거 기능을 추가한다.

- 이미 없음: 성공.
- 키링 접근·제거 오류: 실패.
- 저장 JSON 손상: 파싱하지 않고 항목 제거 가능.
- 서비스 식별 불가: 성공으로 표시하지 않음.

전체 초기화는 restore_saved_planet_account나 세션 갱신을 먼저 호출하지 않는다. guest와 손상된 저장 세션 상태에서도 가능해야 한다.

현재 서비스의 키링 항목만 제거한다. 다른 서비스 URL의 항목을 탐색·삭제하지 않는다.

서버 로그아웃·토큰 폐기·계정 삭제·reset_my_planet·delete_my_growth_journal·delete_synced_usage·세계 탈퇴·해산·회원 삭제를 호출하지 않는다.

완료 후 일반 작업은 옛 메모리 세션을 사용할 수 없다. 이후 사용자가 공유 시작을 선택하면 새 익명 계정이 만들어진다.

## 9. 과거 로그 재생 방지

초기화 경계는 정확한 UTC 시각으로 저장한다. 새 게임·공유 기여는 occurred_at_utc > cutoff_at_utc 기록만 허용한다.

현재 guest 계산의 같은 초 inclusive 경로가 이 경계를 우회하지 못하게 한다. 일반 “다음 행성으로” 이동의 기존 경계 규칙은 유지한다.

경계는 다음에 적용한다.

- 현재·누적 행성 성장과 지갑 정산.
- 효과 기여·활동일·시대·연속 보상.
- 새 게임 성장 일지.
- guest provenance와 import 후보.
- 새 계정·기기로 보내는 공유 집계·개인 기여.

과거 로그가 사용량 화면에서 다시 집계되는 것은 허용한다. 해당 집계가 게임·공유 적격성을 뜻하지 않는다.

재시작·소스 경로 변경·로그 재작성·프로필 생성·계정 전환으로 경계를 제거하지 않는다. 새 기기 ID로 과거 기여를 중복 업로드하지 않는다.

## 10. 행성 순서

순서는 현재 계정에 기록된 완료 행성 수에 현재 진행 행성 하나를 더한 값이다.

N = 유효한 고유 previous_cycle_id 수 + 1

현재 current_cycle_id는 완료 집합에 포함하지 않는다. 0토큰 정산도 하나로 센다. 성장 일지 배열·잔액·구매·사용 토큰으로 계산하지 않는다.

백엔드 snapshot에 다음을 전달한다.

- status: verified 또는 unknown.
- current: 검증된 양의 정수 또는 null.
- snapshot과 동일한 계정·주기·로컬 세대에 귀속.

상황별 규칙:

- 새 guest와 완전한 빈 서버 기록: 1.
- 정상 guest: 계정 정산 집합으로 계산.
- 로그인 계정: 같은 계정·주기의 완전한 canonical 서버 기록 또는 그 검증된 캐시로 계산.
- 기존 로컬·원격 합집합만 있는 캐시: 완전성을 확인할 때까지 unknown.
- 빈 ID·충돌 중복·현재 주기 완료 기록·알려진 누락·명백한 이동 이력 불일치: unknown.
- 유효한 legacy 기록: “기록된 순서”로 계산. 금액 provenance 검증을 의미하지 않는다.
- 완료된 import: canonical 정산 집합에 포함된 고유 주기만 계산. 대기 payload를 추가하지 않는다.
- 계정 변경: 해당 계정 기록만 사용.
- 일지 삭제·잔액 소비: 변화 없음.
- 전체 초기화: 새 guest는 1, 서버 계정은 변화 없음.
- 사용량 incomplete: 순서 완전성과 별도로 판단.

정확한 중복을 고유 ID로 정규화하되 서버 검증 규칙을 위반한 응답을 정상 기록으로 사용하지 않는다. 별도 경제 순번 카운터는 만들지 않는다.

확인창:

> 현재 N번째 행성입니다. N+1번째 행성으로 이동할까요? 확인된 이번 행성 토큰은 지갑에 적립되고, 새 행성을 자연 생태계부터 시작합니다.

순서 미확정:

> 현재 행성 순서를 확인할 수 없습니다. 다음 행성으로 이동할까요? 확인된 이번 행성 토큰은 지갑에 적립되고, 새 행성을 자연 생태계부터 시작합니다.

미확정만으로 기존 이동 적격성을 변경하지 않는다.

확인 때 계정·주기·세대를 캡처하고 command에서 예상 context를 검증한다. 다른 창·기기의 이동으로 주기가 바뀌면 새 주기를 자동 이동하지 않고 재조회·재확인을 요구한다. 재시도는 순서를 중복 증가시키지 않는다.

## 11. UI

기본 이동 버튼은 “다음 행성으로”, 관련 완료·재조회 문구도 이동 표현으로 바꾼다.

전체 초기화 버튼은 메인 설정 영역에서 프로필 설정 전·guest·로그인 상태 모두 접근 가능하게 한다. 저장소가 사용 가능한 사용량 조회 실패 화면에서도 접근 가능하다.

확인창:

> 이 기기의 Token Planet 데이터를 모두 초기화하고 로그아웃합니다. 로컬 행성·지갑·구매·일지·사용량 저장 데이터와 소스 설정이 삭제됩니다. 원장 시간대는 유지됩니다. 서버와 공동 세계의 데이터, Codex·Claude 원본 파일은 남습니다. 현재 익명 계정은 이 앱에서 다시 접근하지 못할 수 있습니다. 계속할까요?

확인 버튼: “전체 초기화 및 로그아웃”.

원본 로그 재스캔으로 사용량 집계가 다시 표시될 수 있다는 안내를 함께 제공한다. 초기화는 기존 이동 쿨다운에 제한받지 않는다.

완료 후:

> 이 기기의 데이터 초기화와 로그아웃이 완료되었습니다. 새 로컬 행성을 시작할 수 있습니다.

진행·복구 중 관련 작업을 차단한다. 저장 초기화 완료를 사용량 재스캔 성공에 종속시키지 않는다.

## 12. 실패와 재시도

| 실패 | 결과와 처리 |
|---|---|
| 실행권·환경·시간대·진행 상태 준비 실패 | 사용자 삭제 전 실패. 재시도 가능. |
| 키링 제거 실패 | DB 사용자 데이터 유지. 진행 상태 유지, 정상 작업 차단, 재시도. |
| 키링 제거 후 DB 실패 | 로그아웃만 완료된 부분 실패. 복원·스캔·동기화 차단 후 같은 작업 재시도. |
| 트랜잭션 중 종료 | commit 여부에 따라 pending 또는 local_committed에서 복구. |
| DB 완료 후 SourceConfig·메모리 갱신 실패 | 삭제 확정. 정상 작업 차단, 메모리 복구만 재시도. |
| 완료 후 이벤트·화면 조회 실패 | 완료와 화면 갱신 필요를 구분. 삭제 반복 금지. |
| 완료 후 소스 읽기 실패 | 초기화 완료 유지. 일반 source health·사용량 오류 표시. |
| DB·제어 상태 자체 접근 불가 | 완료 판단 불가. 임의 DB 파일 삭제 금지. |

오류에 세션·원본 로그 내용을 포함하지 않는다. 실패 복구가 새 익명 계정을 만들거나 이전 계정으로 자동 복귀하지 않는다.

## 13. 영향 파일

- App.tsx, types/usage.ts, hooks/useShopActions.ts와 상태 소비 경로: 문구·결과·세대 무효화.
- src-tauri/src/lib.rs: command, lifecycle, startup, 기본 소스 구성과 메모리·트레이 상태.
- storage/ledger.rs 및 신규 로컬 초기화 모듈: 전체 등록 목록, 제어 상태, 원자적 삭제·재생성.
- storage/planet_accounts.rs, cosmetic_shop.rs, growth_journal.rs, shop_effects.rs, guest_provenance.rs, guest_shop_import_v2.rs, outbox.rs: 저장 상태·경계·재생성 계약.
- collectors/discovery.rs: 소스 기본값 해석과 게임 기여 경계 적용 경로.
- sync/auth.rs, sync/worker.rs, commands/sharing.rs 및 관련 command: 키링·실행 경계.
- domain/planet.rs와 snapshot 구성: 순서 및 완전성 근거.
- 관련 회귀·초기화 테스트.

서버 migration·RPC·배포 변경은 필요하지 않다.

## 14. 수용 기준과 검증

1. 기존 이동 정산·쿨다운·계정 검증·재시도·화면 실패 처리가 보존된다.
2. 확인창은 N/N+1 또는 미확정 문구를 표시하고 취소·실패·재시도가 중복 증가시키지 않는다.
3. 계정 전환·일지 삭제·구매·잔액 변화가 순서를 오염시키지 않는다.
4. 초기화 대상 등록이 실제 52개 기존 테이블과 신규 제어 테이블을 모두 설명한다.
5. 지연 생성 v2 capture가 있는 경우 모든 상태가 삭제되고, 없는 경우도 성공한다.
6. 모든 계정의 로컬 데이터가 삭제되고 새 guest 기본 상태가 생성된다.
7. world_timezone, Ledger.timezone, SourceConfig.timezone이 초기화·재시작 후 일치한다.
8. custom/active root와 enable 설정이 초기화되며, 앱을 재시작하지 않아도 기본 경로·활성 상태를 사용한다.
9. 늦게 완료된 폴더 선택이 옛 사용자 경로를 다시 저장하지 않는다.
10. 대상 세션만 제거하고 세션 부재는 성공으로 처리한다.
11. 서버 파괴적 API·익명 계정 생성·외부 파일 변경을 수행하지 않는다.
12. 과거·동일 시각·동일 초 로그가 성장·보상·일지·import·공유 기여를 되살리지 않는다.
13. 세대가 다른 command·응답·이벤트가 데이터와 여러 창을 오염시키지 않는다.
14. 각 부분 실패와 프로세스 종료가 재시도·재시작으로 일관되게 복구된다.
15. 취소 시 저장 데이터·키링·설정·대기열이 변경되지 않는다.
16. 프로필 설정 전·guest·로그인 상태에서 사용 가능하며, 새 guest는 1번째다.

TDD와 격리된 DB·주입 가능한 키링·동기화 API·제어된 지연 응답을 사용한다. source 기본값 검증에는 환경변수 있음/없음과 기존 custom root·비활성 설정을 포함한다.

데이터 손실과 lifecycle 위험으로 Coder → 독립 QA → 최종 전체 변경 Reviewer → CEO acceptance를 적용한다. 실제 제품 키링이나 운영 서버 데이터를 검증 목적으로 지우지 않는다. 실행하지 못한 검증은 미확인으로 보고한다.

## 15. 선택 이유와 자기 검토

기존 연결의 트랜잭션 초기화를 선택해 파일 교체·열린 연결·WAL 처리 위험을 줄인다. 키링과 DB의 비원자성은 영속 진행 상태로 복구한다. 원장 시간대는 환경 설정으로 보존해 열린 Ledger와 재시작의 일관성을 유지한다.

CEO 자기 검토 결과:

- guest_shop_import_v2_capture를 명시적으로 추가했다.
- storage의 52개 테이블을 사용자 상태 50개, setting 1개, shop_schema_version 1개로 매핑했다.
- 시간대는 OS 기본값 재적용이 아니라 보존으로 정했다.
- DB enable과 메모리 root가 별도 상태인 점, commit 후 스캔 전에 설정을 갱신하는 순서를 명시했다.
- 폴더 선택 대기 중 잠금을 유지하지 않고, 선택 완료 시 세대를 재검증한다.
- DB commit 이후 메모리 설정 실패에서도 옛 config로 스캔하지 못하게 했다.
- 행성 순서는 계정에 기록된 완료 주기 수와 완전성에 따르며, 일지 순서와 분리했다.
- 서버 삭제·계정 복구·경제 변경을 추가하지 않았다.
- 남은 사용자 범위 선택은 없다. 이 문서는 written-spec 승인 대상이다.
- 검증은 소스 기반 설계 검토만 했다. 테스트·DB 변경·키링 작업은 실행하지 않았다.

이 명세의 수용 후 Planner가 writing-plans로 구현 계획을 작성한다.
