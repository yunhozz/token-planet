# Supabase 운영 배포 Session pooler 인증 변경 구현 계획

> **For agentic workers:** CEO는 Coder → QA → Reviewer 순서로 작업을 조정한다. 작업 단계는 체크박스로 기록한다.

**Goal:** 운영 마이그레이션 workflow가 PAT 없이 프로젝트 DB 비밀번호로 Supabase Session pooler에 연결하도록 변경한다.

**Architecture:** 기존 수동 실행과 승인 보호를 유지한다. workflow의 단일 migration step이 기존 project ref와 DB password로 PostgreSQL URL을 만들고, dry-run 성공 후 같은 URL로 적용한다.

**Tech Stack:** GitHub Actions, Supabase CLI 2.119.0, Python 3 표준 라이브러리, 기존 Python unittest.

**Design approval:** 사용자는 scoped PAT와 `supabase link`를 제거하고 기존 `TOKEN_PLANET_SUPABASE_DB_PASSWORD` 및 Supabase Session pooler의 `--db-url`로 dry-run → push 하도록 승인했다.

**Original spec and plan:**

- `/Users/yunho/Desktop/project/token-planet/docs/superpowers/specs/2026-10-07-token-planet-deployment-pipeline-design.md`
- `/Users/yunho/Desktop/project/token-planet/docs/superpowers/plans/2026-10-07-token-planet-deployment-pipeline.md`

이 delta는 기존 설계의 인증 경로만 대체한다. 운영 자격 증명 경로, URL 인코딩, 로그 마스킹, dry-run 실패 시 중단이 서로 연결되므로 단일 파일·단일 검증으로 끝나는 plan-skip 조건에 해당하지 않는다.

## Global Constraints

- 작업 workspace는 `/Users/yunho/Desktop/project/token-planet`이다.
- project ref는 `TOKEN_PLANET_SUPABASE_PROJECT_REF`, database password는 `TOKEN_PLANET_SUPABASE_DB_PASSWORD`에서 가져온다.
- project Dashboard에서 확인한 연결 정보는 username `postgres.<project-ref>`, host `aws-0-ap-northeast-2.pooler.supabase.com`, port `5432`, database `postgres`, SSL `require`이다.
- URL user/password 구성요소는 RFC 3986 percent encoding을 적용한다. workflow YAML에 credential을 직접 보간하지 않는다.
- encoded password와 완성 URL을 GitHub Actions 로그에서 마스킹한다. URL은 파일, artifact, step output 또는 `$GITHUB_ENV`에 저장하지 않는다.
- PAT secret과 Supabase PAT는 삭제·폐기·변경하지 않는다. workflow에서 사용하지 않도록만 수정한다.
- `workflow_dispatch`, master 제한, `confirm_apply` 기본 false, `supabase-production` Environment, concurrency, permissions, setup-cli pin과 CLI 버전을 유지한다.
- dry-run 실패 뒤에는 적용 명령을 실행하지 않는다. seed, reset, migration history repair와 migration SQL 변경은 포함하지 않는다.
- Coder는 원격 배포를 실행하지 않는다. QA 및 최종 전체 Reviewer가 끝난 후 CEO가 운영 workflow를 재실행한다.
- 새 테스트 파일이나 helper를 만들지 않는다. 기존 `supabase/ci/test_prepare_migrations.py`에 최소 회귀 검증을 추가한다.
- 변경은 production credential과 schema deployment 경로를 다루므로 significant risk로 취급하며 QA 후 독립 Reviewer가 최종 diff 전체를 확인한다.

## Review Focus

1. 예약 문자와 Unicode가 포함된 비밀번호도 올바른 연결 URL로 인코딩되어야 한다.
2. PAT 없이 `--db-url` 경로를 사용하고, 실제 GitHub secret 값은 로그에 노출되지 않아야 한다.
3. dry-run이 실패하면 실제 적용이 호출되지 않아야 한다.
4. 수동 실행, master 제한, Environment 승인과 동시 실행 보호가 유지되어야 한다.

---

### Task 1: 인증 workflow와 운영 안내 변경

**Files — Coder 소유**

- Modify: `.github/workflows/supabase-production-deploy.yml`
- Modify/Test: `supabase/ci/test_prepare_migrations.py`
- Modify: `docs/deployment.md`

**Interfaces**

- Workflow step inputs: `SUPABASE_PROJECT_REF`, `SUPABASE_DB_PASSWORD`.
- PostgreSQL URL: `postgresql://postgres.<project-ref>:<encoded-password>@aws-0-ap-northeast-2.pooler.supabase.com:5432/postgres?sslmode=require`.
- CLI order: `supabase db push --db-url <url> --dry-run`, then `supabase db push --db-url <same-url>`.

- [ ] **Step 1: 기존 테스트 파일에 실패 우선 회귀 테스트 작성**

  기존 unittest 클래스에 `test_production_deploy_uses_encoded_session_pooler_without_pat` 하나를 추가한다. 테스트는 workflow의 실제 inline Python migration block을 추출해 임시 환경에서 실행하고, PATH 앞에 둔 임시 `supabase` executable로 argv와 순서를 기록한다. 별도 suite나 helper는 만들지 않는다.

  가짜 password `p@:/?#%+ 한`에 대해 다음을 검증한다.

  - RFC 3986 인코딩 결과가 `p%40%3A%2F%3F%23%25%2B%20%ED%95%9C`이다.
  - PAT 환경변수 없이 실행되어 정확한 Session pooler URL로 dry-run과 적용을 차례로 호출한다.
  - 두 호출이 같은 URL을 사용한다.
  - dry-run stub의 실패는 nonzero로 전달되고 두 번째 호출은 없다.
  - project ref 또는 password가 비어 있으면 CLI를 호출하지 않는다.
  - encoded password와 URL의 마스킹 지시가 CLI 실행 전에 출력되고 raw 가짜 password는 출력되지 않는다.

- [ ] **Step 2: RED 확인**

  ```sh
  PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s supabase/ci -p test_prepare_migrations.py -k test_production_deploy_uses_encoded_session_pooler_without_pat -v
  ```

  기대 결과는 현재 PAT/link 방식이 새 인증 계약을 충족하지 못해 발생하는 assertion failure다. YAML block 추출 오류나 테스트 자체 오류는 RED 증거가 아니다.

- [ ] **Step 3: workflow 변경**

  - job의 PAT mapping과 필수값 검사를 제거하고 `supabase link`를 없앤다.
  - 기존 workflow 보호 조건과 CLI 설치 설정은 보존한다.
  - DB password를 migration step에만 전달한다.
  - step의 inline Python에서 ref/password 필수값을 확인하고 `urllib.parse.quote(value, safe="")`로 URL component를 인코딩한다.
  - GitHub `::add-mask::`로 encoded password와 완성 URL을 등록한 뒤에만 subprocess를 실행한다.
  - `subprocess.run`의 인자 배열로 같은 URL을 이용해 dry-run 후 적용한다. dry-run 실패 시 즉시 같은 종료 코드로 끝낸다.
  - URL을 명령 문자열, 출력, 파일, artifact 또는 step output으로 기록하지 않는다.

- [ ] **Step 4: 운영 문서 갱신**

  `docs/deployment.md`에서 운영 workflow의 필수 secret을 `TOKEN_PLANET_SUPABASE_DB_PASSWORD`로 설명하고, PAT가 더 이상 workflow 요구사항이 아님을 적는다. Session pooler의 확인된 host·username·port와 SSL, 자동 인코딩·로그 마스킹, dry-run → 적용 순서를 문서화한다. 기존 PAT를 지우거나 폐기하도록 안내하지 않는다. 이전 확인의 migration history 31개 일치 및 pending 0은 다음 실행에서 다시 확인해야 하는 시점 한정 증거로 기록한다.

- [ ] **Step 5: GREEN 및 로컬 검증**

  ```sh
  PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s supabase/ci -p test_prepare_migrations.py -k test_production_deploy_uses_encoded_session_pooler_without_pat -v
  PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s supabase/ci -p 'test_*.py' -v
  git diff --check
  ```

  테스트는 실제 Supabase 연결을 하지 않는다. YAML 문법 및 보호 조건은 QA가 독립 확인한다.

- [ ] **Step 6: QA 및 최종 Reviewer**

  QA는 회귀 테스트와 변경된 workflow/doc을 독립 확인한다. manual trigger, master/confirm 조건, environment, concurrency, `contents: read`, setup-cli pin/version 유지, secret 전달 범위, URL 구성, dry-run 실패 전파, migration 순서를 보고한다. Reviewer는 QA 이후 최종 세 파일 전체에서 인코딩·secret 노출·실패 전파·문서 일치를 검토한다. 수정이 있으면 영향받은 QA 확인을 다시 수행한다.

- [ ] **Step 7: master 반영과 실제 배포 재검증**

  CEO는 승인된 실행 대상에서 계획 커밋, 코드 커밋과 리뷰된 변경을 준비한다. master에 반영되기 전에는 master 전용 workflow를 성공한 것으로 표시하지 않는다. 사용자가 코드를 master에 반영한 뒤 CEO가 migration history와 pending 상태를 다시 확인하고, `confirm_apply=true`로 workflow를 실행해 dry-run 및 적용 결과를 확인한다. GitHub `supabase-production` Environment의 승인 대기를 통과해야 한다. 원격 실행 결과와 실제 DB 변경 여부를 별도로 보고한다.

## Acceptance and Evidence Limits

- PAT 및 Management API link 없이 승인된 Session pooler URL로 마이그레이션 CLI가 실행된다.
- 특수 문자와 Unicode 비밀번호 인코딩, secret 마스킹, dry-run 실패 시 적용 차단이 회귀 테스트로 입증된다.
- 기존 수동 실행·master 제한·확인 입력·Environment 보호가 정적 구조 검토로 유지됨을 확인한다.
- 로컬 테스트는 DB 비밀번호 유효성, 외부 네트워크 접근, 실제 TLS 연결 또는 운영 마이그레이션 결과를 증명하지 않는다.
- 실제 운영 재검증은 master 반영과 Environment 승인 후 별도로 수행한다.

## Self-review

- 사용자가 승인한 인증 경로 변경만 포함했고, 이전 배포 경로 설계의 다른 조건은 유지한다.
- workflow, 기존 테스트 파일, 활성 문서만 수정하며 새 helper·suite·dependency는 추가하지 않는다.
- 테스트의 성공/실패/비밀 로그 경로, dry-run 순서, 운영 배포 책임과 원격 검증 경계를 명시했다.
- 수정된 코드가 master에 도달해야 실제 승인 workflow를 검증할 수 있다는 의존성을 포함했다.
- 저장소 파일 수정, 테스트 실행, Git 변경 및 원격 실행은 계획 초안 단계에서 수행하지 않았다.
