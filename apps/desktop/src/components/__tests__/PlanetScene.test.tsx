import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PlanetScene, planWalk, WALK_POINTS } from "../PlanetScene";
import { restDuration, stepDuration } from "../sceneMotion";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("planet cosmetics", () => {
  it("draws all replacement and new art styles at every planet stage", () => {
    for (const stage of [0, 1, 2, 3, 4]) {
      const { container, unmount } = render(
        <PlanetScene
          stage={stage}
          progress={0.5}
          equippedCosmetics={[
            { slot_id: "sky", sku: "star_cluster_v2" },
            { slot_id: "sky", sku: "aurora_v2" },
            { slot_id: "ring", sku: "thin_ring_v2" },
            { slot_id: "ring", sku: "double_ring_v2" },
            { slot_id: "surface", sku: "flag_v2" },
            { slot_id: "surface", sku: "crystal_tower_v2" },
            { slot_id: "sky", sku: "meteor_shower" },
            { slot_id: "ring", sku: "moonlets" },
            { slot_id: "surface", sku: "flower_garden" },
            { slot_id: "surface", sku: "observatory" },
            { slot_id: "forecourt", sku: "pond" },
            { slot_id: "forecourt", sku: "lantern" },
            { slot_id: "forecourt", sku: "rover" },
            { slot_id: "forecourt", sku: "greenhouse" },
            { slot_id: "sky", sku: "future_item" },
            { slot_id: "future_slot", sku: "star_cluster" },
          ]}
        />,
      );

      const drawn = [...container.querySelectorAll("[data-cosmetic]")].map((node) => node.getAttribute("data-cosmetic"));
      expect(drawn).toHaveLength(14);
      expect(drawn).toEqual(expect.arrayContaining([
        "star_cluster", "aurora", "thin_ring", "double_ring", "flag", "crystal_tower",
        "meteor_shower", "moonlets", "flower_garden", "observatory", "pond", "lantern", "rover", "greenhouse",
      ]));
      expect(drawn).not.toContain("future_item");
      unmount();
    }
  });

  it("draws one equipped item in each of four slots at once", () => {
    const { container } = render(<PlanetScene stage={3} progress={0.4} equippedCosmetics={[
      { slot_id: "sky", sku: "star_cluster_v2" },
      { slot_id: "ring", sku: "moonlets" },
      { slot_id: "surface", sku: "observatory" },
      { slot_id: "forecourt", sku: "pond" },
    ]} />);

    expect([...container.querySelectorAll("[data-cosmetic]")].map((node) => node.getAttribute("data-cosmetic")))
      .toEqual(expect.arrayContaining(["star_cluster", "moonlets", "observatory", "pond"]));
    expect(container.querySelectorAll("[data-cosmetic]")).toHaveLength(4);
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

describe("planet speech interaction", () => {
  it("exposes separate focusable planet and avatar targets with one status bubble", () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    const { container } = render(<PlanetScene stage={1} progress={0} interactive />);
    const planetButton = screen.getByRole("button", { name: "행성에게 말 걸기" });
    const avatarButton = screen.getByRole("button", { name: "아바타에게 말 걸기" });

    expect(planetButton).toHaveAttribute("type", "button");
    expect(avatarButton).toHaveAttribute("type", "button");
    expect(planetButton).toHaveProperty("tabIndex", 0);
    expect(avatarButton).toHaveProperty("tabIndex", 0);

    fireEvent.click(planetButton);
    expect(screen.getByRole("status")).toHaveTextContent("작은 정착지가 생겼어.");
    expect(container.querySelectorAll(".planet-speech-bubble")).toHaveLength(1);

    fireEvent.click(avatarButton);
    expect(screen.getByRole("status")).toHaveTextContent("오늘은 어디를 둘러볼까?");
    expect(container.querySelectorAll(".planet-speech-bubble")).toHaveLength(1);
  });

  it("supports Enter and Space activation on both interaction targets", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    const user = userEvent.setup();
    render(<PlanetScene stage={1} progress={0} interactive />);
    const planetButton = screen.getByRole("button", { name: "행성에게 말 걸기" });
    const avatarButton = screen.getByRole("button", { name: "아바타에게 말 걸기" });

    await user.tab();
    expect(planetButton).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("status")).toHaveTextContent("작은 정착지가 생겼어.");
    await user.tab();
    expect(avatarButton).toHaveFocus();
    await user.keyboard(" ");
    expect(screen.getByRole("status")).toHaveTextContent("오늘은 어디를 둘러볼까?");
  });

  it("restarts the four-second close timer on each interaction", () => {
    vi.useFakeTimers();
    render(<PlanetScene stage={1} progress={0} interactive />);
    const planetButton = screen.getByRole("button", { name: "행성에게 말 걸기" });
    fireEvent.click(planetButton);
    act(() => vi.advanceTimersByTime(3_000));
    fireEvent.click(planetButton);
    act(() => vi.advanceTimersByTime(3_999));
    expect(screen.getByRole("status")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(1));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("allows speech while the document is hidden without starting a walk", () => {
    vi.useFakeTimers();
    vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    const { container } = render(<PlanetScene stage={0} progress={0} interactive animate />);
    const avatar = container.querySelector("[data-planet-avatar]")!;
    const initialTransform = avatar.getAttribute("transform");

    fireEvent.click(screen.getByRole("button", { name: "아바타에게 말 걸기" }));
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(container.querySelector(".planet-figure")).toHaveAttribute("data-motion", "paused");
    act(() => vi.advanceTimersByTime(15_000));
    expect(avatar).toHaveAttribute("transform", initialTransform);
  });

  it("allows speech with reduced motion without starting a walk", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
    const { container } = render(<PlanetScene stage={0} progress={0} interactive animate />);
    const avatar = container.querySelector("[data-planet-avatar]")!;
    const initialTransform = avatar.getAttribute("transform");

    fireEvent.click(screen.getByRole("button", { name: "행성에게 말 걸기" }));
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(container.querySelector(".planet-figure")).toHaveAttribute("data-motion", "reduced");
    act(() => vi.advanceTimersByTime(15_000));
    expect(avatar).toHaveAttribute("transform", initialTransform);
  });

  it("pauses a walk during speech and resumes from its retained path position", () => {
    vi.useFakeTimers();
    vi.spyOn(Math, "random").mockReturnValue(0);
    const { container } = render(<PlanetScene stage={0} progress={0} interactive animate />);
    const avatar = container.querySelector("[data-planet-avatar]")!;
    const initialTransform = avatar.getAttribute("transform");

    act(() => vi.advanceTimersByTime(1_500));
    const walkingTransform = avatar.getAttribute("transform");
    expect(walkingTransform).not.toBe(initialTransform);
    expect(avatar).toHaveClass("planet-avatar--walking");

    fireEvent.click(screen.getByRole("button", { name: "아바타에게 말 걸기" }));
    expect(avatar.getAttribute("transform")).toBe(walkingTransform);
    expect(avatar).not.toHaveClass("planet-avatar--walking");
    act(() => vi.advanceTimersByTime(4_000));
    expect(avatar.getAttribute("transform")).toBe(walkingTransform);
    act(() => vi.advanceTimersByTime(1_499));
    expect(avatar.getAttribute("transform")).toBe(walkingTransform);
    act(() => vi.advanceTimersByTime(1));
    expect(avatar.getAttribute("transform")).not.toBe(walkingTransform);
    expect(avatar).toHaveClass("planet-avatar--walking");
  });

  it("announces only objects added after the current cycle is known", () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    const firstObject = { stage: 0, ordinal: 1, kind: "rock", x: 10, y: 30, seed: 1 } as const;
    const newObject = { stage: 0, ordinal: 2, kind: "tree", x: 20, y: 30, seed: 2 } as const;
    const { rerender } = render(<PlanetScene stage={0} progress={0} interactive cycleId="cycle-1" objects={[firstObject]} />);
    fireEvent.click(screen.getByRole("button", { name: "행성에게 말 걸기" }));
    expect(screen.getByRole("status")).toHaveTextContent("이곳에서 첫발을 떼자.");

    rerender(<PlanetScene stage={0} progress={0} interactive cycleId="cycle-1" objects={[firstObject, newObject]} />);
    fireEvent.click(screen.getByRole("button", { name: "아바타에게 말 걸기" }));
    expect(screen.getByRole("status")).toHaveTextContent("새로운 나무가 생겼어!");

    rerender(<PlanetScene stage={0} progress={0} interactive cycleId="cycle-2" objects={[firstObject, newObject]} />);
    fireEvent.click(screen.getByRole("button", { name: "행성에게 말 걸기" }));
    expect(screen.getByRole("status")).toHaveTextContent("이곳에서 첫발을 떼자.");
  });

  it("does not render interaction controls for compact gallery scenes", () => {
    render(<PlanetScene stage={0} progress={0} compact />);
    expect(screen.queryByRole("button", { name: "행성에게 말 걸기" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "아바타에게 말 걸기" })).not.toBeInTheDocument();
  });
});
