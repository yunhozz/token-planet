# Token Planet CLI 차단 요청 오프라인 진단 계획

> **실행 역할:** Yunho Harness를 따른다. CEO가 승인·handoff·수용을 조정하고, Coder가 승인된 임시 도구를 작성하며, QA가 격리와 증거를 확인하고, Reviewer가 경계를 독립 검토한다.

**Goal:** Supabase CLI 2.119.0이 요청한 차단 작업의 이미지 참조와 PS 필터·형식을 안전한 오프라인 관찰로 확인한다.

**Architecture:** 기존 CLI 소스가 정확한 v2.119.0으로 로컬에 있으면 먼저 읽기 전용으로 조사한다. 동적 관찰이 필요하면 OS 수준 격리 아래 임시 docker shim만 사용하고, 실제 Docker 호출은 항상 차단한다. 관측된 값 중 검증된 이미지 참조와 PS 필터·형식만 기록한다.

**Tech Stack:** 이미 검증된 로컬 Supabase CLI 2.119.0, Python 표준 라이브러리, 호스트 OS 격리 기능, 합성 설정과 fake Docker 명령.

**상위 계획:** [초대 lifecycle 계획](2026-10-08-token-planet-invite-lifecycle.md)

**이 계획의 저장 경로:** /Users/ma-24-007/Desktop/workspace/token-planet/docs/superpowers/plans/2026-10-08-token-planet-cli-operation-diagnostics.md

**계획 깊이:** 제품 동작을 바꾸지 않는 진단 Spike다. 제품 spec은 필요하지 않지만, 기존 stop rule을 유지하며 소스 확인·격리 gate·합성 검사·CLI 관찰의 의존성을 고정하기 위해 이 계획을 작성한다.

**승인 상태:** 2026-10-08 사용자는 `/tmp/token-planet-cli-offline-diagnostics-20261008/`의 임시 harness 작성과 합성 검증 범위를 승인하고 현재 checkout을 실행 대상으로 선택했다. CLI 관찰은 D02의 OS 격리 입증을 조건으로 하며, 해당 조건이 입증될 때까지 차단된다. 이 승인은 실제 Docker/DB, repo runner, 다운로드 또는 초대 기능 구현을 허용하지 않는다.

## Global Constraints

- 상위 계획의 fail-closed 경계와 중단 규칙을 유지한다. 일반 PULL 또는 넓은 PS를 허용하지 않는다.
- repo runner, 실제 Docker/DB, 이미지 pull, 다운로드, 의존성 설치를 실행하지 않는다.
- 저장소의 제품 코드·runner·guard·테스트·설정과 초대 기능을 변경하지 않는다.
- 이전에 검증한 로컬 CLI 2.119.0 바이너리만 사용한다. 새 바이너리·소스·이미지는 받지 않는다.
- OS가 전체 child process tree의 IPv4/IPv6 네트워크와 실제 Docker socket 접근을 차단해야 한다. 환경 변수, PATH, fake 명령만으로 격리를 주장하지 않는다.
- 격리를 입증하지 못하면 CLI를 실행하지 않고 environment blocked로 종료한다.
- fake PULL·PS는 항상 거부한다. 실제 Docker executable이나 socket으로 forwarding하지 않는다.
- 전체 argv, 환경 변수, stdin, CLI stdout/stderr, HTTP body/header를 파일에 저장하지 않는다. 허용된 이미지 참조와 PS 필터·형식만 메모리에서 추출해 기록한다.
- 이번 진단은 과거 요청을 복원하거나 이후 Docker 허용·runner 재실행·초대 구현을 승인하지 않는다.

## 확인된 근거와 한계

- supabase/ci/run.sh:749의 임시 Docker shim은 인자를 guard에 전달한다. 거부 경로는 product_docker_guard.py:1108, :1129, :1143에서 전체 argv를 받지만 :1132에서 operation 이름으로 줄인다.
- guard 기록은 product_docker_guard.py:200, :258의 schema에 따라 phase/code/operation만 남긴다. 관련 테스트는 비밀정보 비노출을 확인한다.
- /tmp/token-planet-invite-red-20261008-3/diagnostics.jsonl은 PULL 9회와 PS 1회만 입증한다. 서로 다른 이미지 9개였는지, PS에 project filter가 있었는지는 확인되지 않는다.
- start.log는 CLI 출력을 redaction했으므로 과거 전체 argv는 복원할 수 없다. 새 fake 실행에서 관측한 결과도 과거 실행의 정확한 인자 증거가 되지 않는다.
- canonical handoff: /tmp/token-planet-multiplayer-handoff-20261008.md.

## Review Focus

| 위험 | 확인할 동작 |
| --- | --- |
| PATH·환경 변수만 바꾸고 OS 격리라고 오인 | 실제 process tree와 Docker socket 차단을 입증하지 못하면 CLI를 실행하지 않는다. |
| 복잡한 argv를 재조합하며 이미지·필터가 달라짐 | 합성 입력에서 공백·순서·반복 필터·tag/digest parsing을 검사한다. |
| 다른 argv나 출력에 든 비밀을 기록 | 필요한 필드만 허용하고 전체 argv·env·stdin·stdout/stderr를 저장하지 않는다. |
| PULL 거부로 PS에 도달하지 못함 | 관찰 범위를 부분으로 기록하고 성공 응답을 꾸며내지 않는다. |
| 횟수 일치로 과거 인자를 복원했다고 주장 | 새 synthetic 실행과 과거 로그의 증거 경계를 분리한다. |

## 역할과 임시 산출물

- CEO: 이 계획의 승인, 실행 전 격리 증거 확인, 결과 수용.
- Coder: 승인 이후 /tmp 전용 진단 harness 작성. 저장소 파일은 편집하지 않는다.
- QA: 합성 검사와 격리 증거를 독립 확인하고 기준별 결과를 기록한다.
- Reviewer: QA 뒤 비밀정보 처리와 격리·forwarding 경계를 읽기 전용 검토한다.

승인 후 사용할 전용 경로:

    /tmp/token-planet-cli-offline-diagnostics-20261008/

경로가 이미 존재하거나 symlink이면 재사용·삭제하지 않고 중단한다. 계획상 임시 파일은 offline_diagnostic.py, bin/docker, fixtures.json, tests/test_offline_diagnostic.py, private/operations.jsonl, summary.json이다. 허용하지 않은 요청 인자나 비밀정보는 파일에 남기지 않는다.

## Task 1: 기존 소스와 바이너리 근거 확인

- [ ] canonical handoff에서 기존 CLI 바이너리의 절대 경로와 공식 checksum 검증 근거를 확인한다.
- [ ] 이미 로컬에 있는 소스 후보가 있을 때만 tag/commit/manifest로 정확한 v2.119.0인지 확인한다. 네트워크 조회나 다운로드는 하지 않는다.
- [ ] 정확한 소스가 있으면 db start의 Docker 명령 생성 경로를 읽어 source-derived 예상으로 기록한다.
- [ ] 정확한 소스가 없으면 exact local source unavailable로 기록한다. 구현을 추정하거나 추가 자료를 받지 않는다.

승인 후 확인 명령:

    rg -n '2\.119\.0|checksum|SUPABASE_BIN' /tmp/token-planet-multiplayer-handoff-20261008.md

예상 결과: 기존 바이너리 근거를 찾거나 정보 부족을 명시한다. 누락은 다운로드로 보완하지 않는다.

## Task 2: OS 격리 gate와 합성 검사

- [ ] 사용 가능한 호스트 OS 격리 기능을 읽기 전용으로 확인한다. 설치, privilege 확대, 시스템 정책 변경은 하지 않는다.
- [ ] 격리는 CLI 전체 process tree에 적용한다. IPv4/IPv6 통신과 실제 Docker socket 접근·Docker executable 실행을 차단하고, 허용되는 network path는 두지 않는다.
- [ ] 정책이 loopback 통신과 실제 Docker socket 경로 접근을 거부한다는 OS 수준 증거를 확인한다. connection refused나 socket 부재만으로 격리를 입증하지 않는다.
- [ ] 하위 프로세스에 사용자 Docker config, credential, proxy, 전체 환경을 상속하지 않는다.
- [ ] QA가 격리 근거를 확인하지 못하거나 OS 정책이 없으면 CLI 실행 없이 environment blocked로 종료한다.
- [ ] /tmp harness의 테스트를 먼저 작성한다. 합성 argv의 순서·공백·반복 필터·--filter=value·image tag/digest 처리, 허용 필드만 기록, sentinel 비밀 미기록, PULL·PS·알 수 없는 명령의 forwarding 없는 거부를 검증한다.
- [ ] 테스트 우선 실행은 실패해야 한다. 테스트를 만족시키는 최소 fake parser/shim을 구현한 뒤 같은 테스트를 통과시킨다.
- [ ] 격리 gate와 합성 테스트를 통과하기 전에는 CLI를 실행하지 않는다.

승인 후 단위 검사 명령:

    python3 -I -m unittest discover -s /tmp/token-planet-cli-offline-diagnostics-20261008/tests -v

첫 실행은 테스트가 아직 없는 상태에서 의도한 assertion failure를 보여야 한다. 구현 후 같은 명령은 argv 파싱, deny-only 동작, 허용 필드 기록, 파일 권한과 비밀정보 비노출 검사가 통과해야 한다. 두 실행 모두 실제 CLI·Docker를 호출하지 않는다.

호스트별 sandbox 실행 방식은 현재 검증되지 않았으므로 이 계획에 명령을 추정해 넣지 않는다. 사용 가능한 OS 정책이 격리 조건을 충족하지 않으면 다음 단계로 넘어가지 않는다.

## Task 3: 제한된 CLI 관찰

선행 조건: Task 1의 바이너리 근거가 확인되고, Task 2의 OS 격리와 QA 검증이 모두 통과해야 한다.

- [ ] 합성 project 설정과 합성 network 인자를 사용한다. 원본 project 설정·workdir·사용자 환경은 복사하지 않는다.
- [ ] 검증된 CLI의 db start --network-id <synthetic network>만 격리된 child로 실행한다. supabase/ci/run.sh, reset, migration, SQL 테스트는 호출하지 않는다.
- [ ] fake docker 명령은 PULL 요청에서 이미지 참조만, PS 요청에서 검증된 filter·format만 메모리에서 추출한다. 전체 argv는 저장하지 않는다.
- [ ] fake PULL·PS는 항상 고정된 거부 결과를 반환한다. 그 외 요청은 사전에 정의한 합성 응답만 사용하고, 예상 밖 요청은 즉시 거부한다.
- [ ] timeout·요청 수 제한을 적용한다. CLI 출력은 저장하지 않고 종료 상태만 기록한다. 중단할 때는 이 probe가 만든 child process만 종료한다.
- [ ] CLI가 fake shim을 거치지 않고 다른 실행 경로나 실제 socket을 사용하려 하면 OS 정책이 차단해야 한다. 격리 위반 가능성 또는 예상 밖 요청이 보이면 즉시 종료하고 environment blocked 또는 partial로 보고한다.

승인 후 실행 명령:

    python3 -I /tmp/token-planet-cli-offline-diagnostics-20261008/offline_diagnostic.py capture

예상 결과: 실제 Docker/DB 연결 없이 허용된 이미지 참조·PS 필터·형식만 기록한다. CLI가 거부로 종료되는 것은 예상 가능하며, 진단 성공 여부와 별도로 보고한다.

## Task 4: 결과 요약과 수용

- [ ] 새 관찰의 operation별 횟수·순서와 기존 PULL 9회·PS 1회를 비교한다.
- [ ] 허용 필드만 요약 문서에 출력한다. 검증하지 못한 값은 원문 대신 withheld로 표기한다.
- [ ] 새 실행에서 9/1이 재현되지 않으면 partial observation으로 보고한다. 횟수가 일치해도 새 synthetic 실행의 관찰일 뿐 과거 정확한 argv 복원이라고 주장하지 않는다.
- [ ] OS 격리·요청 기록·비밀정보 비노출의 증거와 한계를 CEO, QA, Reviewer가 확인한다.
- [ ] 다음 Docker 허용, runner 재실행, 초대 기능 구현은 별도 계획과 승인으로 남긴다.

승인 후 요약 명령:

    python3 -I /tmp/token-planet-cli-offline-diagnostics-20261008/offline_diagnostic.py summarize

예상 결과: 허용된 이미지 참조·PS 필터·형식, 실행 상태, 관찰 한계만 공개한다.

## 수용 기준과 hard stop

| ID | 기준 |
| --- | --- |
| D01 | 기존 바이너리 근거와 정확한 로컬 소스 존재 여부를 구분한다. |
| D02 | OS가 전체 process tree의 network와 실제 Docker 접근을 차단함을 입증한다. |
| D03 | 합성 테스트에서 PULL·PS·알 수 없는 명령이 forwarding 없이 거부된다. |
| D04 | 로그에는 허용된 이미지 참조와 PS 필터·형식만 남고 sentinel 비밀은 없다. |
| D05 | 실제 CLI 관찰은 fake shim 아래에서만 수행하며 repo runner·실제 Docker·DB는 사용하지 않는다. |
| D06 | partial observation과 과거 exact argv 복원 불가를 정확히 보고한다. |

D02를 입증하지 못하면 CLI를 실행하지 않는다. 바이너리 불일치, OS sandbox 미지원, 실제 socket 접근 가능성, 허용되지 않은 요청, 비밀정보 기록 가능성은 모두 hard stop이다. 새 계획 승인 이후에도 이 조건을 완화하지 않는다.

## 승인 기록과 실행 상태

2026-10-08 사용자는 이 계획의 제한된 임시 harness 작성과 합성 검증을 승인했고, 실행 대상으로 현재 checkout을 명시 선택했다. 승인 근거는 canonical handoff의 `오프라인 진단 계획 승인·대상 선택 및 OS preflight`에 기록되어 있다.

CLI 관찰은 D02의 전체 process tree에 대한 OS 수준 network·실제 Docker 접근 차단 입증과 Task 2의 QA 검증을 충족해야 하며, 현재 해당 gate가 충족되지 않아 차단된 상태다. 실제 Docker/DB 작업, repo runner 실행, 다운로드, guard 변경과 초대 기능 구현은 이 승인 범위에 포함되지 않는다.
