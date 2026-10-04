# Planet Landscape Exploration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> 프로젝트의 Yunho Harness가 실행 방식의 상위 규칙이다. 개발 팀장이 Coder에게 전용 파일 소유권을 부여하고, QA와 Reviewer를 순차적으로 사용한다. 중첩 에이전트를 생성하지 않는다.

**Goal:** 내 행성 상세 화면에 전체 너비의 평면 풍경과 오브젝트 탐사를 제공하고 상점·성장 일지를 같은 앱 안에서 전환한다.

**Architecture:** 기존 원형 PlanetScene과 SVG 스프라이트를 재사용하면서 평면 풍경을 별도 컴포넌트로 구현한다. 결정적 배치와 종횡비를 보존하는 카메라 계산을 순수 함수로 분리하고, App이 화면 및 계정/세계/행성별 탐사 상태를 관리한다. 기존 상점·일지 요청 보호를 보존한다.

**Tech Stack:** React 19, TypeScript, SVG, CSS, Tauri 2, Vitest, Testing Library; 새로운 런타임 의존성 없음.

**Spec:** `docs/superpowers/specs/2026-09-30-planet-landscape-exploration-design.md`

## Global Constraints

- 기존 픽셀 아트, 어두운 배경, 민트·금색 중심의 시각 언어를 따른다.
- 홈과 그룹 카드의 원형 `PlanetScene`을 유지하고 내 행성 상세 화면에 별도 평면 풍경 컴포넌트를 사용한다.
- 저장된 성장 오브젝트, 성장 점수, 시대, 토큰, 장식 구매·장착 데이터 모델을 변경하지 않는다.
- 평면 풍경의 풀·지면 무늬 등 장식은 화면 표현용이다. 성장 오브젝트 수, 생성 기록, 토큰에 합산하지 않는다.
- 별도 렌더링 엔진이나 라우팅 라이브러리 없이 기존 React·SVG 구조를 확장한다.
- 사용자가 수정한 `docs/superpowers/.DS_Store`는 이번 변경에 포함하지 않는다.
- 풍경은 콘텐츠 영역의 좌우 끝까지 사용한다. 카드 여백·테두리·최대 너비 제한을 풍경에 적용하지 않는다.
- 픽셀 스프라이트를 가로 또는 세로로 늘이지 않는다. `preserveAspectRatio="none"`을 사용하지 않는다.
- 저장 ordinal은 0부터 시작한다(`src-tauri/src/growth.rs`의 counts..targets 범위). 사용자에게는 `ordinal + 1`을 표시한다. 생성 날짜는 표시하지 않는다.

## Review Focus

1. 계정·공동 세계·사이클 전환 중 늦은 요청 응답: 이전 정보와 탐사 상태가 현재 계정에 섞이지 않아야 한다. Task 5의 지연 응답 검증이 담당한다.
2. 좌표가 겹친 다수 오브젝트와 입력 순서 변화: 모든 항목의 위치와 선택 경로가 안정적이며 숨겨진 `+N` 묶음이 없어야 한다. Task 1 및 3의 배치·목록 검증이 담당한다.
3. 가로/세로 창 및 확대 후 크기 변경: 스프라이트 비율이 유지되고 카메라가 유효 범위 안에 있어야 한다. Task 2 및 3의 카메라·resize 검증이 담당한다.
4. 상점을 떠날 때 미리보기·실패·진행 중 구매: 미리보기는 해제되고 확정 장착만 유지되며 오류는 해당 화면에 표시돼야 한다. Task 4 및 5가 담당한다.
5. 내부 화면과 실제 창 식별자, 키보드 이동: 이벤트 발신자는 `main`이며 내부 화면으로 바뀌지 않고 화면 전환·목록·풍경 조작에 키보드로 접근해야 한다. Task 3, 4, 5 및 6이 담당한다.

## File Ownership and Order

각 구현 작업은 Coder 한 명이 아래 파일을 전용 소유한다. QA와 Reviewer는 읽기·실행만 수행한다. 순서는 Task 1 → 2 → 3 → 4 → 5 → 6이며 후속 작업은 앞 작업의 인터페이스를 사용한다. Task 6은 QA가 동작 확인을 맡고, actionable finding은 팀장이 Coder에게 전달한다.

| 파일 | 책임 |
| --- | --- |
| `apps/desktop/src/components/PlanetObjectSprite.tsx` | 기존 오브젝트 픽셀 SVG를 그대로 공유 |
| `apps/desktop/src/components/planetLandscapeLayout.ts` | 안정적 ID, 충돌 없는 배치 및 경계 |
| `apps/desktop/src/components/planetLandscapeCamera.ts` | 종횡비 보존, fit/focus/zoom/pan/clamp |
| `apps/desktop/src/components/PlanetLandscapeDecorations.tsx` | 시대별 지면과 장착 장식 표현 |
| `apps/desktop/src/components/PlanetLandscape.tsx` | SVG 풍경, 조작·목록·선택 카드 |
| `apps/desktop/src/components/PlanetScene.tsx` | 기존 스프라이트를 import로 대체; 원형 동작 보존 |
| `apps/desktop/src/App.tsx` | 풍경 연결, 내부 화면, 컨텍스트별 상태 및 요청 보호 |
| `apps/desktop/src/App.css` | 전체 너비·반응형 풍경 및 내부 화면 스타일 |
| `apps/desktop/src/lib/featureWindows.ts` | 호출 제거 후 실제 미사용이면 제거; 창 생성 기능의 재사용 금지 |
| `apps/desktop/src/components/__tests__/planetLandscapeLayout.test.ts` | 순수 배치 검증 |
| `apps/desktop/src/components/__tests__/planetLandscapeCamera.test.ts` | 순수 카메라 검증 |
| `apps/desktop/src/components/__tests__/PlanetLandscape.test.tsx` | 풍경 조작·목록·장식 검증 |
| `apps/desktop/src/components/__tests__/PlanetScene.test.tsx` | 기존 원형/장식 회귀 확인 |
| `apps/desktop/src/__tests__/App.test.tsx` | 앱 전환과 계정/요청 보호 검증 |

### Task 1: Deterministic layout and shared object sprite

**Files:** Create `PlanetObjectSprite.tsx`, `planetLandscapeLayout.ts`, `__tests__/planetLandscapeLayout.test.ts` under `apps/desktop/src/components/`; modify `PlanetScene.tsx` in that directory.

**Interfaces:**

```ts
import type { PlanetObject } from "../types/usage";
export type LandscapeBounds = { x: number; y: number; width: number; height: number };
export type LandscapePlacement = {
  id: string; object: PlanetObject; x: number; y: number;
  bounds: LandscapeBounds;
};
export type LandscapeLayout = { bounds: LandscapeBounds; objects: LandscapePlacement[] };
export function landscapeObjectId(object: PlanetObject): string;
export function layoutLandscape(objects: readonly PlanetObject[]): LandscapeLayout;
export function PlanetObjectSprite(props: {
  object: PlanetObject; x: number; y: number; scale: number;
}): React.JSX.Element;
```

- [ ] **Step 1: Add layout acceptance tests with collocated fixtures.**

```ts
const objects = Array.from({ length: 120 }, (_, ordinal) => ({
  stage: ordinal % 5, ordinal, kind: "house", x: 50, y: 50, seed: ordinal,
}));
it("keeps every saved object with stable separated bounds", () => {
  const before = JSON.stringify(objects);
  const result = layoutLandscape(objects);
  expect(result.objects).toHaveLength(objects.length);
  expect(new Set(result.objects.map(item => item.id)).size).toBe(objects.length);
  expect(layoutLandscape([...objects].reverse())).toEqual(result);
  for (let i = 0; i < result.objects.length; i++) {
    const a = result.objects[i].bounds;
    for (const item of result.objects.slice(i + 1)) {
      const b = item.bounds;
      const overlaps = a.x < b.x + b.width && a.x + a.width > b.x
        && a.y < b.y + b.height && a.y + a.height > b.y;
      expect(overlaps).toBe(false);
    }
  }
  expect(JSON.stringify(objects)).toBe(before);
});
it("gives an empty planet a finite landscape", () => {
  const result = layoutLandscape([]);
  expect(result.objects).toEqual([]);
  expect(result.bounds.width).toBeGreaterThan(0);
  expect(result.bounds.height).toBeGreaterThan(0);
});
```

- [ ] **Step 2: Run the focused failing check.** From `apps/desktop`: `npm test -- src/components/__tests__/planetLandscapeLayout.test.ts`; expected missing module/export failure before implementation.
- [ ] **Step 3: Implement stable placement and extract existing art.** Move `ObjectSprite` unchanged to the shared file, rename/export it, and import it in `PlanetScene`. Sort a copy by `(stage, ordinal)`; compute `id = `${object.stage}-${object.ordinal}``; derive preferred cells from x/y/seed; resolve occupancy in a stable row/column search and enlarge terrain when cells run out. Reserve bounds large enough for the largest shared sprite including roofs and trunks, plus spacing. Return all placements; no slicing, tile cap or ledger mutation. Freeze visual placement against input permutation; preserve occupied positions when new generated objects append in stage/ordinal order. Use finite terrain defaults for empty input.

```ts
export function landscapeObjectId(object: PlanetObject) {
  return `${object.stage}-${object.ordinal}`;
}
const ordered = [...objects].sort((a, b) => a.stage - b.stage || a.ordinal - b.ordinal);
// Place each ordered object in its first free preferred/search cell.
// Bounds are sprite footprint plus the spacing chosen from existing SVG extents.
```

Add an assertion that extending a fixture with a later generated object preserves earlier positions. Existing persisted IDs are unique within a cycle; do not invent duplicate growth records.
- [ ] **Step 4: Run layout and existing PlanetScene tests.** `npm test -- src/components/__tests__/planetLandscapeLayout.test.ts src/components/__tests__/PlanetScene.test.tsx`; expected all pass and existing cosmetic sprites unchanged.
- [ ] **Step 5: Commit only task files.** `git commit -m "feat(planet): 평면 풍경 오브젝트 배치 추가"` after staging explicit listed paths.

### Task 2: Aspect-preserving camera and full-width landscape artwork

**Files:** Create `planetLandscapeCamera.ts`, `PlanetLandscapeDecorations.tsx`, `PlanetLandscape.tsx`, `__tests__/planetLandscapeCamera.test.ts`; modify `App.css`.

**Interfaces:** Consumes Task 1 layout and sprite. Produces the camera functions and decoration component below; Task 3 completes the controlled landscape component.

```ts
export type LandscapeViewport = { width: number; height: number };
export type LandscapeCamera = { centerX: number; centerY: number; zoom: number };
export function fitLandscape(bounds: LandscapeBounds): LandscapeCamera;
export function landscapeViewBox(bounds: LandscapeBounds, viewport: LandscapeViewport,
  camera: LandscapeCamera): LandscapeBounds;
export function clampLandscapeCamera(bounds: LandscapeBounds, viewport: LandscapeViewport,
  camera: LandscapeCamera): LandscapeCamera;
export function zoomLandscape(bounds: LandscapeBounds, viewport: LandscapeViewport,
  camera: LandscapeCamera, factor: number): LandscapeCamera;
export function focusLandscape(bounds: LandscapeBounds, viewport: LandscapeViewport,
  camera: LandscapeCamera, target: LandscapeBounds): LandscapeCamera;
export function PlanetLandscapeDecorations(props: {
  stage: number; bounds: LandscapeBounds; viewBox: LandscapeBounds;
  equippedCosmetics: EquippedCosmetic[];
}): React.JSX.Element;
```

- [ ] **Step 1: Write geometry tests.** Exercise square, wide and tall viewport inputs; assert equal world units per screen unit, fit coverage, clamp after resize and limits on invalid/nonfinite inputs.

```ts
it.each([{ width: 1200, height: 400 }, { width: 360, height: 500 }])(
  "fits without stretching", viewport => {
    const bounds = { x: 0, y: 0, width: 900, height: 350 };
    const box = landscapeViewBox(bounds, viewport, fitLandscape(bounds));
    expect(box.width / viewport.width).toBeCloseTo(box.height / viewport.height);
    expect(box.width).toBeGreaterThanOrEqual(bounds.width);
    expect(box.height).toBeGreaterThanOrEqual(bounds.height);
  });
```

Also zoom in, supply an off-terrain center, clamp, and assert the viewBox lies within terrain on axes where it is smaller than terrain; on larger axes assert it is centered. Test zero measured size using a finite fallback until ResizeObserver supplies positive dimensions.
- [ ] **Step 2: Run failing camera tests.** `npm test -- src/components/__tests__/planetLandscapeCamera.test.ts`; expected missing exports.
- [ ] **Step 3: Implement camera math and scenery.** Use center/zoom with zoom 1 meaning full fit. Given viewport aspect ratio, expand one dimension of the world bounds to the same ratio, then divide both dimensions by zoom. Clamp center independently on each axis; if the view is larger than terrain on an axis, center that axis. Normalize nonfinite inputs before calculations. Define maximum zoom from legible sprite size and validate it in tests instead of coupling it to arbitrary pixel coordinates.

```ts
const aspect = viewport.width / viewport.height;
const fitWidth = Math.max(bounds.width, bounds.height * aspect);
const fitHeight = fitWidth / aspect;
const width = fitWidth / camera.zoom;
const height = fitHeight / camera.zoom;
// Return a centered and clamped viewBox using these dimensions.
```

Render SVG with `preserveAspectRatio="xMidYMid meet"`, `shapeRendering="crispEdges"` and the computed viewport-ratio viewBox. Paint background sky and ground across the full viewBox so fit padding shows scenery rather than blank bars. Render deterministic surface dressing and era roads; keep dressing out of actual-object IDs/list. Apply all recognized existing cosmetic styles: sky → sky, surface/forecourt → ground, ring → a recognizable planet emblem in sky. Keep `AvatarSprite` intrinsic size and ground placement. No random render-time positions.

```css
.personal-panel .planet-landscape { width: 100%; max-width: none; border: 0; padding: 0; }
.planet-landscape-viewport { width: 100%; overflow: hidden; }
.planet-landscape-svg { display: block; width: 100%; height: 100%; image-rendering: pixelated; }
```

The personal panel must sit outside the detail content max-width wrapper so the scene reaches available left/right edges; constrain only the text sections below it. Size the viewport responsively; avoid copying the old personal-hero side-by-side summary columns.
- [ ] **Step 4: Run geometry checks and build.** `npm test -- src/components/__tests__/planetLandscapeCamera.test.ts` then `npm run build`; expected pass with no TS errors. Artwork appearance is checked in Task 6, not inferred from build.
- [ ] **Step 5: Commit explicit task files.** `git commit -m "feat(planet): 전체 너비 평면 풍경과 카메라 추가"`.

### Task 3: Exploration controls, selection and persistent parent state

**Files:** Modify `PlanetLandscape.tsx`, `App.tsx`, `App.css`; create `__tests__/PlanetLandscape.test.tsx` under components.

**Interfaces:** Consumes Tasks 1–2. App owns controlled state; viewport dimensions remain scene-local.

```ts
export type PlanetExplorationState = {
  camera: LandscapeCamera; selectedObjectId: string | null;
};
export type PlanetLandscapeProps = {
  stage: number; progress: number; avatar: PlanetAvatar; objects: PlanetObject[];
  equippedCosmetics: EquippedCosmetic[]; incomplete: boolean; cycleId: string;
  exploration: PlanetExplorationState;
  onExplorationChange: (state: PlanetExplorationState) => void;
};
export function PlanetLandscape(props: PlanetLandscapeProps): React.JSX.Element;
```

- [ ] **Step 1: Add component interaction tests with a controlled wrapper.** Use a wrapper with useState and `fitLandscape(layoutLandscape(objects).bounds)`. For fixtures with saved ordinal 0 and 3, assert list selection focuses the object and displays `1번째`/`4번째`, mapped name and era; no inferred date is shown. Render many collocated objects and assert one accessible list button and one scene hit target per saved ID. Empty input shows an empty-list explanation. Equip all recognized cosmetics and check their data-cosmetic markers without increasing the actual-object count.

```tsx
await user.click(screen.getByRole("button", { name: /바위.*1번째/ }));
expect(screen.getByRole("region", { name: "선택한 오브젝트" })).toHaveTextContent("1번째");
expect(container.querySelectorAll("[data-landscape-object-id]")).toHaveLength(objects.length);
expect(screen.queryByText(/^\+\d/)).not.toBeInTheDocument();
```

Test zoom buttons, 전체 보기, arrow keys, pointer drag, pointercancel, and a mocked ResizeObserver callback after zoom. Stub reduced-motion media query and assert no smooth camera transition is requested. On rerender removing a selected object, expect selectedObjectId null.
- [ ] **Step 2: Run the focused failing component tests.** `npm test -- src/components/__tests__/PlanetLandscape.test.tsx`.
- [ ] **Step 3: Implement the controlled scene and App integration.** Replace only the personal-detail PlanetScene with PlanetLandscape; keep home/group usage untouched. Use SVG viewBox space for positions and screen delta-to-world conversion for drag. Use pointer capture/release and pointercancel cleanup. Object click/list selection calls `focusLandscape`; render selection card with `objectName`, `STAGE_NAMES`, `ordinal + 1`. Buttons expose explicit accessible labels; actual object targets are keyboard operable. Prevent arrow-key page scroll only while the focused scene handles that key; list buttons keep native keyboard activation. Keep selected highlighting and list synchronized by ID.

```ts
const selected = layout.objects.find(item => item.id === exploration.selectedObjectId);
function selectObject(item: LandscapePlacement) {
  onExplorationChange({
    selectedObjectId: item.id,
    camera: focusLandscape(layout.bounds, viewport, exploration.camera, item.bounds),
  });
}
// Inspector order text: `${selected.object.ordinal + 1}번째`.
```

ResizeObserver reclamps the camera with the new viewport; maintain aspect ratio. Store exploration in App independently of the currently rendered internal screen. On same-context object updates, preserve valid selection and camera, and clear only missing IDs. Render summary/ledger below the full-width scene. Narrow layouts wrap controls and place inspector/list below without horizontal page overflow.
- [ ] **Step 4: Run relevant component tests and build.** `npm test -- src/components/__tests__/PlanetLandscape.test.tsx src/components/__tests__/PlanetScene.test.tsx` then `npm run build`.
- [ ] **Step 5: Commit task files.** `git commit -m "feat(planet): 오브젝트 선택과 풍경 탐사 연결"`.

### Task 4: In-app shop and journal navigation

**Files:** Modify `App.tsx`, `App.css`, `src/__tests__/App.test.tsx`; remove `lib/featureWindows.ts` only after repository-wide usage search confirms it is unused.

**Interfaces:** Internal screen type is `"planet" | "cosmetic-shop" | "growth-journal"`. It is separate from `desktopEventSource: "main"`, which is the actual current window identity. Legacy `?window=` routes may select the initial internal screen but do not create a window or change desktopEventSource.

- [ ] **Step 1: Extend existing App fixtures for screen transitions.** Use existing invoke/listen/sharing mocks; click detail, select an object, change zoom, enter shop and journal, return. Assert selected ID and camera restore for same context. Spy on `window.open` and mock WebviewWindow constructor; both must remain uncalled. Test legacy URL initial route renders the requested internal screen with a return button. Assert keyboard focus reaches the destination heading or primary control. Keep existing shop/journal failure-retry and deletion permission tests.

```tsx
const popup = vi.spyOn(window, "open").mockReturnValue(null);
await user.click(screen.getByRole("button", { name: "행성 꾸미기" }));
expect(screen.getByRole("button", { name: "내 행성으로 돌아가기" })).toBeVisible();
expect(popup).not.toHaveBeenCalled();
await user.click(screen.getByRole("button", { name: "내 행성으로 돌아가기" }));
expect(screen.getByRole("region", { name: "선택한 오브젝트" })).toBeVisible();
```

- [ ] **Step 2: Run new App transition checks before changes.** `npm test -- src/__tests__/App.test.tsx`; expect new transition assertions fail on popup behavior while existing baseline remains visible.
- [ ] **Step 3: Replace launch flow with local screen state.** Preserve existing shop/journal JSX and callbacks but render in the detail shell; supply return action and remove separate-window error/retry UI. On shop entry invoke the existing guarded shop loader; on journal entry use `loadGrowthJournal`. Clear only cosmeticPreview on shop departure. Keep confirmedCosmetics sourced from matching current cycle shop state. For legacy feature routes initialize detail mode so return leads to personal detail, not a compact viewport; synchronize the native detail mode using existing `set_detail_view` and expose a retry if native sizing fails.

```ts
const [screen, setScreen] = useState<"planet" | "cosmetic-shop" | "growth-journal">(initialScreen);
function navigateScreen(next: typeof screen) {
  if (screen === "cosmetic-shop" && next !== screen) setCosmeticPreview(null);
  setScreen(next);
}
```

Ensure current detailTab is personal when returning from a personal tool. Use destination heading refs/effect to move focus after rendering. Leave home-to-detail window sizing on its existing invoke path. Search `rg -n 'openFeatureWindow|featureWindows|WebviewWindow|window.open' apps/desktop/src` and remove unused creator/imports; no button path creates a window. Keep group tab and home return behavior.
- [ ] **Step 4: Run App and feature-component regression checks.** `npm test -- src/__tests__/App.test.tsx src/components/__tests__/CosmeticShop.test.tsx src/components/__tests__/GrowthJournal.test.tsx` then `npm run build`.
- [ ] **Step 5: Commit explicit task paths.** `git commit -m "feat(desktop): 상점과 성장 일지를 앱 안에서 전환"`.

### Task 5: Context invalidation and asynchronous lifetime safety

**Files:** Modify `App.tsx` and `src/__tests__/App.test.tsx` only.

**Interfaces:** Consumes controlled exploration and internal screen from Tasks 3–4. Exploration context derives from account identity, sharing phase/world ID/ownership and current cycle ID; screen stays selected during context changes. Existing `worldTransitionEpoch`, shop generation/context and journalRequestId/owner protection remain authoritative for requests.

- [ ] **Step 1: Add deferred-response and context tests.** Use a deferred helper for purchase, equip and journal requests. Start a request in one context, trigger existing context refresh events to another account/world/cycle, resolve the previous request, and assert old balance/equip/journal never appear. Parameterize account/world/cycle changes and assert selection clears and camera fits new terrain; same-context screen return must preserve both. Emit world-context-changing with payload `main` and assert it is filtered as the actual current window, even while screen is shop/journal. Other actual window sources still trigger refresh. Verify purchase-only response does not auto-equip; preview is gone after leaving shop; successful pending equip reflects on return; failed shop response is not an error banner on planet.

```ts
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}
```

Use existing App fixtures and exact event listener names; add no artificial backend protocol. Also reject a pending journal request after leaving the screen, then re-enter and assert the current loading/retry state is usable.
- [ ] **Step 2: Run the failing targeted App checks.** `npm test -- src/__tests__/App.test.tsx`.
- [ ] **Step 3: Integrate the context guards.** Use an explicit context-keyed exploration entry so obsolete state cannot render during a context transition. Include `shared?.phase`, `shared?.user_id`, `shared?.world?.id`, and `planet.current_cycle_id`. A mismatching or locked context renders fit/null and does not accept stale scene callbacks. In `lockWorldContext`, invalidate exploration and preview alongside existing caches; preserve internal screen. On accepted context, reload only screen-required data through existing guards. Preserve request completion checks before mutating confirmed state. Scope shop/journal errors to their respective screen and retain retry there.

```ts
const explorationContext = JSON.stringify([
  shared?.phase ?? null, shared?.user_id ?? null,
  shared?.world?.id ?? null, shared?.world?.is_owner ?? false, planet.current_cycle_id,
]);
// Exploration entry carries { context, value }; mismatches use fit/null.
// Callback stores only if its captured context equals the current accepted context.
```

Keep broadcast source and self-event comparisons tied to `main`, independent of screen. Keep purchase/equip epoch checks, generation invalidation, journal owner/request checks, context lock and deletion eligibility. Accept preview callbacks only while the matching shop screen and context remain active so an unmounted shop cannot reinstate a preview on the planet. Test that late preview callback explicitly alongside pending successful equip. On context transition discard cached inspector state before rendering another account. Do not remove existing Tauri listeners merely because tools now share a window.
- [ ] **Step 4: Run combined targeted checks and build.** `npm test -- src/__tests__/App.test.tsx src/components/__tests__/PlanetLandscape.test.tsx src/components/__tests__/CosmeticShop.test.tsx src/components/__tests__/GrowthJournal.test.tsx` then `npm run build`.
- [ ] **Step 5: Commit explicit files.** `git commit -m "fix(desktop): 화면 전환과 계정 변경 상태 보호"`.

### Task 6: Integrated QA and independent review

**Files:** No product edits by QA/Reviewer. Coder owns any corrections in the preceding task files and their relevant checks; update this plan's checkboxes/evidence only as verification proceeds.

**Interfaces:** Receives all task outputs and spec acceptance criteria. Returns a record of exact commands, environments, assertions, screenshots where useful, unresolved findings and disposition.

- [ ] **Step 1: Run the frontend regression suite and build.** From `apps/desktop`: `npm test` and `npm run build`. From repository root: `git diff --check`. Expected pass; investigate failures before continuing. Do not rerun unchanged checks without a new reason.
- [ ] **Step 2: Open the local frontend for visual QA.** From `apps/desktop`, use existing `npm run dev:local` if it supports the current fixture setup; otherwise use `npm run dev` and the repository's existing browser/mock setup. Inspect empty, stage 0–4, collocated many-object and equipped-decoration fixtures. Verify available width reaches both edges, text lies below, sprites are not stretched, and decorative details do not inflate saved-object counts. Capture viewport size and screenshot evidence; screenshot appearance is not proof of native behavior.
- [ ] **Step 3: Check pointer and keyboard journeys at narrow and wide sizes.** Use approximately 360px width and a desktop window. Exercise zoom, drag, arrow keys, list selection, fit, reduced motion, resize after zoom, screen focus transfer, shop preview/return, successful equip/return, journal refresh/retry/delete permission and home/group transitions. Assert no page-level horizontal overflow and every persisted object can be selected through the list, including dense scenes. Record failures through the team lead.
- [ ] **Step 4: Check native window behavior where available.** Start the repository's existing Tauri development command (`npm run tauri dev` from `apps/desktop`) only in a usable native environment. Open detail then shop/journal and verify the number of application windows stays unchanged; return preserves exploration and confirmed equipment. Check main window sizing, self-event filtering, legacy URL entry if supported and data refresh. If native execution is unavailable, explicitly mark native checks unperformed and retain the exact limitation in handoff rather than infer them from browser tests.
- [ ] **Step 5: Independent read-only review.** Reviewer examines changed files against spec, the five Review Focus lines and QA evidence. Concentrate on stale contexts, event-source identity, unbounded layout/camera work, access to all saved objects, preview/equip state and the footprint used for sprite collisions. Resolve actionable findings through Coder, then QA reruns affected checks; request reviewer recheck only for material corrections.
- [ ] **Step 6: Completion handoff.** Report achieved acceptance criteria and actual browser/native evidence, remaining limitations and commit range. Do not stage `.DS_Store`. The team lead owns completion; unresolved functional blockers prevent a complete claim.

## Self-Review and Spec Coverage

| Spec requirement | Tasks |
| --- | --- |
| Full-width flat detail landscape; circular home/group preserved | 1–3, 6 |
| All persisted objects, deterministic separated placement, no +N, empty terrain | 1, 3, 6 |
| Decorative dressing and every recognized equipped style without growth mutation | 2–3, 6 |
| Aspect-preserving fit/focus/zoom/pan, resize bounds, no page overflow | 2–3, 6 |
| Name/era/ordinal+1 inspector, list and keyboard access, no dates | 3, 6 |
| In-app shop/journal, no popup, return and focus | 4, 6 |
| Same-context camera/selection preservation, invalidation on account/world/cycle | 3–5 |
| Preview cleared, purchase/equip reflected, request/permission/retry safety | 4–5, 6 |
| Actual main-window event identity and legacy URL behavior | 4–5, 6 |
| Browser/native evidence distinction and independent review | 6 |

The plan was checked against every spec section. Camera viewBox math preserves aspect ratio; background dressing fills excess viewport area. Types and callback names above agree across tasks. Each Review Focus line maps to explicit tests and integrated checks. No feature decision is left pending: layout reserves the maximum shared sprite footprint; exact art spacing and scene height may be tuned during visual QA without changing these interfaces or acceptance criteria. No product/test/config implementation or execution checks ran during plan writing.

## Approval and Execution

This document requires user review and approval before Coder starts. The repository's Yunho Harness already specifies sequential Coder → QA → Reviewer under the team lead; use that existing method rather than adding another owner or nested specialist tree. Execution preparation must inspect existing artifacts and apply the installed worktree skill before selecting an isolated checkout. No worktree, dependency installation or implementation is part of this document stage.
