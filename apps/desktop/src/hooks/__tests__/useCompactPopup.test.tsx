import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useCompactPopup } from "../useCompactPopup";
function heightMedia(matches: boolean) {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const media = { matches, addEventListener: (_: string, listener: (event: MediaQueryListEvent) => void) => listeners.add(listener), removeEventListener: (_: string, listener: (event: MediaQueryListEvent) => void) => listeners.delete(listener) };
  vi.stubGlobal("matchMedia", () => media);
  return { listeners, change(next: boolean) { media.matches = next; listeners.forEach(listener => listener({ matches: next } as MediaQueryListEvent)); } };
}
afterEach(() => vi.unstubAllGlobals());
it("uses_compact_popup_below_600px", () => { heightMedia(true); expect(renderHook(useCompactPopup).result.current).toBe(true); });
it("updates_when_available_height_changes", () => {
  const media = heightMedia(false); const { result } = renderHook(useCompactPopup);
  expect(result.current).toBe(false); act(() => media.change(true)); expect(result.current).toBe(true);
  act(() => media.change(false)); expect(result.current).toBe(false);
});
it("removes_height_listener_on_unmount", () => {
  const media = heightMedia(false); const { unmount } = renderHook(useCompactPopup);
  expect(media.listeners.size).toBe(1); unmount(); expect(media.listeners.size).toBe(0);
});
it("uses_core_summary_when_content_exceeds_available_height", () => {
  heightMedia(false);
  const element = document.createElement("main");
  element.innerHTML = '<div class="world-layout"></div>';
  const layout = element.firstElementChild!;
  Object.defineProperty(layout, "scrollHeight", { configurable: true, value: 720 });
  Object.defineProperty(layout, "clientHeight", { configurable: true, value: 650 });
  const ref = { current: element };
  expect(renderHook(() => useCompactPopup(ref, true)).result.current).toBe(true);
});
it("does_not_compact_detail_or_other_platforms_for_content_overflow", () => {
  heightMedia(false);
  const element = document.createElement("main");
  element.innerHTML = '<div class="world-layout"></div>';
  Object.defineProperty(element.firstElementChild!, "scrollHeight", { value: 720 });
  Object.defineProperty(element.firstElementChild!, "clientHeight", { value: 650 });
  expect(renderHook(() => useCompactPopup({ current: element }, false)).result.current).toBe(false);
});
it("reports_when_even_core_content_cannot_fit", () => {
  heightMedia(true);
  const element = document.createElement("main");
  element.innerHTML = '<div class="world-layout"></div>';
  Object.defineProperty(element.firstElementChild!, "scrollHeight", { value: 320 });
  Object.defineProperty(element.firstElementChild!, "clientHeight", { value: 200 });
  renderHook(() => useCompactPopup({ current: element }, true));
  expect(element.dataset.popupFit).toBe("insufficient");
  expect(element.dataset.popupOverflow).toBe("120");
});
it("rechecks_full_summary_after_returning_from_detail", () => {
  heightMedia(false);
  const element = document.createElement("main");
  element.innerHTML = '<div class="world-layout"></div>';
  const layout = element.firstElementChild!;
  Object.defineProperty(layout, "scrollHeight", { configurable: true, value: 720 });
  Object.defineProperty(layout, "clientHeight", { configurable: true, value: 650 });
  const ref = { current: element };
  const { result, rerender } = renderHook(({ enabled }) => useCompactPopup(ref, enabled), { initialProps: { enabled: true } });
  expect(result.current).toBe(true);
  rerender({ enabled: false });
  expect(result.current).toBe(false);
  Object.defineProperty(layout, "scrollHeight", { configurable: true, value: 500 });
  rerender({ enabled: true });
  expect(result.current).toBe(false);
});
it("restores_full_summary_after_content_shrinks_without_resize", async () => {
  heightMedia(false);
  const element = document.createElement("main");
  element.innerHTML = '<div class="world-layout"><p>transition error</p></div>';
  const layout = element.firstElementChild!;
  Object.defineProperty(layout, "scrollHeight", { configurable: true, value: 720 });
  Object.defineProperty(layout, "clientHeight", { configurable: true, value: 650 });
  const ref = { current: element };
  const { result } = renderHook(() => useCompactPopup(ref, true));
  expect(result.current).toBe(true);
  await act(async () => {
    Object.defineProperty(layout, "scrollHeight", { configurable: true, value: 500 });
    layout.firstElementChild!.textContent = "";
    await Promise.resolve();
  });
  expect(result.current).toBe(false);
});
it("keeps_short_height_compact_after_content_shrinks", async () => {
  heightMedia(true);
  const element = document.createElement("main");
  element.innerHTML = '<div class="world-layout"><p>transition error</p></div>';
  const layout = element.firstElementChild!;
  Object.defineProperty(layout, "scrollHeight", { configurable: true, value: 720 });
  Object.defineProperty(layout, "clientHeight", { configurable: true, value: 650 });
  const ref = { current: element };
  const { result } = renderHook(() => useCompactPopup(ref, true));
  await act(async () => {
    Object.defineProperty(layout, "scrollHeight", { configurable: true, value: 500 });
    layout.firstElementChild!.textContent = "";
    await Promise.resolve();
  });
  expect(result.current).toBe(true);
});
