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
