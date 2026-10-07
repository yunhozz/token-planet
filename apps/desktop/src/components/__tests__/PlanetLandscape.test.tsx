import { landscapeAvatarRoute } from "../landscapeAvatarRoute";
import { layoutLandscape } from "../planetLandscapeLayout";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PlanetLandscape, planetLandscapeBounds, type PlanetExplorationState } from "../PlanetLandscape";
import { PlanetLandscapeDecorations } from "../PlanetLandscapeDecorations";
import { fitLandscape, landscapeViewBox } from "../planetLandscapeCamera";
import type { AvatarEquipment, LandscapeInstance, NaturalObjectKey, PlanetObject, ShopActionResult, ShopProduct, ShopRequest, ShopState } from "../../types/usage";

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
  avatarEquipment,
  equippedCosmetics = [],
  cycleId = "cycle-1",
  shopState,
  shopAccountId = null,
  terrainObjects,
  selectedLandscapeInstanceId = null,
  pendingShopAction = null,
  onShopAction,
  onSelectLandscapeInstance,
  onRequestNaturalRemoval,
  canRequestNaturalRemoval,
  createShopRequestId,
}: {
  objects?: PlanetObject[];
  stage?: number;
  avatar?: "masculine" | "feminine";
  avatarEquipment?: AvatarEquipment;
  equippedCosmetics?: { slot_id: string; sku: string; version: number }[];
  cycleId?: string;
  shopState?: ShopState | null;
  shopAccountId?: string | null;
  terrainObjects?: PlanetObject[];
  selectedLandscapeInstanceId?: string | null;
  pendingShopAction?: { request: ShopRequest; status: "submitting" | "uncertain"; error: string | null } | null;
  onShopAction?: (request: ShopRequest) => Promise<ShopActionResult | null>;
  onSelectLandscapeInstance?: (instanceId: string | null) => void;
  onRequestNaturalRemoval?: (key: NaturalObjectKey, label: string) => void;
  canRequestNaturalRemoval?: (key: NaturalObjectKey) => boolean;
  createShopRequestId?: () => string;
}) {
  const [exploration, setExploration] = useState<PlanetExplorationState>(() => ({
    camera: fitLandscape(planetLandscapeBounds(terrainObjects ?? objects)),
    selectedObjectId: null,
  }));

  return (
    <PlanetLandscape
      stage={stage}
      progress={0.35}
      avatar={avatar}
      avatarEquipment={avatarEquipment}
      objects={objects}
      equippedCosmetics={equippedCosmetics}
      incomplete={false}
      cycleId={cycleId}
      exploration={exploration}
      onExplorationChange={setExploration}
      shopState={shopState}
      shopAccountId={shopAccountId}
      terrainObjects={terrainObjects}
      selectedLandscapeInstanceId={selectedLandscapeInstanceId}
      pendingShopAction={pendingShopAction}
      onShopAction={onShopAction}
      onSelectLandscapeInstance={onSelectLandscapeInstance}
      onRequestNaturalRemoval={onRequestNaturalRemoval}
      canRequestNaturalRemoval={canRequestNaturalRemoval}
      createShopRequestId={createShopRequestId}
    />
  );
}

const landscapeProducts: ShopProduct[] = [
  {
    sku: "land_pond", category: "landscape", display_name: "연못", price: 5_000_000,
    catalog_revision: 1, purchasable: true, placement_zone: "ground", avatar_slot: null, effect_type: null, effect_value: 0,
  },
  {
    sku: "land_thin_ring", category: "landscape", display_name: "얇은 고리", price: 5_000_000,
    catalog_revision: 1, purchasable: true, placement_zone: "sky", avatar_slot: null, effect_type: null, effect_value: 0,
  },
];

function landscapeShopState(overrides: Partial<ShopState> = {}): ShopState {
  return {
    account_id: "local",
    current_cycle_id: "cycle-1",
    catalog_revision: 1,
    state_revision: 1,
    available_balance: 10_000_000,
    products: landscapeProducts,
    landscape_instances: [],
    placements: [],
    removed_natural_keys: [],
    avatar_owned_skus: [],
    avatar_equipment: {
      head: { sku: null, version: 0 },
      outfit: { sku: null, version: 0 },
      face: { sku: null, version: 0 },
      back: { sku: null, version: 0 },
    },
    effects: {
      token_earning_bps: 0,
      civilization_growth_bps: 0,
      shop_discount_bps: 0,
      reset_cooldown_bps: 0,
      natural_removal_discount_bps: 0,
      era_reward_tokens: 0,
      streak_reward_tokens: 0,
    },
    reward_state: { reward_timezone: "UTC", settled_cycle_tokens: 0, era_reward_tokens: 0, streak_reward_tokens: 0 },
    action_unavailable_reason: null,
    guest_import_pending: false,
    guest_import_error: null,
    ...overrides,
  };
}

function landscapeInstance(instance_id: string, sku = "land_pond", placement_version = 0, variation_index = 0): LandscapeInstance {
  return { instance_id, sku, placement_version, variation_index, seed: `seed:${instance_id}`, variation_version: 1 };
}

function emptyAvatarEquipment(): AvatarEquipment {
  return {
    head: { sku: null, version: 0 },
    outfit: { sku: null, version: 0 },
    face: { sku: null, version: 0 },
    back: { sku: null, version: 0 },
  };
}

function screenPointForWorld(
  svg: SVGSVGElement,
  rect: { left: number; top: number; width: number; height: number },
  x: number,
  y: number,
) {
  const [viewX, viewY, viewWidth, viewHeight] = svg.getAttribute("viewBox")!.split(" ").map(Number);
  const scale = Math.min(rect.width / viewWidth, rect.height / viewHeight);
  return {
    clientX: rect.left + (rect.width - viewWidth * scale) / 2 + (x - viewX) * scale,
    clientY: rect.top + (rect.height - viewHeight * scale) / 2 + (y - viewY) * scale,
  };
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
          const key = node.getAttribute("x") ?? node.getAttribute("d") ?? node.getAttribute("transform") ?? "";
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
    expectSharedGeometry(".planet-landscape-ground-dressing [data-landscape-dressing='grass']", ["transform"]);
    expectSharedGeometry(".planet-landscape-distant-ground path", ["d"]);

    first.unmount();
    second.unmount();
  });

  it("selects a saved scene object and shows its mapped name and era without moving the camera", () => {
    const { container } = render(<ControlledLandscape />);
    const svg = container.querySelector(".planet-landscape-svg")!;
    const initialViewBox = svg.getAttribute("viewBox");

    fireEvent.click(container.querySelector('[data-landscape-hit-id="1-3"]')!);

    const selected = screen.getByRole("region", { name: "선택한 오브젝트" });
    expect(selected).toHaveTextContent("나무");
    expect(selected).toHaveTextContent("정착·농경");
    expect(selected).not.toHaveTextContent(/\d+번째/);
    expect(selected).not.toHaveTextContent(/20\d\d[-./년]/);
    expect(container.querySelector('[data-landscape-hit-id="1-3"]')).toHaveAttribute("aria-pressed", "true");
    expect(svg.getAttribute("viewBox")).toBe(initialViewBox);
  });

  it("requests removal only for the selected natural object's current cycle key", () => {
    const onRequestNaturalRemoval = vi.fn();
    const { container } = render(
      <ControlledLandscape
        cycleId="cycle-7"
        onRequestNaturalRemoval={onRequestNaturalRemoval}
      />,
    );
    fireEvent.click(container.querySelector('[data-landscape-hit-id="1-3"]')!);

    const selection = screen.getByRole("region", { name: "선택한 오브젝트" });
    fireEvent.click(within(selection).getByRole("button", { name: "자연물 제거" }));

    expect(onRequestNaturalRemoval).toHaveBeenCalledWith(
      { cycle_id: "cycle-7", stage: 1, ordinal: 3 },
      "나무",
    );
  });

  it("disables the removal action when its parent rejects the selected natural key", () => {
    const onRequestNaturalRemoval = vi.fn();
    const { container } = render(
      <ControlledLandscape
        onRequestNaturalRemoval={onRequestNaturalRemoval}
        canRequestNaturalRemoval={() => false}
      />,
    );
    fireEvent.click(container.querySelector('[data-landscape-hit-id="0-0"]')!);

    const selection = screen.getByRole("region", { name: "선택한 오브젝트" });
    const remove = within(selection).getByRole("button", { name: "자연물 제거" });
    expect(remove).toBeDisabled();
    fireEvent.click(remove);
    expect(onRequestNaturalRemoval).not.toHaveBeenCalled();
  });

  it("provides a keyboard-accessible scene target for every saved object without a separate list", () => {
    const manyCollocated: PlanetObject[] = Array.from({ length: 120 }, (_, ordinal) => ({
      stage: 0, ordinal, kind: "rock", x: 50, y: 50, seed: 1,
    }));
    const { container } = render(<ControlledLandscape objects={manyCollocated} />);

    expect(container.querySelectorAll("[data-landscape-object-id]")).toHaveLength(manyCollocated.length);
    expect(container.querySelectorAll("[data-landscape-hit-id]")).toHaveLength(manyCollocated.length);
    expect(screen.queryByRole("region", { name: "오브젝트 목록" })).not.toBeInTheDocument();
    expect(screen.queryByText(/^\+\d/)).not.toBeInTheDocument();
  });

  it("omits the list and object detail when an empty planet has no saved objects", () => {
    render(<ControlledLandscape objects={[]} />);

    expect(screen.queryByRole("region", { name: "오브젝트 목록" })).not.toBeInTheDocument();
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
    expect(Number(root.dataset.cameraCenterX)).toBeLessThanOrEqual(planetLandscapeBounds(selectableObjects).width);
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
    expect(container.querySelector('[data-landscape-hit-id="0-0"]')).toHaveAttribute("aria-pressed", "true");

    rerender(<ControlledLandscape objects={[]} />);
    expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "오브젝트 목록" })).not.toBeInTheDocument();
  });
});

describe("canonical shop landscape integration", () => {
  it("places the selected inventory instance at a pointer-mapped location with its canonical edit version", async () => {
    const instance = landscapeInstance("pond-1", "land_pond", 4);
    const state = landscapeShopState({ landscape_instances: [instance] });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "placed",
      request_id: request.request_id,
      confirmed_quote: null,
      state,
    }));
    const { container } = render(
      <ControlledLandscape
        shopState={state}
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={onShopAction}
        createShopRequestId={() => "place-request-1"}
      />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
    const rect = { left: 37, top: 51, width: 900, height: 420 };
    Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
    const [viewX, viewY, viewWidth, viewHeight] = svg.getAttribute("viewBox")!.split(" ").map(Number);
    const scale = Math.min(rect.width / viewWidth, rect.height / viewHeight);
    const letterboxX = (rect.width - viewWidth * scale) / 2;
    const letterboxY = (rect.height - viewHeight * scale) / 2;
    const screenPoint = (x: number, y: number) => ({
      clientX: rect.left + letterboxX + (x - viewX) * scale,
      clientY: rect.top + letterboxY + (y - viewY) * scale,
    });
    const from = screenPoint(760, 160);
    const drop = screenPoint(832, 232);

    await act(async () => {
      fireEvent.pointerDown(viewport, { pointerId: 8, button: 0, ...from });
      fireEvent.pointerMove(viewport, { pointerId: 8, ...drop });
      fireEvent.pointerUp(viewport, { pointerId: 8, ...drop });
    });
    expect(onShopAction).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "배치 확정" }));
      await Promise.resolve();
    });

    expect(onShopAction).toHaveBeenCalledTimes(1);
    expect(onShopAction).toHaveBeenCalledWith({
      kind: "place",
      request_id: "place-request-1",
      cycle_id: "cycle-1",
      instance_id: "pond-1",
      expected_version: 4,
      x: 800,
      y: 200,
    });
  });

  it("uses the pointer-up coordinate when the browser omits a final pointer-move", async () => {
    const instance = landscapeInstance("pond-pointerup");
    const state = landscapeShopState({ landscape_instances: [instance] });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "placed", request_id: request.request_id, confirmed_quote: null, state,
    }));
    const { container } = render(
      <ControlledLandscape
        shopState={state}
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={onShopAction}
        createShopRequestId={() => "pointerup-request-1"}
      />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
    const rect = { left: 0, top: 0, width: 900, height: 420 };
    Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });

    await act(async () => {
      fireEvent.pointerDown(viewport, { pointerId: 9, button: 0, ...screenPointForWorld(svg, rect, 760, 160) });
      fireEvent.pointerUp(viewport, { pointerId: 9, ...screenPointForWorld(svg, rect, 832, 232) });
    });
    expect(onShopAction).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "배치 확정" }));
      await Promise.resolve();
    });
    expect(onShopAction).toHaveBeenCalledWith(expect.objectContaining({ kind: "place", x: 800, y: 200 }));
  });

  it("renders current-cycle purchased placements in stable y and instance-id order", () => {
    const placements = [
      { instance_id: "z-low", cycle_id: "cycle-1", x: 380, y: 240, version: 2 },
      { instance_id: "b-high", cycle_id: "cycle-1", x: 220, y: 120, version: 1 },
      { instance_id: "a-low", cycle_id: "cycle-1", x: 160, y: 240, version: 3 },
      { instance_id: "old-cycle", cycle_id: "cycle-0", x: 100, y: 100, version: 1 },
    ];
    const state = landscapeShopState({
      landscape_instances: [
        landscapeInstance("z-low"),
        landscapeInstance("b-high"),
        landscapeInstance("a-low"),
        landscapeInstance("old-cycle"),
      ],
      placements,
    });
    const { container } = render(<ControlledLandscape shopState={state} />);

    expect([...container.querySelectorAll("[data-shop-instance-id]")].map((node) => node.getAttribute("data-shop-instance-id")))
      .toEqual(["b-high", "a-low", "z-low"]);
    expect(container.querySelector('[data-shop-instance-id="b-high"]')).toHaveAttribute("transform", "translate(220 120)");
    expect(container.querySelector('[data-shop-instance-id="a-low"] [data-landscape-sku="land_pond"]')).toBeInTheDocument();
    expect(container.querySelector('[data-shop-instance-id="old-cycle"]')).not.toBeInTheDocument();
  });

  it("moves an existing purchased placement with its pointer offset and captured edit version", async () => {
    const instance = landscapeInstance("pond-move", "land_pond", 3);
    const state = landscapeShopState({
      landscape_instances: [instance],
      placements: [{ instance_id: instance.instance_id, cycle_id: "cycle-1", x: 800, y: 200, version: 3 }],
    });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "placed", request_id: request.request_id, confirmed_quote: null, state,
    }));
    const { container } = render(
      <ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} createShopRequestId={() => "move-request-1"} />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
    const rect = { left: 40, top: 60, width: 900, height: 420 };
    Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
    const object = container.querySelector<SVGGElement>('[data-shop-instance-id="pond-move"]')!;

    fireEvent.click(screen.getByRole("button", { name: "위치 이동" }));
    await act(async () => {
      fireEvent.pointerDown(object, { pointerId: 10, button: 0, ...screenPointForWorld(svg, rect, 820, 220) });
      fireEvent.pointerMove(viewport, { pointerId: 10, ...screenPointForWorld(svg, rect, 920, 320) });
      fireEvent.pointerUp(viewport, { pointerId: 10, ...screenPointForWorld(svg, rect, 920, 320) });
    });
    expect(onShopAction).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "배치 확정" }));
      await Promise.resolve();
    });

    expect(onShopAction).toHaveBeenCalledWith({
      kind: "place", request_id: "move-request-1", cycle_id: "cycle-1", instance_id: "pond-move",
      expected_version: 3, x: 900, y: 300,
    });
  });

  it("rejects a pointer drag if canonical placement version changes before drop", () => {
    const original = landscapeInstance("pond-race", "land_pond", 3);
    const updated = landscapeInstance("pond-race", "land_pond", 4);
    const state3 = landscapeShopState({
      landscape_instances: [original],
      placements: [{ instance_id: original.instance_id, cycle_id: "cycle-1", x: 800, y: 200, version: 3 }],
    });
    const state4 = landscapeShopState({
      landscape_instances: [updated],
      placements: [{ instance_id: updated.instance_id, cycle_id: "cycle-1", x: 500, y: 200, version: 4 }],
    });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "placed", request_id: request.request_id, confirmed_quote: null, state: state4,
    }));
    const { container, rerender } = render(
      <ControlledLandscape shopState={state3} selectedLandscapeInstanceId={original.instance_id} onShopAction={onShopAction} createShopRequestId={() => "stale-pointer-request"} />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
    const rect = { left: 0, top: 0, width: 900, height: 420 };
    Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
    const object = container.querySelector<SVGGElement>('[data-shop-instance-id="pond-race"]')!;

    fireEvent.click(screen.getByRole("button", { name: "위치 이동" }));
    fireEvent.pointerDown(object, { pointerId: 41, button: 0, ...screenPointForWorld(svg, rect, 820, 220) });
    rerender(<ControlledLandscape shopState={state4} onShopAction={onShopAction} createShopRequestId={() => "stale-pointer-request"} />);
    fireEvent.pointerMove(viewport, { pointerId: 41, ...screenPointForWorld(svg, rect, 920, 320) });
    fireEvent.pointerUp(viewport, { pointerId: 41, ...screenPointForWorld(svg, rect, 920, 320) });

    expect(onShopAction).not.toHaveBeenCalled();
    expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
    expect(container.querySelector('[data-shop-instance-id="pond-race"]')).toHaveAttribute("transform", "translate(500 200)");
  });

  it("cancels a keyboard draft when canonical placement version changes", () => {
    const original = landscapeInstance("pond-key-race", "land_pond", 3);
    const updated = landscapeInstance("pond-key-race", "land_pond", 4);
    const state3 = landscapeShopState({ landscape_instances: [original] });
    const state4 = landscapeShopState({ landscape_instances: [updated] });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "placed", request_id: request.request_id, confirmed_quote: null, state: state4,
    }));
    const { container, rerender } = render(
      <ControlledLandscape
        shopState={state3}
        selectedLandscapeInstanceId={original.instance_id}
        onShopAction={onShopAction}
        createShopRequestId={() => "stale-keyboard-request"}
      />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    viewport.focus();
    fireEvent.keyDown(viewport, { key: " " });
    rerender(
      <ControlledLandscape
        shopState={state4}
        selectedLandscapeInstanceId={updated.instance_id}
        onShopAction={onShopAction}
        createShopRequestId={() => "stale-keyboard-request"}
      />,
    );
    expect(onShopAction).not.toHaveBeenCalled();
    expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
  });

  it("starts keyboard relocation at a placed instance and confirms only after explicit movement", async () => {
    const instance = landscapeInstance("pond-key-move", "land_pond", 5);
    const state = landscapeShopState({
      landscape_instances: [instance],
      placements: [{ instance_id: instance.instance_id, cycle_id: "cycle-1", x: 800, y: 200, version: 5 }],
    });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "placed", request_id: request.request_id, confirmed_quote: null, state,
    }));
    const onSelectLandscapeInstance = vi.fn();
    const { container, rerender } = render(
      <ControlledLandscape shopState={state} onShopAction={onShopAction} onSelectLandscapeInstance={onSelectLandscapeInstance} />,
    );
    const object = container.querySelector<SVGGElement>('[data-shop-instance-id="pond-key-move"]')!;
    fireEvent.click(object);
    expect(onSelectLandscapeInstance).toHaveBeenCalledWith(instance.instance_id);
    rerender(
      <ControlledLandscape
        shopState={state}
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={onShopAction}
        onSelectLandscapeInstance={onSelectLandscapeInstance}
        createShopRequestId={() => "keyboard-move-request"}
      />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    viewport.focus();
    fireEvent.click(screen.getByRole("button", { name: "위치 이동" }));
    expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-x", "800");
    expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-y", "200");
    fireEvent.keyDown(viewport, { key: "ArrowRight" });
    expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-x", "808");
    fireEvent.keyDown(viewport, { key: "Escape" });
    expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
    expect(container.querySelector('[data-shop-instance-id="pond-key-move"]')).toHaveAttribute("transform", "translate(800 200)");
    expect(onShopAction).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "위치 이동" }));
    fireEvent.keyDown(viewport, { key: "ArrowRight" });
    await act(async () => {
      fireEvent.keyDown(viewport, { key: "Enter" });
      await Promise.resolve();
    });
    expect(onShopAction).toHaveBeenCalledWith({
      kind: "place", request_id: "keyboard-move-request", cycle_id: "cycle-1", instance_id: instance.instance_id,
      expected_version: 5, x: 808, y: 200,
    });
    expect(container.querySelector('[data-shop-instance-id="pond-key-move"]')).toHaveAttribute("transform", "translate(800 200)");
  });

  it("renders canonical avatar equipment layers in the live landscape avatar", () => {
    const avatarEquipment = {
      ...emptyAvatarEquipment(),
      head: { sku: "avatar_space_helmet", version: 3 },
      outfit: { sku: "avatar_spacesuit", version: 2 },
      face: { sku: "avatar_goggles", version: 4 },
      back: { sku: "avatar_wings", version: 1 },
    } satisfies AvatarEquipment;
    const state = landscapeShopState({
      avatar_owned_skus: ["avatar_space_helmet", "avatar_spacesuit", "avatar_goggles", "avatar_wings"],
      avatar_equipment: avatarEquipment,
    });
    const { container, rerender } = render(
      <ControlledLandscape shopState={state} avatarEquipment={state.avatar_equipment} />,
    );
    const avatar = container.querySelector(".planet-landscape-avatar-sprite")!;
    expect(avatar.querySelector("[data-avatar-equipment-layers='true']")).toHaveAttribute("data-avatar-style", "masculine");
    expect([...avatar.querySelectorAll("[data-avatar-layer]")].map((layer) => layer.getAttribute("data-avatar-layer")))
      .toEqual(["back", "base", "outfit", "face", "head"]);
    for (const sku of state.avatar_owned_skus) {
      expect(avatar.querySelector(`[data-avatar-equipment='${sku}']`)).toBeInTheDocument();
    }

    rerender(<ControlledLandscape avatar="feminine" shopState={state} avatarEquipment={state.avatar_equipment} />);
    expect(container.querySelector(".planet-landscape-avatar-sprite [data-avatar-equipment-layers='true']"))
      .toHaveAttribute("data-avatar-style", "feminine");
  });

  it("selects a placed object on click without submitting a no-op placement", () => {
    const instance = landscapeInstance("pond-click", "land_pond", 2);
    const state = landscapeShopState({
      landscape_instances: [instance],
      placements: [{ instance_id: instance.instance_id, cycle_id: "cycle-1", x: 800, y: 200, version: 2 }],
    });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "placed", request_id: request.request_id, confirmed_quote: null, state,
    }));
    const onSelectLandscapeInstance = vi.fn();
    const { container } = render(
      <ControlledLandscape
        shopState={state}
        onShopAction={onShopAction}
        onSelectLandscapeInstance={onSelectLandscapeInstance}
      />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
    const rect = { left: 0, top: 0, width: 900, height: 420 };
    Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
    const object = container.querySelector<SVGGElement>('[data-shop-instance-id="pond-click"]')!;
    const point = screenPointForWorld(svg, rect, 820, 220);

    fireEvent.pointerDown(object, { pointerId: 30, button: 0, ...point });
    fireEvent.pointerUp(viewport, { pointerId: 30, ...point });
    fireEvent.click(object);

    expect(onSelectLandscapeInstance).toHaveBeenCalledWith("pond-click");
    expect(onShopAction).not.toHaveBeenCalled();
  });

  it("retains invalid and clears canceled placement previews without changing canonical placements", () => {
    const instance = landscapeInstance("pond-draft");
    const state = landscapeShopState({ landscape_instances: [instance] });
    const onShopAction = vi.fn(async (): Promise<ShopActionResult> => { throw new Error("should not submit"); });
    const { container } = render(
      <ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
    const rect = { left: 0, top: 0, width: 900, height: 420 };
    Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });

    fireEvent.pointerDown(viewport, { pointerId: 11, button: 0, ...screenPointForWorld(svg, rect, 760, 160) });
    fireEvent.pointerMove(viewport, { pointerId: 11, ...screenPointForWorld(svg, rect, 10, 165) });
    expect(container.querySelector("[data-shop-preview-valid='false']")).toBeInTheDocument();
    fireEvent.pointerUp(viewport, { pointerId: 11, ...screenPointForWorld(svg, rect, 10, 165) });
    expect(onShopAction).not.toHaveBeenCalled();
    expect(container.querySelector("[data-shop-preview-instance-id]")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "배치 확정" })).toBeDisabled();
    expect(screen.getByText(/배치 구역을 벗어났습니다/)).toBeInTheDocument();
    expect(container.querySelectorAll("[data-shop-instance-id]")).toHaveLength(0);

    fireEvent.pointerDown(viewport, { pointerId: 12, button: 0, ...screenPointForWorld(svg, rect, 760, 160) });
    fireEvent.pointerMove(viewport, { pointerId: 12, ...screenPointForWorld(svg, rect, 832, 232) });
    expect(container.querySelector("[data-shop-preview-valid='true']")).toBeInTheDocument();
    fireEvent.pointerCancel(viewport, { pointerId: 12 });
    expect(onShopAction).not.toHaveBeenCalled();
    expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
  });

  it("uses keyboard placement with arrow movement, Enter submit, and Escape cancel", async () => {
    const instance = landscapeInstance("pond-keyboard");
    const state = landscapeShopState({ landscape_instances: [instance] });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "placed", request_id: request.request_id, confirmed_quote: null, state,
    }));
    const { container } = render(
      <ControlledLandscape
        shopState={state}
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={onShopAction}
        createShopRequestId={() => "keyboard-request-1"}
      />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    viewport.focus();
    fireEvent.keyDown(viewport, { key: " " });
    expect(container.querySelector("[data-shop-preview-valid='true']")).toBeInTheDocument();
    fireEvent.keyDown(viewport, { key: "ArrowRight" });
    const movedX = Number(container.querySelector<HTMLElement>("[data-shop-preview-instance-id]")?.dataset.shopPreviewX);
    expect(movedX).toBeGreaterThan(8);
    fireEvent.keyDown(viewport, { key: "Escape" });
    expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
    expect(onShopAction).not.toHaveBeenCalled();

    fireEvent.keyDown(viewport, { key: "Enter" });
    await act(async () => {
      fireEvent.keyDown(viewport, { key: "Enter" });
      await Promise.resolve();
    });
    expect(onShopAction).toHaveBeenCalledWith(expect.objectContaining({
      kind: "place", request_id: "keyboard-request-1", instance_id: "pond-keyboard", expected_version: 0,
    }));
  });

  it("sends retrieval with the current cycle and canonical instance version", async () => {
    const instance = landscapeInstance("pond-retrieve", "land_pond", 7);
    const state = landscapeShopState({
      landscape_instances: [instance],
      placements: [{ instance_id: instance.instance_id, cycle_id: "cycle-1", x: 240, y: 200, version: 7 }],
    });
    const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "retrieved", request_id: request.request_id, confirmed_quote: null, state,
    }));
    const onSelectLandscapeInstance = vi.fn();
    const { container, rerender } = render(
      <ControlledLandscape shopState={state} onShopAction={onShopAction} onSelectLandscapeInstance={onSelectLandscapeInstance} />,
    );
    fireEvent.click(container.querySelector('[data-shop-instance-id="pond-retrieve"]')!);
    expect(onSelectLandscapeInstance).toHaveBeenCalledWith("pond-retrieve");
    rerender(
      <ControlledLandscape
        shopState={state}
        selectedLandscapeInstanceId="pond-retrieve"
        onShopAction={onShopAction}
        onSelectLandscapeInstance={onSelectLandscapeInstance}
        createShopRequestId={() => "retrieve-request-1"}
      />,
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "보관함으로" }));
      await Promise.resolve();
    });
    expect(onShopAction).toHaveBeenCalledWith({
      kind: "retrieve", request_id: "retrieve-request-1", cycle_id: "cycle-1", instance_id: "pond-retrieve", expected_version: 7,
    });
    expect(screen.getByRole("status")).toHaveTextContent("장식을 보관함으로 옮겼습니다.");
  });

  it("keeps natural layout positions when a removed natural object remains in the tombstone terrain basis", () => {
    const { container: before } = render(<ControlledLandscape objects={selectableObjects} terrainObjects={selectableObjects} />);
    const originalTransform = before.querySelector('[data-landscape-object-id="1-3"] .planet-object-sprite')?.getAttribute("transform");
    before.remove();

    const { container: after } = render(
      <ControlledLandscape
        objects={[selectableObjects[1]]}
        terrainObjects={selectableObjects}
        shopState={landscapeShopState({ removed_natural_keys: [{ cycle_id: "cycle-1", stage: 0, ordinal: 0 }] })}
      />,
    );
    expect(after.querySelector('[data-landscape-object-id="0-0"]')).not.toBeInTheDocument();
    expect(after.querySelector('[data-landscape-object-id="1-3"] .planet-object-sprite')).toHaveAttribute("transform", originalTransform);
  });

  it("does not turn natural scene objects into shop placement drags", () => {
    const instance = landscapeInstance("pond-available");
    const state = landscapeShopState({ landscape_instances: [instance] });
    const onShopAction = vi.fn(async (): Promise<ShopActionResult> => { throw new Error("should not submit"); });
    const { container } = render(
      <ControlledLandscape
        objects={[selectableObjects[0]]}
        shopState={state}
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={onShopAction}
      />,
    );
    const natural = container.querySelector<SVGGElement>('[data-landscape-hit-id="0-0"]')!;
    fireEvent.pointerDown(natural, { pointerId: 13, button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(container.querySelector(".planet-landscape-viewport")!, { pointerId: 13, clientX: 200, clientY: 200 });
    fireEvent.pointerUp(container.querySelector(".planet-landscape-viewport")!, { pointerId: 13, clientX: 200, clientY: 200 });
    expect(onShopAction).not.toHaveBeenCalled();
    expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-x", "678");
  });

  it("blocks duplicate requests and ignores an in-flight result after the cycle changes", async () => {
    const instance = landscapeInstance("pond-context");
    const state1 = landscapeShopState({ landscape_instances: [instance] });
    const state2 = landscapeShopState({ current_cycle_id: "cycle-2", landscape_instances: [instance] });
    let resolveAction!: (result: ShopActionResult) => void;
    const pendingResult = new Promise<ShopActionResult>((resolve) => { resolveAction = resolve; });
    const onShopAction = vi.fn(() => pendingResult);
    const { container, rerender } = render(
      <ControlledLandscape shopState={state1} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
    const rect = { left: 0, top: 0, width: 900, height: 420 };
    Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
    const from = screenPointForWorld(svg, rect, 760, 160);
    const drop = screenPointForWorld(svg, rect, 832, 232);

    await act(async () => {
      fireEvent.pointerDown(viewport, { pointerId: 20, button: 0, ...from });
      fireEvent.pointerMove(viewport, { pointerId: 20, ...drop });
      fireEvent.pointerUp(viewport, { pointerId: 20, ...drop });
    });
    expect(onShopAction).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "배치 확정" }));
      await Promise.resolve();
    });
    expect(onShopAction).toHaveBeenCalledTimes(1);

    fireEvent.pointerDown(viewport, { pointerId: 21, button: 0, ...from });
    fireEvent.pointerMove(viewport, { pointerId: 21, ...drop });
    fireEvent.pointerUp(viewport, { pointerId: 21, ...drop });
    expect(onShopAction).toHaveBeenCalledTimes(1);

    rerender(
      <ControlledLandscape
        cycleId="cycle-2"
        shopState={state2}
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={onShopAction}
      />,
    );
    expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
    await act(async () => {
      resolveAction({ status: "placed", request_id: "old-response", confirmed_quote: null, state: state1 });
      await Promise.resolve();
    });
    expect(container.querySelector('[data-shop-instance-id="pond-context"]')).not.toBeInTheDocument();
    expect(container.querySelector(".planet-landscape-shop-status")).not.toBeInTheDocument();
  });

  it("clears a preview when canonical shop state belongs to another account", () => {
    const instance = landscapeInstance("pond-account");
    const ownState = landscapeShopState({ account_id: "account-a", landscape_instances: [instance] });
    const foreignState = landscapeShopState({ account_id: "account-b", landscape_instances: [instance] });
    const { container, rerender } = render(
      <ControlledLandscape
        shopState={ownState}
        shopAccountId="account-a"
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={vi.fn(async (): Promise<ShopActionResult> => { throw new Error("not used"); })}
      />,
    );
    const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
    const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
    const rect = { left: 0, top: 0, width: 900, height: 420 };
    Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
    fireEvent.pointerDown(viewport, { pointerId: 22, button: 0, ...screenPointForWorld(svg, rect, 832, 232) });
    expect(container.querySelector("[data-shop-preview-instance-id]")).toBeInTheDocument();

    rerender(
      <ControlledLandscape
        shopState={foreignState}
        shopAccountId="account-a"
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={vi.fn(async (): Promise<ShopActionResult> => { throw new Error("not used"); })}
      />,
    );
    expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
    rerender(
      <ControlledLandscape
        shopState={ownState}
        shopAccountId="account-a"
        selectedLandscapeInstanceId={instance.instance_id}
        onShopAction={vi.fn(async (): Promise<ShopActionResult> => { throw new Error("not used"); })}
      />,
    );
    expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
  });
});

it("places selection information in document flow below the scenery", async () => {
  const { readFileSync } = await import(/* @vite-ignore */ "node:" + "fs");
  const appCss = readFileSync("src/App.css", "utf8");
  expect(appCss).toMatch(/\.app-shell--detail \.planet-landscape-object-detail\s*\{[^}]*position:\s*static/);
});

it("gives detail supporting information a consistent readable surface", async () => {
  const { readFileSync } = await import(/* @vite-ignore */ "node:" + "fs");
  const appCss = readFileSync("src/App.css", "utf8");
  expect(appCss).toMatch(/\.app-shell--detail \.shop-effects-summary[^}]*border-radius:\s*16px/);
});

describe("hidden growth scenery", () => {
  const hidden = ["road", "path", "rail", "fern"].map((kind, ordinal) => ({
    stage: 3, ordinal: ordinal + 10, kind, x: 80 + ordinal, y: 60, seed: ordinal,
  }));
  const original = [...selectableObjects, ...hidden, ...Array.from({ length: 260 }, (_, ordinal) => ({
    stage: 0, ordinal: ordinal + 100, kind: "fern", x: 50, y: 40, seed: ordinal,
  }))];

  it("excludes hidden objects from artwork and hit areas while preserving original layout", () => {
    const { container, rerender } = render(<ControlledLandscape objects={original} />);
    const viewBox = container.querySelector(".planet-landscape-svg")?.getAttribute("viewBox")
      ?? container.querySelector("svg")!.getAttribute("viewBox");
    const tree = layoutLandscape(original).objects.find((placement) => placement.id === "1-3")!;
    const treeSelector = '[data-landscape-object-id="1-3"] .planet-object-sprite';
    expect(container.querySelector(treeSelector)).toHaveAttribute("transform", `translate(${tree.x} ${tree.y})`);
    const rawBounds = planetLandscapeBounds(original);
    const expectedView = landscapeViewBox(rawBounds, { width: 1200, height: 420 }, fitLandscape(rawBounds));
    expect(viewBox).toBe(`${expectedView.x} ${expectedView.y} ${expectedView.width} ${expectedView.height}`);
    expect(container.querySelectorAll("[data-landscape-object-id]")).toHaveLength(2);
    for (const { ordinal } of hidden) {
      expect(container.querySelector(`[data-landscape-object-id="3-${ordinal}"]`)).toBeNull();
    }
    rerender(<ControlledLandscape objects={selectableObjects} terrainObjects={original} />);
    expect(container.querySelector("svg")!.getAttribute("viewBox")).toBe(viewBox);
    expect(container.querySelector(treeSelector)).toHaveAttribute("transform", `translate(${tree.x} ${tree.y})`);
  });

  it.each(hidden)("clears selected $kind without moving its camera", (object) => {
    const camera = { ...fitLandscape(planetLandscapeBounds(original)), zoom: 1.4 };
    const onExplorationChange = vi.fn();
    const { container } = render(<PlanetLandscape stage={3} progress={.5} avatar="masculine" objects={original}
      cycleId="cycle-1" equippedCosmetics={[]} incomplete={false} exploration={{ camera, selectedObjectId: `3-${object.ordinal}` }}
      onExplorationChange={onExplorationChange} />);
    expect(screen.queryByRole("group", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
    expect(container.querySelector(".planet-landscape-object-detail")).toBeNull();
    expect(onExplorationChange).toHaveBeenCalledWith({ camera, selectedObjectId: null });
  });

  it.each([0, 1, 2, 3, 4])("removes era road decoration at stage %s", (stage) => {
    const bounds = { x: 0, y: 0, width: 600, height: 320 };
    const { container } = render(<svg><PlanetLandscapeDecorations stage={stage} bounds={bounds}
      viewBox={{ x: 0, y: -220, width: 800, height: 540 }} equippedCosmetics={[]} /></svg>);
    expect(container.querySelector('[data-landscape-decoration="era-road"]')).toBeNull();
    expect(container.querySelector(".planet-landscape-ground")).toBeInTheDocument();
    if (stage === 0) expect(container.querySelector('[data-landscape-decoration="natural-stream"]')).toBeInTheDocument();
  });
});

it("starts an unplaced selection card and saves a single click draft only on confirmation", async () => {
  const instance = landscapeInstance("click-draft", "land_pond", 2, 3);
  const state = landscapeShopState({ landscape_instances: [instance] });
  const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({ status: "placed", request_id: request.request_id, confirmed_quote: null, state }));
  const { container } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />);
  const card = screen.getByRole("group", { name: "선택한 장식" });
  expect(within(card).getByText("연못")).toBeInTheDocument();
  expect(within(card).getByText("배치 중")).toBeInTheDocument();
  expect(within(card).getByText("풍경을 클릭하거나 드래그해 위치를 고르세요. 방향키로 조정하고 Enter로 확정할 수 있습니다.")).toBeInTheDocument();
  expect(within(card).getByRole("img", { name: "연못 미리보기" })).toBeInTheDocument();
  expect(container.querySelector("[data-shop-preview-valid='true']")).toBeInTheDocument();
  const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
  const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
  const rect = { left: 0, top: 0, width: 900, height: 420 };
  Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
  const point = screenPointForWorld(svg, rect, 832, 232);
  fireEvent.pointerDown(viewport, { pointerId: 88, button: 0, ...point });
  fireEvent.pointerUp(viewport, { pointerId: 88, ...point });
  expect(onShopAction).not.toHaveBeenCalled();
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-x", "800");
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "배치 확정" })); });
  expect(onShopAction).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ kind: "place", instance_id: instance.instance_id, expected_version: 2, x: 800, y: 200 }));
  expect(screen.getByRole("status")).toHaveTextContent("장식을 배치했습니다.");
});

it.each(["확대", "축소"])("keeps preview and submitted world coordinates equal after %s and viewport resize", async (zoom) => {
  vi.stubGlobal("ResizeObserver", TestResizeObserver);
  const instance = landscapeInstance("zoom-draft");
  const state = landscapeShopState({ landscape_instances: [instance] });
  const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({ status: "placed", request_id: request.request_id, confirmed_quote: null, state }));
  const { container } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />);
  act(() => TestResizeObserver.latest!.resize(620, 460));
  fireEvent.click(screen.getByRole("button", { name: zoom }));
  const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
  const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
  const rect = { left: 17, top: 31, width: 620, height: 460 };
  Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
  const point = screenPointForWorld(svg, rect, 832, 232);
  fireEvent.pointerDown(viewport, { pointerId: 90, button: 0, ...point });
  fireEvent.pointerUp(viewport, { pointerId: 90, ...point });
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("transform", "translate(800 200)");
  await act(async () => { fireEvent.keyDown(viewport, { key: "Enter" }); });
  expect(onShopAction).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ x: 800, y: 200 }));
});

it("blocks repeat confirmation and draft input during saving then keeps a failed draft", async () => {
  const instance = landscapeInstance("failed-draft");
  const state = landscapeShopState({ landscape_instances: [instance] });
  let complete!: (result: ShopActionResult | null) => void;
  const onShopAction = vi.fn((_request: ShopRequest) => new Promise<ShopActionResult | null>((resolve) => { complete = resolve; }));
  const { container } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />);
  const viewport = container.querySelector<HTMLElement>(".planet-landscape-viewport")!;
  const before = container.querySelector("[data-shop-preview-instance-id]")!.getAttribute("transform");
  fireEvent.click(screen.getByRole("button", { name: "배치 확정" }));
  fireEvent.keyDown(viewport, { key: "Enter" });
  fireEvent.keyDown(viewport, { key: "ArrowRight" });
  expect(screen.getByRole("button", { name: "배치 확정" })).toBeDisabled();
  expect(screen.getByRole("status")).toHaveTextContent("장식을 저장하고 있습니다.");
  expect(onShopAction).toHaveBeenCalledTimes(1);
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("transform", before!);
  await act(async () => { complete({ status: "invalid_placement", request_id: onShopAction.mock.calls[0][0].request_id, confirmed_quote: null, state }); });
  expect(container.querySelector("[data-shop-preview-instance-id]")).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("설치할 수 없습니다. 위치와 보유 상태를 확인해 주세요.");
  expect(screen.getByRole("button", { name: "배치 확정" })).toBeEnabled();
});

it("cancel ends placement without saving and selecting another instance starts its own draft", () => {
  const a = landscapeInstance("a"), b = landscapeInstance("b", "land_thin_ring");
  const state = landscapeShopState({ landscape_instances: [a, b] });
  const onShopAction = vi.fn(async () => null);
  const { container, rerender } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId="a" onShopAction={onShopAction} />);
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
  expect(onShopAction).not.toHaveBeenCalled();
  rerender(<ControlledLandscape shopState={state} selectedLandscapeInstanceId="b" onShopAction={onShopAction} />);
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-instance-id", "b");
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-valid", "true");
  rerender(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={null} onShopAction={onShopAction} />);
  expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
});

it("requires the move action before editing an already placed decoration", () => {
  const instance = landscapeInstance("explicit-move", "land_pond", 1);
  const state = landscapeShopState({ landscape_instances: [instance], placements: [{ instance_id: instance.instance_id, cycle_id: "cycle-1", x: 800, y: 200, version: 1 }] });
  const onShopAction = vi.fn(async () => null);
  const { container } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />);
  const viewport = screen.getByRole("group", { name: "행성 풍경 탐사" });
  fireEvent.keyDown(viewport, { key: "Enter" });
  expect(container.querySelector("[data-shop-preview-instance-id]")).not.toBeInTheDocument();
  expect(screen.getByText("배치됨")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "위치 이동" }));
  expect(container.querySelector("[data-shop-preview-instance-id]")).toBeInTheDocument();
  expect(onShopAction).not.toHaveBeenCalled();
});


it.each(["null", "throw"])("shows uncertain placement guidance for a %s response and keeps the draft", async (response) => {
  const instance = landscapeInstance("uncertain-draft");
  const state = landscapeShopState({ landscape_instances: [instance] });
  const onShopAction = vi.fn(async (_request: ShopRequest): Promise<ShopActionResult | null> => {
    if (response === "throw") throw new Error("response unavailable");
    return null;
  });
  const { container } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "배치 확정" })); });
  expect(onShopAction).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("status")).toHaveTextContent("설치 결과를 확인할 수 없습니다. 상점에서 요청 상태를 확인해 주세요.");
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-instance-id", instance.instance_id);
  expect(screen.queryByText("장식을 배치했습니다.")).not.toBeInTheDocument();
});

it("keeps the saving draft when Escape is pressed inside the selection card", async () => {
  const instance = landscapeInstance("saving-escape");
  const state = landscapeShopState({ landscape_instances: [instance] });
  let complete!: (result: ShopActionResult | null) => void;
  const onShopAction = vi.fn((_request: ShopRequest) => new Promise<ShopActionResult | null>((resolve) => { complete = resolve; }));
  const { container } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />);
  const card = screen.getByRole("group", { name: "선택한 장식" });
  const confirm = within(card).getByRole("button", { name: "배치 확정" });
  fireEvent.click(confirm);
  fireEvent.keyDown(confirm, { key: "Escape" });
  fireEvent.keyDown(card, { key: "Escape" });
  expect(within(card).getByText("배치 중")).toBeInTheDocument();
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-instance-id", instance.instance_id);
  expect(screen.getByRole("status")).toHaveTextContent("장식을 저장하고 있습니다.");
  expect(onShopAction).toHaveBeenCalledTimes(1);
  await act(async () => { complete(null); });
});

it("shows the current invalid location reason after moving a failed draft with the keyboard", async () => {
  const instance = landscapeInstance("failed-keyboard-location");
  const state = landscapeShopState({ landscape_instances: [instance] });
  const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({ status: "invalid_placement", request_id: request.request_id, confirmed_quote: null, state }));
  const { container } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "배치 확정" })); });
  expect(screen.getByRole("status")).toHaveTextContent("설치할 수 없습니다. 위치와 보유 상태를 확인해 주세요.");
  const viewport = screen.getByRole("group", { name: "행성 풍경 탐사" });
  for (let step = 0; step < 90; step += 1) fireEvent.keyDown(viewport, { key: "ArrowLeft" });
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-valid", "false");
  expect(screen.getByRole("status")).toHaveTextContent("장식이 배치 구역을 벗어났습니다. 다른 위치를 선택해 주세요.");
  expect(screen.getByRole("status")).not.toHaveTextContent("설치할 수 없습니다. 위치와 보유 상태를 확인해 주세요.");
  expect(onShopAction).toHaveBeenCalledTimes(1);
});

it("walks successive route points and cancels old terrain timers before continuing at the closest point", () => {
  vi.useFakeTimers();
  vi.spyOn(Math, "random").mockReturnValue(0);
  try {
    const bounds = layoutLandscape(selectableObjects).bounds;
    const route = landscapeAvatarRoute(bounds);
    const { container, rerender, unmount } = render(<ControlledLandscape />);
    const avatar = () => container.querySelector(".planet-landscape-avatar")!;
    expect(avatar()).toHaveAttribute("transform", `translate(${route[0].x} ${route[0].y})`);
    act(() => vi.advanceTimersByTime(1500));
    expect(avatar()).toHaveAttribute("transform", `translate(${route[1].x} ${route[1].y})`);
    expect(avatar()).toHaveAttribute("data-avatar-walking", "true");
    act(() => vi.advanceTimersByTime(300));
    expect(avatar()).toHaveAttribute("data-avatar-walking", "false");
    act(() => vi.advanceTimersByTime(1500));
    expect(avatar()).toHaveAttribute("transform", `translate(${route[2].x} ${route[2].y})`);
    const expanded = Array.from({ length: 240 }, (_, ordinal): PlanetObject => ({ stage: 1, ordinal, kind: "tree", x: 30, y: 30, seed: ordinal }));
    const nextRoute = landscapeAvatarRoute(layoutLandscape(expanded).bounds);
    const closest = nextRoute.reduce((best, point) => Math.hypot(point.x-route[2].x, point.y-route[2].y) < Math.hypot(best.x-route[2].x, best.y-route[2].y) ? point : best);
    rerender(<ControlledLandscape terrainObjects={expanded} />);
    expect(avatar()).toHaveAttribute("transform", `translate(${closest.x} ${closest.y})`);
    const transform = avatar().getAttribute("transform");
    act(() => vi.advanceTimersByTime(1499));
    expect(avatar()).toHaveAttribute("transform", transform!);
    act(() => vi.advanceTimersByTime(1));
    expect(avatar().getAttribute("transform")).not.toBe(transform);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  } finally { vi.useRealTimers(); vi.restoreAllMocks(); }
});

it("keeps the full terrain avatar static with reduced motion", () => {
  vi.useFakeTimers();
  vi.spyOn(Math, "random").mockReturnValue(0);
  vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  try {
    const { container, unmount } = render(<ControlledLandscape />);
    const avatar = container.querySelector(".planet-landscape-avatar")!;
    const initial = avatar.getAttribute("transform");
    act(() => vi.advanceTimersByTime(10000));
    expect(avatar).toHaveAttribute("transform", initial!);
    expect(avatar).toHaveAttribute("data-avatar-walking", "false");
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  } finally { vi.useRealTimers(); vi.restoreAllMocks(); }
});

it("renders the avatar above natural objects, purchased objects and draft without intercepting pointers", () => {
  const a=landscapeInstance("layer-placed"), b=landscapeInstance("layer-draft");
  const state=landscapeShopState({landscape_instances:[a,b],placements:[{instance_id:a.instance_id,cycle_id:"cycle-1",x:160,y:200,version:0}]});
  const {container}=render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={b.instance_id} onShopAction={async()=>null} />);
  const avatar=container.querySelector(".planet-landscape-avatar")!;
  expect(avatar).toHaveAttribute("pointer-events","none");
  for(const selector of [".planet-landscape-objects", "[data-shop-instance-id]", "[data-shop-preview-instance-id]"]) expect(container.querySelector(selector)!.compareDocumentPosition(avatar) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it.each([{ x: 160, y: 140 }, { x: 1100, y: 200 }, { x: 50, y: 200 }, { x: 1300, y: 200 }])("previews and explicitly saves once on former avatar traffic at %j", async (point) => {
  const instance = landscapeInstance("traffic-draft");
  const state = landscapeShopState({ landscape_instances: [instance] });
  const onShopAction = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({ status: "placed", request_id: request.request_id, confirmed_quote: null, state }));
  const { container } = render(<ControlledLandscape shopState={state} selectedLandscapeInstanceId={instance.instance_id} onShopAction={onShopAction} />);
  const viewport = screen.getByRole("group", { name: "행성 풍경 탐사" });
  const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
  const rect = { left: 0, top: 0, width: 900, height: 420 };
  Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
  const screenPoint = screenPointForWorld(svg, rect, point.x+32, point.y+32);
  fireEvent.pointerDown(viewport, { pointerId: 95, button: 0, ...screenPoint });
  fireEvent.pointerUp(viewport, { pointerId: 95, ...screenPoint });
  expect(container.querySelector("[data-shop-preview-instance-id]")).toHaveAttribute("data-shop-preview-valid", "true");
  expect(screen.queryByText(/통행 구역/)).not.toBeInTheDocument();
  expect(onShopAction).not.toHaveBeenCalled();
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "배치 확정" })); });
  expect(onShopAction).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ kind: "place", x: point.x, y: point.y }));
});

it("reverses avatar progression only at the full route endpoint and retains facing", () => {
  vi.useFakeTimers();
  vi.spyOn(Math,"random").mockReturnValue(0);
  try {
    const route=landscapeAvatarRoute(layoutLandscape(selectableObjects).bounds);
    const {container,unmount}=render(<ControlledLandscape />);
    const avatar=container.querySelector(".planet-landscape-avatar")!;
    act(()=>vi.advanceTimersByTime(1500+1800*(route.length-2)));
    expect(avatar).toHaveAttribute("transform",`translate(${route[route.length - 1].x} ${route[route.length - 1].y})`);
    act(()=>vi.advanceTimersByTime(1800));
    expect(avatar).toHaveAttribute("transform",`translate(${route[route.length - 2].x} ${route[route.length - 2].y})`);
    expect(container.querySelector(".planet-landscape-avatar-sprite")).toHaveClass("avatar-sprite--facing-left");
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  } finally {vi.useRealTimers();vi.restoreAllMocks();}
});

it("pauses avatar timers while the document is hidden and resumes from the same point", () => {
  vi.useFakeTimers();
  vi.spyOn(Math,"random").mockReturnValue(0);
  try {
    const {container,unmount}=render(<ControlledLandscape />);
    const avatar=container.querySelector(".planet-landscape-avatar")!;
    act(()=>vi.advanceTimersByTime(1500));
    const at=avatar.getAttribute("transform");
    Object.defineProperty(document,"hidden",{configurable:true,value:true});
    fireEvent(document,new Event("visibilitychange"));
    act(()=>vi.advanceTimersByTime(10000));
    expect(avatar).toHaveAttribute("transform",at!);
    expect(avatar).toHaveAttribute("data-avatar-walking","false");
    Object.defineProperty(document,"hidden",{configurable:true,value:false});
    fireEvent(document,new Event("visibilitychange"));
    act(()=>vi.advanceTimersByTime(1500));
    expect(avatar.getAttribute("transform")).not.toBe(at);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  } finally {delete (document as unknown as {hidden?:boolean}).hidden;vi.useRealTimers();vi.restoreAllMocks();}
});
