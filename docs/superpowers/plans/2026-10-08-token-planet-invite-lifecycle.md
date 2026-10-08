# Token Planet 그룹 초대 lifecycle 구현 계획

- 작성일: 2026-10-08
- 상태: 기본 초대 lifecycle·CI runner 추가안 및 환경 차단 중 test-first 구현 진행 사용자 승인 (2026-10-08)
- 기준 설계: docs/superpowers/specs/2026-10-08-token-planet-invite-lifecycle-design.md
- 실행 저장소: /Users/ma-24-007/Desktop/workspace/token-planet

> 이 계획은 승인된 설계의 첫 구현 단계인 그룹 초대 lifecycle을 다룬다. 각자 행성, 정확한 개인 사용량·순위는 유지한다. 읽기 전용 방문과 공동 목표는 초대 기반 멤버십이 안정화된 뒤 별도 설계·계획으로 진행한다.

## 목표와 구조

현재 재사용 개인 코드로 가입하는 경로를 소유자 발급·정확히 168시간 만료·한 번의 성공 가입·소유자 철회가 가능한 초대로 교체한다. 가입 권한·멤버 정원·초대 소비는 PostgreSQL transaction에서 판정한다. Rust/Tauri가 인증 세션과 RPC를 담당하고 React는 초대 발급·복사·목록·철회와 가입 입력을 표시한다.

구현 원칙:

- 승인 설계의 64자리 16진수 난수 코드, hash-only 저장, 최대 10명, 계정당 한 그룹, 5회/15분 차단을 지킨다.
- 과거 migration을 편집하지 않고 새 forward migration만 추가한다.
- 기존 멤버십·개인 행성·사용량 집계·업로드·조회는 유지한다.
- 개인 코드 가입을 서버에서 차단하고 새 UI의 fallback 경로를 제거한다.
- 실제 UI 검증은 서로 다른 임시 A/B 앱 프로필과 같은 격리 Supabase 프로젝트에서 수행한다. 기존 프로젝트·앱 데이터는 대상에서 제외한다.
- 이 계획 승인만으로 production migration, 앱 배포, 외부 공개 또는 production rollback을 실행하지 않는다.

## CI runner 호환성 추가안 (과거 사용자 승인, 현재 실행 보류, 2026-10-08)

아래 runner 절차는 과거 승인 기록이다. 현재 구현 handoff에서는 CLI·Docker·DB·runner/CI 실행 및 변경을 금지한다. 후속 환경 검증은 별도 실행 승인과 차단 해소 전까지 보류한다.

사용자는 Supabase CLI 2.119.0이 격리 runner의 Docker guard에서 `PULL` 9회와 `PS` 1회 거부된 문제에 대해 guard/runner 범위 확장을 승인했다. 이 추가안은 실제 SQL RED를 얻기 위한 사전 작업이다. CLI `db start --help`는 이 명령이 로컬 PostgreSQL만 시작한다고 확인한다. 현재 `start --exclude`는 제외한 서비스 이미지도 pull할 수 있으며, 이는 Supabase CLI 이슈 #4194에도 보고돼 있다: https://github.com/supabase/cli/issues/4194.

승인되면 runner의 시작 명령을 `supabase db start --network-id <run-owned-network>`로 바꾸고 테스트용 fake CLI도 같은 계약을 검증한다. Docker guard는 계속 fail-closed로 유지한다. `PULL`은 허용하지 않고 이미 캐시된 pinned PostgreSQL 이미지만 사용한다. `db start`가 임의 서비스 이미지 pull이나 범위가 넓은 `PS`를 요청하면 guard를 우회하거나 일반 허용하지 않고 그 명령에서 중단한다. 추가 Docker 연산이 필요하면 실제 CLI 인자와 생성 리소스 소유권을 먼저 확인하고, 정확한 프로젝트 범위의 최소 동작만 테스트로 잠근다. 사전 승인된 CLI 외에 이미지나 의존성을 내려받지 않는다.

### 추가 변경 파일과 소유권

- Coder: `supabase/ci/run.sh`, `supabase/ci/test_run.py`; guard에 최소한의 검증된 조정이 필요한 경우에만 `supabase/ci/product_docker_guard.py`, 그 회귀 검사는 `supabase/ci/test_product_docker_guard.py`.
- CEO: 이 계획 추가안과 승인·실행 handoff 기록.
- 기존 Secretary 문서 변경은 그대로 보존하며 이 추가안 때문에 수정하지 않는다.

### 추가 작업: PostgreSQL 전용 CI 시작 경로

**담당:** Coder; CEO 계획·안전 경계 수용
**수용 기준:** `supabase/ci/run.sh`가 CLI 2.119.0에서 자체 소유 loopback DB를 시작하고, migration replay 전에 PULL/광범위 PS 거부 없이 종료한다. guard는 비승인 image pull 및 일반/무관한 container listing을 계속 차단한다.

1. `RunnerTests.test_success_uses_database_only_start_and_resets_tests_and_cleans_resources_in_order`를 `supabase/ci/test_run.py`에 추가한다. runner가 `start --exclude` 대신 `db start --network-id <owned network>`를 호출하고, 그 뒤 같은 network로 `db reset --local --no-seed`를 수행하는지 검증한다. `ProductDockerGuardTests.test_unapproved_pull_and_ps_are_denied_before_docker`에서는 비승인 PULL/PS가 실제 Docker 실행 전에 막히는지 검증한다.
2. 변경 전 다음 focused test를 실행해 runner가 아직 `start --exclude`를 호출하므로 실패하는 것을 확인한다: `python3 -m unittest test_run.RunnerTests.test_success_uses_database_only_start_and_resets_tests_and_cleans_resources_in_order -v` (작업 디렉터리: `supabase/ci`).
3. `run.sh`와 fake Supabase CLI fixture를 PostgreSQL 전용 `db start` 계약에 맞춘다. 기존 container/volume/network 소유권, loopback publish 확인, cleanup 순서, migration staging은 유지한다.
4. `python3 -m unittest test_run test_product_docker_guard -v` (작업 디렉터리: `supabase/ci`)가 통과한 뒤 임시 CLI 2.119.0으로 기존 runner를 실행한다. `db start`가 현재 guard에서 거부되면 sanitized operation family와 run-owned target만 확인한다. 추가 허용이 꼭 필요할 경우 exact argv/target을 제한하는 테스트를 먼저 만들고, `PULL`이나 광범위 `PS`는 허용하지 않는다. 그 조건으로 해결되지 않으면 추가 범위 승인을 다시 받기 전 중단한다.
5. SQL regression assertions를 제품 구현보다 먼저 작성한다. 현재 DB runner가 환경 차단 상태이므로 실제 SQL 기능 assertion RED 관측을 구현 시작의 선행 조건으로 두지 않는다. 사용자 승인에 따라 초대 migration·Rust·React 구현을 시작할 수 있으나, SQL RED/GREEN 미관측을 통과로 기록하지 않는다. 격리 DB 검증은 merge/release 전 필수다.

## 역할과 작업 소유

AGENTS.md에 따라 CEO가 범위·순서·수용을 조정하고 구현 파일을 직접 수정하지 않는다.

| 역할 | 책임 |
| --- | --- |
| CEO | 계획·실행 경계, 현재 checkout 확인, 계약 수용, QA 이후 전체 검토 조정 |
| Coder | 승인된 runner 호환성 파일, migration, SQL/Rust/React 테스트 코드와 구현. 지정된 파일 외 변경은 먼저 CEO에 보고 |
| QA | 독립 검증과 증거 기록. migration·제품 파일이나 테스트 환경 설정은 수정하지 않음 |
| Secretary | 승인된 구현과 QA 결과를 README·수용 기준·검증 기록에 반영 |
| Reviewer | QA 뒤 전체 diff를 독립 검토하고 보안·정합성 finding 보고 |

## 실행 전 경계

- 사용자는 2026-10-08 계획 승인 요청에 “ㅇㅇ”라고 답해 이 계획의 구현 진행을 승인했다. 이 승인은 production 배포·게시·rollback을 포함하지 않는다.
- 구현 handoff 때 branch가 feat/multiplayer-policy-verification인지, HEAD와 staged/unstaged 상태를 기록한다. 기존 변경 문서 6개를 보존하고 관련 없는 문서를 되돌리거나 stage하지 않는다.
- 대상 테스트 환경은 사용자가 승인한 임시 격리 환경이다. QA workdir은 /tmp/token-planet-multiplayer-qa-20261008이다. 실행 직전에 프로젝트 ID·컨테이너·volume·포트를 재확인하며 다른 프로젝트와 혼동되면 중단한다.
- migration replay와 pgTAP은 supabase/ci/run.sh가 생성하는 자기 소유의 임시 stack에서 수행한다. artifact 디렉터리는 저장소 밖의 새 빈 경로로 지정한다. 기존 사용자 Supabase stack에는 reset·stop·migration을 하지 않는다.
- CI runner 사전 작업은 PostgreSQL 전용 `supabase db start` 경로를 검증한다. 일반 `PULL`·광범위 `PS` 허용을 추가하지 않고, pinned PostgreSQL 이미지가 없거나 CLI 동작이 guard 경계를 벗어나면 중단한다.
- 과거 CI 검증용 CLI 2.119.0 임시 다운로드·실행 예외는 현재 구현 handoff에 적용하지 않는다. 현재는 Supabase CLI의 version/help/migration 명령, Docker·DB·runner/CI, 추가 다운로드를 실행하지 않는다. 새 forward migration은 기존 저장소 형식의 미사용 14자리 `YYYYMMDDHHMMSS_invite_lifecycle.sql` 이름으로 수동 생성한다. 과거 migration과 기존 QA/사용자 환경은 보존한다.
- 테스트는 임의 테스트 계정과 그룹으로 수행하며 실행별 식별자를 부여해 생성한 데이터만 정리한다. key·token·invite code·DB URL은 출력·문서화하지 않는다.
- CUA 앱 목록에서 A/B 프로필 식별이 불가능하면 UI 흐름을 시작하지 않는다. 그 경우 해당 기준은 미검증으로 남기고 다른 인증·접근 경로를 임의로 만들지 않는다.

## 환경 차단 중 test-first 구현 진행 (사용자 승인, 2026-10-08)

사용자는 “계획 수정하고 구현 시작”으로 실행 순서 변경을 승인했다. 현재 checkout은 `/Users/ma-24-007/Desktop/workspace/token-planet`, branch는 `feat/multiplayer-policy-verification`, 기준 HEAD는 `0ea6aea6e3120faf59fa9dc1c332cca157f35ecc`다. SQL regression assertions를 먼저 작성한 뒤 초대 lifecycle 구현을 진행한다. 이는 실제 SQL 기능 assertion RED 전 제품 구현 금지 조건을 대체하며, 테스트 선작성 요구는 유지한다.

현재 DB runner는 macOS strict sandbox에서 helper가 SIGABRT해 검증을 실행할 수 없는 상태다. SQL DB suite·DB 동시성·실제 A/B UI 검증은 `environment blocked / not run`으로 기록한다. 환경 차단은 기능 실패나 테스트 통과 증거가 아니며 SQL RED/GREEN, migration replay, 동시성 또는 두 사용자 앱 성공을 관측했다고 주장하지 않는다. Rust/React 검사는 실행 가능한 범위에서 실패 테스트 선작성과 관련 검증을 수행하고 결과·차단 범위를 따로 기록한다.

구현 범위는 승인된 초대 lifecycle만이다. 64자리 hex 난수 코드·hash-only 저장·정확히 168시간 만료·한 번의 성공 가입·소유자 철회·최대 10명·계정당 한 그룹·5회/15분 제한과 기존 데이터 보존 계약을 유지한다. 방문·공동 목표는 후속 설계로 남긴다. 이 승인은 Docker guard 우회, 추가 runner 변경·환경 재시도·다운로드 또는 production 변경을 승인하지 않는다. 기존 이미지·리소스·loopback 경계와 환경 실행 중단 조건을 유지한다.

**Merge/release 필수 gate:** 승인된 격리 DB에서 전체 migration replay·SQL DB suite·독립 세션 동시성 검증을 완료하고 실패 및 권한·원자성·deadlock finding을 해결한다. 실제 A/B UI를 포함한 미검증 I 기준은 별도 증거가 생길 때까지 미완료로 유지한다. 환경 차단 상태로 구현·검토를 진행할 수 있어도 DB 검증 없이 merge/release하거나 I01–I16 전체 완료를 수용하지 않는다.

## 변경 파일

### Coder

- 생성: supabase/migrations/ 아래 미사용 14자리 `YYYYMMDDHHMMSS_invite_lifecycle.sql` 이름의 forward migration 1개. Supabase CLI 없이 수동 생성한다.
- 수정: supabase/tests/invites.sql.
- 생성: supabase/tests/invite_lifecycle_concurrency.sh.
- 수정: apps/desktop/src-tauri/src/sync/client.rs.
- 수정: apps/desktop/src-tauri/src/commands/sharing.rs.
- 수정: apps/desktop/src-tauri/src/lib.rs.
- 수정: apps/desktop/src/lib/sharing.ts.
- 수정: apps/desktop/src/components/InvitePanel.tsx.
- 수정: apps/desktop/src/components/SharingSetup.tsx.
- 수정: apps/desktop/src/App.tsx.
- 수정: apps/desktop/src/components/__tests__/SharingPanels.test.tsx.
- 수정: apps/desktop/src/__tests__/App.test.tsx.
- 조건부 수정: apps/desktop/src/App.css. 기존 스타일로 상태를 표현할 수 없는 경우에만 필요한 규칙을 추가한다.

### Secretary

QA 전 CEO가 기존 미커밋 문서를 포함한 소유권 범위를 다시 확인한 뒤 작업한다.

- apps/desktop/README.md
- supabase/README.md
- docs/specs/token-planet-mvp.md
- docs/superpowers/specs/2026-09-26-token-planet-personal-world-design.md
- docs/release-acceptance.md
- docs/2026-10-08-multiplayer-verification.md
- 생성: docs/2026-10-08-invite-lifecycle-verification.md

문서는 실제 구현·실행 결과만 반영한다. 기존 V01–V13의 미검증 상태를 새 기능 결과로 덮어쓰지 않는다.

## 작업 순서

### 1. 초대 데이터·관리 RPC·권한

**담당:** Coder
**수용 기준:** I01–I05, I12 일부, I14의 migration 보존, I16의 DB 단위

1. 제품 구현 전에 SQL regression assertions를 먼저 작성한다. 현재 SQL 실행은 `environment blocked / not run`이므로 실제 RED 관측 없이 사용자 승인 범위의 구현을 시작할 수 있다. 소유자만 발급·목록·철회 가능한지, 비인증·일반 멤버·타 그룹 권한이 거부되는지, 직접 테이블 접근과 PUBLIC/anon 실행이 막혀 있는지 검증한다.
2. world_members.joined_via_invite_id nullable FK를 추가한다. public.world_invites(id)를 참조하고 초대 행 삭제 시 NULL이 되게 한다. 기존 멤버십 값은 NULL로 보존한다.
3. public.world_invites(world_id, created_at DESC, id)와 joined_via_invite_id의 non-NULL 부분 인덱스를 추가한다.
4. world_invites에 코드 원문을 저장하지 않는다. 32 random bytes를 lowercase hex로 표현하고 SHA-256 digest만 저장한다. 원문은 발급 성공 응답에서만 돌려준다.
5. 발급은 그룹 행 잠금 후 현재 owner와 정원을 확인한다. 발급 순간의 서버 시각을 한 번 측정해 created_at과 created_at + 168 hours를 저장한다. 발급은 멤버 자리를 예약하지 않는다.
6. 목록은 현재 owner에게만 ID·발급/만료/철회/사용 시각·계산 상태를 반환한다. 상태 우선순위는 used → revoked → expired → active다. 코드·hash·Auth 사용자 ID는 노출하지 않는다.
7. 철회는 그룹 → 초대 순으로 잠근 뒤 현재 owner와 초대 상태를 다시 확인한다. 미사용 초대만 철회하고 반복 철회는 이미 철회 상태를 반환한다. 가입 완료 멤버십은 바뀌지 않는다.
8. 기존 함수 반환 형식과 의존성을 확인한다. PostgreSQL에서 반환 열을 바꾸려면 필요한 함수만 명시적으로 교체하고 grant를 다시 부여한다. DROP ... CASCADE는 사용하지 않는다.
9. public RPC wrapper는 SECURITY INVOKER로 authenticated에만 허용한다. private SECURITY DEFINER helper는 고정 search_path와 schema-qualified 참조, 내부 auth.uid()/owner 검사를 갖춘다. private schema는 Data API 노출에서 제외한다. wrapper가 호출하는 데 필요한 helper execute만 authenticated에 허용하고 PUBLIC/anon 권한은 회수한다.
10. 역사적 pending 초대는 cutover에서 철회하되 사용·철회 기록은 보존한다. 개인 코드·계정·그룹·멤버십 행을 삭제하거나 변환하지 않는다.
11. 현재 pgTAP은 `environment blocked / not run`으로 기록한다. 승인된 격리 환경이 실행 가능해지면 merge/release 전에 전용 임시 DB에서 pgTAP을 실행해 GREEN을 확인한다. 원문 비노출, 정확한 만료 간격, 권한, 상태 판정, FK·인덱스, 과거 행 보존 결과를 기록한다.

### 2. 가입 원자성·동시성·재시도·legacy 차단

**담당:** Coder
**수용 기준:** I05–I14, I16의 DB 동시성

1. pgTAP 실패 사례를 추가한다: 정규화·입력 형식, 만료 경계, 초대 소비와 멤버십 rollback, 이미 그룹이 있는 사용자, 정원 초과, 같은 사용자 재시도, 타 사용자 재사용, 탈퇴 뒤 재사용, transfer 뒤 초대 상태.
2. 입력은 해시 계산 전에 bounded length를 확인한다. 계획 기본값은 공백 제거 전 256자 상한이며, 상한을 넘으면 초대 unavailable로 처리한다. 양끝 공백을 제거하고 대소문자를 소문자로 정규화한 값이 정확히 64자리 hex인지 검사한다.
3. 모든 가입 경로에서 user attempt row → world row → invite row 순으로 잠근다. 코드 hash로 world/invite ID를 잠금 없이 찾은 뒤 그룹 행을 먼저 잠그고 초대 행을 다시 읽어 그룹·owner·issuer·상태를 검증한다.
4. 잠금을 얻은 뒤 clock_timestamp()로 만료를 검사한다. 시작 시각에 고정되는 now()를 잠금 대기 후 만료 판정에 쓰지 않는다. 만료 시각과 같거나 이후이면 신규 가입을 거부한다.
5. 신규 멤버십과 used_at/used_by 갱신을 같은 transaction에서 처리한다. INSERT ... ON CONFLICT(user_id) DO NOTHING RETURNING으로 사용자당 한 그룹 경합을 처리한다. 삽입되지 않으면 already_member를 반환하고 초대를 소비하지 않는다. 예외 또는 transaction rollback도 부분 상태를 남기지 않아야 한다.
6. 응답 유실 retry는 used_by가 현재 auth.uid()와 같고, 현재 그 그룹에 있는 멤버십의 joined_via_invite_id가 같은 초대 ID일 때만 already_accepted로 성공 확인한다. 만료 뒤에도 이 확인은 허용하되 새 가입으로 계산하지 않는다.
7. 사용 시도는 기존 private.member_code_join_attempts 정책과 호환되게 한다. 15분 창 내 5회 무효 입력 후 15분 차단한다. 차단·실패 횟수 갱신은 정상 상태 반환으로 commit하고 이후 예외를 던져 rollback하지 않는다. already_member와 world_full은 추측 실패 횟수에 넣지 않는다.
8. owner transfer는 그룹을 먼저 잠근 뒤 pending invite를 ID 순으로 잠가 철회하고 역할·owner를 바꾼다. 미사용 초대는 소유권이 이전 소유자에게 되돌아와도 복원하지 않는다. 가입·철회·이전이 같은 그룹 → 초대 순서를 지킨다.
9. join_world_by_member_code의 기존 실행 권한과 가입 우회 경로를 서버에서 차단한다. 개인 코드 저장 데이터 및 사용한 이전 멤버십은 유지한다. 새 클라이언트는 개인 코드 조회·회전·가입 명령을 사용하지 않으며 초대 RPC 실패를 개인 코드로 fallback하지 않는다. 개인 코드 read/rotate 함수는 가입 권한을 허용하지 않도록 현재 grant를 확인한다.
10. invite_lifecycle_concurrency.sh는 독립 DB 세션과 barrier를 사용해 같은 코드 경합, 마지막 자리, 수락 대 철회, 수락 대 이전, lock wait 중 만료, 동일 사용자의 서로 다른 그룹 경쟁, 제한 카운터 commit을 확인한다. 임의 sleep만으로 lock race를 입증하지 않는다.
11. 동시성 스크립트는 승인된 QA 프로젝트의 절대 workdir·project ID·DB container·volume·loopback endpoint를 확인하고 실행한다. 기존 프로젝트 reset/stop 금지, 전역 프로세스 종료 금지, 테스트가 만든 run-ID fixture만 정리한다. secret·코드를 출력하지 않는다. 대상 식별이 불일치하면 실행을 거부한다.

### 3. Rust RPC와 Tauri 명령

**담당:** Coder
**수용 기준:** I03–I04, I09–I10, I13–I14, I16의 Rust 단위

1. HTTP mock 기반 실패 테스트를 먼저 작성한다. RPC 이름·인자, 반환 status, null/불일치 응답, transport 오류, 개인 코드 endpoint 미호출을 확인한다.
2. client.rs에 CreatedWorldInvite, WorldInvite, InviteStatus, InviteRevokeStatus, InviteAcceptStatus DTO를 만든다. 원문 코드를 가진 DTO는 Debug 로그나 분석 이벤트에 출력하지 않는다.
3. 발급·목록·철회·수락 client method를 구현하고 기존 인증 토큰 저장 방식과 post_rpc helper를 재사용한다. 목록 DTO에는 원문·hash·Auth ID 필드를 두지 않는다.
4. commands/sharing.rs의 가입 경로를 새 accept RPC에 연결한다. accepted/already_accepted만 성공으로 취급하고, 이후 get_sharing_state로 서버 상태를 다시 읽는다.
5. 오류 문구는 고정된 사용자 안내로 매핑한다. 원문 코드, server body, Auth token을 오류에 반사하지 않는다.
6. 개인 코드 get/rotate/join Tauri 명령과 lib.rs 등록을 제거한다. 익명 Auth, owner transfer, member 조회, pause/leave/delete, 사용량 sync 명령은 보존한다.
7. cargo test --offline --locked로 신규 Rust 테스트와 기존 관련 테스트를 확인한다. offline 의존성이 없으면 설치하지 않고 환경 차단으로 기록한다.

### 4. React API와 초대 UI

**담당:** Coder
**수용 기준:** I03–I04, I14–I15, I16의 React 단위

1. SharingPanels.test.tsx와 App.test.tsx에 실패 테스트를 추가한다: owner만 관리, 발급 조건 표시, copy, clipboard 오류, 목록 상태, 철회, 가입 오류, 중복 제출 방지, group/user/owner 변경·화면 닫기 후 오래된 응답 폐기, 개인 코드 fallback 부재.
2. sharing.ts의 DTO와 invoke API를 Rust 응답에 맞춘다. 기존 joinWorld 입력 경로는 초대 코드용으로 유지한다.
3. InvitePanel을 소유자 발급·일회 표시·목록·미사용 초대 철회로 바꾼다. 7일/한 번 사용 정책과 코드 재조회 불가를 설명한다. 코드는 React memory에만 두고 context 전환·화면 닫기 때 지운다.
4. SharingSetup을 64자리 코드 붙여넣기 입력으로 바꾸고 정규화·오류·재시도 안내를 제공한다. 같은 코드 재시도는 허용한다.
5. 기존 멤버·행성·사용량·성장 UI 테스트를 새 fixture에 맞게 갱신하되 의미 있는 기존 보장을 삭제하지 않는다.
6. 신규 UI 테스트 후 전체 Vitest suite와 TypeScript/Vite build를 수행한다. 오류가 있으면 무관한 기존 실패와 구분해 보고한다.

### 5. 전체 replay·독립 QA·문서·최종 검토

**담당:** QA, Secretary, Reviewer; CEO 수용
**수용 기준:** I01–I16

1. 현재 SQL DB suite·동시성·실제 A/B UI는 `environment blocked / not run`이다. 환경 실행이 별도로 승인되고 차단이 해소되면 merge/release 전에 추가 작업의 수용을 먼저 확인한 뒤 기존 supabase/ci/run.sh를 고유한 빈 artifact 경로와 임시 경로의 검증된 CLI 2.119.0으로 실행한다. 이 스크립트가 만든 disposable stack에서 전체 migration replay와 SQL suite를 검증한다. artifact와 log에서 비밀이 출력되지 않았는지 확인한다.
2. 초대 경합 스크립트는 식별을 다시 검증한 뒤 승인된 /tmp QA 프로젝트에서 실행한다. 테스트 계정·그룹은 run ID로 구분한다.
3. QA는 A/B가 서로 다른 인증 사용자·로컬 저장소이며 같은 Supabase 프로젝트를 쓰는지 확인하고, 발급→복사·붙여넣기 가입→사용됨 표시→새 초대 철회→철회 코드 거부를 수행한다. 코드나 토큰은 결과 문서에 남기지 않는다.
4. QA는 서버 권한, 시간 경계, 원자성, 재시도, 정원, 제한, 소유권 이전, legacy 차단을 계층별 증거로 기록한다. DB·Rust·React 단위 통과만으로 실제 앱의 성공을 주장하지 않는다.
5. UI 창의 정확한 A/B 식별이 안 되거나 앱이 응답하지 않으면 UI 테스트를 중단한다. 검증되지 않은 기준은 unverified로 기록하며 기존 V01–V13 결과도 유지한다.
6. Secretary는 README, release-acceptance, 기존 멀티 검증 문서와 신규 초대 검증 문서를 결과에 맞춰 동기화한다.
7. Reviewer는 QA 종료 후 migration·grant·SQL/Rust/React·테스트·문서 전체 diff를 독립 검토한다. 높은 심각도 finding과 권한·원문 노출·deadlock·부분 commit 문제를 해소하기 전 CEO는 완료로 수용하지 않는다.
8. CEO는 I01–I16 각각의 검증 결과와 증거 경계를 확인한다. 실제 배포는 이 계획의 완료와 별도 승인으로 남긴다.

## 실행 명령 기준

현재 구현 단계에서는 Supabase CLI(version/help 포함)·Docker·DB·runner/CI를 실행하거나 의존성·이미지를 다운로드하지 않는다. 아래 DB 검증 명령은 merge/release 전에 별도 승인된 격리 환경에서 수행할 후속 기준이며 현재 실행하지 않는다.

신규 migration 파일 생성은 Supabase CLI 없이 수행한다. 저장소 기존 형식에 맞는 14자리 `YYYYMMDDHHMMSS_invite_lifecycle.sql` 이름을 선택하고, 동일 timestamp prefix 및 파일 경로가 미사용인지 확인한 뒤 `supabase/migrations/`에 forward migration 1개만 수동 생성한다. 기존 migration보다 뒤에 정렬되는 이름을 사용하고 과거 migration은 편집하지 않는다. 선택한 이름과 충돌 확인 근거를 구현 보고에 남긴다. `supabase migration new invite_lifecycle` 또는 CLI help를 실행하지 않는다.

새 migration 전체 replay 및 pgTAP:

~~~sh
SUPABASE_BIN="$SUPABASE_BIN" bash supabase/ci/run.sh \
  --artifacts-dir "/tmp/token-planet-invite-ci-$RUN_ID"
~~~

RUN_ID는 실행별 고유한 값이어야 하며 artifact 경로는 저장소 바깥의 빈 디렉터리여야 한다. 이 runner가 정의한 환경 검사를 우회하지 않는다.

Rust:

~~~sh
cargo test --offline --locked \
  --manifest-path apps/desktop/src-tauri/Cargo.toml
~~~

React 단위 및 전체 suite:

~~~sh
npm --prefix apps/desktop test -- \
  src/components/__tests__/SharingPanels.test.tsx \
  src/__tests__/App.test.tsx
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
~~~

DB concurrency:

~~~sh
bash supabase/tests/invite_lifecycle_concurrency.sh \
  --workdir "$TOKEN_PLANET_INVITE_TEST_WORKDIR" \
  --container "$TOKEN_PLANET_INVITE_TEST_CONTAINER"
~~~

각 명령은 비밀·코드·연결 문자열을 출력하지 않아야 한다. CLI/의존성 미설치, 포트 점유, 환경 식별 불일치, UI 접근 실패는 테스트 실패와 구분해 blocked/unverified로 기록한다.

## 수용 기준 추적

| ID | 검증 단계 |
| --- | --- |
| I01 | Task 1 권한 pgTAP, Task 5 실제 owner/non-owner 앱 확인 |
| I02 | Task 1 table/RPC grant와 RLS 검사 |
| I03 | Task 1 hash·응답 검사, Task 3 직렬화 검사 |
| I04 | Task 1 SQL 반환, Task 3 오류·로그, Task 4 UI state 검사 |
| I05 | Task 1·2 정확한 168시간·만료 경계·lock wait 검사 |
| I06 | Task 2 transaction·rollback SQL 검사 |
| I07 | Task 2 같은 코드 동시 가입 검사 |
| I08 | Task 2 마지막 자리 경합 검사 |
| I09 | Task 2 joined_via_invite_id와 응답 유실 retry 검사 |
| I10 | Task 2 타 사용자·탈퇴·새 초대 재가입 검사 |
| I11 | Task 1·2 수락·철회·이전 잠금 순서와 경합 검사 |
| I12 | Task 1·2 owner transfer 뒤 pending 영구 철회 검사 |
| I13 | Task 2 실패 카운터 정상 commit·5/15 제한 검사 |
| I14 | Task 2 legacy join 차단, 기존 멤버십/sync 보존과 새 client fallback 부재 검사 |
| I15 | Task 4 owner UI·가입·오류·오래된 응답 폐기 검사 |
| I16 | Task 5 isolated migration, DB 경합, Rust/React 및 실제 A/B 앱 검증 |

## 검토 초점

- 재시도 허용은 같은 사용자·같은 사용 초대·현재 연결된 멤버십에만 한정한다. 탈퇴 후나 다른 초대 가입 뒤에는 허용하지 않는다.
- 가입은 user attempt row → world → invite 순으로 잠근다. 다른 초대 상태 변경은 world → invite 순서를 지켜 서로 deadlock을 만들지 않는다. 그룹·초대 lock을 가진 채 rate-limit row를 기다리는 새 가입 경로를 만들지 않는다.
- 가입 실패·정원 초과·unique 경합이 초대 소비를 남기지 않는다. 실패 횟수는 정상 반환으로 commit한다.
- create/list/accept의 PostgreSQL return shape 변경을 Rust DTO와 Supabase RPC 호출에 함께 반영한다. 의존 객체·기존 GRANT를 확인하고 불필요한 CASCADE를 피한다.
- RLS·SECURITY INVOKER wrapper·private SECURITY DEFINER helper의 권한을 각각 검증한다. wrapper가 private helper를 호출할 최소 권한과 내부 auth/owner 확인을 모두 유지한다.
- 비밀 없는 DB·앱 증거와 실제 사용자 화면을 구분한다. 두 앱 접근이 막히면 I16 전체를 통과 처리하지 않는다.
- 현재 invite QA는 기존 V01–V13을 소급 완료하지 않는다.

## 자체 검토

- 승인 spec의 I01–I16을 각 구현·검증 단계에 연결했다.
- 7일 만료·1회 사용·소유자 철회, 64자리 코드, 정확한 수치·순위 공개라는 선택을 보존하고 방문·공동 목표는 후속 단계로 남겼다.
- joined_via_invite_id로 응답 유실 retry를 특정 초대에 한정했고, 탈퇴·다른 가입 후 재사용을 구분했다.
- lock order, 만료 대기, rate-limit commit, transfer, membership unique 경쟁을 계획에 넣었다.
- 기존 그룹·멤버십·집계·sync와 legacy private code 데이터 보존을 명시했다.
- 실행 환경·QA UI 제약을 검증 완료로 과장하지 않으며 테스트와 배포를 자동 승인하지 않는다.
- 과거 CLI 다운로드·runner 실행 승인은 역사 기록으로 유지하되 현재 구현 단계에서는 적용하지 않는다. Supabase CLI·Docker·DB·runner/CI 실행과 변경, 추가 다운로드는 금지하며 forward migration 파일명은 미사용 14자리 timestamp 형식으로 수동 생성한다. 기존 QA 및 이전 local Supabase stack에는 reset·stop·migration을 수행하지 않는다.
- 사용자는 2026-10-08에 CI runner의 Docker guard/runner 호환성 범위 추가안을 승인했다. runner는 DB-only `db start`를 사용하고 fail-closed image/resource 경계를 유지한다. 이후 사용자의 “계획 수정하고 구현 시작” 승인으로 SQL regression assertions 선작성 후 실제 SQL RED 미관측 상태에서도 초대 lifecycle 구현을 시작하도록 순서를 변경했다. SQL DB suite·동시성·실제 A/B UI는 `environment blocked / not run`이며 격리 DB 검증은 merge/release 전 필수다.
- CI 시작 문제를 초대 구현과 분리된 선행 작업으로 배치하고, exact runner/guard tests와 유지할 loopback·소유권·이미지 경계를 명시했다. 해결 불가한 CLI Docker 동작을 일반 허용하지 않는 중단 기준도 포함했다.
- 이 문서의 설계·계획 승인은 구현 범위와 위의 한정된 검증 예외를 승인한다. production 변경은 별도 승인 대상으로 남는다.

## 참고 문서

- Supabase database migrations: https://supabase.com/docs/guides/deployment/database-migrations
- Supabase test db CLI: https://supabase.com/docs/reference/cli/supabase-test-db
- Supabase CLI 2.119.0 official release and assets: https://github.com/supabase/cli/releases/tag/v2.119.0
- Supabase CLI start pulls excluded service images (issue #4194): https://github.com/supabase/cli/issues/4194


## 구현·검증 현황 (2026-10-08)

초대 lifecycle 소스 구현과 실행 가능한 로컬 검증을 마쳤으며 최종 Reviewer 전체 소스 검토는 no findings다. 최종 수정 후 targeted SharingPanels 23 passed, source-only path predicate 3 passed, concurrency source `bash -n` pass가 보고됐다. 이전 QA의 offline Cargo 390 passed / 0 failed / 1 ignored 및 전체 frontend 463 passed·build pass는 당시 상태의 증거다. 최종 수정 후 전체 frontend/build 재실행은 not run이다. 상세 범위는 [검증 기록](../../2026-10-08-multiplayer-verification.md)에 기록한다.

DB replay·pgTAP·적용 DB grants·실제 동시성·native A/B UI는 environment blocked / not run이다. 전체 계획 또는 I01–I16 완료로 수용하지 않으며, 로컬 구현 수용은 기존 merge/release 전 격리 DB 검증 gate를 대체하지 않는다.
