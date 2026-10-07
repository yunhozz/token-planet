# Token Planet 배포 파이프라인 구현 계획

**상태:** 사용자 승인 완료
**날짜:** 2026-10-07
**설계:** [배포 파이프라인 설계](../specs/2026-10-07-token-planet-deployment-pipeline-design.md)

> For agentic workers: CEO는 범위와 수용 기준을 조정하고 각 작업을 담당 역할에 배정한다. Coder가 앱·워크플로 설정을 수정하고, QA와 Reviewer가 서로 독립적으로 확인한 뒤 CEO가 수용한다.

## Goal

승인된 설계에 따라 Token Planet 데스크톱 앱의 macOS DMG/Windows MSI 초안 배포와 별도의 보호된 Supabase 운영 마이그레이션 경로를 구성한다. 앱의 활성 환경변수, 사용자 표시명, 패키지·번들·키체인·트레이 식별자를 `TOKEN_PLANET` / `Token Planet`으로 정리한다.

## Architecture

- `.github/workflows/desktop-release.yml`은 PR과 `master` 푸시에서 기존 프런트엔드 테스트와 빌드를 수행한다. `v*` 태그에서는 같은 검증을 통과한 뒤 macOS와 Windows 네이티브 러너에서 각각 패키지를 만든다. 두 산출물이 모두 성공한 경우에만 GitHub CLI로 비공개 초안 Release를 만들고 파일을 첨부한다.
- `.github/workflows/supabase-production-deploy.yml`은 `workflow_dispatch`에서만 동작한다. `master` 참조, 명시적인 적용 확인, `supabase-production` Environment 보호를 요구하고 dry-run 이후에만 실제 `db push`를 실행한다.
- 기존 `.github/workflows/supabase-migrations.yml`의 임시 로컬 DB 재생 검증은 그대로 유지한다.
- GitHub Actions의 공개 앱 설정은 `TOKEN_PLANET_SUPABASE_URL`, `TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY`, `TOKEN_PLANET_SUPABASE_PROJECT_REF` 변수로 둔다. CLI 전용 자격 증명은 `TOKEN_PLANET_SUPABASE_ACCESS_TOKEN`, `TOKEN_PLANET_SUPABASE_DB_PASSWORD` 시크릿으로 보관하고 앱 빌드에는 전달하지 않는다.
- 앱 표시명과 승인된 내부 식별자만 갱신한다. 과거 문서의 역사적 기록과 저장된 사용자 데이터는 재작성하지 않는다.

## Tech Stack

- GitHub Actions, GitHub CLI, GitHub Releases
- Tauri 2, React, Rust, Node.js 22.21.0, npm 10.9.4
- Supabase CLI 2.119.0

## Spec

- `docs/superpowers/specs/2026-10-07-token-planet-deployment-pipeline-design.md`
- 사용자는 GitHub Releases, 태그 기반 초안 릴리스, 별도 승인형 운영 마이그레이션, `TOKEN_PLANET_*` 변수명, 내부 식별자 변경, 새 익명 계정으로 시작하는 데이터 처리 방식을 승인했다.

## Global Constraints

- `apps/desktop/.env.local`을 읽거나 수정하거나 커밋하지 않는다. 추적되는 `.env.example`에는 예시 자리표시자만 둔다.
- 앱 번들에는 Supabase URL과 publishable key만 전달한다. access token, DB password, secret/service-role key는 번들·로그·산출물에 넣지 않는다.
- 운영 DB는 실제 적용하지 않는다. workflow 파일과 문서만 준비하고, 사용자 자격 증명이나 GitHub 환경 설정을 만들지 않는다.
- 태그 릴리스는 초안으로만 만든다. 이번 구현 중 태그를 푸시하거나 원격 Release를 생성·공개하지 않는다.
- SQLite 파일과 기존 키체인 항목은 옮기거나 지우지 않는다. 새 번들/키체인 식별자로 앱이 새 익명 계정을 만들며 이전 공동 세계 멤버십이 자동 승계되지 않는다는 점을 사용자 문서에 적는다.
- 기존 테스트를 새 환경변수명에 맞춰 갱신할 수 있지만, 별도 테스트 스위트나 테스트 헬퍼를 추가하지 않는다.
- 활성 제품 설정과 설명만 `Token World`에서 `Token Planet`으로 바꾼다. 역사적인 설계·계획 문서는 유지한다.

## Ownership and File Scope

### Coder — 앱 설정과 제품 식별자

- `apps/desktop/package.json`
- `apps/desktop/package-lock.json`
- `apps/desktop/src-tauri/tauri.conf.json`
- `apps/desktop/src-tauri/src/sync/auth.rs`
- `apps/desktop/src-tauri/src/sync/worker.rs`의 기존 환경변수 fixture
- `apps/desktop/src-tauri/src/platform/tray.rs`
- `apps/desktop/scripts/dev-hosted.mjs`
- `apps/desktop/scripts/dev-local.mjs`
- `apps/desktop/.env.example`
- `apps/desktop/index.html`
- `supabase/templates/otp.html`

기존 환경변수 두 개를 `TOKEN_PLANET_SUPABASE_URL`, `TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY`로 변경한다. NPM 이름은 `token-planet-desktop`, Tauri identifier는 `io.github.yunhozz.tokenplanet`, 키체인 service는 `Token Planet session`, 트레이 ID는 `token-planet`으로 맞춘다. 활성 트레이 문구도 갱신한다. Rust crate 이름은 사용자 승인 범위에 포함되지 않았으므로 바꾸지 않는다.

### Coder — 배포 워크플로

- `.github/workflows/desktop-release.yml` (신규)
- `.github/workflows/supabase-production-deploy.yml` (신규)
- `.github/workflows/supabase-migrations.yml` (읽기 전용 범위: 기존 동작 보존 확인)

데스크톱 워크플로는 아래 순서를 보장한다.

1. 모든 트리거에서 Node/npm을 고정하고 `npm ci`, 기존 `npm test`, `npm run build`를 실행한다.
2. `v*` 태그에서는 package.json과 tauri.conf.json 버전이 태그 버전과 일치하는지 검사한다.
3. macOS 네이티브 러너는 DMG를, Windows 네이티브 러너는 MSI를 만든 뒤 별도 Actions 산출물로 업로드한다. 태그 빌드에만 Actions 공개 URL/key 변수를 전달하고 필수값과 HTTPS URL을 확인한다.
4. 릴리스 작업은 두 플랫폼 빌드 성공에 의존한다. 최소 권한 `contents: write`와 기본 `GITHUB_TOKEN`으로 `gh release create --draft --verify-tag --generate-notes`를 실행해 두 파일을 첨부한다. 자동 공개나 기존 Release 덮어쓰기는 하지 않는다.

운영 마이그레이션 워크플로는 아래 보호를 구현한다.

1. 수동 `workflow_dispatch` 외에는 트리거를 두지 않는다. 확인 boolean 기본값은 false이며, 입력이 true이고 참조가 `master`일 때만 적용 job을 실행한다.
2. job은 `supabase-production` Environment를 사용한다. required reviewers는 GitHub 저장소 설정에서 구성해야 한다고 문서화한다.
3. 프로젝트 ref와 CLI 자격 증명은 job에만 전달하고, Supabase CLI 2.119.0으로 `supabase link`, `supabase db push --dry-run`, `supabase db push` 순서로 실행한다. seed, reset, migration history repair 단계는 추가하지 않는다.

### CEO — 사용 문서와 수용

- `README.md`
- `apps/desktop/README.md`
- `docs/release-acceptance.md`
- `docs/deployment.md` (신규)

배포 문서에 GitHub 변수/시크릿 이름과 등록 위치, `supabase-production` reviewer 설정, 버전 태그와 초안 확인 절차, 수동 DB 배포 절차, 로컬 `.env.local` 키 이름 갱신을 적는다. 기존 설치에서 새 익명 ID가 생기고 옛 공동 세계에 자동 재접속되지 않는 영향을 명시한다. `docs/release-acceptance.md`의 기존 체크 결과는 지우거나 새로 통과한 것으로 표시하지 않는다.

## Review Focus

- PR이나 `master` 푸시로 운영 Supabase 작업이 실행될 수 없는가?
- 확인 입력이 false이면 실제 `db push`가 실행되지 않는가?
- macOS/Windows 산출물 중 하나라도 실패하면 초안 Release 생성이 차단되는가?
- 태그, NPM 버전, Tauri 버전 불일치가 실패하는가?
- 특권 자격 증명이 앱 빌드 환경과 번들에서 배제되는가?
- 변경된 앱 식별자와 새 익명 계정으로 시작하는 데이터 영향이 문서와 일치하는가?
- 기존 마이그레이션 replay workflow와 과거 출시 검증 기록이 보존되는가?

## Execution Steps

1. Coder는 `apps/desktop`의 사용자 환경변수와 승인된 제품 식별자를 갱신한다. 예시 파일만 바꾸고 ignored `.env.local`에는 접근하지 않는다.
2. Coder는 데스크톱 릴리스 workflow를 추가하고 검증, 버전 일치, 플랫폼별 패키징, 산출물 수집, 초안 생성 간의 job 의존성을 설정한다.
3. Coder는 운영 Supabase 수동 workflow를 추가한다. trigger, boolean 확인, 브랜치 제한, GitHub Environment, CLI 변수 매핑과 dry-run 순서를 검토한다.
4. CEO는 사용자 배포 문서 및 기존 출시 체크리스트를 업데이트하고 토큰·계정 전환 안내가 승인된 설계와 맞는지 자체 검토한다.
5. QA는 소스 수준으로 workflow 조건·권한·시크릿 전파·버전 검증·artifact 경로와 문서 이름을 점검한다. 실제 원격 배포, Release 생성, 태그 푸시는 수행하지 않는다.
6. Reviewer는 앱 식별자 변경으로 인한 저장 위치/키체인 전환, Actions 권한, 운영 배포 게이트와 릴리스 실패 경로를 독립 검토한다.
7. CEO는 리뷰 결과를 반영하고 변경 파일, 검증 범위, 미완료 수동 출시 조건을 정리한다.

## Verification and Acceptance

- CI 정의에서 기존 `npm test`와 `npm run build`가 PR, `master`, `v*` 태그에서 실행됨을 확인한다. 새 테스트 코드는 추가하지 않는다.
- workflow의 tag 비교는 `vX.Y.Z`에서 `package.json` 및 `tauri.conf.json`의 동일 버전을 요구해야 한다.
- workflow 정적 점검에서 macOS DMG와 Windows MSI가 각각의 네이티브 runner에서 생성되고, 두 job 성공 이후에만 draft release job이 실행되는지 확인한다.
- 운영 workflow는 수동 시작, false 기본 확인 입력, `master` 제한, `supabase-production` 보호, dry-run 다음 push를 모두 가져야 한다. 기존 migration replay workflow의 변경도 확인한다.
- 저장소의 활성 앱 코드/설정/로컬 런처/템플릿에 이전 환경변수나 승인된 구형 식별자가 남지 않았는지 검색한다. 역사 문서와 cargo crate 이름은 검색 예외로 분류한다.
- 산출물 실행, 실제 Supabase 적용, 두 기기의 공유 초대, macOS 설치 후 tray 확인, Windows 실행/트레이 검증은 별도 출시 승인 단계로 남긴다. 이 계획 작업에서는 수행하지 않는다.

## Self-Review

- 설계가 승인한 두 배포 경로, 다섯 GitHub 이름, 내부 식별자, 초안 Release, 새 익명 계정 영향을 모두 포함했다.
- 기존 CI, 로컬 `.env.local`, 기존 데이터, 과거 검증 기록과 원격 운영 상태에 손대지 않는 제약을 명시했다.
- 새 테스트 스위트, 자동 공개, 자동 운영 배포, 실제 사용자 시크릿, 태그 푸시를 범위에 넣지 않았다.
- 코드/config 소유권은 Coder에 배정하고 CEO는 문서와 수용을 맡도록 구분했다.
- 문서는 사용자 승인 이후 planning 문서 검토/커밋 절차를 마친 다음 Coder handoff에 사용한다.
