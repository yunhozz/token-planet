import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PlanetLandscape, planetLandscapeBounds, type PlanetExplorationState } from "../PlanetLandscape";
import { PlanetLandscapeDecorations } from "../PlanetLandscapeDecorations";
import { fitLandscape } from "../planetLandscapeCamera";
import type { PlanetObject } from "../../types/usage";

class TestResizeObserver {
  static latest: TestResizeObserver | null = null;
  private target: Element | null = null;

  constructor(private callback: ResizeObserverCallback) {
    TestResizeObserver.latest = this;
  }

  observe(target: Element) { this.target = target; }
  disconnect() {}
  unobserve() {}

  resize(width: number, height: number) {
    if (!this.target) throw new Error("ResizeObserver was not attached");
    const entry = { target: this.target, contentRect: { width, height } as DOMRectReadOnly } as ResizeObserverEntry;
    this.callback([entry], this as unknown as ResizeObserver);
  }
}

afterEach(() => vi.unstubAllGlobals());

const objects = [
  { stage: 0, ordinal: 0, kind: "water", x: 34, y: 29, seed: 14 },
  { stage: 2, ordinal: 0, kind: "house", x: 52, y: 45, seed: 39 },
] as const;

const selectableObjects: PlanetObject[] = [
  { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
  { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
];

function ControlledLandscape({
  objects = selectableObjects,
  stage = 2,
  avatar = "masculine",
  equippedCosmetics = [],
}: {
  objects?: PlanetObject[];
  stage?: number;
  avatar?: "masculine" | "feminine";
  equippedCosmetics?: { slot_id: string; sku: string; version: number }[];
}) {
  const [exploration, setExploration] = useState<PlanetExplorationState>(() => ({
    camera: fitLandscape(planetLandscapeBounds(objects)),
    selectedObjectId: null,
  }));

  return (
    <PlanetLandscape
      stage={stage}
      progress={0.35}
      avatar={avatar}
      objects={objects}
      equippedCosmetics={equippedCosmetics}
      incomplete={false}
      cycleId="cycle-1"
      exploration={exploration}
      onExplorationChange={setExploration}
    />
  );
}

describe("planet landscape artwork", () => {
  it("shows a flat sky and ground without stretching or hiding saved objects", () => {
    const { container } = render(
      <ControlledLandscape stage={2} objects={[...objects]} />,
    );
    const svg = container.querySelector(".planet-landscape-svg");

    expect(svg).toHaveAttribute("preserveAspectRatio", "xMidYMid meet");
    expect(svg).toHaveAttribute("shape-rendering", "crispEdges");
    expect(container.querySelector("clipPath")).not.toBeInTheDocument();
    expect(container.querySelector(".planet-landscape-sky")).toBeInTheDocument();
    expect(container.querySelector(".planet-landscape-ground")).toBeInTheDocument();
    expect([...container.querySelectorAll("[data-landscape-object-id]")].map((node) => node.getAttribute("data-landscape-object-id")))
      .toEqual(["0-0", "2-0"]);
  });

  it("keeps decorative era scenery separate from actual objects on an empty planet", () => {
    const { container } = render(
      <ControlledLandscape stage={0} avatar="feminine" objects={[]} />,
    );

    expect(container.querySelector("[data-landscape-decoration='natural-stream']")).toBeInTheDocument();
    expect(container.querySelector("[data-landscape-object-id]")).not.toBeInTheDocument();
    expect(container.querySelector(".planet-landscape-avatar")).toBeInTheDocument();
  });

  it("draws each recognized equipped cosmetic in its matching landscape layer", () => {
    const { container } = render(
      <ControlledLandscape
        stage={4}
        objects={[{ stage: 0, ordinal: 0, kind: "rock", x: 12, y: 40, seed: 8 }]}
        equippedCosmetics={[
          { slot_id: "sky", sku: "star_cluster_v2", version: 1 },
          { slot_id: "sky", sku: "aurora_v2", version: 1 },
          { slot_id: "sky", sku: "meteor_shower", version: 1 },
          { slot_id: "ring", sku: "thin_ring_v2", version: 1 },
          { slot_id: "ring", sku: "double_ring_v2", version: 1 },
          { slot_id: "ring", sku: "moonlets", version: 1 },
          { slot_id: "surface", sku: "flag_v2", version: 1 },
          { slot_id: "surface", sku: "crystal_tower_v2", version: 1 },
          { slot_id: "surface", sku: "flower_garden", version: 1 },
          { slot_id: "surface", sku: "observatory", version: 1 },
          { slot_id: "forecourt", sku: "pond", version: 1 },
          { slot_id: "forecourt", sku: "lantern", version: 1 },
          { slot_id: "forecourt", sku: "rover", version: 1 },
          { slot_id: "forecourt", sku: "greenhouse", version: 1 },
        ]}
      />,
    );

    const cosmetics = [...container.querySelectorAll("[data-cosmetic]")];
    expect(cosmetics.map((node) => node.getAttribute("data-cosmetic"))).toEqual([
      "star_cluster", "aurora", "meteor_shower", "thin_ring", "double_ring", "moonlets",
      "flag", "crystal_tower", "flower_garden", "observatory", "pond", "lantern", "rover", "greenhouse",
    ]);
    expect(container.querySelectorAll("[data-landscape-object-id]")).toHaveLength(1);
    expect(container.querySelector("[data-cosmetic='star_cluster']")?.closest("[data-landscape-slot='sky']"))
      .not.toBeNull();
    expect(container.querySelector("[data-cosmetic='thin_ring']")?.closest("[data-landscape-slot='ring']"))
      .not.toBeNull();
    expect(container.querySelector("[data-cosmetic='pond']")?.closest("[data-landscape-slot='forecourt']"))
      .not.toBeNull();
  });

  it("keeps stars, grass, and distant silhouettes fixed in world space while the camera pans", () => {
    const bounds = { x: 0, y: 0, width: 600, height: 320 };
    const renderDecorations = (x: number) => render(
      <svg>
        <PlanetLandscapeDecorations
          stage={2}
          bounds={bounds}
          viewBox={{ x, y: -220, width: 800, height: 540 }}
          equippedCosmetics={[]}
        />
      </svg>,
    );
    const first = renderDecorations(0);
    const second = renderDecorations(145);

    const expectSharedGeometry = (selector: string, attributeNames: string[]) => {
      const geometry = (container: HTMLElement) => new Map(
        [...container.querySelectorAll(selector)].map((node) => {
          const key = node.getAttribute("x") ?? node.getAttribute("d") ?? "";
          return [key, attributeNames.map((name) => node.getAttribute(name))];
        }),
      );
      const firstGeometry = geometry(first.container);
      const secondGeometry = geometry(second.container);
      const sharedKeys = [...firstGeometry.keys()].filter((key) => secondGeometry.has(key));

      expect(sharedKeys.length).toBeGreaterThan(0);
      for (const key of sharedKeys) {
        expect(secondGeometry.get(key)).toEqual(firstGeometry.get(key));
      }
    };

    expectSharedGeometry(".planet-landscape-stars rect", ["x", "y", "width", "height"]);
    expectSharedGeometry(".planet-landscape-ground-dressing rect", ["x", "y", "width", "height"]);
    expectSharedGeometry(".planet-landscape-distant-ground path", ["d"]);

    first.unmount();
    second.unmount();
  });

  it("selects a saved object from the list and shows its mapped name, era, and one-based order", () => {
    const { container } = render(<ControlledLandscape />);
    const svg = container.querySelector(".planet-landscape-svg")!;
    const initialViewBox = svg.getAttribute("viewBox");

    fireEvent.click(screen.getByRole("region", { name: "오브젝트 목록" }).querySelector('[data-object-list-id="1-3"]')!);

    const selected = screen.getByRole("region", { name: "선택한 오브젝트" });
    expect(selected).toHaveTextContent("나무");
    expect(selected).toHaveTextContent("정착·농경");
    expect(selected).toHaveTextContent("4번째");
    expect(selected).not.toHaveTextContent(/20\d\d[-./년]/);
    expect(within(screen.getByRole("region", { name: "오브젝트 목록" }))
      .getByRole("button", { name: /나무.*4번째/ })).toHaveAttribute("aria-pressed", "true");
    expect(svg.getAttribute("viewBox")).not.toBe(initialViewBox);
  });

  it("provides a separate keyboard-accessible scene target and list entry for every saved object", () => {
    const manyCollocated: PlanetObject[] = Array.from({ length: 120 }, (_, ordinal) => ({
      stage: 0, ordinal, kind: "rock", x: 50, y: 50, seed: 1,
    }));
    const { container } = render(<ControlledLandscape objects={manyCollocated} />);

    expect(container.querySelectorAll("[data-landscape-object-id]")).toHaveLength(manyCollocated.length);
    expect(container.querySelectorAll("[data-landscape-hit-id]")).toHaveLength(manyCollocated.length);
    expect(within(screen.getByRole("region", { name: "오브젝트 목록" })).getAllByRole("button"))
      .toHaveLength(manyCollocated.length);
    expect(screen.queryByText(/^\+\d/)).not.toBeInTheDocument();
  });

  it("explains when an empty planet has no saved objects", () => {
    render(<ControlledLandscape objects={[]} />);

    expect(screen.getByRole("region", { name: "오브젝트 목록" }))
      .toHaveTextContent("아직 생성된 오브젝트가 없습니다.");
    expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
  });

  it("zooms, restores the full view, and pans with arrow keys only while the scene is focused", () => {
    const { container } = render(<ControlledLandscape />);
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector(".planet-landscape-svg")!;
    const initialViewBox = svg.getAttribute("viewBox");

    fireEvent.click(screen.getByRole("button", { name: "확대" }));
    const zoomedViewBox = svg.getAttribute("viewBox");
    expect(zoomedViewBox).not.toBe(initialViewBox);

    viewport.focus();
    fireEvent.keyDown(viewport, { key: "ArrowDown" });
    expect(svg.getAttribute("viewBox")).not.toBe(zoomedViewBox);

    fireEvent.click(screen.getByRole("button", { name: "전체 보기" }));
    expect(svg.getAttribute("viewBox")).toBe(initialViewBox);
  });

  it("pans by pointer and ends the drag on pointer cancellation", () => {
    const { container } = render(<ControlledLandscape />);
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector(".planet-landscape-svg")!;

    fireEvent.click(screen.getByRole("button", { name: "확대" }));
    const zoomedViewBox = svg.getAttribute("viewBox");
    fireEvent.pointerDown(viewport, { pointerId: 7, button: 0, clientX: 240, clientY: 220 });
    fireEvent.pointerMove(viewport, { pointerId: 7, clientX: 240, clientY: 170 });
    const draggedViewBox = svg.getAttribute("viewBox");
    expect(draggedViewBox).not.toBe(zoomedViewBox);
    expect(viewport).toHaveClass("is-dragging");

    fireEvent.pointerCancel(viewport, { pointerId: 7 });
    expect(viewport).not.toHaveClass("is-dragging");
    fireEvent.pointerMove(viewport, { pointerId: 7, clientX: 240, clientY: 80 });
    expect(svg.getAttribute("viewBox")).toBe(draggedViewBox);
  });

  it("reclamps camera geometry after ResizeObserver reports a new viewport", () => {
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
    const { container } = render(<ControlledLandscape />);
    const svg = container.querySelector(".planet-landscape-svg")!;
    const root = container.querySelector<HTMLElement>(".planet-landscape")!;

    fireEvent.click(screen.getByRole("button", { name: "확대" }));
    expect(Number(root.dataset.cameraZoom)).toBeGreaterThan(1);
    act(() => TestResizeObserver.latest!.resize(360, 500));

    const [x, y, width, height] = svg.getAttribute("viewBox")!.split(" ").map(Number);
    expect(width / 360).toBeCloseTo(height / 500);
    expect(Number.isFinite(x) && Number.isFinite(y)).toBe(true);
    expect(Number(root.dataset.cameraCenterX)).toBeGreaterThanOrEqual(0);
    expect(Number(root.dataset.cameraCenterX)).toBeLessThanOrEqual(600);
    expect(Number(root.dataset.cameraZoom)).toBeGreaterThan(1);
  });

  it("applies camera changes instantly when reduced motion is preferred", () => {
    vi.stubGlobal("matchMedia", () => ({
      matches: true,
      media: "(prefers-reduced-motion: reduce)",
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    } as MediaQueryList));
    const { container } = render(<ControlledLandscape />);
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;

    fireEvent.click(screen.getByRole("button", { name: "확대" }));

    expect(svg.style.transition).toBe("none");
    expect(Number(container.querySelector<HTMLElement>(".planet-landscape")!.dataset.cameraZoom)).toBeGreaterThan(1);
  });

  it("selects a scene target and clears that selection if the object disappears", () => {
    const { container, rerender } = render(<ControlledLandscape />);

    fireEvent.click(container.querySelector('[data-landscape-hit-id="0-0"]')!);
    expect(screen.getByRole("region", { name: "선택한 오브젝트" })).toHaveTextContent("바위");
    expect(within(screen.getByRole("region", { name: "오브젝트 목록" }))
      .getByRole("button", { name: /바위.*1번째/ })).toHaveAttribute("aria-pressed", "true");

    rerender(<ControlledLandscape objects={[]} />);
    expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "오브젝트 목록" }))
      .toHaveTextContent("아직 생성된 오브젝트가 없습니다.");
  });
});
