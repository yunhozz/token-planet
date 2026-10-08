# 멀티 공유 정책·두 사용자 검증 기록

- 작성일: 2026-10-08
- 단계: Task 6 QA 부분 관찰 및 별도 초대 lifecycle 소스·로컬 검증 결과 통합; 두 사용자 공유 검증 미완료
- 기준: [승인 spec](superpowers/specs/2026-10-08-token-planet-multiplayer-policy-design.md), [실행 plan](superpowers/plans/2026-10-08-token-planet-multiplayer-policy.md)
- 문서 검토 기준 commit: `956f2a82b22d9f9d16d17042294c0daaffb35cd2` 이후 이 문서와 함께 작성된 문서 변경분
- 문서 검토는 실제 앱·Auth·서버·동기화 동작을 입증하지 않는다. 아래 QA 결과는 CEO가 전달한 최종 보고와 실행 handoff의 마지막 QA 절을 재사용했다. Secretary가 앱을 재실행하거나 재관찰하지 않았다.

## 실행 환경과 재사용 증거

근거 E1: CEO가 전달한 QA 최종 보고 및 제한된 실행 handoff `token-planet-multiplayer-handoff-20261008.md`의 `QA stopped after partial onboarding — 2026-10-08` 절. handoff는 저장소 외부 임시 실행 기록이며 CEO가 보관한다. 아래 환경 구성 근거는 같은 handoff의 성공한 isolated Supabase/migration gate와 A/B startup 절을 재사용한다. 비밀·원본 경로는 이 문서에 전재하지 않는다.

| 항목 | 기록 |
| --- | --- |
| 실행일·앱 commit | 2026-10-08; `956f2a82b22d9f9d16d17042294c0daaffb35cd2`; 정확한 관찰 시각·별도 앱 버전 미기록 |
| 플랫폼·프로젝트 | macOS 임시 A/B wrapper; disposable local QA `token-planet-multiplayer-qa-20261008`; 같은 Supabase 서비스 사용 |
| dev URL·API | A `http://localhost:1422`, B `http://localhost:1423`; API의 loopback 별칭은 같은 QA 서비스 도달 근거 확인됨 |
| migration·환경 gate | 31개 적용, 누락·divergence 없음; Auth/REST HTTP 200은 구성 단계 보고 재사용이며 익명 Auth 생성의 행동 증거가 아님 |
| 프로필·사용자 | 별도 wrapper/앱 데이터 프로필·빈 Codex/Claude source roots 구성 근거 있음; QA A/QA B 표시는 서로 다른 Auth ID의 증거가 아님 |
| UI 도구·viewport | CUA; 초기 780×1400 pixels (390×700 logical) |
| 테스트 데이터·그룹 | 전용 QA 프로필의 초기 onboarding만 수행; 그룹 없음, 사용량 증분·기대값 없음 |
| 허용 범위·실제 수행 | CEO brief의 전용 환경 범위; 실제로는 닉네임 입력과 `행성 시작하기`만 수행 |
| payload·연결 차단·날짜 경계 | 수행하지 않음; 그룹 시간대·cutoff·다음 날 기록 미관찰 |
| 종료·보존 | QA는 프로세스·서비스를 중지하지 않음; 두 wrapper와 QA Supabase 유지. 무관한 listener와 이전 프로젝트 스택 보존 |

정상 갱신 관찰 기준은 5분이며 SLA가 아니다. 이번에는 공유 흐름에 진입하지 않아 해당 관찰을 수행하지 않았다. 기존 keychain·SQLite·원본 기록을 지우거나 실제 사용자 로그를 채우지 않았다.

## QA 부분 관찰과 중단 근거 — E1

선택된 두 wrapper에 `QA A`와 `QA B`를 입력하고 `행성 시작하기`를 클릭했다. 각각 자신의 표시명·초기 행성·0 사용량과 “이 기기에만 저장 중”, Codex/Claude 기록 폴더 없음 상태를 보였다. 이는 로컬 onboarding 관찰이며 익명 인증·공유 시작·그룹 참여 성공이 아니다.

이후 A의 CUA 접근이 끊겼다. 정확한 wrapper 경로와 등록 ID로 접근하면 timeout이 발생했고, `Raise`는 `noWindowsAvailable`을 반환했지만 inventory는 실행 중으로 표시했다. B는 응답했다. 한 번의 안전한 rebind도 A 접근을 복원하지 못해 QA가 행동 검증을 중단했다. 앱 내부 결함 또는 서버 실패로 확정하지 않는다.

그룹·코드를 생성하거나 공개하지 않았고 API/RPC 직접 호출·payload 관찰, 사용량 증분, 재시작, 공유 정지/재개, 삭제, 탈퇴, 소유권 이전, 날짜 cutoff 검증도 수행하지 않았다.

### E1 이후 접근 복구 시도

사용자의 `진행해` 지시 이후 Coder의 읽기 전용 점검에서는 A/B 프로세스와 QA 서비스가 실행 중이었다. 소스의 `Setup→Popup` 전환, Popup의 blur 시 숨김, macOS Accessory 활성화 방식으로 인해 창이 숨겨졌을 가능성이 제기됐다. 이는 소스 기반 가설이며 확인된 crash나 근본 원인이 아니다.

A의 네이티브 프로세스만 재시작하면서 A 프로필 데이터와 Vite·B·QA 서비스를 보존했다. 이후 CUA inventory는 A와 B를 표시했지만 정확한 A 앱 경로 선택은 timeout이었다. 기존 A 앱의 정확한 경로를 Launch Services로 활성화할 때 `-n`, 추가 인자·환경 변수·키를 사용하지 않았으며 네이티브 프로세스는 하나로 유지됐다. 활성화 이후에도 정확한 A 경로 선택과 bundle ID 선택 모두 timeout이었다.

트레이 조작, 앱 API/RPC 호출, 그룹·코드 생성/공개, backend·데이터 흐름 검증 또는 추가 UI 상호작용은 없었다. 이 복구 기록은 CEO의 후속 보고를 재사용하며 Secretary가 실행하거나 재관찰하지 않았다. P01–P05의 문서 판정 및 V01–V13의 상태·판정은 변경하지 않는다. 다음 단계는 명시적으로 승인된 대체 접근 경로 또는 수정이다.

## 증거 작성 규칙

각 실행 결과는 criterion_id, method_scope, environment_command_version, test_state_timezone_viewport, steps, expected, observed, evidence, run_status, criterion_verdict, unverified_reason_remaining_scope, next_action을 기록한다. 실행 상태와 기준 판정을 분리한다. `passed`만으로 `proven`을 대신하지 않는다.

개인 코드 실제 값, Auth access/refresh token, secret key, 원본 로그·프롬프트·원본 파일 경로를 저장하지 않는다. 경로는 저장소 문서 상대 경로와 비밀 없는 증거 참조만 사용한다. 집계 값은 전용 테스트 사용자 데이터임을 QA가 확인한 경우에만 기록한다. payload 증거는 endpoint·필드 범위·판정 근거로 남긴다.

## 정책 문서 검토 — P01–P05

검토 방법: 승인 spec §4–8과 아래 다섯 문서의 현재 기준 안내·본문을 대조했다. 모든 P 판정은 문서 범위에 한정한다. UI viewport·실행 환경은 해당 없음이다. QA는 Secretary의 이 P01–P05 문서 검토를 재사용했으며 모순을 보고하지 않았다. 새 런타임 입증으로 해석하지 않는다.

| ID | 기대 기준 | 수정·관찰 근거 | 실행 상태 / 기준 판정 | 남은 범위·다음 조치 |
| --- | --- | --- | --- | --- |
| P01 | 독립 행성·정확한 개인 수치·비공개 최대 10명 일관 명시 | 다섯 문서의 Current multiplayer baseline/policy 및 현재 기준 안내에 현재·누적 토큰·성장 크레딧·순위 명시 | passed / proven (문서) | 실제 표시 V02–V04 미검증; 10명 경계는 두 사용자만으로 입증 불가 |
| P02 | 익명 인증·현재 개인 코드와 원하는 초대 정책 구분 | 다섯 안내에 기기별 익명 Auth, 재사용·재발급, 원하는 7일·한 번 사용·소유자 철회 및 복구 한계 명시 | passed / proven (문서) | 역사적 개인 코드 동작 V01·V08·V12·V13 미검증 유지; 별도 초대 lifecycle은 소스 구현·로컬 검증 완료, 적용 DB·실제 앱 미검증 (아래 E2) |
| P03 | 과거 OTP·수치 비공개를 현재 증거로 사용하지 않음 | MVP Historical specification, 개인 행성 설계 현재 기준 안내, release의 historical scope 설명; README의 이전 checkpoint 범위 안내 | passed / proven (문서) | 역사 기록 보존; 현재 검증으로 재사용 금지 |
| P04 | 공개 경계·V 방법·기대 결과·환경 제약 명시 | 다섯 안내의 원본/토큰/지갑 경계와 승인 spec 링크; 이 문서 실행 환경·V 표·증거 규칙 | passed / proven (문서) | payload 관찰 V05와 모든 실제 동작 미검증 |
| P05 | 이후 구현을 현재 범위·완료로 주장하지 않음 | 다섯 안내에 복구·새 방문·공동 목표·원하는 초대 구현 유보; release의 여러 기기 동일 계정 기대를 후속 연결 과제로 구분 | passed / proven (문서) | 이후 기능은 별도 설계·구현; 현재 상세 조회는 새 방문 완료 아님 |

수정 근거 문서:

- [역사 MVP](specs/token-planet-mvp.md#current-multiplayer-baseline-2026-10-08)
- [개인 행성 설계](superpowers/specs/2026-09-26-token-planet-personal-world-design.md#현재-기준-안내-2026-10-08)
- [desktop README](../apps/desktop/README.md#current-multiplayer-policy-2026-10-08)
- [Supabase README](../supabase/README.md#current-multiplayer-policy-2026-10-08)
- [출시 체크리스트](release-acceptance.md#current-multiplayer-baseline-2026-10-08)

## 실제 두 사용자 검증 — V01–V13

아래 방법·기대 결과는 승인 spec의 예정 기준이다. 실제 수행·관찰 근거와 미검증 범위는 각 행의 마지막 열과 E1에 연결한다. 공통 중단 요인은 A 창의 CUA 접근 불가이며, 다음 단계는 CEO가 대체 접근 경로 또는 수정에 대한 명시적 승인을 확보한 후 필요한 재실행 범위를 확정하는 것이다. 현재 런타임 통과·실패로 판정한 항목은 없다.

| ID | 방법·범위 (예정) | 기대 결과·수용 기준 | 실행 상태 / 기준 판정 | 실제 관찰·증거 / 남은 범위 |
| --- | --- | --- | --- | --- |
| V01 | A·B에서 공유 시작, 재시작 후 세션 확인 | 서로 다른 사용자이며 재시작 후 각각 같은 멤버로 유지된다. 이메일·OTP 절차를 요구하지 않는다 | not run (partial onboarding observed) / unverified | E1: 두 wrapper의 로컬 onboarding만 관찰. 별도 Auth ID·공유 시작·재시작 후 멤버 유지 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V02 | A가 그룹 생성, B가 A의 개인 코드로 참여 | 같은 그룹에 두 독립 멤버가 보이고 A가 소유자다. 각 개인 행성은 독립적으로 유지된다 | not run / unverified | E1: 그룹 생성·가입 없음. 동일 그룹·소유자·독립 행성 공유 상태 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V03 | A·B의 사용량과 행성 상태 업로드, 양쪽 조회 | 승인된 정확한 개인 수치·순위가 양쪽에서 일관되게 표시된다. 각 행성의 수치를 하나의 공동 성장값으로 합치지 않는다 | not run / unverified | E1: 초기 0 표시는 있음; 증분·업로드·양쪽 정확한 수치/순위 비교 없음; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V04 | 현재 그룹 화면에서 상대 행성 선택·상세 조회 | 선택한 상대의 공개 정보가 보인다. 상대 행성을 수정하거나 소유권을 얻지 않는다. 새 방문 기능 완료로 판정하지 않는다 | not run / unverified | E1: 상대 멤버·행성 선택/상세 조회 없음. 읽기/편집 권한 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V05 | A·B의 집계·그룹 조회 payload 확인 | 원본 로그·프롬프트·경로·원본 세션 식별자가 없으며 그룹 응답에 Auth access·refresh token·지갑 잔액·적립 이력이 없다 | not run / unverified | E1: payload·API/RPC 직접 관찰 없음. 개인정보·지갑·Auth 필드 경계 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V06 | B의 앱 연결만 차단하고 로컬 수집·재시작 후 연결 복원 | 대기열과 로컬 수집이 유지되고 복원 후 같은 집계가 재시도로 중복 가산되지 않는다. 오프라인 캐시는 최신 서버 상태로 오인하여 기록하지 않는다 | not run / unverified | E1: 연결 차단·수집 증분·재시작·재시도 없음. 대기열·중복 방지 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V07 | B의 공유 일시정지, 수집 변화 관찰 후 재개 | 정지 중 로컬 수집은 계속되고 업로드는 중지된다. 재개 후 제외되지 않은 집계가 반영된다 | not run / unverified | E1: 공유 정지/재개·수집 증분 없음. 업로드 중지/재개 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V08 | B가 최초 참여에 사용한 코드로 탈퇴 후 재참여하여 재사용 여부를 관찰한다. B가 다시 탈퇴한 상태에서 A가 코드를 재발급하고 B가 이전·새 코드로 참여를 시도한다 | 재발급 전 같은 코드로 재참여할 수 있다. 재발급 후 이전 코드는 거부되고 새 코드로 참여할 수 있다. 현재 코드 재사용·재발급 검증이며 한 번 사용·개별 초대 철회 완료가 아니다 | not run / unverified | E1: 코드 생성/공개·재사용·재발급 없음. 이전/새 코드 참여 결과 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V09 | 재참여 전·후 B의 과거 집계와 cutoff 확인 | 탈퇴로 제외된 과거 사용량이 재참여만으로 복원되지 않는다. 새 참여 이후 허용된 기록의 반영을 구분한다 | not run / unverified | E1: 탈퇴/재참여·과거 집계·cutoff 없음. 제외 기록 및 새 허용 기록 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V10 | B가 공유 집계 삭제 | 서버의 대상 집계가 삭제되고 공유가 일시정지된다. 로컬 원본 로그는 유지된다. 현재일까지 제외된 기록이 재업로드되지 않는다 | not run / unverified | E1: 집계 삭제 없음. 서버 삭제·일시정지·원본 유지·재업로드 제외 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V11 | B 탈퇴 후 B의 인증으로 이전 그룹 RPC 조회 | 서버가 멤버십 없는 B의 해당 그룹 조회를 거부한다. 화면에서 그룹을 숨기는 것만으로 입증하지 않는다 | not run / unverified | E1: 탈퇴·비멤버 RPC 호출 없음. 서버 접근 거부 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V12 | 두 멤버가 있을 때 A의 소유자 탈퇴 조건과 소유권 이전 경로 확인 | 소유자 탈퇴에는 먼저 이전이 필요하고 이전 후 역할이 양쪽에 일관되게 표시된다. 이전 소유자의 코드는 해당 그룹 참여권을 제공하지 않는다 | not run / unverified | E1: 그룹·이전·탈퇴·코드 시도 없음. 역할 일관성과 이전 소유자 코드 거부 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |
| V13 | 관찰 결과와 원하는 초대 정책 비교 | 현재 재사용 가능한 개인 코드와 7일 만료·한 번 사용·소유자 철회 정책의 차이를 명시한다. 원하는 정책 충족으로 판정하지 않는다 | not run / unverified | E1: 현재 코드 동작 관찰 없음. 문서상 원하는 정책과의 차이는 P02에만 입증; 실행 비교 미확인; CEO의 명시적으로 승인된 대체 접근 경로 또는 수정 필요 |

## 범위 제한과 이후 조치

V05는 실제 payload 관찰 없이는 입증할 수 없다. V06은 기준값·기대 증분·재시도 전후 비교가 필요하다. V09·V10은 제외된 과거 기록과 다음 날 허용 기록을 구분한다. V11은 비멤버 인증의 서버 RPC 거부가 필요하다. V12는 이전 소유자가 탈퇴한 상태에서 이전·현재 소유자 코드의 비교가 필요하다. V13은 역사적 개인 코드 검증 기준이며 미실행 상태를 유지한다. 별도 초대 lifecycle의 소스·로컬 검증 결과는 아래 E2에 기록하며 이 결과로 V13을 완료 처리하지 않는다.

현재 문서 검토에서 해결되지 않은 정책 충돌은 없다. QA 환경과 로컬 onboarding 일부만 관찰되었으며, Windows/macOS 전체 출시·production 배포·10명 정원 경계는 이번 두 사용자 검증으로 대체할 수 없다. 계정 복구·안정적 공개 멤버 ID·동기화 시각/캐시 개선·새 방문·공동 목표는 후속 범위다.

## Task 1 문서 검사 기록

- 승인 plan의 관련 문구 `rg -n` 검색 후 역사 문맥과 현재 안내를 대조했다. 검색 성공 자체는 정책이나 런타임 동작의 증명이 아니다.
- 기존 문서 5개에 대한 `git diff --check`: 출력 없음, exit 0. 미추적 결과 문서는 해당 검사 대상이 아니므로 별도로 18개 기준·링크 대상·상태 문구와 내용을 검토했다.
- Task 1에서 Secretary는 앱 실행, QA, 네트워크 payload 관찰, DB·환경 검사, 테스트 실행을 수행하지 않았다. 이후 QA의 부분 관찰은 E1로 통합했다.

## Task 6 통합 검사 기록

- QA 보고를 E1로 재사용하고 P01–P05 문서 판정을 유지했다. V01 부분 onboarding과 V02–V13 미실행을 명시했다.
- 18개 기준의 상태·판정·근거/방법·남은 범위와 문서 링크 대상, diff 공백을 자체 검토했다. 추가 앱 상호작용이나 환경 재검사는 수행하지 않았다.


## 별도 초대 lifecycle 구현·로컬 검증 — E2

근거 E2는 CEO가 전달한 최종 QA·Reviewer 결과와 canonical handoff의 해당 기록이다. Secretary는 테스트를 재실행하지 않았다. 기준 checkout은 `/Users/ma-24-007/Desktop/workspace/token-planet`, branch `feat/multiplayer-policy-verification`, HEAD `4c1baa648dd76497ce50469af4ad5d083ed1e771`의 작업 트리 변경분이다. [초대 설계](superpowers/specs/2026-10-08-token-planet-invite-lifecycle-design.md)와 [초대 계획](superpowers/plans/2026-10-08-token-planet-invite-lifecycle.md)을 따른다.

초대 lifecycle은 소스 구현·실행 가능한 로컬 검증을 완료했다. 개인별 독립 행성, 정확한 개인 사용량·순위 공개를 유지하며 초대는 7일(168시간)·한 번의 성공 가입·소유자 철회 계약을 구현한다. 방문·공동 목표는 후속 설계다. 이는 로컬 구현 수용 범위이며 적용 DB·실제 앱 성공 또는 전체 I01–I16 완료를 뜻하지 않는다.

| 증거 범위 | 보고된 결과 | 한계 |
| --- | --- | --- |
| 기존 QA state의 offline Cargo | 390 passed / 0 failed / 1 ignored | ignored는 기존 local API DB E2E. 최종 수정 후 전체 재실행 결과로 취급하지 않음 |
| 기존 QA state의 전체 frontend·build | Vitest 24 files / 463 passed; build pass | 최종 수정 후 전체 frontend/build 재실행은 not run |
| 최종 수정 후 targeted SharingPanels | 23 passed | JSDOM 검사이며 native A/B UI 증거 아님 |
| 최종 수정 후 path predicate | 3 passed | source-only 경로 판정 검사; CLI/Docker/DB 호출 없음 |
| 최종 수정 후 concurrency source | `bash -n` pass | 셸 문법 검사이며 DB 경합 실행 아님 |
| 최종 Reviewer | 전체 feature source review: no findings | 적용 DB 권한·동시성·실제 앱 동작을 입증하지 않음 |

Reviewer가 제기했던 macOS `/tmp` canonical path 비교, 오래된 initial list 응답·오류의 최신 목록 덮어쓰기, visibilitychange 중 pending 초대 발급 잠금 해제의 세 finding은 수정과 해당 QA 재검증 후 최종 소스 검토에서 해소됐다.

DB migration replay·pgTAP·적용 DB grants·실제 concurrency·native A/B 앱은 `environment blocked / not run`이다. 로컬 구현 수용은 merge/release gate를 열지 않는다. 승인된 격리 DB 검증과 남은 기준의 증거를 확보해야 한다. 위 E2는 과거 P 문서 검토와 V01–V13의 실행 결과를 대체하거나 소급 완료하지 않는다.
