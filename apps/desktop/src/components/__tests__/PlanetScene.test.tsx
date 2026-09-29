import { act, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PlanetScene, planWalk, WALK_POINTS } from "../PlanetScene";
import { restDuration, stepDuration } from "../sceneMotion";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("planet cosmetics", () => {
  it("draws supported slot and SKU pairs at every planet stage", () => {
    for (const stage of [0, 1, 2, 3, 4]) {
      const { container, unmount } = render(
        <PlanetScene
          stage={stage}
          progress={0.5}
          equippedCosmetics={[
            { slot_id: "sky", sku: "star_cluster" },
            { slot_id: "sky", sku: "aurora" },
            { slot_id: "ring", sku: "thin_ring" },
            { slot_id: "ring", sku: "double_ring" },
            { slot_id: "surface", sku: "flag" },
            { slot_id: "surface", sku: "crystal_tower" },
            { slot_id: "sky", sku: "future_item" },
            { slot_id: "future_slot", sku: "star_cluster" },
          ]}
        />,
      );

      expect([...container.querySelectorAll("[data-cosmetic]")].map((node) => node.getAttribute("data-cosmetic")))
        .toEqual(["thin_ring", "double_ring", "star_cluster", "aurora", "flag", "crystal_tower"]);
      unmount();
    }
  });
});

it("keeps compact member scenes static even when motion is requested", () => {
  const { container } = render(<PlanetScene stage={0} progress={0} compact animate />);
  expect(container.querySelector(".planet-figure")).toHaveAttribute("data-motion", "paused");
});

it("plans bounded walks from the middle and both path edges", () => {
  const starts = [0, Math.floor(WALK_POINTS.length / 2), WALK_POINTS.length - 1];
  for (const start of starts) {
    for (const value of [0, 0.5, 0.99]) {
      const route = planWalk(start, null, () => value);
      expect(route.length).toBeGreaterThanOrEqual(1);
      expect(route.length).toBeLessThanOrEqual(5);
      let previous = start;
      for (const index of route) {
        expect(index).toBeGreaterThanOrEqual(0);
        expect(index).toBeLessThan(WALK_POINTS.length);
        expect(Math.abs(index - previous)).toBe(1);
        previous = index;
      }
    }
  }
});

it("rerolls a route that would immediately return to the previous destination", () => {
  const start = Math.floor(WALK_POINTS.length / 2);
  const previousDestination = start + 5;
  const route = planWalk(start, previousDestination, () => 0.99);
  expect(route[route.length - 1]).not.toBe(previousDestination);
  expect(route[0]).toBe(start - 1);
});

it("samples rest and step durations at their inclusive endpoints", () => {
  expect(restDuration(() => 0)).toBe(1500);
  expect(restDuration(() => 1)).toBe(5000);
  expect(stepDuration(() => 0)).toBe(300);
  expect(stepDuration(() => 1)).toBe(550);
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

it("does not replay an interrupted object entrance when the scene returns", async () => {
  let callback: IntersectionObserverCallback | undefined;
  let target: Element | undefined;
  class VisibleObserver {
    constructor(observerCallback: IntersectionObserverCallback) { callback = observerCallback; }
    observe(observed: Element) {
      target = observed;
      callback?.([{ isIntersecting: true, target: observed } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
    }
    disconnect() {}
    unobserve() {}
    takeRecords() { return []; }
  }
  vi.stubGlobal("IntersectionObserver", VisibleObserver);
  const object = { stage: 0, ordinal: 1, kind: "rock", x: 10, y: 30, seed: 1 } as const;
  const { container, rerender } = render(<PlanetScene stage={0} progress={0} objects={[]} animate />);
  const figure = container.querySelector(".planet-figure")!;
  const observer = callback!;
  rerender(<PlanetScene stage={0} progress={0} objects={[object]} animate />);
  expect(container.querySelector(".planet-object--entering")).toBeInTheDocument();
  act(() => observer([{ isIntersecting: false, target: target! } as IntersectionObserverEntry], {} as IntersectionObserver));
  expect(figure).toHaveAttribute("data-motion", "paused");
  await waitFor(() => expect(container.querySelector(".planet-object--entering")).not.toBeInTheDocument());
  act(() => observer([{ isIntersecting: true, target: target! } as IntersectionObserverEntry], {} as IntersectionObserver));
  expect(figure).toHaveAttribute("data-motion", "active");
  expect(container.querySelector(".planet-object--entering")).not.toBeInTheDocument();
});

it("pauses movement while the document is hidden", () => {
  vi.useFakeTimers();
  vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  const { container } = render(<PlanetScene stage={0} progress={0} animate />);
  const avatar = container.querySelector("[data-planet-avatar]");
  expect(avatar).not.toBeNull();
  const transform = avatar?.getAttribute("transform");
  act(() => vi.advanceTimersByTime(15_000));
  expect(avatar).toHaveAttribute("transform", transform);
});

it("pauses movement while the scene is off-screen", () => {
  vi.useFakeTimers();
  class OffscreenObserver {
    constructor(private callback: IntersectionObserverCallback) {}
    observe(target: Element) {
      this.callback([{ isIntersecting: false, target } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
    }
    disconnect() {}
    unobserve() {}
    takeRecords() { return []; }
  }
  vi.stubGlobal("IntersectionObserver", OffscreenObserver);
  const { container } = render(<PlanetScene stage={0} progress={0} animate />);
  const avatar = container.querySelector("[data-planet-avatar]");
  expect(avatar).not.toBeNull();
  const transform = avatar?.getAttribute("transform");
  act(() => vi.advanceTimersByTime(15_000));
  expect(avatar).toHaveAttribute("transform", transform);
});

it("resumes movement after the idle pause when the scene returns on-screen", () => {
  vi.useFakeTimers();
  let callback: IntersectionObserverCallback | undefined;
  let target: Element | undefined;
  class ControlledObserver {
    constructor(observerCallback: IntersectionObserverCallback) { callback = observerCallback; }
    observe(observed: Element) {
      target = observed;
      callback?.([{ isIntersecting: false, target: observed } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
    }
    disconnect() {}
    unobserve() {}
    takeRecords() { return []; }
  }
  vi.stubGlobal("IntersectionObserver", ControlledObserver);
  vi.spyOn(Math, "random").mockReturnValue(0);
  const { container } = render(<PlanetScene stage={0} progress={0} animate />);
  const avatar = container.querySelector("[data-planet-avatar]")!;
  const initialTransform = avatar.getAttribute("transform");
  const observer = callback!;
  const scene = target!;

  act(() => observer([{ isIntersecting: true, target: scene } as IntersectionObserverEntry], {} as IntersectionObserver));
  expect(container.querySelector(".planet-figure")).toHaveAttribute("data-motion", "active");
  act(() => vi.advanceTimersByTime(1500));
  const walkingTransform = avatar.getAttribute("transform");
  expect(walkingTransform).not.toBe(initialTransform);
  expect(container.querySelector("[data-planet-avatar]")).toHaveClass("planet-avatar--walking");

  act(() => observer([{ isIntersecting: false, target: scene } as IntersectionObserverEntry], {} as IntersectionObserver));
  expect(container.querySelector(".planet-figure")).toHaveAttribute("data-motion", "paused");
  expect(container.querySelector("[data-planet-avatar]")).not.toHaveClass("planet-avatar--walking");
  act(() => vi.advanceTimersByTime(5000));
  expect(avatar).toHaveAttribute("transform", walkingTransform);

  act(() => observer([{ isIntersecting: true, target: scene } as IntersectionObserverEntry], {} as IntersectionObserver));
  act(() => vi.advanceTimersByTime(1499));
  expect(avatar).toHaveAttribute("transform", walkingTransform);
  act(() => vi.advanceTimersByTime(1));
  expect(avatar.getAttribute("transform")).not.toBe(walkingTransform);
});

it("uses a complete static scene when reduced motion is requested", async () => {
  vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  const { container } = render(<PlanetScene stage={0} progress={0} animate />);
  await waitFor(() => expect(container.querySelector(".planet-figure")).toHaveAttribute("data-motion", "reduced"));
});
