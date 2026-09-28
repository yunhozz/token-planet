# Token Planet UI Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the compact and detailed planet screens easier to scan, with a focused group view and restrained, accessible planet motion.

**Architecture:** Keep the existing usage snapshot, group data, and domain commands as the source of truth. In `App.tsx`, render one compact view or one of two complete detail panels so the group tab never leaves the personal planet visible. `WorldCommunity` owns member selection; `PlanetScene` owns presentation-only animation and uses `AvatarSprite` for the enlarged character.

**Tech Stack:** React 19, TypeScript, CSS, Tauri 2, Vitest, Testing Library.

**Spec:** `docs/superpowers/specs/2026-09-28-token-planet-ui-refresh-design.md`

## Global Constraints

- Keep the existing pixel planet style, usage meanings, group visibility, and reset rules.
- Do not change server APIs, growth formulas, ledgers, group ranking calculations, shared data, or reset rules.
- Do not add generated images, animation libraries, or user settings.
- Keep errors and incomplete collection status next to the relevant values.
- Keep long numbers readable by wrapping them instead of clipping or truncating them.
- Make the tabs, planet return action, and group planet selection usable from the keyboard.
- Preserve the existing shop, journal, source controls, invitation, group management, and sync actions in their relevant views.

## Review Focus

- Compact window at 390×700: planet, current era, this planet's tokens, and next-era progress appear before secondary information; check DOM order in `App.test.tsx` and actual first-viewport visibility in the final visual review.
- Reopening detail view: “내 행성” is selected each time; tabs expose selection and keyboard navigation; cover in `App.test.tsx`.
- Group with zero or multiple members: show the syncing empty state or the first member's selected planet, and keep ranking values unchanged; cover the empty group tab in `App.test.tsx` and member selection/ranks in `SharingPanels.test.tsx`.
- Reduced motion, hidden window, or off-screen scene: stop repeated animation; compact group cards stay still; cover the pause/resume behavior in `PlanetScene.test.tsx`.
- Long values, reset, and group privacy: preserve values, explain reset effects before confirmation, and disclose what group members can see; cover in `App.test.tsx` and responsive CSS review.

---

## File and data map

| File | Responsibility in this plan |
| --- | --- |
| `apps/desktop/src/App.tsx` | Compact/detail composition, tabs, information order, reset/privacy copy, motion ownership |
| `apps/desktop/src/components/UsageSummary.tsx` | Rename the personal historical total and its incomplete state without changing the value |
| `apps/desktop/src/components/WorldCommunity.tsx` | First-member fallback, selected large planet, uniform choice cards, existing rankings |
| `apps/desktop/src/components/PlanetScene.tsx` | Surface path, movement scheduler, visibility gating, new-object entrance detection |
| `apps/desktop/src/components/AvatarSprite.tsx` | Share the pixel character drawing with the scene; keep non-scene avatars static |
| `apps/desktop/src/App.css` | Type scale, layout, controls, selection, and motion styling |
| `apps/desktop/src-tauri/src/lib.rs` | Detail-window target and monitor-size cap only |

Keep these value sources distinct in both rendering and tests:

| Label | Existing source |
| --- | --- |
| `이번 행성 토큰` | `snapshot.planet.current_planet_tokens` |
| `누적 토큰 (개편 후)` | `snapshot.planet.lifetime_tokens` |
| `전체 사용량 (과거 포함)` | `snapshot.usage.confirmed_subtotal`, as rendered by `UsageSummary` |
| `성장 점수` | `snapshot.planet.growth_credit` |
| Group token and civilization ranks | Existing `WorldPlanet.token_rank` and `WorldPlanet.civilization_rank` |

The compact DOM order is top bar, planet scene, current-era/token/progress summary, primary detail button, then usage/source/sync details. The detailed DOM order is top bar, return action and tab list, then exactly one panel. The personal panel starts with a large scene and four quick facts; its remaining values and actions follow below. The group panel starts with either the existing sign-in/create/join flow or the selected member scene, then member choices, rankings, invitations, management, and sharing disclosure. Keep sync and actionable errors visible in either view. Remove the desktop-only nested `.world-info` scroll so narrow layouts use one page scroll.

### Task 1: Personal view hierarchy and detail navigation

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/App.css`
- Modify: `apps/desktop/src/components/UsageSummary.tsx`
- Modify: `apps/desktop/src/__tests__/App.test.tsx`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Keep the current `WorldSnapshot`, usage, growth, reset, sharing, and shop interfaces unchanged.
- Add local detail-tab state in `App`; opening detailed view selects the personal tab.
- Expose two tabs with `aria-selected`, `aria-controls`, a matching `tabpanel`, roving `tabIndex`, and arrow/Home/End keyboard navigation.
- Render the current planet scene only in compact mode or the personal detail panel. Keep `SyncStatus` outside the conditional panels and move the compact detail button directly after its quick facts.

- [ ] **Step 1: Add failing tests for the compact summary, tab reset, reset confirmation, and privacy disclosure**

```tsx
it("shows the planet summary first and reopens details on the personal tab", async () => {
  render(<App />);
  await screen.findByText("Orbit의 행성");
  const summary = screen.getByRole("region", { name: "행성 요약" });
  expect(summary).toHaveTextContent("현재 시대");
  expect(summary).toHaveTextContent("이번 행성 토큰");
  expect(summary.querySelector('[role="progressbar"][aria-label="다음 시대 진행도"]')).not.toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  const personal = await screen.findByRole("tab", { name: "내 행성" });
  const group = screen.getByRole("tab", { name: "그룹" });
  expect(personal).toHaveAttribute("aria-selected", "true");
  personal.focus();
  fireEvent.keyDown(personal, { key: "ArrowRight" });
  expect(group).toHaveFocus();
  expect(group).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("button", { name: "행성으로 돌아가기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByRole("tab", { name: "내 행성" })).toHaveAttribute("aria-selected", "true");
});

it("keeps the three token totals tied to their original fields", async () => {
  const distinct = structuredClone(localSnapshot);
  distinct.planet.current_planet_tokens = 7;
  distinct.planet.lifetime_tokens = 11;
  distinct.usage.confirmed_subtotal = 19;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "current_usage") return distinct;
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "list_world_members" || command === "list_invites") return [];
    return null;
  });
  render(<App />);
  const summary = await screen.findByRole("region", { name: "행성 요약" });
  expect(summary).toHaveTextContent("7");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  expect(screen.getByText("누적 토큰 (개편 후)").parentElement).toHaveTextContent("11");
  expect(screen.getByRole("region", { name: "전체 사용량 (과거 포함)" })).toHaveTextContent("19");
});

it("explains reset effects and group visibility before the user acts", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  fireEvent.click(screen.getByRole("button", { name: "행성 초기화" }));
  expect(confirm).toHaveBeenCalledWith(expect.stringContaining("지갑"));
  expect(confirm).toHaveBeenCalledWith(expect.stringContaining("자연 생태계"));
  fireEvent.click(screen.getByRole("tab", { name: "그룹" }));
  expect(screen.getByText("그룹에는 행성 모습과 집계값만 공유됩니다")).toBeInTheDocument();
  confirm.mockRestore();
});

it("shows the group's syncing state without the personal scene", async () => {
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("tab", { name: "그룹" }));
  expect(screen.getByText("멤버의 행성을 동기화하고 있습니다.")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "나의 행성" })).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run the focused test and confirm it fails on the missing summary/tab behavior**

Run from `apps/desktop`: `npm test -- src/__tests__/App.test.tsx`

Expected: the new tests fail because the named summary and detail tabs do not exist; the historical-total label and reset confirmation are also still old.

- [ ] **Step 3: Add the summary, personal tab panel, and keyboard-operable tab list**

```tsx
<div role="tablist" aria-label="행성 자세히 보기" onKeyDown={handleDetailTabKeyDown}>
  <button role="tab" id="tab-personal" aria-controls="panel-personal" aria-selected={detailTab === "planet"} tabIndex={detailTab === "planet" ? 0 : -1} onClick={() => setDetailTab("planet")}>내 행성</button>
  <button role="tab" id="tab-group" aria-controls="panel-group" aria-selected={detailTab === "group"} tabIndex={detailTab === "group" ? 0 : -1} onClick={() => setDetailTab("group")}>그룹</button>
</div>
```

Implement `handleDetailTabKeyDown` to select and focus the neighboring tab on Left/Right, the first on Home, and the last on End. Reset `detailTab` to `"planet"` in `changeView` when opening detail and when `show-compact` arrives. Place current era, current planet tokens, and era progress before secondary information in compact mode; keep the existing final-era note instead of inventing a next-era progress value for stage 4. In the personal panel, place the approximately 420px scene next to nickname, era, current tokens, and era progress; put wallet, lifetime tokens, growth credit, object progress, recent object, historical personal usage, shop, source settings, journal, and reset in the following information area. Put incomplete status next to its affected number. Move the existing group setup, community, invitation, management, and privacy content into `panel-group`. Preserve the existing callbacks, loading states, and action errors when moving JSX.

- [ ] **Step 4: Update reset/privacy copy and the native detailed-window target**

Use `이번 행성 토큰`, `누적 토큰 (개편 후)`, `전체 사용량 (과거 포함)`, and `일부 기록 확인 중` for the specified values/states, keeping the values from the map above. Shorten repetitive normal-state copy while retaining collector and sync errors with a next action. Update the reset confirmation to state both wallet credit and return to the natural ecosystem. In the group panel, use a native `<details>` with summary `그룹에는 행성 모습과 집계값만 공유됩니다`; its body names the visible aggregates and the private prompts, responses, file paths, and original session records. Update the existing sign-in, create/join, and owner-transfer app tests to select the group tab before exercising those flows. Set the Tauri detail target to 960×700 logical pixels, cap it to the current monitor's logical size with room for window chrome, and preserve the compact target at 390×700.

- [ ] **Step 5: Run focused tests and the TypeScript production build**

Run from `apps/desktop`: `npm test -- src/__tests__/App.test.tsx`, `npm run build`, and `cargo check --manifest-path src-tauri/Cargo.toml`.

Expected: the focused UI tests, TypeScript/Vite build, and native compile pass.

### Task 2: Selected group planet and rankings

**Files:**
- Modify: `apps/desktop/src/components/WorldCommunity.tsx`
- Modify: `apps/desktop/src/components/__tests__/SharingPanels.test.tsx`
- Modify: `apps/desktop/src/App.css`

**Interfaces:**
- Continue accepting the existing `name` and `members` props.
- Use the existing `WorldPlanet` fields; do not change rank values or group payloads.
- Keep the selected large scene above uniform compact member choices and both existing ranking lists. Task 3 adds motion only to the selected scene.

- [ ] **Step 1: Add a failing test for the default selected member and keyboard selection**

```tsx
const member = (nickname: string, rank = 1): WorldPlanet => ({
  nickname, avatar: "masculine", stage: 0, current_planet_tokens: rank * 10,
  lifetime_tokens: rank * 20, growth_credit: rank * 0.5, progress_to_next: 0.1,
  incomplete: false, objects: [], equipped_cosmetics: [],
  token_rank: rank, civilization_rank: rank,
});

it("opens the first member planet by default and selects another with the keyboard", () => {
  render(<WorldCommunity name="Together" members={[member("Nova", 2), member("Mira", 1)]} />);
  expect(screen.getByRole("region", { name: "Nova의 행성 자세히 보기" })).toBeInTheDocument();
  const mira = screen.getByRole("button", { name: "Mira의 행성 크게 보기" });
  fireEvent.keyDown(screen.getByRole("button", { name: "Nova의 행성 크게 보기" }), { key: "ArrowRight" });
  expect(mira).toHaveFocus();
  expect(mira).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("region", { name: "Mira의 행성 자세히 보기" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "누적 토큰 사용량" }).querySelector("li")?.textContent).toContain("Mira");
  expect(screen.getByRole("region", { name: "문명 발전" }).querySelector("li")?.textContent).toContain("Mira");
});

it("falls back to the first available member when the selected member disappears", () => {
  const { rerender } = render(<WorldCommunity name="Together" members={[member("Nova"), member("Mira")]} />);
  fireEvent.click(screen.getByRole("button", { name: "Mira의 행성 크게 보기" }));
  rerender(<WorldCommunity name="Together" members={[member("Nova")]} />);
  expect(screen.getByRole("region", { name: "Nova의 행성 자세히 보기" })).toBeInTheDocument();
});

```

Add `import type { WorldPlanet } from "../../types/usage"` to this test file.

- [ ] **Step 2: Run the focused test and confirm the first-member selection assertion fails**

Run from `apps/desktop`: `npm test -- src/components/__tests__/SharingPanels.test.tsx`

Expected: the new test fails because no member is selected on first render.

- [ ] **Step 3: Select the first available member and promote that scene above the member list**

Start with index 0. Resolve `members[selectedIndex] ?? members[0] ?? null` at render time so a shrinking roster never leaves a blank hero; normalize the selected button state to that resolved index. Keep the selected member as the detail scene above a same-sized list of member choices. Arrow keys move focus and selection through the list; Enter/Space use the button's native click. Remove the close-to-no-selection action. When `members` is empty, keep the existing synchronization message and omit rankings until data exists. Keep both ranking sort orders and their numeric sources exactly as they are below the scene and member list. Update the existing cosmetic test to expect both the selected hero and compact card on initial render, and verify clicking the selected card does not close the hero.

- [ ] **Step 4: Style the selected scene and keyboard-visible member selection**

Size the selected scene as the hero, give every compact card the same dimensions, and make `aria-pressed` and `:focus-visible` states distinct. Leave the full text-size audit to Task 4; do not shrink names or numbers to make cards fit.

- [ ] **Step 5: Run the group component tests**

Run from `apps/desktop`: `npm test -- src/components/__tests__/SharingPanels.test.tsx`

Expected: the new selection test and existing group privacy/cosmetic/ranking tests pass.

### Task 3: Restrained planet and avatar motion

**Files:**
- Modify: `apps/desktop/src/components/PlanetScene.tsx`
- Modify: `apps/desktop/src/components/AvatarSprite.tsx`
- Modify: `apps/desktop/src/components/__tests__/PlanetScene.test.tsx`
- Modify: `apps/desktop/src/App.css`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/WorldCommunity.tsx`

**Interfaces:**
- Add `animate?: boolean` to `PlanetScene`, defaulting to false. The compact scene stays static even if `animate` is true. Pass `animate` from `App` to the visible personal scene and from `WorldCommunity` only to its selected large scene.
- Extend `AvatarSprite` with optional facing/closed-eye/walking presentation props whose defaults preserve the existing static avatar in cards, rankings, and profile setup. Replace the duplicated inline scene avatar drawing with the shared sprite.
- Keep motion presentation-only and derive object entrance effects from newly added `(stage, ordinal)` identities in the same mounted scene.
- Pause timers when the scene is off-screen, the document is hidden, or reduced motion is requested.

- [ ] **Step 1: Add failing tests for compact-scene motion, reduced motion, and object entrance identity**

```tsx
it("keeps compact member scenes static even when motion is requested", () => {
  const { container } = render(<PlanetScene stage={0} progress={0} compact animate />);
  expect(container.querySelector(".planet-figure")).toHaveAttribute("data-motion", "paused");
});

it("chooses different nearby directions and stays inside the surface path", async () => {
  type MotionExports = { planWalk?: (start: number, random: () => number) => number[]; WALK_POINTS?: { x: number; y: number }[] };
  const { planWalk, WALK_POINTS } = await import("../PlanetScene") as typeof import("../PlanetScene") & MotionExports;
  expect(planWalk).toBeTypeOf("function");
  expect(WALK_POINTS).toBeDefined();
  if (!planWalk || !WALK_POINTS) return;
  const start = Math.floor(WALK_POINTS.length / 2);
  const left = planWalk(start, () => 0);
  const right = planWalk(start, () => 0.99);
  expect(left).toHaveLength(2);
  expect(right).toHaveLength(4);
  expect(left[0]).toBe(start - 1);
  expect(right[0]).toBe(start + 1);
  for (const [origin, route] of [[start, left], [start, right], [0, planWalk(0, () => 0)]] as const) {
    let previous = origin;
    for (const index of route) {
      expect(index).toBeGreaterThanOrEqual(0);
      expect(index).toBeLessThan(WALK_POINTS.length);
      expect(Math.abs(index - previous)).toBe(1);
      const point = WALK_POINTS[index];
      expect(Math.hypot(point.x + 12 - 180, point.y + 15 - 157)).toBeLessThan(80);
      previous = index;
    }
  }
});

it("animates a new object once but not a mount or data refresh", () => {
  const object = { stage: 0, ordinal: 1, kind: "rock", x: 10, y: 30, seed: 1 } as const;
  const { container, rerender } = render(<PlanetScene stage={0} progress={0} objects={[object]} animate />);
  expect(container.querySelector(".planet-object--entering")).not.toBeInTheDocument();
  rerender(<PlanetScene stage={0} progress={0} objects={[{ ...object }, { ...object, ordinal: 2 }]} animate />);
  expect(container.querySelector(".planet-object--entering")).toBeInTheDocument();
  fireEvent.animationEnd(container.querySelector(".planet-object--entering")!);
  rerender(<PlanetScene stage={0} progress={0} objects={[{ ...object }, { ...object, ordinal: 2 }]} animate />);
  expect(container.querySelector(".planet-object--entering")).not.toBeInTheDocument();
});

it("uses a complete static scene when reduced motion is requested", async () => {
  vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  const { container } = render(<PlanetScene stage={0} progress={0} animate />);
  await waitFor(() => expect(container.querySelector(".planet-figure")).toHaveAttribute("data-motion", "reduced"));
  vi.unstubAllGlobals();
});
```

In this test file, import `vi` from Vitest and `act`/`fireEvent`/`waitFor` from Testing Library. Use the dynamic import above so the RED run fails an assertion instead of failing module loading before `planWalk` exists. For hidden-window and off-screen tests, set `document.hidden` or invoke a stubbed `IntersectionObserver` callback with `isIntersecting: false`, then use fake timers to assert the `data-planet-avatar` transform remains unchanged. Restore mocked globals, document properties, and timers after each test.

- [ ] **Step 2: Run the focused scene test and confirm it fails for the missing motion contract**

Run from `apps/desktop`: `npm test -- src/components/__tests__/PlanetScene.test.tsx`

Expected: the new motion assertions fail because scene motion state is not represented yet.

- [ ] **Step 3: Implement bounded walking, blinking, scene float, and one-time object entrances**

Define `WALK_POINTS` as at least nine ordered positions on the existing visible terrain, inside the `cx=180, cy=157, r=107` planet clip with enough margin for the enlarged sprite. Export `planWalk(startIndex, random): number[]`: its first random draw chooses left or right, its second chooses 2–4 adjacent points; reverse at a path end and never skip an index. Keep the planet orientation fixed and float the complete planet by only 2–3px. After an idle pause, transition the avatar through those points one at a time, face travel, pause again, and blink occasionally only while idle. The scene avatar uses `AvatarSprite` at a larger scale; other avatars remain static. Seed known object identities on mount; apply a short entrance class only to identities added later and clear it on `animationend`. A data refresh with the same objects or a remounted tab cannot replay the object entrance. Do not change any `PlanetObject`, growth, or ranking data.

- [ ] **Step 4: Pause motion for compact/off-screen/hidden/reduced-motion scenes**

Use `IntersectionObserver`, `document.visibilitychange`, and `matchMedia("(prefers-reduced-motion: reduce)")` to gate recurring movement. Cancel the pending walk/blink timer when any gate closes; restart with an idle pause when it opens. Disconnect observers/media listeners and clear timers on cleanup. With reduced motion, render the complete static scene without entrance, float, blink, or walking effects. Key the personal scene by `planet.current_cycle_id` and the selected group scene by resolved index plus nickname; switching cycles or members seeds that scene's existing objects instead of treating them as additions. Do not key on ranks, token values, or objects, because a normal refresh must preserve the mounted scene.

- [ ] **Step 5: Add CSS motion rules only for active scenes**

Add a short entrance for a detailed scene, one slow 2–3px vertical float for its planet, a discrete leg cycle during walks, a brief idle blink, and a short entrance for genuinely new objects. Apply these classes only when motion is active. In the existing `prefers-reduced-motion: reduce` rule, force the final visible state with no transition or animation; do not rotate the scene.

- [ ] **Step 6: Run the focused motion tests**

Run from `apps/desktop`: `npm test -- src/components/__tests__/PlanetScene.test.tsx`.

Expected: compact scenes remain static; new-object entrance, path bounds, and pause conditions pass without leaked timers.

### Task 4: Readable type, responsive layout, and final review

**Files:**
- Modify: `apps/desktop/src/App.css`

**Interfaces:**
- Keep all markup and behaviors from Tasks 1–3; this task changes layout and presentation only.
- Let the document scroll vertically at compact and narrow widths; remove the old detail `.world-info` height cap and nested scrollbar.

- [ ] **Step 1: Replace the small-text scale throughout the existing stylesheet**

Set system sans-serif on the root, with the existing monospace feel limited to `.brand` and numeric values. Set ordinary body copy to 16px, supporting/status copy to 14px, section headings to 20–24px, and main numbers to 28–32px. Audit selectors for setup, sources, sync, ledger, group cards and ranks, sharing forms, shop, journal, reset, and error states; remove the current 7–13px exceptions in those areas. Preserve the current dark background and mint, water, and gold pixel palette; reduce panel decoration so the planet carries the strongest color.

- [ ] **Step 2: Make controls and long content fit without truncation**

Give buttons, inputs, and selects at least 44px height. Use `minmax(0, 1fr)`, `min-width: 0`, wrapping, and `overflow-wrap: anywhere` where needed. Remove `.rank-name` ellipsis and the source-count `white-space: nowrap`; allow long Korean labels and numbers to occupy extra lines. Keep visible focus rings and selected tab/member states.

- [ ] **Step 3: Size the two layouts without shrinking text**

At 390×700, stack scene, summary, and primary detail action before secondary content. At the approximately 960×700 detail target, place a scene up to about 420px beside four quick facts, then show the remaining personal information below; make the selected group scene equally prominent. Below a width that cannot hold both columns, switch to one column and use the document scrollbar. Keep the card list uniform and static.

- [ ] **Step 4: Run the final automated checks**

Run from `apps/desktop`: `npm test` and `npm run build`. Run `git diff --check` from the repository root.

Expected: the existing and added UI tests pass, the production build has no type errors, and the diff has no whitespace errors.

- [ ] **Step 5: Review the rendered UI in the running desktop app or populated preview**

Inspect 390×700 compact, 960×700 detail, 320px narrow detail, and 200% text zoom. Check the first viewport for planet/character/era/current tokens, then scroll through sources, journal, shop, reset, group selection/rankings, and sharing disclosure. Check long nicknames and long token counts, keyboard focus, and reduced motion. Record any environment that could not be rendered instead of treating DOM tests as a visual pass.

## Final Acceptance Review

| Spec criterion | Evidence to collect |
| --- | --- |
| Detail first view | At 960×700, inspect that the personal planet, character, era, this planet's tokens, and group tab are visible before scrolling. |
| Readability and resizing | Inspect 390×700, 960×700, 320px detail, and 200% text zoom; confirm no clipped numbers, labels, controls, or horizontal overflow. |
| Walking and group motion | Run the path/object tests, then watch several walks for different directions and continuous steps; confirm compact member cards remain still. |
| Motion and keyboard accessibility | Run pause/resume and tab/selection tests; inspect reduced motion and visible focus in the rendered app. |
| Data meaning and disclosure | Run the distinct-total and group-rank tests; inspect reset confirmation and sharing disclosure before acting. |

Keep the change set uncommitted for the user. If rendered UI inspection is unavailable in the execution environment, report exactly which viewport or motion behavior was not observed.
