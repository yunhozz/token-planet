# macOS 네이티브 팝오버와 스크롤 없는 요약 구현 계획

> **For agentic workers:** Follow the Yunho Harness: one assigned Coder implements the listed code files sequentially; CEO coordinates, then independent QA and a final Reviewer inspect the complete change. Steps use checkbox syntax for tracking.

**Goal:** 트레이 아이콘으로 여는 macOS 팝오버를 네이티브 반투명 패널로 표시하고, 스크롤 없이 요약을 보여 준다. 상세 보기 버튼과 상세 화면은 기존 그대로 유지한다.

**Architecture:** 기존 단일 main 웹뷰와 Popup, Detail, Setup 모드를 재사용한다. macOS 창 효과를 모드 전환에 연결해 Popup에서만 적용하고 다른 모드 진입과 복구 때 해제·복원한다. 높이에 따라 Popup의 보조 정보를 접고, 팝오버 전용 CSS를 조정해 스크롤 없이 배치한다.

**Tech Stack:** React 19, TypeScript, CSS, Vitest 5, Rust, Tauri 2.11.6.

**Spec:** 기존 설계 문서 docs/superpowers/specs/2026-09-29-token-planet-menubar-popover-design.md를 따른다. 2026-10-06에 승인된 이번 변경은 해당 문서의 어두운 픽셀 팝오버 외관과 작은 화면 스크롤 조건만 대체한다. 승인된 기준은 macOS 팝오버의 시스템 반투명 재질, 모든 높이에서 무스크롤, 높이가 부족하면 핵심 요약만 표시, 상세 화면 보존이다.

**Workspace:** /Users/ma-24-007/.codex/worktrees/macos-native-popover/token-planet

## 전역 제약

- 창 모드는 기존 Popup, Detail, Setup을 유지한다. 별도 창이나 웹뷰를 추가하지 않는다.
- 트레이 토글, show-compact, set_detail_view, hide_popover, 바깥 클릭, Escape, 포커스, 창 닫기 계약을 유지한다.
- 네이티브 재질은 macOS Popup에서만 적용한다. Setup, Detail 및 다른 운영체제의 창 모드는 기존 외관으로 유지한다.
- Tauri 창은 이미 transparent로 설정되어 있다. macOS에서 효과를 실제 제거하려면 Tauri가 이미 잠근 `window-vibrancy 0.6`을 target-specific 직접 의존성으로 노출한다. 새 패키지나 버전은 추가하지 않는다.
- Popup은 어떤 높이에서도 스크롤하지 않는다. 스크롤바만 감추거나 overflow로 핵심 내용을 자르지 않는다.
- 일반 높이에서는 기존 요약 정보와 동작을 유지한다.
- 세로 공간이 부족하면 행성, 닉네임, 현재 시대, 이번 행성 토큰, 진행도 또는 최종 시대 안내, 행성·그룹 자세히 보기 버튼만 표시한다. 보조 사용량, 수집, 동기화 정보는 기존 Detail 화면에서 확인할 수 있어야 한다.
- 화면 전환 실패 메시지와 다시 시도 동작은 Popup에서도 유지한다.
- 상세 버튼의 문구와 핸들러, 상세 JSX·스타일·스크롤·창 크기·기능을 변경하지 않는다.
- 팝오버 CSS 변경은 macOS Popup 식별자 아래에 한정한다. 공유 컴포넌트의 스타일이나 전역 색상 변수를 바꾸지 않는다.
- 기본 팝오버 크기 400×700 논리 픽셀, 위치 계산, 화면 inset, Detail/Setup 크기와 배치를 유지한다.
- 테스트 또는 화면 검증에서 최소 핵심 요약도 표시할 수 없는 극단적으로 낮은 높이가 발견되면 잘림으로 통과시키지 말고 높이와 상태를 기록해 CEO에게 보고한다.

## 검토 초점

1. Popup에서 네이티브 재질이 실제로 나타나며 Detail 또는 Setup에 남지 않는가.
2. 높이 변경에 따라 일반 요약과 핵심 요약이 전환되고, 핵심 액션과 재시도는 계속 보이는가.
3. 일반 높이에서 기존 보조 정보가 유지되고, 작은 높이에서 접은 정보가 기존 Detail에 남아 있는가.
4. 긴 닉네임, 큰 수치, 최종 시대, 오류 문구가 텍스트나 버튼을 잘라내지 않는가.
5. 작은 viewport, Retina, 화면 가장자리, 다중 모니터에서 위치와 무스크롤 조건이 유지되는가.

## 변경 파일과 책임

한 Coder가 아래 코드를 순차적으로 편집한다. 공통 기준 경로는 위 Workspace다.

- apps/desktop/src-tauri/src/platform/window_appearance.rs (생성): 창 모드별 네이티브 재질 적용·전체 해제와 rollback.
- apps/desktop/src-tauri/Cargo.toml 및 Cargo.lock: Tauri가 이미 사용하는 window-vibrancy 0.6 직접 경계만 선언.
- apps/desktop/src-tauri/src/platform/mod.rs: 새 플랫폼 모듈 선언.
- apps/desktop/src-tauri/src/lib.rs: 기존 창 모드 전환과 이전 모드 복구에 효과 적용 연결.
- apps/desktop/src/hooks/useCompactPopup.ts (생성): macOS Popup의 사용 가능 높이 구독.
- apps/desktop/src/hooks/__tests__/useCompactPopup.test.tsx (생성): 초기 높이, 변경 알림, 구독 해제 검사.
- apps/desktop/src/App.tsx: macOS Popup 식별자, 핵심 요약 변형, 보조 정보 조건부 표시.
- apps/desktop/src/App.css: macOS Popup의 시스템 패널 표현과 무스크롤 배치.
- apps/desktop/src/__tests__/App.test.tsx: 일반·핵심 요약, 재시도, Detail 복원 검사.

tray.rs, popover.rs, 공유 사용량·수집·동기화 컴포넌트와 다른 dependency 항목은 수정하지 않는다.

## 작업 1: 모드별 네이티브 재질과 복구

소비 인터페이스: 기존 WindowMode와 apply_window_mode.

생산 인터페이스: `apply(&WebviewWindow, WindowMode)`와 rollback 경계. macOS Popup은 `window_vibrancy::apply_vibrancy`로 Popover 재질, Active 상태, radius 16.0을 설정한다. Popup 재진입 전과 Detail·Setup 진입 때는 `clear_vibrancy`가 false를 반환할 때까지 기존 재질 뷰를 모두 제거한다. AppKit 호출은 Tauri `with_webview`의 main-thread closure에서 실행한다. 비 macOS 빌드는 효과가 없는 동작을 유지한다.

Tauri 2.11.6 문서의 `set_effects(None)` 설명은 플랫폼별 “가능하면” 동작이다. 잠긴 Tauri 소스에서 macOS의 None 경로는 clear 함수를 호출하지 않는다. 따라서 이미 Tauri 의존성으로 잠긴 `window-vibrancy 0.6`의 공개 `clear_vibrancy` API를 직접 사용한다. 참고: https://docs.rs/window-vibrancy/0.6.0/window_vibrancy/fn.clear_vibrancy.html

Tauri 2.11.6은 효과 적용을 메인 스레드에 전달한 뒤 내부 macOS 호출 결과를 무시한다. 따라서 전환 rollback은 전달 단계의 오류에 대응하고, 실제 재질은 macOS 화면에서 별도로 확인한다. API 호출 성공만으로 vibrancy를 검증했다고 보지 않는다.

- [ ] 새 플랫폼 모듈 테스트 popup_uses_native_popover_material을 작성한다. 기존 효과 전체 제거 후 Popup 효과를 한 번 적용하는 순서를 검사한다.
- [ ] detail_and_setup_clear_popup_material 테스트를 작성한다. Detail과 Setup이 남은 효과 뷰를 모두 제거하고 Popup 효과를 추가하지 않는지 검사한다.
- [ ] apps/desktop/src-tauri 디렉터리에서 cargo test --locked platform::window_appearance::tests --lib를 실행해 구현 전 실패를 확인한다. 실패가 테스트 누락 때문인지 확인하고 컴파일 오류를 RED 증거로 취급하지 않는다.
- [ ] 네이티브 효과 적용·전체 제거 함수와 순수 모드 dispatch 경계를 구현한다. Popup 재진입에서도 tagged native effect view가 중복되지 않도록 한다.
- [ ] apply_window_mode 전환에서 네이티브 효과를 창 표시 및 포커스 이전에 적용한다. Popup 진입 때만 설정하고 Detail·Setup에서는 제거한다.
- [ ] 기존 rollback이 실패한 target 모드 대신 이전 모드의 효과도 복원하는지 확인한다. 적용 순서를 검증할 최소 경계를 두고 failed_mode_application_restores_previous_material 테스트로 전달 단계 오류 후 이전 모드가 다시 적용되는지 검사한다. Tauri 내부 AppKit 오류는 API에서 관찰할 수 없으므로 테스트가 이를 복구한다고 주장하지 않는다.
- [ ] 새 테스트를 통과시킨 뒤 기존 트레이·창 모드·팝오버 위치의 전체 Rust 단위 검사를 실행한다.

실행 디렉터리: apps/desktop/src-tauri
검증 명령: cargo test --locked --lib, cargo check --locked.
기존 검사에는 tray_click_toggles_popup, blur_hides_popup_only, blur_before_tray_release_keeps_popup_open_to_toggle_closed, tray_click_toggles_popup_and_switches_from_detail, shrinks_on_short_screen, converts_logical_rect_at_2x가 포함된다.

## 작업 2: 높이에 따른 일반·핵심 요약

생산 인터페이스: `useCompactPopup(): boolean`. React hook은 matchMedia("(max-height: 599px)")와 실제 `.world-layout` 넘침을 감시한다. 내용 높이가 가용 높이를 초과하면 보조 정보를 접고, compact 모드에서도 행성 장면부터 줄인다. 핵심 정보까지 물리적으로 맞지 않으면 `data-popup-fit="insufficient"`와 가용·필요 높이를 기록한다. Popup을 떠나 Detail로 갈 때 내용 기반 compact 상태와 측정값을 초기화하고, 돌아올 때 전체 배치를 다시 측정한다. App.tsx의 macOS 판별은 navigator.platform.startsWith("Mac")을 사용한다. macOS가 아닌 플랫폼과 Detail에서는 compact 결과가 렌더링이나 외관에 영향을 주지 않는다.

- [ ] controllable matchMedia와 layout measurement를 사용하는 useCompactPopup.test.tsx를 먼저 작성한다. 테스트는 높이 경계, resize, cleanup, overflow 시 compact, Detail 재진입 초기화, 최소 핵심 높이 한계를 확인한다.
- [ ] apps/desktop에서 npm test -- src/hooks/__tests__/useCompactPopup.test.tsx를 실행해 구현 전 실패를 확인한다.
- [ ] 최소 hook을 구현하고 GREEN 확인한다. UI 높이 변화마다 listener를 중복 등록하지 않고 unmount 때 등록을 해제한다.
- [ ] App.tsx에서 hook을 항상 호출하고, navigator.platform.startsWith("Mac") 및 Popup 상태일 때만 compact 결과를 사용한다. 새 UI 라이브러리를 추가하지 않는다.
- [ ] macOS Popup root에만 app-shell--macos-popover 식별자를 추가한다. 낮은 높이에서는 app-shell--compact-popover를 적용한다. Detail root에는 두 식별자를 적용하지 않는다.
- [ ] compact Popup에는 핵심 행성 요약, 진행도 또는 최종 시대 안내, 자세히 보기 버튼과 전환 오류 재시도만 렌더링한다. 일반 높이의 Popup과 기존 Detail에는 사용량, 수집 상태, 동기화 정보가 유지된다.
- [ ] App.test.tsx에 shows_only_core_summary_in_short_popup 검사를 추가한다. 닉네임, 시대, 이번 행성 토큰, 진행도 또는 최종 시대 안내, 자세히 보기 버튼은 존재하고 전체 사용량, 수집 상태, 동기화 영역은 compact Popup에서 빠지는지 검사한다.
- [ ] short_popup_keeps_transition_retry를 추가해 전환 실패 메시지와 다시 시도 버튼을 확인한다.
- [ ] short_popup_opens_unchanged_detail_with_secondary_information을 추가해 상세 버튼 후 기존 상세 root와 보조 정보를 확인하고 compact 식별자가 사라지는지 검사한다.
- [ ] 일반 높이에서 기존 정보와 액션 순서를 확인하는 기존 회귀 검사를 유지한다. React 테스트 통과만으로 CSS 무스크롤을 주장하지 않는다.
- [ ] hook 및 App 검사를 실행한다.

실행 디렉터리: apps/desktop
검증 명령: npm test -- src/hooks/__tests__/useCompactPopup.test.tsx src/__tests__/App.test.tsx.

## 작업 3: macOS 패널 외관과 스크롤 제거

- [ ] 변경 전 실제 macOS Popup에서 불투명 배경과 문서·본문 overflow를 기록한다. JSDOM 또는 CSS 선언 검색을 시각 RED 증거로 쓰지 않는다. (실행 기록: worktree 앱은 1421 포트로 빌드·실행했으나 CUA 앱 inventory 누락 및 `getApp` timeout으로 실제 baseline 캡처 불가.)
- [ ] macOS Popup에만 시스템 글꼴, macOS 반투명 표면에 맞는 중립 색상, 가는 구분선, 둥근 표면과 컨트롤을 적용한다. 시스템 appearance를 강제 고정하지 않는다.
- [ ] 팝오버 전용 기존 그라데이션과 불투명 footer 배경을 제거해 Tauri 네이티브 재질이 보이게 한다. CSS 배경을 투명하게 유지하고 native radius와 모서리를 맞춘다.
- [ ] 행성 장면 크기와 여백부터 줄여 일반 400×700 viewport의 기존 정보와 액션을 맞춘다. 초기 행성 최대 크기는 160 CSS px로 두되 실제 화면 측정에 따라 조정한다.
- [ ] compact Popup에서는 행성 장면이 우선 축소되고 핵심 텍스트, 버튼, 재시도가 남는 흐름으로 배치한다. 큰 화면 내용을 transform으로 전체 축소하지 않는다.
- [ ] macOS Popup의 world-layout 내부 세로 overflow 자동 스크롤과 footer sticky 동작을 제거한다. 핵심 내용이 실제 viewport에 모두 들어오는지 확인한 뒤 무스크롤을 적용한다. 상세 화면 및 비 macOS 스타일/스크롤은 바꾸지 않는다.
- [ ] 긴 닉네임·큰 수치의 줄바꿈과 클릭 영역을 확인한다. 보조 정보는 compact Popup에서 상세 화면으로 이동하고 핵심 항목은 말줄임·숨김으로 대체하지 않는다.
- [ ] TypeScript와 Vite build를 실행한다.

실행 디렉터리: apps/desktop
검증 명령: npm run build.
예상 결과: build 성공. Build 결과만으로 네이티브 재질이나 layout overflow가 검증된 것으로 취급하지 않는다.

## 작업 4: 독립 QA와 최종 검토

위 변경은 Popup/Detail/Setup의 단일 창 수명과 네이티브 상태 전환을 함께 바꾸므로 significant-risk 흐름을 적용한다. Coder 이후 독립 QA, 최종 diff 전체에 대한 독립 Reviewer, CEO acceptance 순으로 진행한다.

QA 실행 디렉터리: apps/desktop
실행 명령: npm run tauri -- dev.

- [ ] 일반 400×700 logical viewport에서 기존 요약 정보와 모든 액션이 보이는지 확인한다.
- [ ] 400×600, 400×599, 400×456 및 가능한 더 낮은 viewport에서 일반/핵심 요약 경계와 핵심 정보, 재시도를 확인한다.
- [ ] 팝오버 및 문서 크기와 요소의 실제 사각형을 확인한다. 세로·가로 overflow가 없고 모든 핵심 항목이 viewport 안에 있어야 한다. (실행 기록: 화면 자동화가 트레이 앱에 연결되지 않아 실제 rect/overflow 검증 미완료.)
- [ ] wheel, trackpad, 키보드로 macOS Popup 내용이 움직이지 않는지 확인한다.
- [ ] 짧은 높이에서 숨긴 사용량·수집·동기화 정보를 Detail에서 확인한다. 상세 화면의 탭, 색상, 배치, 동작과 기존 스크롤이 유지되어야 한다.
- [ ] 긴 닉네임, 큰 수치, 최종 시대, 전환 실패 상태에서 핵심 내용·다시 시도를 확인한다.
- [ ] 밝고 어두운 데스크톱 배경에서 실제 네이티브 재질이 비치는지와 둥근 모서리 클리핑을 확인한다.
- [ ] Popup→Detail→Popup, Setup→Popup과 전환 실패 복구를 확인한다. 재질이 Detail·Setup에 남지 않고 Popup 복귀 시 적용되어야 한다.
- [ ] 아이콘 재클릭, 바깥 클릭, Escape, Retina 2×, 화면 양쪽 가장자리, 보조 모니터와 icon rect 누락 fallback을 확인한다.
- [ ] 실제 macOS 렌더링을 실행할 수 없으면 그 환경 한계와 미검증 기준을 CEO에게 보고하고 검증 완료로 주장하지 않는다.
- [ ] QA 후 독립 Reviewer가 최종 diff에서 native effect 누출, 모드 rollback, Detail 회귀, overflow, 플랫폼 누출을 검토한다.
- [ ] 발견된 문제는 Coder가 수정하고 영향받은 QA 확인과 Reviewer 판단을 다시 수행한다.

## 수용 완료와 선택적 commit

- [ ] macOS Popup에서 네이티브 반투명 패널이 보이고 스크롤이 없으며 핵심 요약이 잘리지 않는다.
- [ ] 일반 높이의 팝오버 정보와 액션, 낮은 높이의 compact 정책, 전환 재시도가 유지된다.
- [ ] 상세 보기 버튼으로 열린 Detail이 이전 스타일·배치·탭·기능·스크롤을 유지한다.
- [ ] 비 macOS 및 Setup 화면에 원치 않는 스타일 변경이 없다.
- [ ] 적용 위험 등급에 필요한 QA 및 Reviewer 확인을 끝내고 발견 사항을 해결 또는 명시적으로 처리한다.
- [ ] 선택적으로 commit할 경우 이 task diff만 staging하며 메시지는 다음을 사용한다.

feat(desktop): macOS 네이티브 팝오버와 높이별 핵심 요약 표시

## 실행 전제와 한계

- 600 CSS px 미만을 compact의 초기 경계, 행성 최대 160 CSS px를 초기 스타일 값으로 사용한다. 실제 viewport 측정 결과에 맞게 조정한다.
- 사용자는 짧은 화면에서 핵심 요약만 표시하고 보조 정보를 기존 상세 화면으로 옮기는 정책을 승인했다.
- 극단적으로 낮은 viewport가 핵심 텍스트와 버튼조차 물리적으로 담을 수 없으면 그 잘림을 수용하지 말고 실제 수치와 상태를 보고한다.
- `window-vibrancy::clear_vibrancy`는 Tauri와 동일한 잠금 버전 0.6.0을 사용한다. Cargo tree로 추가 package/version이 없는 것을 확인한다.
- 본 계획은 새 native appearance 코드와 테스트 구현을 포함하지 않는다. 테스트 또는 실제 앱 검증은 Coder/QA 단계에서 수행한다.

## 자체 검토

- 승인된 설계, 추가 무스크롤 요청, 작은 화면 compact 정책, 상세 화면 보존 조건이 각각 작업과 수용 기준에 포함됐다.
- 플랫폼 효과와 CSS는 macOS Popup에 한정하고 다른 OS·Setup·Detail 경계를 명시했다.
- 새 테스트 이름, 실제 테스트 명령, 코드 책임과 독립 QA·Reviewer 흐름을 지정했다.
- JSDOM과 build만으로 실제 viewport overflow 또는 native material이 증명된다고 주장하지 않았다.
- 추가 package/version이나 데이터 구조 변경은 없다. window-vibrancy 0.6은 Tauri가 이미 잠근 transitive package를 macOS target direct dependency로 노출한다.

## 실행 기록

- 기준선: `npm test` 18 files/327 passed, `cargo test --locked --lib` 380 passed/1 ignored.
- 최종 Coder 검사: `npm test` 19 files/337 passed, `cargo test --locked --lib` 383 passed/1 ignored, `cargo check --locked`, `npm run build`, `git diff --check` 성공.
- 독립 QA 최종 focused 검사: hook/App 88 passed, Rust native appearance 3 passed. `cargo tree --locked --offline -i window-vibrancy`는 기존 0.6.0 잠금 버전에 직접 의존성이 연결됨을 확인.
- 독립 Reviewer가 macOS clear 경로, 반복 Popup의 중복 effect view, content overflow 및 Detail 복귀 상태 문제를 발견했다. 수정과 회귀 검사를 통과했다.
- 앱은 Rust 빌드 후 실행됐지만 CUA에서 앱 창을 찾을 수 없었다. 실제 macOS 재질·화면별 overflow·wheel/keyboard·밝고 어두운 배경은 미검증이며, 검증 완료로 주장하지 않는다.
- `with_webview`는 closure 실행을 main thread로 전달한다. 내부 `window-vibrancy` 오류는 closure에서 로그로 남지만 dispatch 반환값만 동기 전달되므로 mode rollback으로 되돌릴 수 없다.
