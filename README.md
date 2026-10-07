# 🪐 Token Planet

Codex와 Claude Code의 확인된 토큰 사용량으로 행성을 키우는 데스크톱 앱입니다. Tauri 2·React·Rust로 구성되며, 계정 없이 혼자 사용하거나 Supabase를 통해 친구와 공유 월드에 참여할 수 있습니다.

## ✨ 주요 기능

- 로컬 사용 기록을 읽어 토큰 사용량을 집계하고 행성 성장에 반영
- 에이전트별 수집 상태와 불완전한 집계 표시, 수집 폴더 선택 및 사용 여부 설정
- 개인 코드로 공유 월드 초대·참여, 사용량 동기화 일시 정지 및 공유 데이터 삭제
- 원본 로그를 변경하지 않고 로컬 SQLite에 집계 저장; 공유 시 원문·프롬프트·로컬 경로는 업로드에서 제외

## 🚀 로컬 실행

필수 도구: **Node.js 22.21.0**, **npm 10.9.4**, **Rust 1.98.1** (`rust-toolchain.toml`), [Tauri 2 플랫폼별 사전 요구 사항](https://v2.tauri.app/start/prerequisites/).

저장소 루트에서 실행합니다.

```sh
npm --prefix apps/desktop ci
npm --prefix apps/desktop run tauri -- dev
```

기본 실행은 솔로 모드로 사용할 수 있습니다. 로컬 공유 개발에는 Docker와 Supabase CLI가 추가로 필요하며, 로컬·호스팅 환경 설정은 아래 데스크톱 문서를 참고하세요.

## 📚 상세 문서

- [데스크톱 실행·공유 설정·수집 방식·개인정보 및 검증 상태](apps/desktop/README.md)
- [GitHub Releases 및 운영 Supabase 배포 절차](docs/deployment.md)
- [Supabase 데이터베이스·마이그레이션·CI 안내](supabase/README.md)
- [설계 문서](docs/superpowers/specs/) · [구현 계획](docs/superpowers/plans/)

데스크톱 클라이언트는 `apps/desktop/`, 데이터베이스 설정과 마이그레이션은 `supabase/`, 설계와 계획은 `docs/`에 있습니다. 공유 기능과 플랫폼별 출시 검증 상태는 상세 문서에서 확인하세요.
