# Token Planet 그룹 채팅 구현 계획

> 이 계획은 사용자 승인된 기능 명세를 구현 작업으로 나눈다. 계획 승인 뒤에도 실행 checkout과 구현 대상은 별도 합의가 필요하다.

**목표:** 비공개 그룹에서 가입 이후 텍스트 메시지를 실시간으로 교환하고, 작성 당시 표시명과 내장 픽셀 아바타, 개인 안 읽음 위치, 본인 메시지 삭제를 제공한다.

**구조:** Supabase가 메시지·가입 경계·읽음 위치·작성자 스냅샷과 접근 권한을 관리한다. Rust는 보안 저장소의 세션으로 RPC와 Postgres Changes WebSocket을 소유한다. React는 좁은 Tauri 명령·이벤트로 채팅 UI를 제공한다. 메시지 순번과 변경 순번을 분리해 재연결 때 기존 메시지의 tombstone 변경도 복구한다.

**기술:** React 19, TypeScript, Tauri 2, 저장소 고정 Rust toolchain, PostgreSQL, Supabase Auth·RPC·RLS·Postgres Changes. WebSocket 라이브러리와 버전은 작업 0에서 호환성을 확인한 뒤 확정한다.

**기준 명세:** [2026-10-10-token-planet-group-chat-design.md](../specs/2026-10-10-token-planet-group-chat-design.md)

**상태:** 사용자 계획 승인 완료 (2026-10-10). 구현 실행 대상 선택 대기.

## 공통 제약

- 구현과 검증은 승인된 작업공간 및 격리된 local/staging Supabase 프로젝트에서만 한다. production에는 쓰지 않는다.
- 메시지는 최대 2,000 Unicode 코드 포인트, 최신 50개 조회, 이전 기록은 커서 페이지네이션으로 제공한다.
- 일반 텍스트만 표시한다. HTML·마크다운으로 해석하지 않는다.
- 메시지 전송 API에는 그룹 ID·요청 ID·본문만 둔다. 작성자 ID·표시명·아바타 입력을 받지 않는다.
- 서버 RPC가 `auth.uid()`의 `planet_member_state.nickname`과 `avatar`만 읽어 작성자 스냅샷을 만든다. `shared_visible=false`여도 이 두 값은 사용할 수 있다.
- 프로필 행이 없으면 `행성 동기화 대기` 및 `masculine` 기본값을 사용하고 전송을 허용한다. 토큰·행성 통계를 조회하거나 공개하지 않는다.
- 가입 시 마지막 메시지 순번을 원자적으로 캡처한다. 기록·Realtime·보충 동기화·안 읽음 집계에는 같은 가입 경계 `message_seq > joined_after_seq`를 적용한다.
- Auth 토큰과 내부 Auth 사용자 ID를 React 응답이나 이벤트에 싣지 않는다. 공개 행의 작성자 식별자는 Auth ID와 연결할 수 없는 불투명 키로 둔다.
- 읽음 커서는 사용자·그룹별 서버 비공개 데이터다. 다른 멤버의 읽음 상태는 제공하지 않는다.
- 실패 초안은 보존하고 자동 전송하지 않는다. 재시도는 같은 요청 ID를 사용한다.
- 삭제는 본문을 비우는 tombstone UPDATE다. 탈퇴는 과거 메시지를 보존하고 그룹 삭제는 관련 채팅 데이터를 함께 삭제한다.
- 그룹 탭이 보이는 동안 구독하고, 채팅 패널만 닫을 때는 구독을 유지한다. 그룹 탭 이탈·앱 종료에는 구독을 해제한다. OS·푸시 알림은 추가하지 않는다.
- 공유 일시정지·집계 삭제는 채팅 읽기·전송을 막지 않는다.
- 정상 연결에서 서버 저장 후 상대 화면 표시 목표는 P95 1초 이내다.
- 구현 작업은 테스트 우선 순서로 수행한다. 각 작업은 의도한 실패 확인, 최소 구현, 해당 회귀 확인을 포함한다.
- 과거 migration은 수정하지 않는다. 새 migration은 `supabase migration new`로 생성한 실제 경로를 기록한다.
- 검증에 production이나 기존 실제 사용자 데이터를 사용하지 않는다. 로그·테스트 보고서에 메시지 본문, Auth 토큰, 비밀 키를 남기지 않는다.

## 담당과 파일 책임

| 역할 | 책임 |
| --- | --- |
| CEO | 승인된 범위 조율, 작업공간·검증 대상 준비, 담당 배정, 문서 통합, 최종 수용. 구현·설정·테스트 코드 직접 수정은 하지 않는다. |
| Coder | 승인된 DB migration·테스트 코드·Rust·Tauri·React·로컬 설정·운영 문서 수정과 focused 검증. 작업별 파일 경계를 따른다. |
| QA | 독립된 local/staging 대상에서 DB/API/Realtime/앱 수용 기준을 검증하고 증거 보고서를 작성한다. 검증 중 제품 파일을 편집하지 않는다. |
| Reviewer | QA 이후 전체 변경의 권한 경계·동시성·개인정보·회귀 위험을 독립 검토한다. |

계획 승인 후 구현 실행을 시작할 때는 CEO가 현재 branch, HEAD, 기존 변경, 격리 대상 식별을 확인하고 별도 실행 brief를 제공한다. 승인 전에는 실행 checkout이나 외부 환경을 변경하지 않는다.

## 검토 초점

1. 가입·전송 경쟁으로 가입 전 메시지가 새 멤버에게 노출되지 않는가 — 작업 1·2·7.
2. 삭제 뒤 오래된 INSERT·재시도 응답이 본문을 복원하지 않는가 — 작업 2·4·6.
3. 위조된 프로필을 무시하고 서버 본인 프로필만 스냅샷하는가 — 작업 2·3·7.
4. 공유 정지 중에도 채팅이 되고 비공개 통계가 다시 노출되지 않는가 — 작업 2·3·7.
5. 앱 창·그룹 변경·재연결에서 중복 구독과 이전 컨텍스트 이벤트를 막는가 — 작업 4·5·6.
6. 큰 순번·Unicode·한글 조합에서 정밀도와 글자 수 검증이 일관적인가 — 작업 2·3·6.

## 선행 게이트와 순서

`작업 0 → 작업 1 → 작업 2 → 작업 3 → 작업 4 → 작업 5 → 작업 6 → 작업 7`

작업 0의 기존 계정 삭제·백업 정책 및 WebSocket 기술 확인을 마친 뒤 구현한다. 메시지 테이블의 접근 권한이나 RPC 검증이 실패하면 이에 의존하는 Rust·UI 작업을 진행하지 않는다. 최종 전체 검토는 QA가 증거를 제출한 뒤 Reviewer가 맡는다.

## 작업 0. 기존 정책·검증 환경·WebSocket 호환성 확인

**담당:** CEO 조사·결정 기록, 필요 시 Coder 기술 조사. **파일 수정:** 없음.

참조 경로: `rust-toolchain.toml`, `apps/desktop/src-tauri/Cargo.toml`, `Cargo.lock`, `src-tauri/src/sync/auth.rs`, `supabase/config.toml`, `supabase/README.md`.

- [ ] 기존 서비스의 영구 Auth 계정 삭제 경로가 있는지 확인한다. 경로가 있으면 기존 계정 삭제 정책에 맞춰 메시지 본문·작성자 스냅샷·중복 방지 매핑 처리를 기록한다. 새 계정 삭제 동작을 임의로 추가하지 않는다.
- [ ] 현재 Supabase 백업·로그 보존 정책을 확인하고 tombstone이 활성 DB·클라이언트 캐시·백업 각각에서 보장하는 범위를 기록한다. 정책을 확인할 수 없으면 그 한계를 명시한다.
- [ ] Supabase Realtime protocol·Postgres Changes의 인증 갱신·heartbeat·가입 승인·RLS·UPDATE payload 동작을 현재 공식 문서와 격리 환경에서 대조한다.
- [ ] `tokio-tungstenite`를 우선 후보로 저장소 Rust toolchain·현재 Tokio·TLS backend·macOS/Windows와 호환성을 확인한다. 버전·feature를 조사한 뒤 확정한다.
- [ ] 공개 메시지의 불투명 작성자 키, 테이블 publication, 탈퇴 중 구독, 삭제 이벤트에서 이전 본문이 노출되지 않는 조건을 확정한다.
- [ ] QA 전용 격리 대상의 절대 경로, project ID, 포트, 컨테이너·볼륨 소유권, 두 독립 사용자와 데이터 범위를 실행 brief에 기록한다. local 두 클라이언트는 같은 Supabase 인스턴스를 사용한다.
- [ ] 설치된 Supabase CLI 버전과 `--help`를 확인해 마이그레이션·테스트 명령을 확정한다. staging 데이터나 다른 Supabase 프로젝트를 대상으로 삼지 않는다.

**완료 조건:** 삭제·백업의 현재 정책과 기술 선택, 재현 가능한 격리 검증 대상이 기록되었다. 준비된 대상이 없으면 실제 검증만 차단 사유로 남긴다.

## 작업 1. 메시지 저장소·가입 경계·RLS

**담당:** Coder. **생성 파일:** CLI로 생성한 `supabase/migrations/*_group_chat_storage.sql`, `supabase/tests/group_chat_access.sql`, `supabase/tests/group_chat_membership.sql`.

**논리 저장 계약:**

| 데이터 | 책임 |
| --- | --- |
| `private.group_chat_state` | 그룹별 `last_message_seq`, `last_change_seq`를 직렬화해 관리한다. |
| `private.group_chat_authors` | 불투명 작성자 키와 실제 Auth 사용자 간 서버 전용 매핑을 둔다. |
| `public.group_chat_messages` | ID, 그룹, 메시지·변경 순번, 불투명 작성자 키, 작성 당시 표시명·아바타, 본문, 생성·삭제 시각을 둔다. Auth 사용자 ID는 저장하거나 Realtime에 내보내지 않는다. |
| `private.group_chat_requests` | 그룹·작성자·요청 ID의 중복 방지 상태와 본문 일치 검증용 digest를 보관한다. 요청 본문을 복제 저장하지 않는다. |
| `private.group_chat_reads` | 사용자·그룹별 단조 증가 읽음 순번을 비공개로 보관한다. |
| `world_members.joined_after_seq` | 현재 멤버십이 접근할 수 있는 첫 메시지 순번을 보관한다. |

순번은 DB `bigint`, JSON 경계에서는 10진 문자열로 직렬화한다. 그룹 생성 때 채팅 상태 행을 준비하고 기존 그룹은 migration에서 채운다. 기존 멤버의 초기 경계는 0으로 설정한다. 새 멤버십은 그룹별 마지막 메시지 순번을 원자적으로 캡처한다. 메시지 작성 순번과 INSERT·tombstone UPDATE 변경 순번은 분리한다.

- [ ] 비멤버·다른 그룹·가입 경계 전후·탈퇴·재가입·그룹 삭제에 대한 pgTAP 테스트를 먼저 작성하고 격리 DB에서 의도한 실패를 확인한다.
- [ ] 새 그룹과 기존 그룹의 상태 행 초기화, `(world_id, message_seq)` 고유성 및 변경 순번 커서용 인덱스를 구현한다.
- [ ] 그룹 멤버십과 `joined_after_seq`를 확인하는 SELECT RLS를 구현한다. 첫 생성·초대 승인·재가입 경로 모두 경계를 정확히 설정한다.
- [ ] `authenticated`의 직접 INSERT·UPDATE·DELETE를 거부하고, private 매핑·요청·읽음 행의 직접 REST 접근도 제한한다. 쓰기는 다음 작업의 제한 RPC만 사용한다.
- [ ] Auth 사용자 ID는 private 매핑에만 두고 public 메시지에는 불투명 키를 사용한다. public 메시지 행에서 직접 식별 가능한 Auth ID가 나오지 않도록 컬럼 grants와 RLS를 확인한다.
- [ ] 멤버십 삭제에는 메시지가 cascade되지 않게 하고 그룹 삭제에는 메시지·상태·요청·읽음 데이터가 함께 cascade되게 한다.
- [ ] 영구 계정 삭제 FK 동작은 작업 0에서 확인한 기존 정책에 맞춘다.
- [ ] 새 suite와 기존 `world_membership.sql`, `world_lifecycle.sql`, invite suite를 실행한다.

**산출물:** 직접 DB 조회에도 가입 경계를 적용하고 탈퇴·그룹 삭제 정책을 보장하는 저장 구조.

## 작업 2. 서버 프로필 스냅샷·송신·삭제·읽음 RPC

**담당:** Coder. **생성 파일:** CLI migration `*_group_chat_rpc.sql`, `supabase/tests/group_chat_rpc.sql`, `supabase/tests/group_chat_concurrency.sh`, `supabase/tests/test_group_chat_target_guard.py`.

| RPC | 요청·응답 계약 |
| --- | --- |
| `get_group_chat_context` | 그룹 ID를 받고 가입 경계, 본인 불투명 작성자 키, 읽음·최신·변경 순번, 안 읽음 수를 반환한다. |
| `list_group_chat_messages` | 그룹·이전 메시지 순번·limit를 받고 최대 50개와 다음 커서를 반환한다. |
| `sync_group_chat_changes` | 그룹·마지막 변경 순번·상한·limit를 받고 접근 가능한 현재 메시지 상태와 다음 변경 커서를 반환한다. |
| `send_group_chat_message` | `p_world_id`, `p_request_id`, `p_body`만 받는다. 서버 확정 메시지를 반환한다. |
| `delete_group_chat_message` | 그룹 ID·메시지 ID를 받고 본인 메시지를 tombstone 처리한다. |
| `mark_group_chat_read` | 그룹 ID·확인한 메시지 순번을 받고 본인 읽음 커서와 안 읽음 수를 반환한다. |

**프로필 경계:** 전송 RPC는 `auth.uid()`에 해당하는 `planet_member_state`의 `nickname`, `avatar`만 선택한다. `shared_visible=true` 조건을 적용하지 않고 `get_world_planets`를 호출하지 않는다. 프로필 행이 없으면 `행성 동기화 대기`·`masculine`을 저장한다. 통계·지갑·행성 상태를 읽거나 변경하지 않는다.

**재연결 경계:** context가 반환한 `last_change_seq`를 상한으로 삼아 보충 페이지를 끝까지 읽는다. INSERT와 tombstone UPDATE는 모두 변경 순번을 증가시킨다. 조회 도중 변경되어 상한보다 큰 순번을 받은 메시지는 다음 보충 주기에 회수한다.

- [ ] 2,000/2,001 코드 포인트, supplementary Unicode, 빈 본문, 공백, 한글 조합을 포함한 서버 검증 테스트를 작성한다.
- [ ] 프로필·작성자 인자를 주입한 요청을 거부하고 해당 인자를 받는 송신 overload가 없음을 확인한다. 저장 스냅샷이 `auth.uid()` 본인 값인지 검증한다.
- [ ] `shared_visible=false`에서도 본인 nickname/avatar 스냅샷으로 전송되고, 공유 상태·통계 응답은 바뀌지 않는지 검증한다.
- [ ] 프로필이 없는 인증 멤버도 기본 스냅샷으로 보낼 수 있고, 프로필 변경 뒤 새 메시지만 새 스냅샷을 갖는지 검증한다.
- [ ] 같은 요청 ID·같은 본문 재시도 멱등성, 같은 ID·다른 본문 거부, 타인 메시지 삭제 거부, 가입 전 데이터 접근 거부를 검증한다.
- [ ] 미래 읽음 순번과 커서 후퇴를 거부한다. 본인·삭제 메시지는 안 읽음 수에서 제외한다.
- [ ] 삭제 뒤 동일 요청 재시도로 본문이 복원되지 않고 반복 삭제가 멱등적인지 검증한다.
- [ ] 독립 DB 세션에서 가입/전송·탈퇴/전송·동일 요청 경쟁을 재현한다. concurrency harness는 승인된 격리 대상만 허용한다.
- [ ] 테스트의 의도한 실패를 확인한 뒤 인증·가입 경계·프로필 스냅샷·순번·중복 방지·삭제·읽음 RPC를 구현하고 DB 회귀를 실행한다.

**산출물:** 클라이언트가 제시한 프로필을 신뢰하지 않는 서버 권한 RPC.

## 작업 3. Rust 타입·HTTP RPC 계층

**담당:** Coder. **파일:** 새 `apps/desktop/src-tauri/src/chat/{mod.rs,types.rs,client.rs}` 및 `src-tauri/src/lib.rs` 등록.

타입은 `ChatMessage`, `ChatContext`, `ChatPage`, `ChatChangePage`, `ChatError`, `ChatScope`를 포함한다. 표시명·아바타는 서버 응답에서만 온다. API 메서드는 context, list, sync, send, delete, mark-read를 제공한다. 송신은 그룹 ID·요청 ID·본문과 내부 인증만 전달한다.

- [ ] 모의 HTTP RPC 테스트를 작성해 RPC 이름·요청 필드·인증 헤더·응답 검증을 고정한다.
- [ ] 송신 JSON에 프로필·nickname·avatar·작성자 ID가 없고 기본 프로필 서버 응답을 정상 수용하는지 테스트한다.
- [ ] 순번 문자열·아바타 enum·그룹·본문 상태 및 서버 오류 매핑을 테스트한다.
- [ ] 의도한 실패를 확인한 뒤 기존 AuthConfig·키링 세션 갱신을 재사용하는 제한 클라이언트를 구현한다.
- [ ] 로컬 profile 읽기·전달·전송 차단 경로를 만들지 않는다. `shared_visible`이나 사용량 업로드도 변경하지 않는다.
- [ ] focused Rust 테스트와 기존 Auth·RPC 회귀를 실행한다.

**산출물:** 서버 작성자 스냅샷을 그대로 사용하는 Rust RPC 계층.

## 작업 4. Rust Postgres Changes WebSocket·재연결

**담당:** Coder. **파일:** 새 `protocol.rs`, `realtime.rs`, `state.rs`; 수정 `chat/mod.rs`, `Cargo.toml`, `Cargo.lock`.

`ChatRuntime`는 scope별 단일 구독을 소유한다. `start(scope)`와 `stop(scope)`는 멱등적이다. 연결 상태는 connecting·connected·reconnecting·unavailable·stopped로 표현한다.

- [ ] 모의 WebSocket에서 join 승인·거부, heartbeat, 인증 갱신, 취소·종료·재연결을 검증하는 테스트를 작성한다.
- [ ] 중복 INSERT, tombstone 뒤 늦은 INSERT, 다른 그룹·세대 이벤트, malformed JSON, 큰 순번을 테스트한다.
- [ ] 구독 시작과 최초 조회 사이 삽입, 연결 단절 중 삭제, 보충 페이지 중 변경을 테스트한다.
- [ ] 작업 0에서 선택한 의존성 및 Tokio 기능만 추가하고 lockfile을 갱신한다.
- [ ] hosted `wss`와 승인된 loopback local `ws`만 허용하고 원격 평문 `ws`를 거부한다.
- [ ] 공개 메시지 테이블의 그룹별 INSERT·UPDATE를 구독한다. hard DELETE 이벤트를 사용하지 않는다.
- [ ] 현재 Realtime protocol에 맞는 heartbeat·join timeout·토큰 갱신을 구현한다.
- [ ] 재시도는 1·2·4·8·16·30초 상한과 jitter를 사용한다. stop 요청은 대기와 연결을 즉시 취소한다.
- [ ] 구독 승인 뒤 이벤트를 버퍼링하고 context·최초 기록·변경 보충을 합친다. 재연결마다 멤버십을 다시 확인한다.
- [ ] 낮은 `change_seq`가 최신 상태를 덮지 않게 하고 tombstone 수신 즉시 본문을 제거한다.
- [ ] focused 테스트와 Rust 회귀를 실행한다.

**산출물:** 사용량 동기화 주기·장기 잠금과 독립된 Realtime 런타임.

## 작업 5. Tauri 명령·이벤트·수명 관리

**담당:** Coder. **파일:** 새 `commands/chat.rs`; 수정 `commands/mod.rs`, `commands/sharing.rs`, `lib.rs`; 필요 시 capability 파일.

명령은 `get_group_chat_context`, `list_group_chat_messages`, `sync_group_chat_changes`, `send_group_chat_message`, `delete_group_chat_message`, `mark_group_chat_read`, `start_group_chat`, `stop_group_chat`을 제공한다. 전송 명령 인자에도 프로필·작성자 필드를 두지 않는다.

- [ ] 비로그인·다른 그룹·잘못된 scope·추가 프로필 입력에 대한 명령 경계 테스트를 작성한다.
- [ ] 중복 start/stop, 다중 창 소유권, 그룹 탭 이탈, 탈퇴, 앱 종료를 검증한다.
- [ ] 공유 중지·집계 삭제 중 채팅은 가능하고 행성 상태는 변하지 않는지 테스트한다.
- [ ] `AppState` 런타임 초기화와 명령 등록을 구현한다. 네트워크·소켓 대기 중 ledger lock이나 `sync_gate`를 보유하지 않는다.
- [ ] 탈퇴 성공 시 해당 scope의 세대·구독·메모리 상태를 정리한다. 탈퇴 실패 시 현재 scope를 유지한다.
- [ ] 앱 종료·창 소멸·구독 소유자 해제를 연결하고 필요한 capability만 추가한다.
- [ ] 명령·수명 테스트 및 기존 sharing 회귀를 실행한다.

**산출물:** 토큰과 프로필 위조 입력을 노출하지 않는 Tauri 경계.

## 작업 6. React 화면·안 읽음·아바타·접근성

**담당:** Coder. **파일:** 새 `types/chat.ts`, `lib/chat.ts`, `hooks/useGroupChat.ts`, hook/component tests, `components/GroupChat.tsx`; 수정 `App.tsx`, `App.css`, 앱 테스트; 기존 `AvatarSprite.tsx` 재사용.

hook는 Tauri 명령·이벤트·scope·요청 ID·읽음을 관리한다. 컴포넌트는 메시지 표시·입력·삭제·스크롤을 관리한다. JSON 순번 문자열은 JavaScript `BigInt`로 비교한다.

- [ ] 닫힌 패널 배지, 탭 이탈 stop, 이전 scope 폐기, 이벤트/응답 중복 제거를 테스트한다.
- [ ] 일반 텍스트, 2,000 코드 포인트, 한글 조합, 실패 초안 유지, 동일 요청 ID 수동 재시도를 테스트한다.
- [ ] 서버가 반환한 표시명·기존 픽셀 아바타를 사용하고 로컬 프로필로 과거 메시지를 대체하지 않는지 테스트한다.
- [ ] 기본 프로필 스냅샷의 메시지도 표시하며 전송 UI를 막지 않는지 테스트한다.
- [ ] 본인 메시지 삭제·tombstone·반복 삭제·늦은 이벤트에서 본문이 복구되지 않는지 테스트한다.
- [ ] 최신 위치에서만 읽음 갱신, 과거 기록 조회 시 스크롤 유지, 새 메시지 표시, 안 읽음 제외 규칙을 테스트한다.
- [ ] 테스트에서 의도한 실패를 확인한 뒤 그룹 채팅 컴포넌트·hook·그룹 진입점·배지를 구현한다.
- [ ] `AvatarSprite`에 서버 아바타 snapshot과 접근성 작성자 이름을 전달한다. 업로드 사진·이미지 첨부·장비 정보는 추가하지 않는다.
- [ ] 입력·전송·삭제·이전 기록 레이블, 상태·오류 알림, 키보드 focus 표시와 복귀를 구현한다. 재접속 기록을 반복 낭독하지 않는다.
- [ ] focused Vitest, 전체 frontend 테스트, build를 실행한다.

**산출물:** 작성 당시 서버 아바타를 표시하는 접근 가능한 그룹 채팅 UI.

## 작업 7. 격리 Realtime·보안·수용 검증·문서

**담당:** Coder는 harness·운영 문서, QA는 제품 파일 수정 없이 독립 검증, 이후 Reviewer는 전체 diff 검토. **파일:** 필요 시 `supabase/config.toml`, `apps/desktop/scripts/dev-local.mjs`; `supabase/tests/group_chat_realtime.py`와 harness tests; `supabase/README.md`, `apps/desktop/README.md`; 생성 `docs/2026-10-10-group-chat-verification.md`.

현재 local 설정의 Realtime 활성화 상태를 확인하고, 데스크톱 시작 안내에서 Realtime을 제외하지 않도록 맞춘다. DB-only CI와 실제 Realtime 검증을 분리한다. `realtime` 스키마에 사용자 테이블이나 함수를 추가하지 않는다.

- [ ] 격리 대상 외 연결 거부·timeout·결과 집계·민감 정보 제거를 검증하는 harness를 작성하고 실패를 확인한다.
- [ ] 격리 harness는 독립 Auth 사용자 A·B, 신규 C, 다른 그룹 D로 모든 경로를 확인한다.
- [ ] Realtime publication에는 필요한 public 메시지 테이블만 포함한다. INSERT/UPDATE 수신에 RLS·가입 경계가 적용되는지 검사한다.
- [ ] REST 조회/쓰기, RPC, Postgres Changes 각각에서 비멤버·다른 그룹·가입 전·탈퇴 후 권한을 검사한다.
- [ ] 위조 프로필 송신을 직접 시도하고, 작성자 A의 메시지가 서버의 A 프로필 스냅샷을 사용하는지 검사한다.
- [ ] `shared_visible=false`에서 전송·표시를 확인하고 행성·토큰 통계가 다시 공개되지 않는지 확인한다. 프로필 행이 없는 사용자도 기본 스냅샷으로 보낼 수 있어야 한다.
- [ ] tombstone UPDATE의 old payload에 삭제 전 본문이 포함되지 않는지 확인한다. 원문 노출을 유발하는 `REPLICA IDENTITY FULL` 설정은 쓰지 않는다.
- [ ] 가입/송신 경쟁, 탈퇴 후 메시지 보존, 재가입 경계, 그룹 삭제 cascade, 읽음 비공개, 재시작 복구를 확인한다.
- [ ] 실제 앱의 두 독립 사용자로 송수신·아바타·삭제·공유 중지·탭 이동·수동 재시도·단절 중 tombstone 복구를 확인한다.
- [ ] 정상 연결 100개 이상의 표본에서 서버 저장부터 상대 화면 표시까지 지연을 측정해 P95 1초 목표를 판정한다. 연결 단절 표본과 측정 오차는 분리 기록한다.
- [ ] macOS·Windows 검증 환경이 있으면 TLS·키링·구독 종료를 확인한다. 없는 플랫폼은 unverified로 남긴다.
- [ ] 최종 DB suite·migration replay·Rust 전체 테스트·frontend 전체 테스트/build를 수행한다. 격리 DB runner는 저장소의 기존 guard와 수용된 대상만 사용한다.
- [ ] 운영 문서에 서버 프로필 출처·기본값·통계 분리·보관·삭제·실패 재시도·local Realtime 안내를 기록한다.
- [ ] 승인 명세의 수용 기준 1–15마다 환경·실행·관찰·증거 위치·proven/unmet/unverified 판정을 기록한다. 사용자 데이터·본문·토큰은 보고서에 복사하지 않는다.
- [ ] QA 완료 뒤 Reviewer가 전체 변경을 검토하고, 발견 사항 수정 후 영향받은 검증을 다시 실행한다.

**완료 조건:** 기준별 QA 증거와 Reviewer 결과를 CEO가 검토·수용했다. 준비되지 않은 OS나 외부 운영 정책 범위는 검증 완료로 주장하지 않는다.

## CEO 자체 검토

- 승인 명세의 텍스트 전용, 픽셀 아바타, 서버 안 읽음 커서, 재연결 복구, P95 1초 및 탈퇴·삭제 정책을 작업 1–7에 연결했다.
- `p_profile`이나 로컬 프로필 전달 경로가 없다. 전송 인자는 그룹·요청 ID·본문이며 서버가 `auth.uid()`의 nickname/avatar만 스냅샷한다.
- `shared_visible=false`와 프로필 부재 모두 전송을 허용하고 토큰·행성 통계 공개를 추가하지 않는다.
- 작성자 Auth ID는 private 매핑에 한정하고 Realtime 메시지에는 불투명 키만 둔다.
- 메시지 순번과 변경 순번을 분리해 tombstone 재연결 복구를 포함했다.
- 가입·송신 경계와 중복 요청 경쟁, 직접 RLS·RPC·Realtime 경로를 따로 검증한다.
- 계정 삭제와 백업 보존은 기존 서비스 정책을 확인하는 게이트로 남겼고 새로운 삭제 정책을 임의 생성하지 않는다.
- CEO는 문서 통합·조율·수용만 담당하며 Coder가 구현·테스트·설정 파일을 수정한다.
- 현재 수행된 것은 설계·계획 문서 작성뿐이다. 구현 파일 변경, 테스트 실행, 외부 환경 변경은 없다.
