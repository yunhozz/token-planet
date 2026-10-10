# 그룹 채팅 검증 기록 — 2026-10-10

## 대상과 범위

- 승인된 체크아웃: `/Users/yunho/.codex/worktrees/group-chat/token-planet`, `feat/group-chat`, 기준 HEAD `f20342d3144f26894c4e5b0b0d1f625f0c2344b7`. 명세/계획은 해당 커밋에 포함되어 있으며 기능 변경은 아직 커밋하지 않았다.
- 실제 실행 대상: 이미 실행 중인 `/tmp/token-planet-group-chat-supabase`, project `token-planet-group-chat-qa`, API `http://127.0.0.1:54321`, DB `54322`, CLI `2.118.0`. Docker 로컬 Unix context, 프로젝트/workdir/포트 및 immutable DB container ID를 fixture 쓰기 전에 확인했다. Hosted/production 호출, stack 시작·중지·reset, Docker 데이터 삭제를 수행하지 않았다.
- macOS에서 Python 표준 라이브러리 harness, Rust lib tests/build, Node/Vitest/jsdom 및 TypeScript/Vite build를 실행했다. 실제 네이티브 앱 두 개와 VoiceOver/Windows는 실행하지 않았다.
- `proven`은 해당 검증 범위에서 관찰한 성공, `unmet`은 실행한 기준 실패, `unverified`는 필요한 관찰을 하지 않은 상태다. 하위 검사가 통과해도 전체 수용 기준에 네이티브 UI/재시작 조건이 남으면 `unverified`로 표시한다.

## 명세의 15개 수용 기준

아래 번호는 승인 명세의 순서를 그대로 따른다. API/DB/Realtime 관찰은 같은 local 인스턴스의 독립 Auth 사용자 A/B, 늦게 가입하는 C, 다른 그룹 D로 실행했다. 이 표의 기준 번호는 harness의 R01–R16과 별개다.

| 기준 | 상태 | 실행·관찰 및 증거 | 남은 검증 |
| --- | --- | --- | --- |
| 1. 같은 그룹의 독립 사용자 실시간 송수신과 앱 재시작 후 기록 | unverified | R02/R10/R15: 독립 Auth 계정의 WebSocket 수신 및 새 socket/history 복구 성공 | 실제 앱 2개 송수신과 앱 재시작 후 화면 기록 |
| 2. 온라인 화면 수신 P95 1초 | unverified | R15: 100개 메시지의 server-created timestamp→WebSocket JSON parser 수신 P95 532.399 ms, API 시작→parser 수신 P95 534.804 ms. 운송 기준 1,000 ms 이내 | 실제 Tauri/React 화면 표시까지 측정해야 함 |
| 3. 가입 전 메시지는 첫 페이지·과거 조회·증분 동기화·안 읽음·Realtime에서 제외 | proven | R07: C의 가입 cutoff 이전 메시지 REST/list/sync/unread 제외, 이전 메시지 tombstone UPDATE도 C에게 전달되지 않음. SQL membership/access tests | local 테스트 범위 |
| 4. 비멤버·다른 그룹·탈퇴 사용자의 REST/RPC/Realtime 접근 차단 | proven | R03/R08/R12: 익명/비멤버/다른 그룹 거부, 탈퇴 뒤 기존 socket에 신규 메시지 미전달 및 REST/RPC 거부 | 음성/화면이 아닌 권한 검증 |
| 5. 작성자·그룹·cutoff·시간 위조 및 타인 메시지 삭제 거부 | proven | R04: 추가 작성자/프로필/cutoff/시간 인자 거부, 다른 그룹 전송 거부, 다른 작성자의 삭제 거부. SQL RPC tests | local RPC/RLS 범위 |
| 6. 응답 유실 재시도 시 한 번 저장·표시, 같은 ID 다른 본문 거부 | unverified | R06: 첫 응답을 다시 사용하지 않고 동일 request ID/body 재호출하여 동일 message ID/단일 row 확인, 변경 본문 거부. hook tests: 중복 merge/같은 ID 재시도 | 실제 네이티브 통신 응답 유실과 화면 중복 관찰 |
| 7. 끊김 중 insert/delete 복구, 삭제 본문 복원 금지 | unverified | R10: B socket close 동안 insert+tombstone 생성, sync/list 및 새 socket 연결 후 body null 확인. Rust/history race 및 hook merge tests | 실제 앱 네트워크 단절/재연결 화면 |
| 8. 닫힌 채팅은 그룹 안 읽음만, 열어 최신 도달 시 감소, 개인 커서 보호 | unverified | R11: 개인별 context/read cursor 단조 증가, 최신 표시 0/뒤로 이동 거부. hook/component tests: collapsed cue/read gating | 네이티브 화면 안 읽음 동작 |
| 9. 본인 삭제는 tombstone, 반복 삭제 안전, 탈퇴해도 기록 보존 | unverified | R07/R09/R13: body null UPDATE, 동일 반복 삭제 change seq, membership/Auth deletion 후 snapshot/body 보존. component tombstone tests | 실제 화면 삭제 표시·다른 멤버 기록 관찰 |
| 10. 신규/재가입 cutoff와 탈퇴 즉시 구독·초안·UI 정리 | unverified | R12: 탈퇴 후 데이터 차단/재가입 새 cutoff/이전 request 재사용 거부. Rust exact-scope successful-leave tests 및 hook cleanup tests | 실제 네이티브 탈퇴 화면/초안 정리 |
| 11. 공유 pause·사용량 삭제와 채팅 독립, pause 프로필 snapshot과 통계 비노출 | proven | R05: paused profile의 서버 nickname/avatar, profile 없는 B fallback, profile 변경 후 이전 snapshot 유지. 사용량 삭제 뒤 채팅 유지, world planet token 통계 0 | local 서버 및 React tests 범위 |
| 12. 그룹 삭제 시 모든 채팅 데이터 삭제 | proven | R14: 마지막 owner 탈퇴 후 messages/state/authors/requests/reads cascade count 0. SQL tests | local fixture 범위 |
| 13. HTML/script 안전한 텍스트, 빈 내용·2,000자 초과 서버 거부 | proven | R16: HTML literal 저장, 공백/빈 문자열/2,001 Unicode code points 거부, 2,000 허용. component tests: plain React text·IME·code points | jsdom rendering 범위; 네이티브 시각 QA 별도 |
| 14. 작성 당시 이름·픽셀 아바타와 접근 가능한 작성자 표시 | unverified | R05: 서버 snapshot 및 fallback 유지. GroupChat tests: pixel avatar/author accessible label | 실제 픽셀 렌더·작은 창·VoiceOver |
| 15. Realtime 포함 local 개발 안내와 독립 사용자 2개 검증 | proven | 두 README에 workdir/프로젝트 및 Realtime 포함 안내. launcher mock은 repo 기본·승인 scratch override·외부 target 거부 검증. R02/R15 실제 독립 local Auth 계정 수신 성공 | 안내의 fresh start 명령은 실행하지 않음 |

전체 수용 완료를 주장하지 않는다. 이 실행에서 위 수용 기준의 `unmet`은 없지만 네이티브 UI/재시작 조건은 남아 있다. R15 운송 지표가 1,000 ms를 초과하면 harness는 반드시 `unmet`과 실패 exit를 반환한다.

## 실제 Realtime 하위 검사

최종 strict readiness/heartbeat 버전 실행은 R01–R16 모두 성공했다. 직전 중간 실행은 R15 안에서 100개 수집 완료 전에 중단되어 R15 unmet/후속 검사 unverified였고 숫자 지표는 없었다. 당시 wrapper가 오류를 고정 generic 코드로 축약하여 원인은 미확인이다. 이후 static allowlist 오류만 보존하는 회귀 검사를 추가했고, 다른 DB 검사와 겹치지 않은 최종 재실행이 성공했다. 단일 성공으로 중간 실패의 원인이 해결됐다고 주장하지 않는다.

`/tmp/token-planet-group-chat-realtime-results.json`은 고정 check ID/status와 숫자 지표만 포함한다. 사용자 ID·세계 ID·프로필·본문·토큰·키·원본 오류는 기록하지 않는다. 테스트 계정/세계는 무작위 local fixture이며 마지막에 제거한다.

| 하위 검사 | 관찰 | 결과 |
| --- | --- | --- |
| R01 | 정확한 local identity, 채팅 publication은 public.group_chat_messages만, replica identity DEFAULT | proven |
| R02 | 독립 Auth A/B/C/D 및 invite 가입 | proven |
| R03 | REST/RPC grant와 private schema/read state, 익명 및 직접 write 차단 | proven |
| R04 | 위조 send·타인 delete 거부 | proven |
| R05 | paused 서버 프로필·fallback·snapshot 보존·사용량 분리 | proven |
| R06 | 동일 request idempotency/다른 본문 거부 | proven |
| R07 | 신규 가입 cutoff: REST/list/sync/unread/UPDATE 수신 | proven |
| R08 | 비멤버/다른 world Realtime 및 REST/RPC 차단 | proven |
| R09 | tombstone UPDATE 본문 null, old payload 본문 비노출 | proven |
| R10 | socket 끊김 중 insert+tombstone 및 sync/rejoin 복구 | proven |
| R11 | private 개인 읽음 cursor·단조 증가 | proven |
| R12 | 탈퇴 후 차단·재가입 cutoff·이전 request 차단 | proven |
| R13 | 멤버/Auth row 삭제 후 메시지 보존·private mapping 제거 | proven |
| R14 | 그룹 cascade | proven |
| R15 | 온라인 100 samples 운송 P95 1,000 ms 이하 | proven |
| R16 | 서버 본문 validation·literal HTML | proven |

운송 시간은 같은 macOS host의 `time.time()`과 서버 `created_at` UTC를 비교한 값이다. DB/server clock 동기화 오차를 별도로 교정하지 않았다. API 왕복 시작→parser 수신은 `time.monotonic()`으로 측정했다. 저장 시각에서 JSON parser까지의 값은 화면 paint와 다르며 앱 P95의 대체 증거가 아니다. R10은 client socket을 닫았다가 다시 여는 방식이며 Supabase service 또는 desktop app을 재시작하지 않았다.

## 명령 및 결과

체크아웃 루트에서 실행했다. CLI status와 원본 응답을 terminal에 출력하지 않는다.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 supabase/tests/test_group_chat_realtime.py
PYTHONDONTWRITEBYTECODE=1 python3 supabase/tests/test_group_chat_target_guard.py
PYTHONDONTWRITEBYTECODE=1 python3 supabase/tests/group_chat_realtime.py --run
npx --yes supabase@2.118.0 test db --local --workdir /tmp/token-planet-group-chat-supabase supabase/tests/group_chat_access.sql supabase/tests/group_chat_membership.sql supabase/tests/group_chat_rpc.sql
bash supabase/tests/group_chat_concurrency.sh
npx --yes supabase@2.118.0 test db --local --workdir /tmp/token-planet-group-chat-supabase
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

- Harness unit: 14 checks 성공. 추가 안전한 failure code RED와 readiness/heartbeat RED는 없는 `subscription_ready`와 호출되지 않은 `tick`에서 실패한 뒤 GREEN.
- 최초 harness RED는 구현 모듈 부재. 이어 안전한 오류 식별자, minimal world insertion, launcher target, heartbeat, P95 판정 각각 회귀 실패를 확인하고 구현했다.
- Focused DB: 3 files / 59 checks 성공. Concurrency 3 cases 및 target guard 2 checks 성공. 전체 DB는 43 files / 1,072 checks에서 exit 1: 기존 `shop_guest_import_v2_public.sql`, `public_scene.sql`, `validation.sql`, `writer.sql` 네 파일이 필요한 psql `rollback_probe` 변수를 CLI runner가 전달하지 않아 TAP plan 전에 실패했다. 해당 파일과 runner는 변경하지 않았다. `/tmp/token-planet-group-chat-final-focused-db.log`, `/tmp/token-planet-group-chat-final-db.log`.
- Rust 전체: 417 passed / 1 기존 ignored / 0 failed, lib build 성공. `/tmp/token-planet-group-chat-final-rust.log`.
- Frontend 전체: 26 files / 490 passed, TypeScript/Vite build 성공. `/tmp/token-planet-group-chat-final-frontend.log`.
- harness HTTP/WebSocket deadline 5초, child process timeout 20초, send 시나리오 deadline 5분, heartbeat는 15초 이상 경과하면 active send/receive polling에서 전송. Postgres Changes readiness는 `system`의 `extension=postgres_changes`, `status=ok`만 성공으로 인정한다. row denial은 별도 RLS 미수신 검사다.

## 제한과 후속 검증

- 환경 제한: 독립 QA는 정확한 승인 scratch 프로젝트 `token-planet-group-chat-qa`와 해당 프로젝트의 DB/Kong 컨테이너를 확인하고 `127.0.0.1`로 접속했다. Docker는 DB `54322`와 Kong/API `54321`을 호스트 `0.0.0.0` 및 `::`에 publish한다. 외부 네트워크에서의 실제 접근성은 테스트하지 않았으므로 loopback 전용 네트워크 격리가 입증된 것은 아니다. stack 또는 방화벽 변경은 수행하지 않았다.
- 이미 실행 중인 승인 scratch를 reset/중지할 수 없으므로 fresh migration replay와 실제 Supabase 재시작은 미검증. 적용된 DB에 대한 focused/전체 SQL 검사는 replay의 대체가 아니다.
- macOS Rust build 성공은 actual native IPC/keyring/Auth refresh/exit/hide lifecycle 실증과 다르다. 작은 창/긴 본문/스크롤 스크린샷, VoiceOver, 실제 두 desktop 세션, 앱 재시작/오프라인 UI 및 화면 P95가 필요하다.
- Windows 실행 환경은 없으므로 Windows build/behavior 미검증.
- Postgres Changes INSERT/UPDATE는 SELECT RLS로 걸러지지만 DELETE는 같은 RLS 보호를 받지 않는다. 본 기능은 본문을 지우는 tombstone UPDATE이며 hard delete를 채팅 이벤트로 사용하지 않는다. `realtime` schema에 객체를 만들지 않았다.
- Out-of-band admin 멤버 제거/그룹 삭제의 즉각적인 UI 감지는 범위 밖이다. RLS는 전달을 차단하며 다음 연결/context 검증 때 확인한다. 지원되는 본인 탈퇴는 exact scope를 즉시 정리한다.
- 영구 계정 삭제와 실제 백업 만료 정책은 미결. harness의 synthetic Auth row 삭제는 mapping/기록 보존의 DB 동작 검증이며 제품 계정 삭제 정책을 승인하는 것은 아니다.
- 반복 실행은 anonymous signup local rate limit을 소비한다. rate limit에 막히면 기록하고 중단하며 config/reset으로 우회하지 않는다.
