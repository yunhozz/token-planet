# Token Planet 배포 파이프라인 설계

**상태:** 사용자 승인 완료
**날짜:** 2026-10-07

## 목표

공유 세계를 사용할 수 있는 Token Planet 데스크톱 앱을 GitHub Releases로 배포하고, 호스팅 Supabase 스키마는 앱 배포와 분리된 승인 경로로 반영한다. 기존 `Token World` 런타임·패키지 식별자와 Supabase 환경변수를 `Token Planet` / `TOKEN_PLANET_*`로 정리한다.

## 현재 상태

- 앱은 `apps/desktop/`의 Tauri 2·React·Rust 데스크톱 앱이다. Tauri 제품명과 창 제목은 이미 `Token Planet`이지만 HTML 제목, 트레이, README, 패키지명, 번들 식별자 및 키체인 서비스에는 `Token World` 계열 이름이 남아 있다.
- 호스팅 개발 런처는 `apps/desktop/.env.local`에서 `TOKEN_WORLD_SUPABASE_URL`과 `TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY`를 읽는다. Rust 앱도 같은 이름을 실행 환경 또는 컴파일 환경에서 읽는다.
- `.github/workflows/supabase-migrations.yml`은 PR과 `master` 푸시에서 임시 로컬 DB에 마이그레이션을 재생한다. 호스팅 프로젝트에는 배포하지 않는다.
- 저장소 문서는 최근 익명 인증·개인 초대 코드 변경에 대한 새 검증과 실제 기기 간 초대 확인이 필요하다고 기록한다. Windows 앱의 네이티브 검증도 미완료다.

## 선택한 접근

### 데스크톱 릴리스

새 `.github/workflows/desktop-release.yml`은 PR, `master` 푸시, `v*` 태그에서 앱 검증을 수행한다. 검증 단계는 고정된 Node/npm/Rust 버전으로 의존성을 설치하고 기존 Vitest 테스트, 프런트엔드 빌드 및 네이티브 패키징을 실행한다.

`v*` 태그에서는 macOS DMG와 Windows MSI를 각각 네이티브 GitHub-hosted runner에서 빌드한다. 빌드 두 개가 모두 성공한 뒤 GitHub CLI로 태그에 대한 초안 Release를 만들고 설치 파일을 첨부한다. 초안은 자동 공개하지 않는다. 운영 DB 반영, 실제 두 기기의 초대 확인, 플랫폼별 수동 출시 점검 후 사람이 공개한다.

릴리스 태그 버전은 `apps/desktop/package.json`과 `apps/desktop/src-tauri/tauri.conf.json`의 버전과 일치해야 한다. 릴리스 작업은 `contents: write` 권한이 필요하며, 빌드 작업은 읽기 권한만 가진다. GitHub 기본 `GITHUB_TOKEN`만 사용한다.

### 운영 Supabase 마이그레이션

새 `.github/workflows/supabase-production-deploy.yml`은 `workflow_dispatch`로만 시작한다. 태그, PR, 브랜치 푸시로 운영 DB를 변경하지 않는다. 작업은 `supabase-production` GitHub Environment를 사용하고 명시적인 적용 확인 입력이 참일 때만 진행한다.

고정된 Supabase CLI로 프로젝트를 연결하고 `supabase db push --dry-run` 결과를 출력한 뒤 `supabase db push`를 수행한다. 시드 데이터, 원격 DB 리셋, 마이그레이션 기록 수동 수정은 하지 않는다. 실제 승인 대기는 GitHub 저장소 설정에서 `supabase-production` 환경의 required reviewers를 구성해야 활성화된다. 워크플로 문서에는 환경과 자격 증명 설정 단계를 적는다.

Supabase CLI가 기대하는 `SUPABASE_ACCESS_TOKEN`과 `SUPABASE_DB_PASSWORD`는 GitHub에 각각 `TOKEN_PLANET_SUPABASE_ACCESS_TOKEN`, `TOKEN_PLANET_SUPABASE_DB_PASSWORD` 시크릿으로 저장하고 작업 환경에만 전달한다. 대상은 `TOKEN_PLANET_SUPABASE_PROJECT_REF` Actions 변수로 고정한다.

### 변수 및 제품 식별자

데스크톱 앱과 Actions는 다음 프로젝트별 이름을 사용한다.

| 이름 | 용도 | 저장 위치 |
| --- | --- | --- |
| `TOKEN_PLANET_SUPABASE_URL` | 앱이 연결할 HTTPS Supabase URL | Actions variable, 로컬 `.env.local` |
| `TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY` | 데스크톱 클라이언트 키 | Actions variable, 로컬 `.env.local` |
| `TOKEN_PLANET_SUPABASE_PROJECT_REF` | 운영 마이그레이션 대상 | Actions variable |
| `TOKEN_PLANET_SUPABASE_ACCESS_TOKEN` | Supabase CLI 인증 | Actions secret |
| `TOKEN_PLANET_SUPABASE_DB_PASSWORD` | 원격 DB 연결 | Actions secret |

URL과 publishable key만 앱 빌드에 포함한다. Supabase secret/service-role 키, CLI access token, DB password는 앱 번들에 넣지 않는다. Supabase publishable key는 배포 클라이언트에 포함될 수 있는 공개 키이며 데이터 접근은 RLS와 인증 정책이 제한한다. [Supabase API keys](https://supabase.com/docs/guides/getting-started/api-keys)

활성 애플리케이션 설정에서 기존 `TOKEN_WORLD_SUPABASE_URL`과 `TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY`를 위의 `TOKEN_PLANET_*` 이름으로 바꾼다. 대상은 Rust 인증 로더와 환경변수를 조작하는 기존 Rust 테스트, `dev-hosted.mjs`, `dev-local.mjs`, `.env.example`, 활성 README 및 출시 설정이다. 실제 ignored `.env.local` 파일의 비밀값은 읽거나 커밋하지 않으며, 사용자는 로컬 키 이름을 직접 새 이름으로 갱신할 수 있도록 문서화한다.

제품 표시명은 `Token Planet`으로 맞춘다. 앱 패키지명은 `token-planet-desktop`, Tauri 식별자는 `io.github.yunhozz.tokenplanet`, 키체인 서비스는 `Token Planet session`, 트레이 ID는 `token-planet`을 사용한다. `Token World`로 남아 있는 창 제목, 트레이 라벨·툴팁, 활성 README와 OTP 템플릿도 갱신한다. 과거 설계 문서의 역사적 표현은 수정하지 않는다.

## 기존 설치의 데이터 처리

사용자는 새 설치 ID로 시작해도 괜찮다고 승인했다. Tauri 식별자와 키체인 서비스명을 바꾸므로 기존 설치의 로컬 SQLite 위치와 키체인 세션은 새 앱에서 찾지 않는다. 이전 파일과 키체인 항목은 삭제하거나 옮기지 않는다. 앱은 새 익명 Supabase 사용자를 생성한다.

기존 공유 세계의 멤버십과 소유권은 서버의 이전 익명 사용자 ID에 그대로 남는다. 앱 이름 변경은 해당 계정을 복구하거나 기존 세계 소유권을 이전하지 않는다. 기존 사용자는 새 설치에서 이전 멤버로 자동 복귀하지 못하며, 소유자 세션을 잃은 세계는 접근 또는 초대 관리가 막힐 수 있다.

## 보안 및 공개 범위

- 운영 Supabase 배포는 명시적 수동 실행 및 GitHub Environment 승인으로 제한한다.
- 빌드에서 앱 클라이언트에 공개 URL과 publishable key만 전달한다. Secret/service-role 키는 브라우저·데스크톱 번들에 넣지 않는다.
- Actions 권한은 작업별 최소 권한으로 둔다. 릴리스 생성 작업만 `contents: write`를 가진다.
- 이번 변경은 서명·공증, 자동 업데이트, Windows 코드 서명, 자동 Release 공개, 실제 호스팅 DB 마이그레이션 또는 실제 두 기기 검증을 수행하지 않는다.
- 저장소의 기존 출시 기록에 Windows 및 공유 기능 검증 미완료가 남아 있으므로 Release는 초안으로 두고, 공개 전 수동 점검을 요구한다.

## 완료 조건

1. PR/master 검증에서 앱 테스트·프런트엔드 빌드와 기존 Supabase 임시 DB 재생이 수행된다.
2. 버전 태그가 앱 버전과 일치하지 않으면 릴리스 작업이 실패한다.
3. 유효한 버전 태그에서 macOS DMG와 Windows MSI를 만들고, 두 빌드 성공 후에만 초안 Release에 첨부한다.
4. 초안 Release는 자동 공개되지 않는다.
5. Supabase 운영 마이그레이션은 `workflow_dispatch`에서만 실행되고, 확인 입력이 없거나 거짓이면 변경 없이 종료한다.
6. 마이그레이션 작업은 dry-run 후 적용하며 seed나 reset을 실행하지 않는다.
7. 앱·로컬 개발·Actions 설정 이름이 `TOKEN_PLANET_*`와 일치하고, 특권 Supabase 자격 증명이 앱 번들에 포함되지 않는다.
8. 활성 UI/트레이/템플릿 표시는 `Token Planet`을 사용하고, 새 bundle/keychain namespace로 실행한다.
9. 사용 설명에는 필요한 Actions variable/secret, `supabase-production` reviewer 설정, 로컬 `.env.local` 키 갱신, 기존 설치에 대한 새 익명 계정/세계 접근 영향을 기록한다.

## 운영 순서

1. GitHub Actions variables/secrets와 `supabase-production` Environment 보호 규칙을 설정한다.
2. 호스팅 변경을 별도 테스트 프로젝트에서 검증하고, 새 기기 두 대의 공유 초대 흐름을 확인한다.
3. 앱 버전을 갱신하고 해당 버전과 일치하는 `vX.Y.Z` 태그를 푸시한다.
4. macOS·Windows 초안 산출물을 확인한다.
5. 승인된 운영 DB 마이그레이션 워크플로를 실행한다.
6. 실제 공유 테스트와 플랫폼별 수동 점검을 마친 뒤 초안 Release를 공개한다.

