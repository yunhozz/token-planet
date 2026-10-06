import { useEffect, useLayoutEffect, useState, type RefObject } from "react";

const COMPACT_HEIGHT = "(max-height: 599px)";

export function useCompactPopup(popupRef?: RefObject<HTMLElement | null>, enabled = true): boolean {
  const [shortHeight, setShortHeight] = useState(() => window.matchMedia?.(COMPACT_HEIGHT).matches ?? false);
  const [contentCompact, setContentCompact] = useState(false);
  const compact = enabled && (shortHeight || contentCompact);

  useEffect(() => {
    if (!window.matchMedia) return;
    const media = window.matchMedia(COMPACT_HEIGHT);
    const update = () => setShortHeight(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  useLayoutEffect(() => {
    const root = popupRef?.current;
    if (!enabled) {
      setContentCompact(false);
      root?.style.removeProperty("--popup-scene-width");
      if (root) {
        delete root.dataset.popupFit;
        delete root.dataset.popupOverflow;
        delete root.dataset.popupAvailableHeight;
        delete root.dataset.popupRequiredHeight;
      }
      return;
    }
    if (!root) return;
    const layout = root.querySelector<HTMLElement>(".world-layout");
    if (!layout) return;
    const measure = () => {
      root.style.removeProperty("--popup-scene-width");
      let overflow = Math.max(0, layout.scrollHeight - layout.clientHeight);
      if (!compact && overflow > 0) {
        setContentCompact(true);
        return;
      }
      if (compact && overflow > 0) {
        const scene = root.querySelector<HTMLElement>(".planet-scene-canvas");
        if (scene) {
          // Shrink only the scene; text and controls keep their readable size.
          const bounds = scene.getBoundingClientRect();
          const width = Math.max(0, bounds.width - overflow * 360 / 320);
          root.style.setProperty("--popup-scene-width", `${width}px`);
          overflow = Math.max(0, layout.scrollHeight - layout.clientHeight);
        }
      }
      root.dataset.popupFit = overflow > 0 ? "insufficient" : "fit";
      root.dataset.popupOverflow = String(overflow);
      root.dataset.popupAvailableHeight = String(layout.clientHeight);
      root.dataset.popupRequiredHeight = String(layout.scrollHeight);
    };
    measure();
    const resize = () => {
      // Reconsider the full summary after the available window size changes.
      setContentCompact(false);
      measure();
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(root);
    observer?.observe(layout);
    for (const child of layout.children) observer?.observe(child);
    const mutations = new MutationObserver(() => {
      if (compact && !shortHeight) {
        // Content changed: render the full summary once and measure it in the
        // next layout effect. ResizeObserver alone must not cause this probe.
        setContentCompact(false);
      } else {
        measure();
      }
    });
    mutations.observe(layout, { subtree: true, childList: true, characterData: true });
    window.addEventListener("resize", resize);
    return () => {
      observer?.disconnect();
      mutations.disconnect();
      window.removeEventListener("resize", resize);
    };
  }, [popupRef, enabled, compact, shortHeight]);

  return compact;
}
