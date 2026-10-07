# Token Planet 배포 안내

데스크톱 앱 릴리스와 운영 Supabase 데이터베이스 변경은 서로 다른 GitHub Actions 경로를 사용합니다. 데스크톱은 `vX.Y.Z` 태그에서 macOS DMG와 Windows MSI를 만들고 초안 Release로 올립니다. 운영 데이터베이스는 `master`의 `workflow_dispatch` 실행과 `supabase-production` 환경 승인을 거쳐 마이그레이션을 적용합니다.

## GitHub Actions 설정

저장소의 **Settings → Secrets and variables → Actions**에서 다음 Repository variables를 만듭니다.

| 이름 | 값 | 사용 위치 |
| --- | --- | --- |
| `TOKEN_PLANET_SUPABASE_URL` | `https://<project-ref>.supabase.co` 또는 프로젝트의 HTTPS URL | 태그 빌드의 앱 설정 |
| `TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY` | Supabase 공개 publishable key | 태그 빌드의 앱 설정 |
| `TOKEN_PLANET_SUPABASE_PROJECT_REF` | Supabase 프로젝트 ref | 운영 마이그레이션 |

앱에는 URL과 publishable key만 들어갑니다. publishable key는 클라이언트에 포함되는 공개 키이며, Supabase secret/service-role 키를 앱 빌드 변수나 `.env.local`에 넣지 마세요.

**Settings → Environments**에서 정확히 `supabase-production` 환경을 만들고 다음을 설정합니다.

1. `master` 브랜치만 배포 대상으로 허용합니다.
2. required reviewers를 추가하고, 가능하면 배포 시작자가 자기 실행을 승인할 수 없게 설정합니다.
3. 아래 값을 이 환경의 Environment secrets로 등록합니다.

| 이름 | 값 | CLI에 전달되는 이름 |
| --- | --- | --- |
| `TOKEN_PLANET_SUPABASE_DB_PASSWORD` | 대상 프로젝트의 데이터베이스 비밀번호 | `SUPABASE_DB_PASSWORD` |

이 GitHub 환경 승인은 운영 job이 시작되기 전에 이뤄집니다. 승인이 끝나면 workflow가 Session pooler에 `supabase db push --db-url <url> --dry-run`으로 연결하고, 성공하면 동일 URL로 `supabase db push --db-url <url>`를 실행합니다. dry-run 뒤에는 별도의 두 번째 승인 단계가 없으므로 적용 대상 마이그레이션과 실행 내용을 먼저 검토한 뒤 승인하세요. 환경 시크릿은 해당 환경을 사용하는 job에만 제공하고, 로그에 값을 출력하지 마세요. GitHub는 환경 보호 규칙을 저장소 설정에서 관리하며, required reviewers와 private repository 환경 시크릿의 사용 가능 여부는 저장소 공개 범위와 플랜에 따라 다를 수 있습니다. 필요한 보호 규칙을 설정할 수 없다면 운영 workflow를 실행하지 말고 승인 방식을 먼저 정리하세요. [GitHub Environments 문서](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments)

## 데스크톱 초안 Release

1. `apps/desktop/package.json`과 `apps/desktop/src-tauri/tauri.conf.json`의 버전을 같은 `X.Y.Z`로 맞춥니다.
2. 변경을 `master`에 병합하고 그 버전을 가리키는 `vX.Y.Z` 태그를 만듭니다.
3. 해당 태그의 GitHub Actions 실행을 확인합니다. 워크플로는 기존 Vitest 검사와 프런트엔드 빌드를 수행하고, 버전이 일치하면 macOS/Windows 네이티브 runner에서 DMG/MSI를 만듭니다.
4. 두 플랫폼 빌드가 모두 성공하면 workflow가 해당 태그의 draft Release를 만들고 두 설치 파일을 첨부합니다. 한 플랫폼이라도 실패하면 draft 단계에 도달하지 않습니다.
5. 초안의 버전과 설치 파일을 확인합니다. [`docs/release-acceptance.md`](release-acceptance.md)의 호스팅 공유, macOS 설치/tray, Windows 네이티브 점검을 마친 뒤 사람이 GitHub에서 Release를 공개합니다.

태그 workflow는 자동 공개하지 않습니다. 앱 버전과 태그가 다르면 실패합니다. 코드 서명·공증·자동 업데이트와 실제 사용자 기기 검증은 이 파이프라인에 포함되지 않습니다.

## 운영 Supabase 마이그레이션

1. 변경된 migration 파일을 검토하고 별도 테스트 프로젝트에서 확인한 뒤 변경을 `master`에 병합합니다.
2. GitHub Actions에서 **Supabase production deploy** workflow를 수동 실행하고 branch/ref로 `master`를 선택합니다.
3. 기본값이 false인 적용 확인 입력을 명시적으로 true로 설정하고 실행합니다.
4. `supabase-production` 환경 reviewer가 workflow job을 승인합니다. 그 뒤 Supabase CLI 2.119.0이 프로젝트를 연결하고 dry-run과 적용을 같은 job에서 차례로 수행합니다.

이 workflow는 migration만 적용합니다. seed, remote reset, migration history repair는 실행하지 않습니다. dry-run과 적용 사이에 GitHub에서 멈추거나 별도 승인받지 않으므로, 승인 전에 `master`의 migration 내용을 확인해야 합니다. Supabase CLI의 dry-run은 적용할 변경 목록을 미리 보여주는 기능입니다. [Supabase migration 안내](https://supabase.com/docs/guides/deployment/database-migrations)

## 로컬 호스팅 개발 설정

`apps/desktop/.env.local`은 각 개발자의 컴퓨터에서만 관리합니다. `apps/desktop/.env.example`을 복사하고 다음 두 이름을 사용하세요.

```dotenv
TOKEN_PLANET_SUPABASE_URL=https://<project-ref>.supabase.co
TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY=<publishable-key>
```

이 파일을 Git에 추가하거나 팀원에게 키 파일을 보내지 마세요. 공유 개발을 위해 필요한 앱 설정은 각자 승인된 공개 URL/publishable key를 넣으면 됩니다. CLI access token, DB 비밀번호, secret/service-role key는 로컬 앱 설정에 넣지 않습니다.

## 기존 설치에서 전환할 때

Token Planet은 `io.github.yunhozz.tokenplanet` 앱 식별자와 `Token Planet session` 키체인 service를 사용합니다. 이 네임스페이스 변경은 기존 로컬 SQLite 데이터나 키체인 항목을 지우지는 않지만, 새 앱이 그 데이터를 자동으로 이어받지 않습니다. 새 설치는 새 익명 Supabase 사용자를 만들고, 예전 익명 사용자 ID에 연결된 공동 세계 멤버십·소유권도 승계하지 않습니다. 사용자가 이 데이터 단절을 승인했으며 자동 복구나 세계 이전 절차는 제공하지 않습니다.

운영 workflow의 필수 시크릿은 `TOKEN_PLANET_SUPABASE_DB_PASSWORD` 하나입니다. PAT는 이 workflow에서 사용하지 않으며 기존 토큰을 삭제하거나 폐기할 필요는 없습니다. Session pooler 연결은 사용자명 `postgres.<project-ref>`, 호스트 `aws-0-ap-northeast-2.pooler.supabase.com`, 포트 `5432`, 데이터베이스 `postgres`, SSL `require`를 사용합니다. workflow는 사용자명과 비밀번호를 RFC 3986으로 자동 인코딩하고 인코딩된 비밀번호와 URL을 명령 실행 전에 마스킹합니다. URL은 파일이나 step output에 저장하지 않습니다. dry-run이 실패하면 적용은 실행하지 않습니다.

이전 확인에서 migration history 31개가 일치하고 pending 0개였다는 결과는 당시 상태입니다. 다음 운영 실행에서 다시 확인해야 합니다.
