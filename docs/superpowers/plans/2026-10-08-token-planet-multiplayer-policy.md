# Token Planet 멀티 공유 정책·검증 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> 설치된 Harness에 따라 CEO가 순차 조율·수용하고 Secretary가 문서, QA가 실제 검증을 담당한다. 이 plan은 별도 실행 방식 선택이나 작업별 자동 Reviewer 배정을 요구하지 않는다.

**Goal:** 승인된 첫 하위 프로젝트의 공유 정책 문서를 정합화하고, 현재 코드의 두 독립 사용자 흐름을 V01–V13·P01–P05 기준으로 검증·기록한다.

**Architecture:** 현재 Rust 수집·SQLite 원장·OS 보안 저장소·Supabase RPC·React 화면을 사용한다. 문서에서 현재 정책·역사 기록·원하는 초대 정책을 구분하고, 같은 local 또는 staging 프로젝트에서 실제 흐름을 검증한다. 제품 코드·DB 스키마·환경 구성은 변경하지 않는다.

**Tech Stack:** Markdown, Tauri 2, React, Rust, SQLite, Supabase Auth/PostgreSQL, 기존 npm launch scripts.

**Spec:** /Users/ma-24-007/Desktop/workspace/token-planet/docs/superpowers/specs/2026-10-08-token-planet-multiplayer-policy-design.md

**Plan:** /Users/ma-24-007/Desktop/workspace/token-planet/docs/superpowers/plans/2026-10-08-token-planet-multiplayer-policy.md

**승인 근거:** 설계 승인 답변 `네, spec 작성`, written-spec 승인 답변 `ㅇㅇ`. 이 계획은 아직 사용자 승인 전이다.

## Global Constraints

- “각 멤버는 독립된 개인 행성을 유지한다.”
- “그룹 멤버에게 현재 행성 토큰, 누적 토큰, 성장 크레딧과 순위를 정확한 값으로 공개한다.”
- “그룹은 비공개이며 최대 10명이다.”
- “원하는 초대 정책은 7일 만료, 한 번 사용, 소유자 철회이다.”
- “이번 단계는 위 구조의 현재 동작을 검증하며 인터페이스나 DB 스키마를 변경하지 않는다.”
- “local 또는 staging 중 하나의 동일한 Supabase 프로젝트를 두 사용자 모두 사용한다. production과 기존 실제 사용자 데이터는 이번 검증 대상에서 제외한다.”
- “익명 계정 분리를 위해 기존 사용자의 keychain이나 SQLite를 삭제하지 않는다.”
- “원본 로그, Auth access·refresh token, 비밀 키와 개인 코드의 실제 값은 결과 문서에 저장하지 않는다. 검증에 필요한 집계 수치는 테스트 데이터 범위에서 기록할 수 있으며 사용자·그룹 식별에는 별칭을 사용한다.”
- “정상 연결에서 관련 업로드·조회 갱신을 5분 동안 관찰한다. 이는 현재 약 60초 동기화 주기를 고려한 검증 관찰 시간이며 제품의 갱신 SLA가 아니다.”
- “시스템 시계 변경이나 새로운 테스트 전용 코드로 관찰 시간을 단축하지 않는다.”
- “QA는 제공된 기존 검증 환경에서 수행하며 소스·문서·테스트·설정 편집, 의존성 설치 또는 환경 구성 변경으로 부족한 조건을 보완하지 않는다.”
- “정리 대상은 QA가 생성한 프로세스와 세션이며 기존 공유 서비스·세션을 보존한다.”

계정 복구·초대 기능·멤버 ID·동기화 시각 개선, 방문·공동 목표·채팅·실시간 편집 구현은 이번 범위에 추가하지 않는다.

## Review Focus

1. 서로 다른 local 스택 또는 같은 Auth 사용자를 두 사용자로 오인할 위험 → Task 2에서 프로젝트·Auth·로컬 저장소 분리를 확인한다.
2. 이전 이메일·OTP·개인 수치 비공개 정책이나 원하는 초대 정책을 현재 기능으로 설명할 위험 → Task 1의 P01–P05와 Task 5의 V13으로 확인한다.
3. 개인정보 경계를 UI만으로 입증하거나 재시도 중복을 최종 값만으로 판정할 위험 → Task 3에서 payload 범위와 집계 기준값·증분을 함께 기록한다.
4. 당일 cutoff 때문에 새 업로드가 없는 것을 결함으로 오인하거나 과거 재업로드를 놓칠 위험 → Task 4에서 삭제·탈퇴 날짜와 다음 날 허용 기록을 나누어 확인한다.
5. 이전 소유자 코드 거부가 “이미 참여 중” 때문인데 소유권 검증으로 통과할 위험 → Task 5에서 A가 탈퇴한 상태의 이전·현재 소유자 코드 시도를 비교한다.

---

## 역할·파일 책임

저장소 경로는 /Users/ma-24-007/Desktop/workspace/token-planet 기준이다.

| 담당 | 파일·산출물 | 책임 |
| --- | --- | --- |
| CEO | 승인 spec·이 계획·실행 brief | 승인·통합·실행 환경과 상호작용 범위 확정·수용 |
| Secretary | docs/specs/token-planet-mvp.md | 역사적 MVP임을 표시하고 현재 기준 spec 연결 |
| Secretary | docs/superpowers/specs/2026-09-26-token-planet-personal-world-design.md | 기존 승인 내용을 보존하며 현재 정책과 초대 차이에 대한 안내·연결 |
| Secretary | apps/desktop/README.md | 현재 개인 행성·공개 수치·익명 인증·개인 코드·복구 한계를 일관되게 설명 |
| Secretary | supabase/README.md | 현재 RPC·공개 경계·원하는 초대 정책과 현재 코드의 차이 설명 |
| Secretary | docs/release-acceptance.md | 이전 OTP 증거와 현재 검증을 구분하고 결과 문서 연결 |
| Secretary | docs/2026-10-08-multiplayer-verification.md 생성 | P/V별 증거·판정·제약을 QA 보고와 대조하여 기록 |
| QA | 파일 편집 없음 | 허용된 실제 흐름 실행·증거·기준별 결과를 CEO에게 반환 |

승인 spec과 plan은 Secretary의 수정 대상이 아니다. 실행 중 spec 변경이 필요하면 CEO에게 보고한다. 이번 범위에는 Coder 작업이 없다.

역할 간 결과 전달 형식: criterion_id, method_scope, environment_command_version, test_state_timezone_viewport, steps, expected, observed, evidence, run_status, criterion_verdict, unverified_reason_remaining_scope, next_action.

파일·문서·UI·서버 응답 근거를 구분한다. QA는 문서를 편집하지 않으며 Secretary는 QA 결과 없이 실제 실행 결과를 작성하지 않는다.

## Task 1: 공유 정책 문서 정합화

**담당:** Secretary
**의존성:** 이 계획 승인과 CEO의 문서 작업 brief.
**Files:** 위 책임표의 기존 문서 5개 수정, 결과 문서 1개 생성.
**Consumes:** 승인 spec과 기존 문서·소스 근거.
**Produces:** P01–P05 검토 결과와 V01–V13 미실행 상태의 결과 문서.

- [ ] **Step 1:** 기존 문서의 충돌을 P01–P05에 매핑한다. 초기 공동 행성·개인 수치 비공개·이메일 OTP·이전 초대 정책을 현재 정책과 구분한다.
- [ ] **Step 2:** 역사적 spec 두 개에는 현재 기준 spec으로 연결하는 상태 안내를 추가한다. 과거 승인 내용과 당시 증거를 현재 동작으로 다시 쓰지 않는다.
- [ ] **Step 3:** README 두 개와 release 문서를 승인 정책에 맞게 정합화한다. 현재 익명 인증·재사용 개인 코드·재발급과 원하는 7일·한 번 사용·철회 정책을 별도로 설명한다.
- [ ] **Step 4:** 결과 문서에 실행 환경 기록란과 P01–P05·V01–V13의 18개 기준 항목을 만든다. V 기준은 not run / unverified로 시작하고 실행 완료를 암시하지 않는다.
- [ ] **Step 5:** 관련 문구를 읽기 전용 검색으로 찾아 문맥별로 확인한다.

~~~~sh
rg -n 'OTP|이메일|email|leaderboard|순위|개인 코드|single.use|7일|7 days|복구|recover|공동 행성|shared planet' \
  /Users/ma-24-007/Desktop/workspace/token-planet/docs/specs/token-planet-mvp.md \
  /Users/ma-24-007/Desktop/workspace/token-planet/docs/superpowers/specs/2026-09-26-token-planet-personal-world-design.md \
  /Users/ma-24-007/Desktop/workspace/token-planet/apps/desktop/README.md \
  /Users/ma-24-007/Desktop/workspace/token-planet/supabase/README.md \
  /Users/ma-24-007/Desktop/workspace/token-planet/docs/release-acceptance.md
~~~~

예상 결과: 관련 문구와 위치가 출력된다. 역사 기록 검색 결과는 남아도 정상이다. 검색 성공만으로 P 기준을 입증하지 않고 현재 기능 설명의 문맥을 확인한다.

~~~~sh
git -C /Users/ma-24-007/Desktop/workspace/token-planet diff --check -- \
  docs/specs/token-planet-mvp.md \
  docs/superpowers/specs/2026-09-26-token-planet-personal-world-design.md \
  apps/desktop/README.md \
  supabase/README.md \
  docs/release-acceptance.md
~~~~

예상 결과: 출력 없음, exit 0. diff --check는 미추적 결과 문서를 검사하지 않으며 정책 정확성도 입증하지 않는다. 생성 문서는 별도로 내용·형식을 검토한다.

- [ ] **Step 6:** P01–P05 각각에 수정 위치와 판정 근거를 연결해 CEO에게 반환한다. 실제 흐름은 미검증으로 유지한다.

## Task 2: QA 실행 조건 확인

**담당:** CEO가 실행 brief 확정, QA가 제공된 실제 조건 확인.
**의존성:** Task 1의 정책 기준 문서.
**Files:** 읽기만. QA 보고는 CEO에게 반환한다.
**Produces:** 실행 가능한 환경 또는 기준별 정확한 차단 사유.

- [ ] **Step 1:** CEO가 A·B의 workspace·앱 경로 또는 기존 launch command, 플랫폼·버전·동일 프로젝트, 테스트 사용자·데이터, 허용 상호작용과 증거 관찰 방식을 실행 brief에 기록한다.
- [ ] **Step 2:** QA가 A·B의 로컬 데이터·OS 보안 저장소·Auth 사용자가 분리되고 같은 프로젝트를 사용하는지 확인한다. 닉네임 차이만으로 사용자 분리를 입증하지 않는다.
- [ ] **Step 3:** 이미 구성된 local 또는 staging 환경에서 적용 migrations·Anonymous Sign-Ins·Auth·REST 접근 가능 여부를 확인한다. QA는 migration 적용·DB reset·설정 편집으로 환경을 만들지 않는다.
- [ ] **Step 4:** payload 관찰, B 앱의 연결 차단·복원, 삭제·탈퇴·소유권 이전이 허용된 전용 테스트 데이터 범위인지 확인한다. 관찰 도구가 없으면 해당 기준을 환경 차단으로 보고한다.
- [ ] **Step 5:** CEO가 공급한 기존 launch 경로로 앱을 실행한다.

staging에 연결하도록 이미 구성된 이 workspace의 기존 명령:

~~~~sh
npm --prefix /Users/ma-24-007/Desktop/workspace/token-planet/apps/desktop run dev:hosted
~~~~

예상 결과: .env.local의 유효한 HTTPS URL과 publishable key를 사용하여 Tauri 앱이 실행된다. secret 값은 출력·문서화하지 않는다. 파일 부재·잘못된 설정·의존성 부족은 환경 차단으로 기록하며 QA가 구성하지 않는다.

이미 실행 중인 적합한 local 환경의 기존 명령:

~~~~sh
npm --prefix /Users/ma-24-007/Desktop/workspace/token-planet/apps/desktop run dev:local
~~~~

예상 결과: 같은 local 프로젝트의 HTTP loopback API와 publishable key를 읽고 Tauri 앱이 실행된다. 별도 기기의 별도 local 스택은 같은 프로젝트가 아니다.

A·B의 경로가 다르면 CEO가 확인된 절대 경로·기존 명령을 공급한다. 임의 경로를 만들지 않는다. 패키지 앱이면 정확한 앱 또는 실행 파일 경로를 brief에 둔다. 앱 실행은 V01–V13 통과 증거가 아니다. local 경로가 독립 사용자·동일 프로젝트 조건을 만족하지 못하면 환경 차단을 기록하고 준비된 staging을 사용한다. 준비된 대안도 없으면 검증을 시작하지 않는다.

## Task 3: 기본 공유·데이터 공개·오프라인 검증

**담당:** QA
**Files:** 파일 편집 없음; CEO가 허용한 전용 local/staging 테스트 그룹·데이터만 변경.
**의존성:** Task 2 통과 및 개인정보 payload 관찰 방식 확보.
**Consumes:** 제공된 검증 환경과 실행 brief.
**Coverage:** V01–V07.
**Produces:** 상태 변경 전 기준값, 기대 증분, UI·payload·재시도 관찰 결과.

- [ ] **Step 1 — V01:** A·B에서 현재 익명 공유 시작 경로를 실행하고 앱을 재시작한다. 서로 다른 사용자와 각각의 멤버 유지 여부를 기록한다. OTP는 사용하지 않는다.
- [ ] **Step 2 — V02:** A가 테스트 그룹을 만들고 B가 A의 코드로 참여한다. 동일 그룹·두 멤버·A의 소유자 역할과 독립 개인 행성을 확인한다.
- [ ] **Step 3 — V03:** 로컬 기준값과 확인된 사용량 증분을 기록한다. 5분 관찰 동안 양쪽의 현재·누적 토큰, 성장 크레딧·순위를 비교한다. 기대값을 계산해 대조하고 같은 값만으로 정확성을 입증하지 않는다.
- [ ] **Step 4 — V04:** 상대 행성 상세 조회를 확인한다. 선택 대상과 표시 정보를 기록한다. 관찰한 UI 열람·편집 동작의 범위만 판정한다.
- [ ] **Step 5 — V05:** A·B의 집계 요청과 그룹 조회 응답에서 원본 정보·Auth 토큰·지갑 비공개 필드의 포함 여부를 확인한다. 증거에는 비밀과 원본 내용을 저장하지 않고 관찰 endpoint·필드 범위·판정 근거만 기록한다.
- [ ] **Step 6 — V06:** B의 앱 연결만 허용된 방식으로 차단한다. 새 사용량·대기열·재시작 유지 여부를 기록하고 연결을 복원한다. 복원 후 기대 증분만 반영되는지 확인한다. 재시도 전후 값과 기대 증분이 없으면 중복 방지는 미검증이다.
- [ ] **Step 7 — V07:** B 공유를 정지하고 알려진 새 사용량을 관찰한다. 로컬 증가와 서버 업로드 중지를 구분한 뒤 재개하여 허용 기록의 반영을 확인한다.

한 기준이 실패하면 원인·영향 범위를 CEO에게 보고한다. 실패한 기준에 의존하는 후속 기준은 중지한다. 독립적으로 관찰 가능한 기준은 CEO가 영향 범위를 확인한 뒤 진행할 수 있다.

## Task 4: 삭제·코드 재사용·재발급·cutoff·접근 거부 검증

**담당:** QA
**Files:** 파일 편집 없음; CEO가 허용한 전용 local/staging 테스트 그룹·데이터만 변경.
**의존성:** Task 3의 정상 기준값과 허용된 전용 테스트 데이터.
**Consumes:** Task 3의 기준값·A/B 계정 상태와 허용된 전용 테스트 데이터.
**Coverage:** V08–V11.
**Produces:** 삭제·탈퇴 cutoff, 재참여와 코드 동작, 비멤버 서버 거부 결과.

삭제와 탈퇴는 당일까지 기록을 제외하므로 다음 날짜까지 이어질 수 있다. D0·D1·D2는 그룹 시간대의 실제 서로 다른 날짜다. 날짜 대기는 긴 sleep으로 처리하지 않고 상태와 다음 재개 조건을 기록한다.

- [ ] **Step 1 — D0, V10:** B에 실제 서버 집계가 존재함을 기록한 뒤 공유 집계를 삭제한다. 대상 집계 삭제·일시정지·원본 보존과 당일 기록의 재업로드 방지를 확인한다. 삭제 전 집계가 없으면 실제 삭제 효과는 미검증이다.
- [ ] **Step 2 — D1, V10 잔여:** 다음 날짜에 B가 명시적으로 재개하고 그 날짜의 알려진 새 사용량을 공유한다. D0까지의 제외 기록은 복원되지 않고 D1의 허용 기록이 반영되는지 확인한다.
- [ ] **Step 3 — V08·V09·V11:** B의 D1 서버 집계·시간대를 기록한 후 탈퇴한다. B 인증으로 이전 그룹의 기존 get_world_planets(uuid) RPC를 조회하여 멤버십 거부를 관찰한다. 토큰을 문서나 shell command에 노출하지 않는 사전 확정 관찰 도구가 없으면 V11은 환경 차단이다.
- [ ] **Step 4 — D2, V08·V09:** 다음 날짜에 B가 최초 참여에 사용한 A의 코드로 재참여하고 공유를 명시적으로 다시 시작한다. 탈퇴 전 집계는 복원되지 않고 D2의 알려진 새 사용량은 반영되는지 확인한다. D2 확인이 불가능하면 당일 비복원 관찰만 부분 확인하고 이후 허용 기록 반영은 미검증으로 남긴다.
- [ ] **Step 5 — V08:** B가 다시 탈퇴한 상태에서 A가 코드를 재발급한다. B가 이전 코드로 실패하고 새 코드로 참여할 수 있는지 비교한다. 정원·기존 멤버십 오류가 코드 거부를 대신하지 않도록 한다.

시스템 시계 변경·테스트용 업로드 코드·새 fixture 생성으로 날짜를 우회하지 않는다. 기존 사용 기록으로 기대값을 설명할 수 없으면 관련 수치 기준을 미검증으로 기록한다.

## Task 5: 소유권 이전과 원하는 초대 정책 차이 확인

**담당:** QA
**Files:** 파일 편집 없음; CEO가 허용한 전용 local/staging 테스트 그룹·데이터만 변경.
**의존성:** A 소유자·B 멤버인 두 명 상태와 Task 4 기록 보존.
**Consumes:** Task 4에서 보존된 두 멤버 상태와 코드 재발급 결과.
**Coverage:** V12–V13.
**Produces:** 현재 소유권 경로 결과와 초대 정책 차이.

- [ ] **Step 1 — V12:** 두 멤버가 있는 상태에서 A가 이전 없이 탈퇴하려 할 때 거부되는지 확인한다. 멤버십이 유지되는지도 관찰한다.
- [ ] **Step 2 — V12:** 기존 UI의 소유권 이전 경로로 B에게 이전한다. 양쪽 역할을 확인한다. UI가 동작하지 않으면 실패를 기록하고, 기존 RPC 관찰은 CEO가 brief에서 허용한 경우에만 별도로 수행한다. RPC 성공으로 UI 실패를 통과 처리하지 않는다.
- [ ] **Step 3 — V12:** 이전 소유자 A가 탈퇴한 뒤 A의 이전 코드로 참여를 시도하고, 같은 상태에서 현재 소유자 B의 코드로 참여를 시도한다. 앞선 거부가 이미 참여 중이기 때문이 아님을 확인한다.
- [ ] **Step 4 — V13:** 현재 재사용·재발급 코드의 실제 관찰과 소스 근거를 7일 만료·한 번 사용·소유자 철회 정책과 비교한다. 실제 7일 만료 검증 또는 구현 완료로 주장하지 않는다.

## Task 6: 증거 통합·문서 최종 확인·수용 보고

**담당:** QA가 결과 반환, Secretary가 문서 통합, CEO가 수용.
**의존성:** Task 1–5 결과 또는 정확한 미실행·환경 차단 보고.
**Files:** Secretary가 결과 문서와 docs/release-acceptance.md 수정. 필요한 정책 문구 오류는 기존 문서 담당 범위에서 바로잡는다.
**Produces:** 18개 기준을 빠짐없이 판정한 결과 문서와 다음 조치.

- [ ] **Step 1:** QA는 P 검토에 필요한 문서 근거와 V01–V13 실행 결과를 CEO에게 반환한다. 실패·부분 확인·환경 차단을 모두 포함한다.
- [ ] **Step 2:** Secretary는 결과 문서에 기준별 검증 방법·범위, 환경·command·버전, 테스트 상태·시간대·viewport, 단계·기대·관찰, 증거, 실행 상태·기준 판정, 남은 범위를 기록한다.
- [ ] **Step 3:** 실행 상태는 passed / failed / not run / environment blocked로, 기준 판정은 proven / unmet / unverified로 각각 기록한다. 부분 확인은 하위 조건별로 남기고 전체 입증으로 합치지 않는다.
- [ ] **Step 4:** release 문서는 실제 관찰로 충족한 범위만 연결한다. 두 사용자 개발 환경 성공으로 패키지 앱·Windows·production·모든 출시 체크를 완료 처리하지 않는다.
- [ ] **Step 5:** 최종 문서 변경 범위에서 Task 1의 검색·diff 검토를 다시 한다. P01–P05를 최종 문서 위치에 연결한다. 결과 문서의 V/P 항목 수·누락·증거 연결은 직접 검토한다.
- [ ] **Step 6:** CEO에게 현재 흐름의 입증 범위, 미충족·미검증, 원하는 초대 정책의 잔여 차이와 후속 제안을 반환한다.

## 실패 처리와 검증 한계

- 환경 차단은 코드 결함으로 단정하지 않는다. QA가 환경을 수정해 우회하지 않는다.
- 현재 동작 실패는 재현 조건·관련 기준·기대·관찰·증거와 함께 CEO에게 보고한다.
- 제품 코드 수정은 이 계획에서 자동 수행하지 않는다. CEO가 새 설계·범위와 필요한 승인·계획·검증 경로를 정한다.
- 이후 수정이 이루어지면 영향받은 기존 증거를 다시 사용하기 전에 재검증한다.
- 원하는 초대 정책의 미충족은 승인 spec에 명시된 현재 차이다. 이를 숨기거나 현재 코드 경로의 성공과 합쳐 정책 완료로 보고하지 않는다.
- 실제 10명·11번째 가입, 세 번째 그룹 격리, 같은 계정 다중 기기, 전체 플랫폼 출시, 원본 집계 조작 방지는 이번 두 사용자 증명 범위가 아니다.
- Reviewer를 작업마다 자동 배정하지 않는다. CEO가 정책·권한·데이터 위험과 실제 발견에 따라 필요한 검토를 정한다.

## 승인·실행 준비

현재 적용 문서 세트는 이 승인 spec과 이 plan이다. 이후 기능의 spec·plan을 이번 실행 세트에 포함하지 않는다.

계획 승인을 받기 전에는 실행 checkout 선택·branch/worktree 변경·문서 staging·commit을 진행하지 않는다. 이후 실행 준비와 Git 작업은 Harness의 승인·handoff 계약에 따라 수행한다. 이 plan의 단계별 작업은 commit 지시가 아니다.

검증 환경·두 독립 사용자·관찰 도구·실제 날짜를 넘기는 재개 일정은 아직 확보된 것으로 간주하지 않는다. Task 2에서 확인해야 할 실행 조건이다.

## CEO self-review

- **범위·승인:** 승인 spec의 첫 하위 프로젝트만 포함했다. Coder 작업과 후속 기능 구현은 넣지 않았다.
- **기준 커버리지:** P01–P05는 Task 1·6, V01–V07은 Task 3, V08–V11은 Task 4, V12–V13은 Task 5에 연결했다. V13은 Task 4와 중복되지 않는다.
- **의존 순서:** 삭제 전 실제 집계, 다음 날 허용 업로드, 탈퇴 cutoff와 재참여, 이전 소유자의 탈퇴 상태를 명시했다.
- **증거:** 실행 상태와 기준 판정, UI·RPC·문서 근거를 구분한다. 검색·diff 확인은 문서 구조와 공백만 확인하며 실제 흐름을 입증하지 않는다고 명시했다.
- **범위 비례성:** 제품 코드·새 테스트·전체 테스트 실행을 포함하지 않은 문서 정합화와 기존 흐름 검증 계획이다.
- **환경 한계:** spec·plan 근거는 확인했지만 두 사용자 흐름, 환경 준비, 실제 staging 설정은 아직 검증하지 않았다.
