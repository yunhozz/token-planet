# Token Planet Menubar Popover Implementation Plan

> **For agentic workers:** Follow the execution method confirmed at handoff. When lean-work-harness is active, route code and test-code edits through that skill. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the personal planet in a macOS menu bar popover and switch between it and the existing detailed window.

**Architecture:** Reuse the single `main` Tauri webview and its React state. Rust owns native window mode, placement and visibility; React owns popover content and detail tabs. A small pure geometry function keeps placement testable without an OS window.

**Tech Stack:** Tauri 2/Rust, React 19/TypeScript, existing CSS, Rust unit tests, Vitest/Testing Library.

**Spec:** `docs/superpowers/specs/2026-09-29-token-planet-menubar-popover-design.md`

## Global Constraints

- Keep the existing dark pixel planet style and the data meanings; do not add the plant-game features or light palette from the reference image.
- Keep one `main` webview. The detail target remains approximately 960×700, and the popover targets 390–420px width within the available display.
- Preserve the existing `내 행성` and `그룹` detail functions, growth rules, collection, sharing API, shop and storage.
- The profile setup uses a normal window. A saved profile starts in popover mode.
- macOS menu bar anchoring is the primary acceptance target. Windows tray behavior must remain functional and needs native verification on Windows.

## Review Focus

1. Icon near a display edge or on a monitor with a nonzero origin: popover remains entirely on that monitor. Pin with Task 1 geometry tests.
2. Short display or large display scale: popover height fits and its body scrolls. Pin with Task 1 geometry test and Task 3 layout check.
3. Fast icon/detail/return clicks and loss of focus during a mode change: one visible mode remains; detail does not vanish on blur. Pin with Task 2 transition tests and native check.
4. Profile missing on first launch: setup stays a normal window, then the saved profile opens the popover. Pin with Task 2 mode test and Task 3 UI test.
5. Unavailable or incomplete usage: popover shows unknown/error separately from zero and the detail button remains usable. Pin with Task 3 UI test.

---

## File Structure

- Create `apps/desktop/src-tauri/src/platform/popover.rs`: physical-screen bounds and pure popover placement.
- Modify `apps/desktop/src-tauri/src/platform/mod.rs`: expose the new platform module.
- Modify `apps/desktop/src-tauri/src/platform/tray.rs`: tray click and icon-rectangle based popover opening.
- Modify `apps/desktop/src-tauri/src/lib.rs`: native window modes, startup/setup selection, detail command and focus behavior.
- Modify `apps/desktop/src-tauri/tauri.conf.json`: prevent a default centered window flash before mode selection, if required by the native startup path.
- Modify `apps/desktop/src/App.tsx`: popover information order, mode transition feedback and Escape handling.
- Modify `apps/desktop/src/App.css`: dark popover shell, constrained height and body scrolling.
- Modify `apps/desktop/src/__tests__/App.test.tsx`: React transition, error and content checks.

### Task 1: Popover placement

**Files:** Create `apps/desktop/src-tauri/src/platform/popover.rs`; modify `apps/desktop/src-tauri/src/platform/mod.rs`.

**Interfaces:** `pub(crate) struct Bounds { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }`; `pub(crate) fn physical_icon_bounds(rect: tauri::Rect, scale_factor: f64) -> Bounds`; `pub(crate) fn popup_bounds(icon: Option<Bounds>, screen: Bounds, preferred_width: u32, preferred_height: u32) -> Bounds`. Placement inputs and result are physical pixels. Place below the icon with an 8px gap, center horizontally on it, clamp to the chosen screen with a 12px inset, and shorten height to fit. If the icon rectangle is absent, use the top center of the chosen screen.

- [ ] **Step 1: Write failing Rust tests** named `places_below_icon` (1440×900 screen, icon at 1000,0 sized 24×24, preferred 400×700 → origin 812,32), `clamps_right_edge` (icon x=1420 → popup x=1028), `respects_monitor_origin` (screen x=1440 → result x≥1452), `centers_when_icon_missing` (same 1440px screen → x=520, y=12), `shrinks_on_short_screen` (500px screen, icon bottom=24 → height=456), and `converts_logical_rect_at_2x` (logical icon x=100,width=24 → physical x=200,width=48).
- [ ] **Step 2: Run** `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml platform::popover::tests --lib`; expect the new tests to fail.
- [ ] **Step 3: Implement** `Bounds`, `physical_icon_bounds` and `popup_bounds`. Keep Tauri window calls out of the placement function.
- [ ] **Step 4: Run the same targeted Rust test**; expect all placement cases to pass.
- [ ] **Step 5: Commit** this independent placement unit with `feat(desktop): 팝오버 위치 계산 추가`.

### Task 2: Native window modes and tray behavior

**Files:** Modify `apps/desktop/src-tauri/src/platform/tray.rs`, `apps/desktop/src-tauri/src/lib.rs`, and `apps/desktop/src-tauri/tauri.conf.json` if needed for startup visibility.

**Interfaces:** Consume Task 1 `popup_bounds`. Preserve the frontend command payload `set_detail_view({ detail: boolean })` and add `hide_popover() -> Result<(), String>` for Escape. Add `WindowMode { Popup, Detail, Setup }` and pure helpers `initial_mode(has_profile: bool) -> WindowMode`, `tray_target(mode: WindowMode, visible: bool) -> Option<WindowMode>` (`None` means hide), and `should_hide_on_blur(mode: WindowMode) -> bool`. A managed native mode state applies decorations, size, position and visibility before announcing a compact view to React. Obtain the current icon rectangle on every popover entry and convert it to physical pixels at the relevant display scale. `Detail` stays centered at the existing size; `Setup` keeps a normal 390×700 window.

- [ ] **Step 1: Write failing Rust tests** named `initial_mode_depends_on_profile` (`true` → `Popup`, `false` → `Setup`), `tray_click_toggles_popup` (`Popup,true` → `None`, `Popup,false` → `Some(Popup)`, `Detail,true` → `Some(Popup)`), and `blur_hides_popup_only` (`Popup` → true, `Detail`/`Setup` → false). Keep existing tray menu/open/quit tests.
- [ ] **Step 2: Run** `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`; expect the new mode cases to fail.
- [ ] **Step 3: Implement** mode state and native transitions in `lib.rs`/`tray.rs`: startup selection, tray toggle, `set_detail_view`, `hide_popover`, close-to-hide, and popup-only blur-to-hide. After `set_planet_profile` succeeds, move from `Setup` to `Popup`. Ensure a transition to detail cannot be hidden by its own focus event. Failure must leave the last usable mode visible.
- [ ] **Step 4: Run** `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib` and `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml`; expect success.
- [ ] **Step 5: Commit** this native behavior with `feat(desktop): 메뉴바 팝오버 창 전환 구현`.

### Task 3: Popover content and frontend transitions

**Files:** Modify `apps/desktop/src/App.tsx`, `apps/desktop/src/App.css`, `apps/desktop/src/__tests__/App.test.tsx`.

**Interfaces:** Keep `detail: boolean`, existing `show-compact` event, `set_detail_view` invocation and detail tab behavior. Consume Task 2 `hide_popover` when Escape is pressed in compact mode. The compact branch becomes the popover body: header/refresh, planet and era, current-cycle tokens and progress, `행성·그룹 자세히 보기`, then usage/source/sync status. A rejected native transition keeps the prior React mode and shows a retryable message; browser-only preview may continue without a native window.

- [ ] **Step 1: Write failing Vitest cases** named `shows_popover_summary_in_order` (planet < token/progress < detail button < source/sync), `returns_to_popup_then_reopens_personal_detail` (`그룹` then return then detail → `내 행성` selected), `retains_view_when_native_transition_fails` (rejected invoke → same mode and alert), `escape_hides_only_popup` (Escape → `hide_popover` once; detail Escape → zero calls), `opens_popover_after_profile_setup` (save profile → summary), and `keeps_unknown_usage_distinct_from_zero` (unavailable value displays unavailable state). Mock the native environment for transition failure.
- [ ] **Step 2: Run** `npm --prefix apps/desktop test -- src/__tests__/App.test.tsx`; expect the new cases to fail.
- [ ] **Step 3: Implement** the compact layout, transition feedback and Escape path. Add popover CSS with a 390–420px panel, max-height from the viewport, body scrolling, dark palette, rounded outer corners and shadow, visible keyboard focus and ≥44px primary controls. Keep the detailed layout intact.
- [ ] **Step 4: Run** the targeted Vitest file, `npm --prefix apps/desktop run build`, and `git diff --check`; expect success.
- [ ] **Step 5: Commit** this screen change with `feat(desktop): 행성 팝오버 요약 화면 구성`.

## Final Verification

- Run the focused Rust and React tests once after integration.
- Launch the macOS Tauri app and check actual menu bar anchoring, icon double-toggle, outside click, Escape, detail/return loop, small display, multi-monitor positioning and first-time setup. Check that detail stays visible when focus leaves it.
- If Windows is available, check taskbar tray open/return/quit there. Otherwise report native Windows behavior as unverified rather than inferred from compilation.
- Check `git status --short` and report commits, observed behavior and any native validation that could not be completed.
