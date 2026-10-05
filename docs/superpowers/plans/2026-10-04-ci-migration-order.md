# CI 마이그레이션 순서 재생 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Harness에 따라 Lead가 Coder를 순차 조정하고 QA 후 독립 Reviewer를 배정한다.

**Goal:** 원본 migration 파일명·SQL·기존 DB 이력을 보존하면서 새 CI DB에서 의존 순서에 맞춘 전체 migration 재생과 SQL 회귀를 실행한다.

**Architecture:** 원본 버전 문자열을 사전식으로 정렬한 뒤 새 임시 프로젝트에 균일한 14자리 synthetic 버전으로 SQL bytes를 복사한다. Supabase CLI `2.119.0`의 표준 start/reset 의미론을 사용하며 manifest로 일대일 대응·hash를 검증한다. 임시 DB 이력은 synthetic이며 배포·기존 DB 업데이트에 재사용하지 않는다.

**Tech Stack:** Python 표준 라이브러리, Bash, Supabase CLI2.119.0, Docker, PostgreSQL17, 기존 pgTAP SQL, GitHub Actions.

**Spec:** 사용자가 승인한 CI 전용 재생기 범위 및 아래 통합 설계.

## 브리프와 통합 설계

- Workspace: `/Users/yunho/Desktop/project/token-planet`, clean `master`에서 시작. git-dir/common-dir 모두 `.git`, submodule guard 빈 결과. 사용자가 현재 checkout 작업을 선택해 worktree를 만들지 않는다.
- 실패: `20261001000101_shop_actions.sql` statement0의 `private.shop_action_request` 부재. 파일명 byte 정렬은 001 자식을 부모보다 먼저 적용하고 002 family에도 동일하다.
- 기존 로컬 DB read-only history는 부모001/002와 자식00101–103/00200–208을 이미 포함한다. 원본 이름/SQL/기존 이력을 수정하지 않는다.
- 원격 origin/master tree와 모든 로컬 ref 이력에 `.github`가 없다. 신규 `.github/workflows/supabase-migrations.yml`을 연결점으로 추가한다. 기존 Actions run의 성공을 주장하지 않는다.
- Scope는 bounded지만 실행 순서/공유 이력/환경 수명 의존성이 있어 계획을 생략하지 않는다. migration/integration-order 위험에 따라 Coder→QA→전체 독립 Reviewer 순서가 필요하다.
- 공식 CLI2.119.0 JS migration-list는 UTF8 파일명 정렬이며 regex는 숫자버전+underscore다. suffix-only 변경으로 원래 version을 유지하면서 순서를 바꿀 수 없다.
- 원본 버전 문자열 순서는 부모001→00101..103→부모002→00200..208이며 정수 비교를 사용하지 않는다.
- missing verification-before-completion은 사용 가능 skill 목록에 없었다. 실제 systematic-debugging의 실패 재현→회귀→수정→검증 및 TDD 체크리스트로 확인한다.

## 전역 제약과 수용 기준

- 기존 pinned DB/hosted DB reset, migration, history repair, link, db push, remote db-url, push/배포 금지.
- 원본 migrations/config/tests/fixtures는 입력이며 수정하지 않는다.
- 로컬 설치·이미지 pull 금지. 설치된 `/opt/homebrew/bin/supabase`2.119.0과 cached `public.ecr.aws/supabase/postgres:17.6.1.171`만 사용한다. 필요 image가 다르면 실행 전에 중단한다.
- GitHub hosted CI는 새 runner에서 pinned CLI와 필요 image를 준비한다. 사용자 로컬 설치 금지와 별개이며 hosted Supabase 연결 권한은 없다.
- 새 project `token-planet-ci-<UUID>`, 새 mktemp workdir, DB56432/shadow56430. 충돌 시 기존 프로젝트 fallback 없이 종료한다.
- Docker binding은 loopback을 우선한다. CLI가 wildcard를 강제하면 시작 전에 해당 범위를 CEO에게 보고해 결정받는다. network_restrictions는 Docker bind가 아니다.
- synthetic version은 `20000101000000 + ordinal`의14자리 숫자이고 파일 접미사는 원본 filename을 보존한다. namespace 재입력은 거절한다.
- 원본/복사본 bytes/hash/집합/순서가 같아야 한다. 전체 migration replay와 현재 SQL suite PASS가 수용 조건이다.
- 실패 시 후속 단계가 멈추고 생성한 exact resource만 정리한다. 로그/manifest는 남기고 임시 DB/container/volume/workdir는 제거한다.

## 검토 초점

1. source가 정렬/복사 중 변경되면 실패한다.
2. symlink·잘못된 이름·중복 버전·예약 namespace를 staging 전에 거절한다.
3. source/output 겹침 또는 nonempty output을 덮어쓰지 않는다.
4. 실패/취소 cleanup은 최초 오류를 보존하고 다른 DB를 건드리지 않는다.
5. synthetic 이력은 배포용이 아님을 manifest/문서/입력 계약에 명시한다.

## Task1: staging과 manifest

**Files:** `supabase/ci/prepare_migrations.py`, `supabase/ci/test_prepare_migrations.py`

**Interfaces:** `prepare(source: Path, output: Path) -> dict`, `verify(source: Path, output: Path) -> None`. CLI `prepare|verify --source <absolute migrations> --output <absolute empty staging>`. verify는 read-only다.

Manifest `format_version:1`, `history_mode:synthetic_ci_only`, entries의 `ordinal,source_filename,source_version,synthetic_version,staged_filename,sha256`을 기록한다.

- [ ] 두 family fixture와 각기 다른 SQL bytes로 순서/hash 테스트를 작성한다. 기대 순서는 `sorted(names, key=lambda name: name.split('_',1)[0])`; staged filename 정렬과 manifest 순서도 같아야 한다.
- [ ] `python3 -m unittest discover -s supabase/ci -p 'test_*.py' -v`로 구현 부재 RED를 확인한다. 환경 오류는 RED로 간주하지 않는다.
- [ ] 빈 입력, 잘못된 이름, duplicate version, symlink/directory, nonempty output, source/output overlap, synthetic namespace 입력, source/staged/manifest 변조, 추가/삭제 파일 거절 테스트를 작성한다.
- [ ] regex `^([0-9]+)_(.+)\.sql$`로 전체 입력을 먼저 검사하고 source_version 문자열로 정렬한다. 원본 bytes를 그대로 복사하고 SHA256을 다시 검증한다. JSON manifest를 생성한다.
- [ ] verify는 전체 expected manifest를 재계산하고 source/staged 집합과 bytes/hash를 비교한다. Python 전체 테스트 GREEN을 확인한다.

## Task2: 새 DB runner

**Files:** `supabase/ci/run.sh`, `supabase/ci/test_prepare_migrations.py`

**Consumes:** Task1 CLI. **Produces:** `run.sh --artifacts-dir <absolute path>`와 manifest/prepare/start/reset/test-db/cleanup 로그.

- [ ] fake CLI/Docker invocation recorder로 RED 테스트: 잘못된 CLI version/remote token/충돌은 start 이전 거절; start/reset/test 실패는 후속 호출 중단; cleanup은 최초 exit를 유지; 성공 순서 start→reset local→test→cleanup.
- [ ] `set -Eeuo pipefail` 사용. UUID와 mktemp를 내부 생성하고 외부 project/db/workdir 입력을 받지 않는다. SUPABASE_BIN은 절대 실행 경로만 허용한다.
- [ ] 임시 config에 project-id/ports/seed disabled 설정; tomllib로 결과 확인. linked metadata/env/credentials는 복사하지 않는다. tests/fixtures만 복사한다.
- [ ] exact container/volume 부재, port 충돌, CLI2.119.0, cached DB image와 binding을 preflight한다. 조건 실패 시 Docker mutation 전 종료한다.
- [ ] 표준 CLI DB-only start 또는 full start의 exclude 목록으로 cached Postgres만 시작한다. `db reset --local --no-seed --workdir <new>`만 실행한다. psql로 migration replay를 대체하지 않는다.
- [ ] 원본/staging을 각 단계 전후 검증한다. 새 DB history의 synthetic version 목록과 manifest를 비교한다. SQL 회귀는 CLI testdb 또는 image 내 psql 실행+엄격한 TAP 결과 판독으로 수행하며 후자는 migration 실행과 구별한다. 테스트 도구 이미지가 추가 필요하면 로컬 설치/pull 없이 기존 psql을 사용한다.
- [ ] 생성한 container/volume의 exact 이름/ID를 기록한다. cleanup trap에서 그 resource만 확인하고 제거한다. prune/image삭제/다른 project stop 금지. 최초 오류와 cleanup 오류를 모두 보고한다.
- [ ] Python 행동 테스트와 `bash -n supabase/ci/run.sh` GREEN을 확인한다.

## Task3: workflow와 README

**Files:** `.github/workflows/supabase-migrations.yml`, `supabase/README.md`, Python structural tests.

- [ ] workflow 부재 RED 구조검사: read-only contents permission, pull_request/master push/workflow_dispatch, pinned CLI2.119.0, Python tests, runner, always artifact, 원격 secrets/link/push/repair/db-url 없음.
- [ ] 공식 setup-cli의 검증한 revision/version input으로 workflow 작성. job timeout, cancel-in-progress false, 각 실행 UUID isolation을 사용한다.
- [ ] `bash supabase/ci/run.sh --artifacts-dir "$RUNNER_TEMP/supabase-migration-results"` 호출. always upload는 logs/manifest만 포함하고 credentials/DB volume은 제외한다.
- [ ] README에 원본 보존과 synthetic 임시 history를 설명한다. 원본 checkout의 stock db reset 파일정렬 제약은 남아 있으며 새 CI 경로만 해결한다. 배포/기존 DB에 재사용 금지 및 로컬 cache/preflight 조건 명시.
- [ ] 구조/Python/shell checks 및 `git diff --check`를 실행하고 원본 migration/config/tests 변경 없음 확인.

## Task4: 독립 검증과 수용

- [ ] QA가 Python/shell/workflow 구조검사를 독립 실행한다.
- [ ] QA가 승인된 새 disposable 환경에서 전체31개 replay와 현재 전체 SQL suite를 실행한다. 실제 counts/failures를 기록한다.
- [ ] synthetic history/원본 filename hash/config hash/기존 pinned history 불변/새 resource cleanup을 확인한다.
- [ ] QA 후 Reviewer가 최종 전체 diff의 ordering/validation/remote차단/resource ownership/error cleanup/workflow/docs를 독립 검토한다.
- [ ] 행동 finding은 Coder 수정→영향 QA rerun→필요 Reviewer recheck. 증거 gap이 있으면 완료/commit 금지.
- [ ] 수용 후 optional focused commit `fix(ci): 상점 마이그레이션의 CI 재생 순서 교정`; push 금지.

## 자체 검토 및 다음 단계

승인 범위와 모든 수용 조건을 task에 연결했다. 인터페이스/key/paths가 일치하며 원본 history 보존과 synthetic 임시 history 차이를 명시했다. 입력 validation→staging→runner→workflow의 의존 순서 때문에 비례 계획을 유지했다. 남은 실행 결정은 CLI image/binding 지원의 실제 preflight이며 조건 불충족은 환경 blocker로 보고한다. 이 계획의 승인 전 Coder 구현을 시작하지 않는다.

## 실행 기록

- 2026-10-04 CEO 전달: 사용자가 계획 승인 요청에 `ㅇ`으로 답해 본 계획을 승인했다. Coder에게 위 exact ownership를 부여했다.
- Coder는 Docker를 실행하지 않는다. 실제 QA disposable Docker 실행은 socket escalation이 필요하므로 exact command/project를 CEO에게 보고한 뒤 승인 경로를 사용한다.
- CLI2.119 공식 source 확인: 기본 port publish는 hostPort만 지정한다. loopback 요구를 충족하려고 실행 소유 bridge network에 `com.docker.network.bridge.host_binding_ipv4=127.0.0.1`을 설정하고 CLI `--network-id`로 전달한다. 시작 전 network 옵션과 시작 후 actual HostIp를 확인한다. network도 exact 소유 cleanup에 포함한다. Docker 공식 문서: https://docs.docker.com/engine/network/drivers/bridge/#default-host-binding-address
- CLI resource 이름은 project-id를40자로 자른다. project-id는 `token-planet-ci-`와 UUID hex의 앞24자(총40자)로 내부 생성하여 truncation 불일치를 방지한다.
- CLI `.temp/postgres-version`은 새 프로젝트에만 `17.6.1.171`로 작성한다. 기존 linked metadata를 복사하지 않으며 cached image 참조를 고정한다. 공식 image resolver는 cache hit 시 pull하지 않는다. Source 증거 `/private/tmp/ci-migration-{db-image,image-resolve,postgres-service,docker-ids,container-lifecycle}.ts`.
- CEO는 `SUPABASE_TELEMETRY_DISABLED=1 supabase --version` 및 start/reset/test/stop 도움말이 sandbox에서 성공함을 확인했다. runner도 telemetry를 비활성화해 사용자 홈의 telemetry 파일 쓰기를 피한다.
- 실행 safety 통합: Docker daemon override(`DOCKER_HOST`, `DOCKER_CONTEXT`, TLS/cert override)를 거절하며 read-only context endpoint 검사로 local Unix socket만 허용한다. SSH/TCP daemon은 신규 local disposable 범위가 아니므로 mutation 전 종료한다.
- macOS 임시경로 `/var`와 `/private/var` 차이로 workdir 소유 label 검증이 실패한 fake RED를 확인했다. workdir를 physical absolute path로 정규화한다.
- production socket 포트 preflight를 유지한다. sandbox fake 검증은 테스트 전용 python3 wrapper가 그 stdin probe만 에뮬레이트한다. 제품 bypass flag를 추가하지 않는다.
- CLI raw stdout/stderr와 Docker info/inspect/collision 출력은 임시workdir에만 보관하며 성공/실패 모두 허용된 상태/마이그레이션 진행 로그만 artifact로 필터링한다. 원문을 run.log에 합치지 않는다.
- 공식 CLI2.119 PG17 reset source는 container/volume를 제거·재생성한다. reset에도 실행 소유 `--network-id`를 전달하며, 이후 bridge attachment·모든publishedHostIp를 재검증한다. cleanup은 exact generated container 이름으로 inspect하고 project/workdir labels를 확인한 새ID만 제거한다. reset ID교체/partialfailure를 fake 회귀로 검증한다.
- CLI는 inherited `SUPABASE_PROJECT_ID`/DB/image override를 config보다 먼저 적용할 수 있다. 내부 생성 프로젝트 선택을 보장하려고 허용된 BIN/telemetry 변수 외 Supabase override를 mutation 전에 거절한다.
- 첫 runtime(CEO 상승실행)은 CLI start 성공 후 `HostConfig.PortBindings` 요청HostIp 빈값을 실제binding으로 오판하여 failclosed 종료했다. Root/Lead elevated readonly 확인에서 실행 container/volume/network 및56432listener 모두 없으므로 cleanup 완료다. 기존pinned DB의 field-only inspect는 요청HostIp `""`와 실제NetworkSettings.Ports의 wildcard값을 확인했다.
- Binding 판정은 `NetworkSettings.Ports` 실제publishedHostIp를 사용한다. bridge attachment는 `NetworkSettings.Networks[*].NetworkID`로 분리하며 safe failure code/필드summary를 남긴다. realistic inspect의 requested-empty/actual-loopback shape를 RED→GREEN으로 고친다. 원인해결·freshQA·CEO검토 전runtime재실행금지다.
- 증거 구분: 첫 신규DB의actual published HostIp는inspect가cleanup으로삭제되어확인하지못했다. requested-empty/actual-loopback의fixture로guard버그를재현했다. 기존pinnedinspect는requested와actualfield의차이만입증한다.
- CEO 추가gate:2차실행전에수정된frozen-tree독립staticQA→독립fullReviewer를완료한다.2차runtimeQA후코드불변이면동일Reviewer가새실행증거를확인해최종수용을마무리한다.
- 독립fullReviewer(P2): cleanup inspect 실패를 부재로 처리하면 daemon 장애 때 resource 누수를 성공으로 보고한다. exact not-found만 부재이며 다른 검사실패는 safe code와 cleanup_status=1; 원래실패status는보존하고검증되지않은resource는삭제하지않는다.
- 독립fullReviewer(P2): artifact 경로 문자열검사에 `..`/부모symlink를쓰면실제로source안에쓸수있다. physical root/output 정규화후쓰기전에검사하고회귀를추가한다.
- CEO는 `.temp/cli-latest`의8bytes `v2.119.0`을확인하고원래0bytes/hash로복원했다. CLI버전조회도mktemp cwd에서실행해sourcecache보존회귀로확인한다.
- 두번째실제상승실행은actual `NetworkSettings.Ports`가 `0.0.0.0:56432`여서failclosed됐다. bridge option `127.0.0.1` 확인만으로실제binding을보장하지못한다. Root는exactcontainer57491b…/volume `supabase_db_token-planet-ci-fd879b49ee084fdd899a2174`/network8ad2f… 제거후engine정상·세resource부재·56432listener부재를상승읽기전용확인했다.
- 현재runtime수용은BLOCKED다. fixture는guard거절을검증했으며실제loopback성공을입증하지않는다. 기존bridge/default-binding 설계로재시도하지않는다. failedrunartifact를보존하고explicitHostIp 또는hostpublish없는실행의새설계/승인전추가Dockerprobe·구현을하지않는다. 전체reset/SQL40은아직미검증이다.

## 재설계 검토: 명시적 HostIp bounded spike (승인 대기)

실패 증거는 첫 실행 `/private/tmp/token-planet-ci-replay-qa-20261004-01`, 두 번째 실행 `/private/tmp/token-planet-ci-replay-qa-20261004-02`에 보존한다. 첫 실행은 actual binding 미수집, 두 번째 `container-binding-start.log`는 actual_ip=0.0.0.0 및 host_port=56432를 기록했다. 두 번째 `cleanup.log`와 CEO의 상승 읽기전용 검사는 exact container/volume/network 및 listener 부재를 확인했다.

Planner의 공식 pinned source 조사에서 `formatPortBindingFlag()`는 `56432:5432`를 생성하고 HostIp 설정 연결점은 발견되지 않았다. CLI가 이름 `docker`를 subprocess로 실행하는 연결점을 이용한 실행 소유 임시 PATH 인수 어댑터를 bounded spike로 권고한다. bridge 옵션이 적용되지 않은 근본 원인은 미확인이다. 명시적 HostIp의 성공도 아직 가설이다.

권고 초안은 CLI 프로세스에만 임시 어댑터를 적용하고 exact project/container 이름, workdir label, network ID, cached image 및 DB publish 인수를 검증한 create에만 `127.0.0.1:56432:5432`를 넣는다. real Docker 절대경로를 고정하며 argv, stdin/stdout/stderr, exit status를 보존한다. 예상 밖 publish/create는 실행 전 거절한다. 원문 argv/environment는 artifact에 남기지 않는다. global PATH, daemon, 설치 CLI는 변경하지 않는다. 기존 actual binding guard와 소유 cleanup을 유지한다.

승인 요청 범위는 mock 인수 검사 후 제품 migration/fixture 없는 새 UUID 프로젝트에서 cached image만 사용한 start 한 번이다. 실제 loopback PASS일 때만 reset 한 번과 재생성 binding을 확인한다. 모든 경로에서 exact 소유 cleanup을 수행하고 실패 시 재시도하지 않는다. 전체 제품 replay는 이 spike에 포함하지 않는다. spike 결과 후 수정 설계·계획 승인과 Coder TDD, 독립 QA→Reviewer를 거친다.

수용 기준은 start/reset actual HostIp loopback, 표준 CLI migration/history 의미 보존, 원본 및 source cache/pinned resource 불변, cache miss/download/foreign create 거절, exact cleanup이다. 기존 static QA46/46는 어댑터 실제 실행 증거가 아니다. 대안인 hostpublish 제거는 CLI 연결 계층까지 영향이 미확인이고 experimental stack은 legacy image pin을 무시하며 현재 cache 밖 image를 요구하므로 우선 권고하지 않는다.

- CEO 승인 해석: 사용자는 이미 CI 전용 신규 disposable replay와 loopback 전용 실행을 명시적으로 승인했다. 명시적 HostIp PATH 어댑터는 DB/image/remote 범위를 확대하지 않고 binding을 좁히므로 중복 승인 없이 bounded 구현을 진행한다. Coder TDD와 fresh static QA→독립 Reviewer 후 빈 프로젝트 start 1회, 실제 loopback PASS일 때만 reset 1회를 실행한다. 실패 시 재시도하지 않는다. 성공 후 전체 runner 통합과 다시 static QA→Reviewer를 거쳐 기존 승인된 전체 replay를 진행한다. 두 실패 artifacts는 보존한다.
- 어댑터 mock 행동 RED→GREEN: import 오류는 유효 RED로 세지 않고 importable passthrough stub의 기대 assertion 실패를 기록했다. container create identity/publish/mount/labels와 phase marker, CLI exact volume create 소유권 검사를 추가했다. root create 외 container create/run alias, pull/image pull, network create를 거절한다. 독립 Reviewer는 Docker create의 기본 missing-image pull을 추가 P2로 발견했다. CEO는 cache-only 범위를 좁히는 정확한 `--pull=never` 삽입을 승인했다. 이는 기존 publish-only 변경 초안에 추가되는 의도적 안전 계약이며, 원본 CLI migration/history 의미는 보존한다. 수정 후 fresh QA와 Reviewer 재검토 전 실제 spike는 실행하지 않는다.
- SIGTERM P2 수정 후 독립 full static QA84/84(39.628초), 원본32/cache baseline 불변, Reviewer 재검토 통과. frozen spike SHA `3ed470d077678dacb14bf5e489082633049e9d133790fd722180760d929d0c17`.
- sandbox artifact `/private/tmp/token-planet-ci-hostip-spike-20261004-03`는 Docker Unix socket permission denied preflight에서 종료했고 workdir를 제거했다. start/reset 호출은 없었다.
- CEO 상승 one-shot artifact `/private/tmp/token-planet-ci-hostip-spike-20261004-04`: network option 확인 PASS, CLI start 한 번 exit1, binding 검사까지 도달하지 못함, reset0회. cleanup summary는 container/volume absent, network/workdir removed다. 재시도하지 않는다. raw CLI output과 정확한 resource identity는 임시경로 cleanup으로 보존되지 않아 실패 원인/독립 exact-resource 검증에 증거 gap이 있다. 실제 explicit loopback 성공은 미입증이며 전체 product replay/SQL40은 수행하지 않았다. 추가 실행 없이 읽기전용 상태 확인과 실패 원인 진단 계획이 필요하다.
- 2026-10-05 사용자 승인: 안전 진단 보완 후 새 disposable project 1회 재시도 허용. 진단 TDD3RED→focused24GREEN, 독립fullQA87/87(42.706초) 및 Reviewer 통과 후 CEO 상승 실행.
- artifact `/private/tmp/token-planet-ci-hostip-spike-20261005-01`: project `token-planet-ci-968a087e0b0b45c8ad024453`, network `345d78b5d59fe07e401e97f2b10f06cf945f34e6bb972ccf1c34ff8fdd31f4b8`. network PASS, start1회 exit1, actual binding 미관측, reset0회. container/volume absent, network/workdir removed 요약 보존. 새 승인 재시도는 소진됐으며 추가 start 금지.
- 안전 classifier는 존재하고 start 실패 때 호출됐지만 매치되지 않은/빈 출력은 아무 분류도 기록하지 않는 증거 gap이 남았다. 따라서 이번 artifact는 실패 원인을 확인하지 못했다. raw output은 보존하지 않았다. exact Docker state/events 읽기전용 확인과 진단 fallback 보완 검토가 다음 단계이며 전체 migration31/SQL40 replay는 미실행이다.
- CEO 상승 읽기전용 Docker events 확인: 이번 exact project에는 DB container create/start/healthy 후 cleanup kill/stop/destroy 이벤트가 있었고 다른 project container는 없었다. 이는 artifact의 cleanup 전 `container=absent`만으로 create 미호출을 추정할 수 없음을 보여준다. CLI start exit1의 원인은 여전히 미확인이다.
- 후속 승인 범위는 로컬 진단 fallback 수정과 fake TDD/full QA/Reviewer뿐이다. stdout/stderr empty/unclassified 및 byte/line count, adapter marker present/absent를 cleanup 전에 항상 기록하며 원문 CLI/State.Error는 제외한다. 세 번째 disposable start는 별도 사용자 승인 전 금지한다.
- fallback TDD3RED(6assertion 실패)→focused26GREEN, 독립fullQA89/89(41.702초), 원본32/cache baseline/10file hash 불변, Reviewer 추가finding없음. frozen spike SHA `e40044ac86ff52e1788698b11aa10e69cec8f57b141c7dfa1b5a671b8d73d4f5`.
- counts는 decoded 문자열의 UTF-8 재인코딩 크기와 stream 전체 line 수다. marker presence는 create 시도 기록이며 성공 증명이 아니다. 원인은 여전히 미확인이다. 세 번째 runtime은 실행하지 않았으며 별도 사용자 승인 전 멈춘다.
- 사용자 `ㅇㅇ` 승인 후 2026-10-05 세 번째 fresh 시도 artifact `/private/tmp/token-planet-ci-hostip-spike-20261005-02`, frozen e40044ac... 실행. project `token-planet-ci-4a5ecdf3604d472bb1aa99fb`, network `a5cb3ac07bd19925561191626fa551e5de8571682159eb5ff07c429f44166097`. start1회 exit1/reset0회/binding 미관측. container/volume create marker는 present, stdout unclassified95bytes/1line, stderr unclassified67bytes/3lines. cleanup 전 container absent, cleanup은 volume/container absent 및 network/workdir removed를 기록했다. 원문 미보존으로 operational cause는 여전히 미확인이다.
- Lead 상승 읽기전용 확인에서 exact container/volume/network 각각 명확한 not-found, TCP56432 listener 없음(lsof exit1/출력없음)을 확인했다. 추가 start/reset 재시도하지 않으며 전체31migration/40SQL replay는 미실행이다.

## 2026-10-05 bounded 정적 보완 승인

- 고정 CLI source에서 fresh volume은 PostgreSQL health 이후 `runFreshDbSetup`을 실행하고, `[auth].enabled`, `[storage].enabled`, `[realtime].enabled`가 setup 경로를 제어함을 확인했다. 생성 TOML은 `[api].enabled=false`만 명시해 나머지 기본값과의 설정 불일치가 있다. 이는 확인된 정적 계약 차이지만, 보존된 `unclassified` 출력만으로 최근 세 번의 `start` 실패 원인이라고 단정하지 않는다.
- 제품 runner `supabase/ci/run.sh`는 표준 Supabase CLI를 사용하며 host-port adapter를 거치지 않는다. 제품 replay의 Auth bootstrap은 유지하고 이번 변경에서 제품 `run.sh`나 원본 migration/config/tests는 수정하지 않는다.
- CEO 승인 범위는 빈 host-port spike 전용이다. `supabase/ci/host_port_spike.py`의 임시 설정에서 Auth/Storage/Realtime를 각각 명시적으로 비활성화하고, preflight가 TOML 계약을 검증해 값 누락·변조 시 CLI/Docker 호출 전에 거절한다. 기존 API 비활성, PostgreSQL 17/image pin, DB/shadow port, migrations 활성, seed 비활성 및 빈 migrations 디렉터리는 보존한다.
- 승인된 순서: 설정 계약 회귀 테스트 작성→실패 확인→최소 구현→관련 Python test 및 `bash -n`/`git diff --check` 등 필요한 정적 검사. 생성된 spike의 런타임 실행, 이미지 pull, DB reset, commit은 승인 범위 밖이다.
- spike는 제품 migration이나 SQL suite가 없는 host-binding 확인용이므로 Auth 비활성화가 제품 runner로 전파되지 않아야 한다. 정적 검증만으로 이전 실패 원인, loopback binding 성공, 전체 31 migration 및 SQL suite 수용을 주장하지 않는다.

## 2026-10-05 승인된 start-only 모드

- 목표는 재시도 전에 disposable host-port spike가 `supabase start` 한 번만 수행하고 reset/replay는 호출하지 않게 하는 것이다. 이 변경도 실제 Docker mutation을 수행하므로 코드/가짜 검증만 진행하고 별도 사용자 승인 전 실행하지 않는다.
- 승인 인터페이스: CLI `--start-only` 및 `run_spike(..., start_only=False)`. flag 생략 시 기존 start→binding/ownership check→reset→binding/ownership check→공통 cleanup 순서를 보존한다.
- start-only는 기존 start 성공 후 actual HostIp, network attachment, container/volume ownership을 확인한 뒤 reset 호출만 건너뛴다. 조기 return은 금지하며 cleanup/error 처리와 handler 복원은 기존 공통 경로를 따른다. artifact에 `spike_mode=start_only` 또는 `spike_mode=start_reset`을 기록한다.
- TDD: CLI/function forwarding 및 start-only 성공·실패 시나리오를 먼저 추가해 실패를 확인한다. start-only 성공은 `start` 1회·reset/replay 0회·정확한 owned cleanup을 확인하고, start 실패는 후속 호출 없이 cleanup을 확인한다. cleanup 자체가 실패하면 모드는 성공으로 끝나지 않아야 한다. default 경로의 기존 start+reset 테스트도 계속 통과해야 한다.
- scope는 `supabase/ci/host_port_spike.py`, `supabase/ci/test_host_port_spike.py`와 이 기록만 포함한다. 제품 `run.sh`, 원본 migration/config/tests는 수정하지 않는다. 런타임 실행, 이미지 pull, DB reset/replay, commit은 승인되지 않았다.
- 코드/가짜 검증이 끝난 뒤 제안할 단일 런타임 호출은 pinned CLI `2.119.0`, 캐시된 PostgreSQL `17.6.1.171`, 새 빈 UUID 프로젝트, ports `127.0.0.1:56432` 및 shadow `56430`, Auth/Storage/Realtime 비활성 설정을 사용한다. start 1회 후 실제 binding을 검사하고 exact owned container/volume/network/workdir만 cleanup한다. 실패 시 1회에서 멈추며 reset/replay와 pull은 하지 않는다. artifact path `/private/tmp/token-planet-ci-hostip-start-only-20261005-01`는 실행 직전 부재 또는 빈 상태여야 한다.
- 구현 TDD는 focused RED에서 `--start-only` 거부/API 부재 등 기대한 assertion 실패 6건, GREEN에서 동일 6건 통과로 확인했다. 독립 fake-only QA에서 spike 34건 및 전체 `supabase/ci` 97건 통과, `git diff --check` 통과를 확인했다. 이 검증은 실제 Supabase CLI/Docker/runtime 동작이나 이전 `start` 실패 원인을 입증하지 않는다.

## 2026-10-05 승인된 start-only 런타임 결과

- 사용자 승인에 따른 단 1회 실행이 exit 0으로 끝났다. artifact `/private/tmp/token-planet-ci-hostip-start-only-20261005-01/spike.log`는 `spike_mode=start_only`, `network_status=PASS binding=127.0.0.1`, `phase=start status=PASS`, container/volume/network/workdir cleanup 모두 PASS, `spike_status=PASS`를 기록한다.
- 이번 실행에서 loopback start와 exact owned resource cleanup 성공을 확인했다. 이전 `start` 실패 원인은 미확인이고, 전체 migration 및 SQL replay는 미실행이다.

## 2026-10-05 CI runner Docker guard 통합 설계

- 목표: 제품 CI runner의 표준 Supabase CLI `start`/`db reset` 흐름에서 Auth/Storage/Realtime DB bootstrap을 보존하면서, 소유 Postgres publish를 loopback으로 제한하고 모든 pull 및 foreign Docker mutation을 차단한다. 외부 migration/config/SQL test 입력은 변경하지 않는다.
- Workspace는 사용자가 선택한 `/Users/yunho/Desktop/project/token-planet` 현재 `master` checkout이다. 기존 dirty 변경을 보존하며 새 worktree나 commit은 만들지 않는다.
- 승인 경계: 허용 파일은 `supabase/ci/run.sh`, 관련 CI Python guard/helper와 fake tests, 이 계획 문서다. Supabase CLI/Docker/runtime, network mutation, image pull, DB reset/replay, commit은 구현 검증 단계에서 실행하지 않는다. full replay는 별도 사용자 승인 전 미실행이다.
- 설계: 새 `supabase/ci/product_docker_guard.py`를 start/reset CLI 자식의 임시 PATH shim으로만 연결한다. CLI 실행 전에 real Docker의 절대 경로를 고정한다. runner가 직접 수행하는 loopback bridge 생성, 실제 binding/ownership 검사, exact cleanup은 기존 unshimmed 경로를 유지한다. 기존 `host_port_adapter.py`에서는 순수 `adapt_docker_argv()` DB create 변환만 재사용하고 spike의 기존 `main()` 정책은 바꾸지 않는다.
- DB create는 generated project/container/workdir/network/image/mount/labels 및 56432:5432 publish 계약을 모두 검사한 경우에만 `127.0.0.1:56432:5432`로 바꾸고 `--pull=never`를 추가한다. DB volume create/remove 및 container start/remove는 해당 run의 정확한 generated 이름/ID와 Supabase project/workdir labels를 inspect해 소유권을 확인한다.
- pinned CLI 2.119.0 fresh-volume setup은 `--exclude`와 독립적으로 Realtime→Storage→Auth 순서로 실행한다. Helper argv는 정확히 `realtime eval`의 `realtime-dev` tenant health expression, `node dist/scripts/migrate-call.js`, `gotrue migrate`이다. 각 helper의 exact image candidate, key-only environment key set, owned network, 두 project labels, `--rm`, mount/port 부재를 검증하고 `--pull=never` 및 unique `--cidfile`을 추가한다. helper cleanup은 cidfile의 단일 container ID를 inspect해 exact project/compose labels, allowed image, owned network, no-publish/no-mount를 확인한 후 그 ID만 제거한다. credentials와 원문 argv는 artifact에 기록하지 않는다.
- Helper image는 CLI `v2.119.0`의 공식 `Dockerfile` pinned tag와 default registry resolver로 제한한다: Auth `gotrue:v2.197.0`, Realtime `realtime:v2.140.3`, Storage `storage-api:v1.79.28`; 각각 public.ecr.aws mirror, ghcr.io, Docker Hub의 정확한 3개 후보만 허용한다. Runner는 fresh workdir에 helper pin이나 `.env`를 쓰지 않고 외부 nonempty `SUPABASE_*` override를 거절한다. Cache miss 때 CLI의 `docker pull`은 guard가 거절하며, 실제 pull로 이어지지 않는다.
- Reset PG15+는 named DB container와 volume을 제거한 뒤 Postgres를 재생성하고 fresh setup을 다시 실행한다. reset의 remove/create는 exact identity 검사와 기존 post-reset actual HostIp/network/ownership 검사를 유지한다. CLI start rollback의 `dockerRemoveAll`은 project label로 다중 container를 stop한 다음 container/volume/network prune을 수행하지만, prune은 exact ownership을 보장하지 않으므로 제품 guard가 거절한다. pinned `rollbackStart`는 rollback 실패를 stderr에 남기고 원래 start 실패를 보존하며, runner의 기존 exact inspect→remove cleanup이 최종 정리를 담당한다. 미분류 command, Docker global option/alias, pull, prune, network mutation, 임의 run/create는 fail closed한다.
- 공식 pinned source 근거: [db-setup.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/db-setup.ts), [realtime-env.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/realtime-env.ts), [docker-run.args.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/docker-run.args.ts), [Dockerfile](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/shared/services/Dockerfile), [docker-registry.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/docker-registry.ts), [recreate-local-database.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/recreate-local-database.ts), [container-lifecycle.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/container-lifecycle.ts), [docker-remove-all.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/docker-remove-all.ts), [rollback.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/rollback.ts).

### 추가로 확인한 literal Docker 계약

- Postgres `create`는 pinned builder의 고정 순서로 구성된다: `--name`, key-only env(`POSTGRES_PASSWORD`, `POSTGRES_HOST`, `JWT_SECRET`, `JWT_EXP`; `POSTGRES_HOST` 값은 literal `/var/run/postgresql`), `supabase_db_<project>:/var/lib/postgresql/data`, 단일 `56432:5432` publish, healthcheck (`pg_isready -U postgres -h 127.0.0.1 -p 5432`, interval 10s, timeout 2s, retries 3), restart `unless-stopped`, Linux의 `host.docker.internal:host-gateway`, owned network, aliases `db`/`db.supabase.internal`, CLI project/compose/workdir labels, entrypoint `sh`, pinned Postgres image, `-c`와 단일 CLI-generated PG15+ init script다. `--pull=never`를 추가하고 유일한 publish를 `127.0.0.1:56432:5432`로 rewrite한다. 생성된 PG17 config에서 tmpfs, volumes-from, exposed ports, security opts는 없다.
- CLI가 Postgres secret file을 전달하는 source-backed operation은 `cp - <created-db-id>:/`이며, 이어서 `start <created-db-id>`를 실행한다. PG17 reset은 `container rm -f supabase_db_<project>`와 `volume rm -f supabase_db_<project>` 후 새 volume/container/cp/start 흐름을 수행한다. Volume create는 `volume create --label com.supabase.cli.project=<project> --label com.docker.compose.project=<project> supabase_db_<project>`다. Runner가 bridge를 CLI 밖에서 생성하므로 guarded CLI는 그 network ID inspect만 필요하다. Guard는 create 결과 ID를 기록하고 동일 ID에만 cp/start를 허용한다. reset rm은 exact generated DB name 또는 recorded ID를 받아 소유성 검증 후 immutable recorded ID로만 제거한다.
- Helper env key sets는 Realtime `PORT DB_HOST DB_PORT DB_USER DB_PASSWORD DB_NAME DB_AFTER_CONNECT_QUERY DB_ENC_KEY API_JWT_SECRET API_JWT_JWKS METRICS_JWT_SECRET APP_NAME SECRET_KEY_BASE ERL_AFLAGS DNS_NODES RLIMIT_NOFILE SEED_SELF_HOST RUN_JANITOR MAX_HEADER_LENGTH`, Storage `DB_INSTALL_ROLES DB_MIGRATIONS_FREEZE_AT ANON_KEY SERVICE_KEY PGRST_JWT_SECRET DATABASE_URL FILE_SIZE_LIMIT STORAGE_BACKEND STORAGE_FILE_BACKEND_PATH TENANT_ID REGION GLOBAL_S3_BUCKET`, Auth `API_EXTERNAL_URL GOTRUE_LOG_LEVEL GOTRUE_DB_DRIVER GOTRUE_DB_DATABASE_URL GOTRUE_SITE_URL GOTRUE_JWT_SECRET`다. 공식 spelling은 `SEED_SELF_HOST`다. Storage/Auth DB URLs target generated DB container on port 5432/database `postgres`, using roles `supabase_storage_admin`/`supabase_auth_admin`.
- CLI container-state inspection is exactly `container inspect <db-name-or-id> --format '{{json .State}}'`; network inspection is `network inspect <owned-network-id>` and volume inspection is `volume inspect <db-volume-name>`. These read-only calls accept no extra fields/formats/targets. Source: [docker-lifecycle.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/docker-lifecycle.ts).
- Literal command sources: [docker-create-args.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/docker-create-args.ts), [postgres.service.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/postgres.service.ts), [internal-db-connection.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/internal-db-connection.ts), and [container-lifecycle.ts](https://raw.githubusercontent.com/supabase/cli/v2.119.0/apps/cli/src/command-internal/db-bootstrap/container-lifecycle.ts).

- Reset의 `restartSatelliteServices()`는 `restart supabase_<storage|auth|realtime|pooler>_<project>`를 concurrent 실행하고, nonzero stderr의 `no such container|no such object|no container with name or id`만 허용된 부재로 처리한다. Runner는 이 서비스를 전부 exclude하므로 guard는 reset phase의 exact generated 이름만 read-only state inspect하고 정확한 not-found 응답을 확인한 경우 tolerated not-found를 반환한다. 실제 `restart`는 forwarding하지 않으며 기존 container 또는 inspect 오류는 fail closed한다. Concurrent probe는 guard state를 변경하지 않는다.
- 이어지는 Kong 계약은 `container inspect supabase_kong_<project> --format '{{json .State}}'`뿐이다. 부재/비실행은 CLI no-op이며, 실행 중이면 CLI가 요청하는 `exec <kong-name> kong reload --nginx-conf /home/kong/custom_nginx.template`를 guard가 거절한다. 다른 service mutation/target/alias/option은 허용하지 않는다. 공식 pinned 근거: [restart-services.ts](https://github.com/supabase/cli/blob/v2.119.0/apps/cli/src/command-internal/db-bootstrap/restart-services.ts), [container-cli.ts](https://github.com/supabase/cli/blob/v2.119.0/apps/cli/src/command-internal/container-cli.ts), [docker-lifecycle.ts](https://github.com/supabase/cli/blob/v2.119.0/apps/cli/src/command-internal/docker-lifecycle.ts), [docker-ids.ts](https://github.com/supabase/cli/blob/v2.119.0/apps/cli/src/command-internal/docker-ids.ts).

### 승인된 구현 계획

1. **Guard 계약 TDD:** 새 product guard 테스트에 DB create loopback/cache-only 변환, 각 3 helper의 exact images/argv/env/network/labels 및 registry candidate를 fixture로 추가한다. explicit pull, arbitrary image/command/option, `run` alias, prune, network mutation, 외부 target, Docker global overrides는 fake real Docker에 도달하지 않는 RED 테스트를 먼저 확인한다. 그 뒤 source contract만 허용하는 strict dispatcher를 구현한다.
2. **Helper ownership TDD:** unique cidfile 생성, 정상 `--rm` 후 not-found 처리, 잔존 helper의 exact inspect/remove, 손상 cidfile·inspect 실패·foreign ID·잘못된 labels/image/network/mount/ports 거절을 테스트한다. 다중 대상 검증은 전부 성공하기 전 mutation을 0회 수행한다.
3. **Runner integration TDD:** `test_run.py`의 fake CLI가 start/reset 경로에서 docker shim을 통해 Postgres create와 helper jobs를 실행하게 한다. 두 phase의 argv/env/order, pull/prune 거절, reset container 교체, start/reset partial failure, 최초 exit 보존, 다음 단계 중단, helper→container→volume→network exact cleanup을 확인한다. 실제 Supabase CLI/Docker는 호출하지 않는다.
4. **Verification:** focused guard/run tests와 CI 전체 Python suite, `bash -n supabase/ci/run.sh`, `git diff --check`, migration/config/tests byte/hash 불변을 확인한다. QA 뒤 독립 Reviewer가 최종 전체 diff와 foreign-resource 경계를 검토한다. 실제 replay 통과, helper image cache 존재, live cleanup은 주장하지 않는다.

- 위험/한계: unknown CLI Docker call은 허용하지 않아 fake/source coverage가 빠뜨린 command가 있으면 disposable run은 fail closed할 수 있다. Cache miss는 의도적으로 pull 없이 실패한다. CLI rollback prune를 거부한 뒤 실패 시 helper는 `--rm`, inline cidfile cleanup 및 runner EXIT의 기록 helper 재검증/재시도에 의존한다. EXIT은 phase/helper/image ledger와 cidfile을 검증하고 모든 pending helper의 소유성을 확인한 뒤 exact ID만 DB/volume/network보다 먼저 제거한다. helper recovery 실패 시 원래 CLI exit을 보존하고 cleanup 실패를 보고하며 workdir의 ledger/cidfile 증거를 남긴다. 정적 검증은 실제 DB bootstrap, loopback binding 또는 전체 replay를 증명하지 않는다.
- 자체 검토: plan scope와 task ownership은 위 허용 CI files에 한정했다. Auth DB bootstrap flag/command를 수정하지 않으며, broad prune나 passthrough allow rule을 사용하지 않는다. start/reset migration 결과는 향후 별도 승인 runtime이 필요하다.

## 2026-10-05 실패 진단 안전 증거 보강

- 목표: CLI start 실패 원인을 다음 승인된 replay에서 더 좁힐 수 있도록, product Docker guard의 고정 거절 코드와 소유권이 확인된 DB 상태 요약만 artifact에 남긴다. 이번 변경은 CI fake/static 검증만 수행하며 CLI/Docker/runtime, pull, reset/replay는 실행하지 않는다.
- Workspace는 사용자가 지정한 `/Users/yunho/Desktop/project/token-planet`의 `master` 주 checkout이다. 기존 dirty 변경과 미추적 파일을 보존하며 새 worktree를 만들지 않는다.
- 허용 파일은 `supabase/ci/run.sh`, `supabase/ci/product_docker_guard.py`, `supabase/ci/test_run.py`, `supabase/ci/test_product_docker_guard.py`, 이 계획 문서다. 원본 migration/config/SQL tests, spike, adapter 변경은 범위 밖이다.
- Guard의 거절은 선언된 고정 code enum으로 표현한다. 검증된 temp workdir의 private sidecar에는 `phase`와 `code`만 기록하고, runner는 schema/enum 검증 후 정규화된 `diagnostics.jsonl`로 내보낸다. CLI raw stdout/stderr를 넓게 허용하지 않는다. Sidecar sink나 schema가 안전하게 확인되지 않으면 원문 없이 진단 증거 부재/무효 상태만 기록하며 원래 CLI exit를 보존한다.
- DB ownership 거절은 판정 predicate를 바꾸지 않고 고정 code로만 세분화한다: `DB_INSPECT_IDENTITY_REJECTED`, `DB_INSPECT_IMAGE_REJECTED`, `DB_INSPECT_LABELS_REJECTED`, `DB_INSPECT_NETWORK_MODE_REJECTED`, `DB_INSPECT_NETWORK_ATTACHMENT_COUNT_REJECTED`, `DB_INSPECT_NETWORK_ATTACHMENT_ID_REJECTED`, `DB_INSPECT_NETWORK_ATTACHMENT_INVALID`, `DB_INSPECT_PUBLISH_REJECTED`, `DB_INSPECT_VOLUME_MOUNT_REJECTED`, `DB_INSPECT_INVALID`, `DB_INSPECT_COMMAND_FAILED`. Artifact에는 검사한 실제 metadata나 값은 남기지 않는다.
- `supabase start` 실패 직후 runner는 recorded DB ID를 대상으로 read-only inspect한다. 정확한 generated 이름/ID, Supabase project/compose/workdir labels, pinned Postgres image, owned network 및 volume mount를 확인한 경우에만 `State.Status`, `State.Health.Status`, 정수 `State.ExitCode`를 enum/타입 검사 후 남긴다. binding 결과나 health 성공은 ownership의 전제가 아니다. 정확한 not-found, inspect 실패, 잘못된 응답, ownership 불일치는 고정 `lookup` code만 남긴다. 원문 inspect JSON, argv/env, stdout/stderr, container/health logs, `State.Error`, IDs, credentials는 artifact에 기록하지 않는다.
- Inspect는 2초 timeout과 stdout/stderr 합계 65,536 byte hard limit을 사용한다. timeout·초과 출력·malformed 응답은 고정 실패 분류로 귀결하고 cleanup은 계속한다. 출력 제한은 전체 결과를 무제한 메모리에 받은 뒤 길이만 검사하는 방식으로 구현하지 않는다. 진단 프로세스나 artifact sink 실패는 원래 start exit와 cleanup을 보존하며 원시 오류를 출력하지 않는다. Artifact sink 자체가 쓸 수 없을 때 진단 artifact가 빠질 수 있다는 점은 acceptance 한계로 기록한다.
- TDD 순서: (1) 고정 guard code/sidecar schema 및 sink 오류·손상 회귀, (2) owned/unavailable/foreign/malformed DB inspect 결과와 metadata allowlist, (3) start exit 유지·후속 reset 차단·exact cleanup 및 artifact 비밀 sentinel 회귀를 먼저 RED로 확인하고 구현 후 GREEN으로 바꾼다. 이어 전체 `supabase/ci` offline suite, Python/Bash syntax 및 `git diff --check`를 수행한다. QA가 승인된 offline/static 범위를 독립 실행하고, Reviewer가 QA 후 전체 diff의 데이터 노출/ownership 경계를 독립 검토한다.
- 수용 기준: artifact에는 고정 code와 `lookup/state/health/exit_code` allowlist만 포함한다. Guard/inspect 원문과 credentials가 남지 않고, ownership이 검증되지 않은 container 상태는 출력되지 않는다. 실패 진단은 원래 CLI exit·정확한 cleanup을 바꾸지 않으며 migration/replay 성공이나 이전 runtime 실패 원인을 확정했다고 주장하지 않는다.
- 독립 QA/Reviewer disposition: `host_port_spike.py`의 기존 실패 진단은 이번 addendum의 승인 파일 범위 밖이며 그대로 보존한다. 별도 경로에는 무제한 inspect subprocess, network/volume 미검증 상태 요약, generated container ID artifact가 남아 있어 별도 hardening 작업으로 다룬다. 이는 제품 runner의 승인된 guard 진단 수용 결과에 포함하지 않는다.
- 2026-10-05 승인된 replay 결과: `bash supabase/ci/run.sh --artifacts-dir /private/tmp/token-planet-ci-full-replay-20261005-02`는 `supabase start`에서 실패했다. [diagnostics artifact](/private/tmp/token-planet-ci-full-replay-20261005-02/diagnostics.jsonl)는 `DATABASE_CONTAINER_OWNERSHIP_COULD_NOT_BE_VERIFIED`, `DOCKER_COMMAND_IS_NOT_APPROVED`, `lookup=ownership_unverified`만 기록해 구체적인 metadata 불일치는 확인되지 않았다. loopback/network·port preflight, Docker engine, cached pinned Postgres image 검사는 통과했고 cleanup은 helper→container→volume→network 순서로 성공했다. `db reset`, migrations, SQL tests에는 도달하지 않았다. 이후 추가한 세부 ownership code는 offline QA/Reviewer까지 통과했으며 이번 실행에는 아직 사용되지 않았다.
- 2026-10-05 추가 승인 replay (`...-03`, `...-04`)도 `supabase start`에서 실패했다. [03 artifact](/private/tmp/token-planet-ci-full-replay-20261005-03/diagnostics.jsonl)는 `DB_INSPECT_NETWORK_REJECTED`, `DOCKER_COMMAND_IS_NOT_APPROVED`, `lookup=ownership_unverified`를, [04 artifact](/private/tmp/token-planet-ci-full-replay-20261005-04/diagnostics.jsonl)는 `DB_INSPECT_NETWORK_ATTACHMENT_REJECTED`, `DOCKER_COMMAND_IS_NOT_APPROVED`, `lookup=ownership_unverified`를 기록했다. 두 번 모두 cleanup은 helper→container→volume→network 순서로 성공했고 reset/migrations/SQL tests에는 도달하지 않았다.
- 이후 network mode, attachment 구조, attachment 개수, 단일 ID 불일치를 고정 코드로 세분화했다. 판정은 여전히 정확히 하나의 generated network ID attachment를 요구하며 raw metadata는 기록하지 않는다. malformed/count/ID fixture, positive exact-contract fixture를 포함한 offline suite 160건·독립 QA·Reviewer가 통과했다. 사용자는 개발용 disposable DB를 백업 없이 초기화해도 된다고 승인했다. 다음 replay는 생성된 임시 volume으로 진행한다.

## 2026-10-05 DB 시작 전 네트워크 설정과 시작 후 연결 검증

**목표:** 생성 직후에는 소유 network 설정을 검증하고, Docker start 성공 직후에는 실제 attachment ID를 엄격히 확인해 disposable DB 전체 CI replay를 진행한다.

**근거:** `...-05` replay는 `supabase start`에서 `DB_INSPECT_NETWORK_ATTACHMENT_ID_REJECTED`로 실패했고 reset/migrations/SQL tests에는 도달하지 않았다. Moby는 생성 시 HostConfig의 network를 이름으로 정규화해 빈 endpoint 설정을 만들고, `allocateNetwork`/`connectToNetwork`에서 시작 시 NetworkID를 기록한다 ([생성 설정](https://github.com/moby/moby/blob/master/daemon/container_operations.go#L2804-L2862), [시작 연결](https://github.com/moby/moby/blob/master/daemon/container_operations.go#L2895-L2908), [attachment ID 기록](https://github.com/moby/moby/blob/master/daemon/container_operations.go#L3403-L3406)). 따라서 생성 전후 검사를 분리하되 소유권 확인은 유지한다.

**범위:** Coder가 아래 파일만 수정한다: `supabase/ci/run.sh`, `supabase/ci/product_docker_guard.py`, `supabase/ci/test_run.py`, `supabase/ci/test_product_docker_guard.py`. 기존 dirty 변경, 원본 migration/config/SQL tests, `host_port_spike.py`, adapter 및 비임시 DB는 범위 밖이다. 새 Docker mutation, network repair, pull, persistent/hosted DB 접근을 추가하지 않는다.

**계약:**

- Runner의 start/reset/cleanup/diagnostic 네 guard 호출마다 `TOKEN_PLANET_GUARD_NETWORK_NAME="$NETWORK_NAME"`을 전달한다. Guard는 network name을 필수로 받고 `token-planet-ci-net-<project의 동일 24자리 hex suffix>` 형식과 기존 generated full network ID를 함께 검증한다.
- 인터페이스는 `verify_db_ownership(context, state, *, pre_start=False) -> None`으로 둔다. 기본은 strict 검증이다.
- `pre_start=True`는 `cp`와 `start` 직전에만 명시적으로 쓴다. `State.Status == "created"`, `State.Running is False`, 정확한 `HostConfig.NetworkMode == context["network"]`, 단일 `NetworkSettings.Networks` entry의 key가 `context["network_name"]`인 조건을 확인한다. entry는 mapping이고 `NetworkID`는 명시된 문자열 `""` 또는 정확한 owned full ID여야 한다. identity/image/labels/mount/publish 검사는 전부 그대로 유지한다.
- Start 명령이 0으로 끝난 직후 기본 strict 검증을 실행한다. 단일 well-formed attachment의 key가 생성된 network name이고 `NetworkID`가 exact owned ID인지 확인한 뒤에만 `state["started"][phase] = True`를 저장한다. Start 실패는 기존 exit를 보존하고 성공 bookkeeping은 하지 않는다.
- Reset DB 제거와 start 진단은 기본 strict 계약을 유지한다. strict 검증은 하나의 key가 generated network name과 일치하는 attachment만 허용한다. pre-start 허용은 ledger나 inspect 내용에서 추론하지 않는다. 실패 시 추가 Docker mutation이나 metadata 원문 출력을 하지 않는다. 새 거절 사유가 필요하면 고정 diagnostic code 및 allowlist를 함께 갱신한다.

### Task: 네트워크 수명주기 계약 TDD

**인터페이스:** 기존 generated name/ID, guard ledger, Docker argv allowlist를 소비한다. 산출물은 모든 guard 호출에서 name 전파, 명시적 pre-start 검사, start 직후 strict 검사와 start/reset 회귀다.

- [ ] Fake Docker가 create 직후 network 이름 key와 빈 `NetworkID`, `State.Status="created"`, `Running=false`를 반환하고 start 성공 시 exact ID attachment로 전환하도록 fixture를 확장한다.
- [ ] `test_prestart_network_accepts_owned_name_and_empty_or_exact_id`: 두 허용 ID 값 각각으로 cp/start가 성공하고, start 후 strict inspect가 ledger 성공 기록보다 먼저 실행되는지 확인한다.
- [ ] `test_prestart_network_rejects_invalid_configuration_before_mutation`: wrong mode/key/ID, missing/null/non-string ID, extra network, malformed record, non-created state, `Running=true`마다 해당 cp/start mutation이 없음을 확인한다.
- [ ] `test_prestart_network_context_requires_generated_name`: name 누락, 공백, 다른 project suffix를 거절하고 Docker mutation이 없음을 확인한다.
- [ ] `test_prestart_network_retains_all_common_ownership_checks`: ID/name/image/labels/mount/publish 거절이 cp/start mutation을 막는지 확인한다.
- [ ] `test_prestart_network_poststart_requires_exact_attachment_before_ledger`: start가 성공해도 후속 inspect의 빈/누락/foreign/추가/잘못된 key·attachment 또는 inspect 오류는 실패하고 `started[phase]`가 false인지 확인한다.
- [ ] `test_prestart_network_failed_start_preserves_failure`: Docker start 실패 exit와 실패 bookkeeping을 확인한다.
- [ ] `test_prestart_network_reset_removal_remains_strict` 및 `test_prestart_network_diagnostic_remains_strict`: reset 제거/진단이 empty, foreign ID 또는 잘못된 network-name key를 거절하고 상태 metadata를 노출하지 않는지 확인한다.
- [ ] `test_prestart_network_runner_propagates_name_in_all_guard_paths`: start/reset/cleanup/diagnostic의 generated name 전달과 양 DB 생성 주기의 create→cp→start→strict inspect 순서를 확인한다.
- [ ] RED를 확인한다: `python3 -m unittest discover -s supabase/ci -p 'test_product_docker_guard.py' -k pre_start -v` 및 `python3 -m unittest discover -s supabase/ci -p 'test_run.py' -k network_name -v`. 이 선택자는 실제 fixture의 `test_pre_start_...` 및 `test_guard_network_name_...` 이름을 대상으로 한다. 환경/fixture 오류는 유효한 RED가 아니다.
- [ ] 최소 구현 후 같은 두 focused 명령을 실행하고, 전체 offline 회귀 `python3 -m unittest discover -s supabase/ci -p 'test_*.py' -v`, `bash -n supabase/ci/run.sh`, `git diff --check`를 통과시킨다.

### 독립 검증과 수용

- [ ] QA가 최종 변경 상태에서 focused 테스트, 전체 offline suite, shell/diff 검사를 독립 실행하고 start/reset fail-closed 경계를 확인한다.
- [ ] QA가 승인된 disposable DB 권한으로 다음 전체 재생을 실행한다: `bash /Users/yunho/Desktop/project/token-planet/supabase/ci/run.sh --artifacts-dir /private/tmp/token-planet-ci-full-replay-20261005-06`. Artifact 디렉터리는 비어 있어야 한다.
- [ ] 전체 start/reset, 원본 순서의 migration 재생, synthetic history 대조, SQL suite 결과 및 정확한 생성 리소스 cleanup을 기록한다. 원본 migration/config/SQL test 입력의 이전 hash와 비교한다. offline GREEN을 실제 replay 성공으로 취급하지 않는다.
- [ ] QA 뒤 독립 Reviewer가 최종 전체 diff에서 name/ID 연결, explicit pre-start 선택, strict post-start 검사와 bookkeeping 순서, reset 제거, 진단 정보 제한, foreign-resource mutation 차단을 확인한다. 수정사항이 생기면 해당 QA와 최종 Reviewer 범위를 갱신한다.
- [ ] CEO는 모든 수용 기준과 finding이 해결된 뒤 완료를 판단한다. Commit/push는 요청되지 않았다.

**위험과 한계:** pre-start 허용은 명시적으로 지정된 단계에만 적용하며 reset 제거와 진단은 strict다. 검사와 mutation 사이의 기존 경쟁 조건은 남고, start 직후 attachment 소유권은 DB health 성공을 보장하지 않는다. 실제 daemon/cache/CLI 차이가 있으면 고정 진단과 미충족 수용 기준을 보고한다.

## 2026-10-05 Helper 부재 응답 호환성 보강

- Attempt-07은 `HELPER_OWNERSHIP_INSPECTION_FAILED`→`HELPER_CLEANUP_COULD_NOT_BE_VERIFIED`로 실패했다. 기록된 realtime helper ID의 read-only inspect 결과는 exit 1, stdout hex `0a`(`b"\n"`), stderr `Error response from daemon: No such container: <recorded-id>\n`이었다. 기존 guard는 stdout이 빈 `No such object` 응답만 부재로 인정해 이 응답을 거절했다. Runner는 DB/volume/network를 정리했으나 helper cleanup을 검증하지 못해 ledger와 workdir를 보존했다.
- 최소 수정은 관측한 exit/stdout/stderr의 정확한 tuple만 부재 응답으로 추가하는 것이다. 기존 tuple과 존재하는 helper의 전체 ownership 검증을 유지한다. 다른 ID, 추가 출력, 다른 exit 등은 거절하며, 부재가 확인된 helper는 remove 없이 기존 cleaned 처리 경로를 사용한다.
- Reviewer가 helper network 목록에서 잘못된 항목을 건너뛰는 P2 결함도 발견했다. 수정은 모든 항목이 mapping이며 `NetworkID`가 문자열인지 확인한 뒤 단일 owned attachment를 허용한다. `owned + null` inline/recovery fixture는 수정 전 RED, 수정 뒤 exit 125·no removal·pending ledger 보존을 확인했다.
- 최종 독립 QA: focused guard 59건, 전체 offline suite 172건, Bash 문법, tracked diff 및 untracked guard/test 공백 검사가 통과했다. Reviewer는 두 ownership 경계와 계획 선택자를 재확인했고 최종 변경에서 추가 finding이 없다고 보고했다. Offline 결과는 runtime 성공을 증명하지 않는다.
- 사용자의 직접 승인 후 Attempt-08은 DB start, loopback binding, 31개 migration staging/검증까지 통과했지만 `db reset`에서 exit 1로 실패했다. `reset.log`의 19개 unclassified 줄은 민감정보 보호를 위해 모두 가려져 원인을 구분할 수 없었고, SQLSTATE·migration 진행 위치·reset-phase guard code artifact가 없다. Raw reset log와 workdir는 runner cleanup에서 삭제됐다.
- Runner cleanup은 helper recovery, generated DB container, volume, network 제거를 PASS로 기록했다. 추가 read-only 확인에서 Attempt-07의 정확한 DB/helper container ID, volume, network와 Attempt-08의 생성 리소스가 모두 부재함을 확인했다. Attempt-07의 보존 workdir는 제거했고 sanitized artifacts는 남겼다.
- 아직 전체 migration replay, synthetic history 대조, SQL suite 및 입력 hash 비교가 완료되지 않았다. 다음 disposable replay 전에 아래 reset 진단을 구현·QA·Reviewer 검토하고, 그 정확한 새 시도에 대해 사용자의 직접 승인을 받는다. 미도달 단계나 증거 공백이 있으면 replay 완료를 선언하지 않는다.

**자체 검토:** stdout을 추정하지 않고 관측된 `0a`로 고정했다. 원인·수정·Coder 결과·독립 runtime 수용을 구분했으며 기존 DB 권한이나 mutation 범위를 넓히지 않았다.

## 2026-10-05 Reset 실패 진단 보강

**목표:** 다음 직접 승인된 replay에서 reset 단계의 guard 거절과 명시적 SQL 오류 관측을 구분하면서 원래 실패 exit와 cleanup을 보존한다.

**범위:** `supabase/ci/run.sh`, `supabase/ci/test_run.py`만 수정한다. 원문 필터를 넓히거나 Docker mutation을 추가하지 않는다.

**계약:** Reset 실패 직후 `reset_status`를 먼저 저장하고 cleanup 전에 `collect_reset_diagnostics`를 호출한다. 진단 파일은 `reset-diagnostics.json`으로 두고 다음 정보만 허용한다.

- `guard_codes`: 검증된 private sidecar의 reset phase 고정 enum code만 허용한다.
- `last_announced_migration`: 정확한 `Applying migration <staged_filename>...` 줄이며 staging manifest의 `staged_filename` 집합에 있을 때만 기록한다. 이는 시도 공지이지 적용 성공/실패의 단정이 아니다.
- `sqlstate`: 전체 줄이 정확한 `ERROR: <discarded message> (SQLSTATE XXXXX)` 형식과 일치할 때만 5자리 대문자 영숫자를 기록한다. 실제 CLI 출력 형식이 확인되지 않았으므로 다른 형식은 추정하지 않는다. 상충하는 코드가 여러 개면 `null`로 둔다.
- `diagnostic_status`: `ok`, `unavailable`, `invalid`, `truncated`, `ambiguous` 중 하나.
- `error_class`: `unknown`, `guard_rejection_observed`, `sql_error_observed`, `guard_and_sql_error_observed` 중 하나. 증거가 없는 값은 빈 배열 또는 `null`로 둔다.

Raw reset 입력은 소유자·regular file·symlink 여부를 확인하고 최대 65,536 bytes, 한 줄 4,096 bytes로 제한한다. Migration manifest도 private regular file, 현재 사용자 소유, mode 0600, 단일 link, `O_NOFOLLOW | O_NONBLOCK`으로 열고 최대 1 MiB까지만 읽는다. Credential/JWT 표식이 있는 줄은 추출하지 않는다. SQL text, raw error, IDs, credentials, 미분류 문자열, traceback은 기록하지 않는다. 진단·sanitizer·artifact sink 실패는 격리하며 원래 reset exit와 cleanup을 바꾸지 않는다. 진단 실패 시 raw fallback은 없다.

### TDD와 수용

- [x] RED/GREEN: reset-phase sidecar enum, 정확한 manifest migration 진행 줄, strict SQLSTATE, credential/JWT exclusion, 원문 sentinel 부재를 검증했다.
- [x] RED/GREEN: oversized/corrupt/symlink 입력과 diagnostic/sink 오류에서 reset exit 37, cleanup, history/SQL 차단을 확인했다.
- [x] Reviewer P2 수정: manifest `Path.read_text()`가 FIFO에서 cleanup을 막는 RED를 재현하고, bounded nonblocking private-file reader로 교체했다. FIFO/symlink/oversized/missing 회귀를 추가했다.
- [x] 독립 QA: reset focused 5/5, 전체 offline 176/176, `bash -n`, `git diff --check` 통과.
- [x] 독립 Reviewer: manifest read 경계와 privacy/exit/cleanup을 재검토해 P2 해결 및 추가 finding 없음으로 확인했다.
- [x] 사용자 직접 승인으로 Attempt-09를 한 번 실행했다. CLI/Docker/image/loopback·port preflight 및 31개 migration staging은 통과했지만 `db reset`은 exit 1로 실패했다. `reset-diagnostics.json`은 `diagnostic_status=ok`, `error_class=guard_rejection_observed`, `guard_codes=[EXCLUDED_SERVICE_ABSENCE_COULD_NOT_BE_VERIFIED]`, `last_announced_migration=20000101000031_20261004013811_guest_import_schema1_private_receipt_guard.sql`, `sqlstate=null`을 기록했다. 마지막 공지는 적용 성공을 입증하지 않는다.
- [x] Attempt-09 manifest SHA-256은 `f29caaa0fcdf897e142c199a7545c6fad5ade842db4b0bd73294240309c75b85`이며 현재 source migration 31개 hash 모두와 일치했다. Cleanup log 및 읽기전용 inspect로 exact generated DB container, volume, network와 workdir의 부재를 확인했다. History verification 및 SQL/TAP suite에는 도달하지 않았다.
- [x] Read-only inspect로 excluded storage target의 관측 응답을 확인했다: exit 1, stdout hex `0a`, stderr `Error response from daemon: No such container: <generated-name>\n`. Guard는 기존 `Error: No such object: <target>\n` tuple만 받아 이 차이를 `EXCLUDED_SERVICE_ABSENCE_COULD_NOT_BE_VERIFIED`로 거절했다.
- [x] 최소 guard 수정: generated excluded service 4종에 대해 기존 정확 tuple과 Attempt-09에서 관측한 정확한 Docker tuple `(exit=1, stdout=b"\\n", stderr=Error response from daemon: No such container: <same target>\\n)`만 허용한다. 양 tuple 모두 기존 CLI 호환 응답을 돌려주며 실제 `restart` 호출은 전달하지 않는다.
- [x] TDD: 수정 전 네 target 모두 새 tuple에서 거절됐다. focused test는 두 tuple을 네 target에 적용하고 13개 near-miss, foreign target, existing target, inspect 실패, wrong phase를 거절하며 read-only inspect만 실행함을 확인한다.
- [x] 독립 QA: excluded-service focused 5/5, 전체 offline suite 178/178, `bash -n`, tracked diff 및 untracked guard/test whitespace 검사를 통과했다.
- [x] 독립 Reviewer: 두 exact tuple 외에 허용 범위가 늘지 않고 restart가 전달되지 않음을 확인했다. 추가 finding 없음.
- [x] Attempt-10 직접 승인 후 `/private/tmp/token-planet-ci-full-replay-20261005-10`에서 정확히 한 번 실행했다. Exit 0, 31 migrations, versions `20000101000001`–`20000101000031` 및 history 대조 PASS를 확인했다.
- [x] 40개 SQL pgTAP suite 전부 PASS, 총 1,103 assertions·0 failures. Migration staging verification은 reset 전·후와 SQL tests 후 모두 통과했다.
- [x] Helper, generated DB container/volume/network cleanup PASS. Exact ID/name read-only inspection에서 모든 생성 리소스 부재, temporary workdir 삭제를 확인했다.
- [x] Attempt-10 manifest SHA-256은 `f29caaa0fcdf897e142c199a7545c6fad5ade842db4b0bd73294240309c75b85`; source migration 31개 hash가 모두 일치한다. Runner/guard/prepare/adapter/host-port source hash와 git HEAD도 실행 전후 동일했다.

예정 offline 명령:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s supabase/ci -p 'test_run.py' -k reset
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s supabase/ci -p 'test_*.py'
bash -n supabase/ci/run.sh
git diff --check
```

Attempt-10에서 migration replay, history 대조, SQL suite와 cleanup acceptance가 모두 완료됐다. Sanitized runtime evidence는 `/private/tmp/token-planet-ci-full-replay-20261005-10`에 보존했다. 성공한 reset에는 `reset-diagnostics.json`이 생기지 않았고 원문 reset output은 artifact에 기록하지 않는다. 코드와 문서 변경은 아직 commit하지 않았다.

**자체 검토:** 실제 CLI 출력 형식을 확인했다고 주장하지 않는다. Guard code와 SQLSTATE는 관측 증거이며 둘 다 있으면 rollback 과정의 2차 거절일 수 있어 최초 원인을 단정하지 않는다. 진단 부재도 SQL 성공을 뜻하지 않는다.
