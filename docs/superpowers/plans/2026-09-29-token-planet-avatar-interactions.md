# Token Planet Avatar Interactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The user's lean-work-harness instruction routes all code and test-code edits through a GPT-6 Luna Max `lean_coder`; the team lead owns final verification.

**Goal:** Make the avatar's surface walking varied and let users click the planet or avatar for a short state-aware speech bubble.

**Architecture:** Keep walking and speech as local React scene state; neither changes planet data or sync. A pure route planner and a pure dialogue selector make boundary conditions testable. `PlanetScene` provides two accessible scene targets, owns timers, and positions one bubble above the avatar.

**Tech Stack:** React 19, TypeScript, existing SVG/CSS, Vitest and Testing Library.

**Spec:** `docs/superpowers/specs/2026-09-29-token-planet-interactions-and-shop-expansion-design.md`

## Global Constraints

- Preserve the dark pixel planet palette and the existing growth, token, reset, sharing and ranking semantics.
- Use only existing surface `WALK_POINTS`; no teleporting, stored random movement, new dependency, AI dialogue call, raw record access or server change.
- Small group cards stay static. Hidden/off-screen scenes and reduced-motion settings do not run repeated movement.
- Only the selected group planet may speak, using public stage/progress/cosmetics; never pass personal source status or token counts into its dialogue.
- Route code and test edits to `lean_coder`; review and run final checks in the team lead.

## Review Focus

1. At either path edge, a chosen route stays inside the path and advances only one adjacent point per step. Pin in Task 1.
2. Clicking during a timed walk cancels the pending step; closing the bubble resumes without jumping or leaving a stale timer. Pin in Task 3.
3. Initial object data and a new group selection do not announce an object as newly created. Pin in Task 2 and Task 3.
4. A small popover and edge avatar position keep the bubble visible; keyboard focus and status text remain usable. Pin in Task 3.
5. Reduced motion and off-screen transitions cancel movement while still allowing a static click response. Pin in Task 3.

---

## File Structure

- Create `apps/desktop/src/components/sceneMotion.ts`: pure walk route, rest-duration and step-duration helpers.
- Create `apps/desktop/src/components/sceneDialogue.ts`: fixed Korean dialogue pools and pure selection by target/public or personal context.
- Modify `apps/desktop/src/components/PlanetScene.tsx`: consume motion/dialogue helpers, own timers and last-known objects, expose interactive targets and bubble.
- Modify `apps/desktop/src/App.tsx`: pass personal `incomplete` and current cycle identity to personal scenes only.
- Modify `apps/desktop/src/components/WorldCommunity.tsx`: make the expanded scene interactive with public context; keep thumbnails inert.
- Modify `apps/desktop/src/App.css`: bubble, focus, hit-area and per-step walking duration styling.
- Modify `apps/desktop/src/components/__tests__/PlanetScene.test.tsx`: motion and scene interaction tests.
- Create `apps/desktop/src/components/__tests__/sceneDialogue.test.ts`: dialogue priority and privacy tests.
- Modify `apps/desktop/src/components/__tests__/SharingPanels.test.tsx`: selected group interaction and inert thumbnail coverage.

### Task 1: Variable surface walking

**Files:** Create `apps/desktop/src/components/sceneMotion.ts`; modify `apps/desktop/src/components/PlanetScene.tsx` and `apps/desktop/src/components/__tests__/PlanetScene.test.tsx`.

**Interfaces:** Produce `planWalk(startIndex: number, previousDestination: number | null, random: () => number): number[]`, `restDuration(random: () => number): number` (1500–5000 ms) and `stepDuration(random: () => number): number` (300–550 ms). Export `WALK_POINTS` from `sceneMotion.ts` and re-export `planWalk`/`WALK_POINTS` from `PlanetScene.tsx` for existing test imports.

- [ ] **Step 1: Write failing tests** for random values 0, 0.5 and 0.99 from the middle and both edges; assert route length 1–5, every index valid, every delta exactly 1, and a candidate immediate reverse to the previous destination is rerolled when another route exists. Assert duration endpoints 1500/5000 and 300/550.

```ts
const route = planWalk(0, 1, () => 0.99);
expect(route.length).toBeGreaterThanOrEqual(1);
for (let i = 0, at = 0; i < route.length; i += 1) {
  expect(Math.abs(route[i] - at)).toBe(1);
  at = route[i];
}
expect(restDuration(() => 0)).toBe(1500);
expect(stepDuration(() => 1)).toBe(550);
```

- [ ] **Step 2: Run** `npm --prefix apps/desktop test -- src/components/__tests__/PlanetScene.test.tsx`; expect the new route and duration tests to fail.
- [ ] **Step 3: Implement** a destination choice within one to five adjacent points, clamped to the existing path, with one alternate draw for immediate reverse. Build the route by incrementing/decrementing one index at a time; do not choose arbitrary SVG coordinates. Sample rest and step durations once per walk. Store the sampled step duration as an inline CSS custom property on the avatar group, e.g. `style={{ "--avatar-step-ms": `${duration}ms` } as React.CSSProperties}` and consume it in the existing transform transition.

```ts
const sample = () => Math.max(0, Math.min(1 - Number.EPSILON, random()));
const directions = [-1, 1].filter((d) => startIndex + d >= 0 && startIndex + d < WALK_POINTS.length);
let direction = directions[Math.floor(sample() * directions.length)];
let available = direction < 0 ? startIndex : WALK_POINTS.length - 1 - startIndex;
let steps = 1 + Math.floor(sample() * Math.min(5, available));
if (startIndex + direction * steps === previousDestination && directions.length === 2) {
  direction *= -1;
  available = direction < 0 ? startIndex : WALK_POINTS.length - 1 - startIndex;
  steps = Math.min(steps, available);
} else if (startIndex + direction * steps === previousDestination && available > 1) {
  steps = steps === 1 ? 2 : steps - 1;
}
return Array.from({ length: steps }, (_, i) => startIndex + direction * (i + 1));
```
- [ ] **Step 4: Run** the targeted PlanetScene tests and `npm --prefix apps/desktop run build`; expect all tests and TypeScript/Vite build to pass.
- [ ] **Step 5: Commit** only Task 1 files with `feat(planet): 아바타 이동 경로와 속도 다양화`.

### Task 2: State-aware dialogue selection

**Files:** Create `apps/desktop/src/components/sceneDialogue.ts` and `apps/desktop/src/components/__tests__/sceneDialogue.test.ts`.

**Interfaces:** Produce `type DialogueTarget = "avatar" | "planet"`, `type DialogueContext = { target: DialogueTarget; stage: number; progress: number; incomplete?: boolean; newObjectKind?: string | null; publicOnly: boolean; previous?: string | null }`, and `pickDialogue(context: DialogueContext, random: () => number): string`. `newObjectKind` exists only for a personal scene after an in-session object diff; a public scene ignores it and `incomplete` even if accidentally passed.

- [ ] **Step 1: Write failing tests** for new object, incomplete personal collection, progress ≥0.85, each stage, avatar-target default, repeated-choice avoidance, and a public context that receives `incomplete: true`/`newObjectKind: "tree"` yet returns neither private nor new-object copy.

```ts
expect(pickDialogue({ target: "planet", stage: 1, progress: .9, publicOnly: false }, () => 0))
  .toContain("다음 시대");
expect(pickDialogue({ target: "planet", stage: 1, progress: .9, publicOnly: true,
  incomplete: true, newObjectKind: "tree" }, () => 0)).not.toContain("기록");
```

- [ ] **Step 2: Run** `npm --prefix apps/desktop test -- src/components/__tests__/sceneDialogue.test.ts`; expect the new tests to fail.
- [ ] **Step 3: Implement** fixed Korean strings and priority `new object > incomplete > near next stage > current stage > default`, with separate avatar/planet candidate pools. Limit copy to one short sentence, avoid the immediately previous string when more than one candidate exists, and never interpolate raw usage data.

```ts
const kindNames: Record<string, string> = { tree: "나무", rock: "바위", creature: "생물" };
const stageLines = ["이곳에서 첫발을 떼자.", "작은 정착지가 생겼어.",
  "마을이 제법 커졌어.", "도시가 분주해졌어.", "우주까지 닿았어."];
const defaultLines = { avatar: ["오늘은 어디를 둘러볼까?", "잠깐 쉬었다 가자."],
  planet: ["행성을 한 바퀴 돌아볼까?", "새로운 풍경을 찾아보자."] };
const newKind = context.publicOnly ? null : context.newObjectKind;
const incomplete = !context.publicOnly && context.incomplete;
const pool = newKind ? [`새로운 ${kindNames[newKind] ?? "오브젝트"}가 생겼어!`]
  : incomplete ? ["아직 기록을 확인하는 중이야."]
  : context.stage < 4 && context.progress >= .85 ? ["다음 시대가 가까워!"]
  : context.target === "planet" && stageLines[context.stage]
    ? [stageLines[context.stage], ...defaultLines.planet]
    : defaultLines[context.target];
const choices = pool.filter((line) => line !== context.previous);
const available = choices.length ? choices : pool;
return available[Math.min(available.length - 1, Math.floor(random() * available.length))];
```
- [ ] **Step 4: Run** the targeted dialogue tests; expect them to pass.
- [ ] **Step 5: Commit** only Task 2 files with `feat(planet): 상황별 아바타 대사 정의`.

### Task 3: Scene click targets and speech bubble

**Files:** Modify `apps/desktop/src/components/PlanetScene.tsx`, `apps/desktop/src/App.tsx`, `apps/desktop/src/components/WorldCommunity.tsx`, `apps/desktop/src/App.css`, `apps/desktop/src/components/__tests__/PlanetScene.test.tsx`, and `apps/desktop/src/components/__tests__/SharingPanels.test.tsx`.

**Interfaces:** Extend `PlanetScene` props with `interactive?: boolean`, `publicOnly?: boolean`, `incomplete?: boolean`, `cycleId?: string`; default `interactive` to false for compact gallery cards, and pass it explicitly for the personal popover/detail and selected group scene. Keep `stage`, `progress`, `avatar`, `objects` and `equippedCosmetics` unchanged. Consume Task 1 motion helpers and Task 2 `pickDialogue`. Inside `PlanetScene`, `speak(target: DialogueTarget): void` sets `line: string | null`, pauses movement and schedules the four-second close.

- [ ] **Step 1: Write failing interaction tests**: planet and avatar buttons are separately focusable; click or Enter/Space shows one `role="status"` bubble; avatar click does not fire planet dialogue; repeat input resets the four-second close timer; hidden/reduced-motion scenes still speak without starting a walk; walking stops during speech and resumes afterward from the same path index; first mount does not claim a new object; selected group planet speaks but gallery cards still select members.

```tsx
const { container } = render(<PlanetScene stage={1} progress={.9} interactive animate />);
fireEvent.click(screen.getByRole("button", { name: "행성에게 말 걸기" }));
expect(screen.getByRole("status")).toHaveTextContent("다음 시대");
expect(container.querySelectorAll(".planet-speech-bubble")).toHaveLength(1);
```

- [ ] **Step 2: Run** the targeted PlanetScene and SharingPanels tests; expect new interaction cases to fail.
- [ ] **Step 3: Implement** two separate transparent focusable button hit areas over the SVG (avatar above planet in stacking order), a single positioned HTML bubble with `role="status"`, and a close timer that is cleared/restarted on every interaction. Track the last known object IDs by `cycleId`; initial mount seeds the set without announcing growth, later new IDs may supply `newObjectKind`. Do not infer object creation for `publicOnly`. Keep the bubble inside the figure at edge path positions. Pause and cancel walking timers while the bubble is open; resume with the retained index when it closes. Clear every timer on unmount and visibility/motion changes.

```tsx
{interactive && <div className="planet-interaction-layer">
  <button type="button" className="planet-hit-area" aria-label="행성에게 말 걸기"
    onClick={() => speak("planet")} />
  <button type="button" className="avatar-hit-area" aria-label="아바타에게 말 걸기"
    onClick={() => speak("avatar")} />
</div>}
{line && <div className="planet-speech-bubble" role="status">{line}</div>}
```
- [ ] **Step 4: Run** targeted tests, full frontend tests `npm --prefix apps/desktop test`, and `npm --prefix apps/desktop run build`; inspect 390px popover and detailed/selected group scenes for clipped bubble, usable hit areas, visible focus, preserved pixel appearance and reduced motion. Run `git diff --check`.
- [ ] **Step 5: Commit** Task 3 files with `feat(planet): 행성과 아바타 말풍선 상호작용 추가`.

## Final Verification

- Re-run the targeted scene/dialogue/sharing tests and frontend build after integration. Report exact results rather than implying native checks from unit tests.
- Visually inspect own popover, own detail, selected group detail, and small group gallery. Check avatar positions at both path ends, repeated clicks, keyboard focus, bubble clipping, reduced motion and screen hide/show.
- Check that no usage data, score or sync payload changed. Confirm `git status --short` includes only pre-existing unrelated changes after focused commits.
