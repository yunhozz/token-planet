import { resetLocalLifecycleForTests } from "../lib/localLifecycle";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "../App";
import { planetLandscapeBounds } from "../components/PlanetLandscape";
import type { SharingState } from "../lib/sharing";
import type { GrowthJournal as GrowthJournalData, ShopActionResult, ShopQuote, ShopRequest, ShopState, WorldSnapshot } from "../types/usage";

const invokeMock = vi.hoisted(() => vi.fn());
const resetViewMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());
const isTauriMock = vi.hoisted(() => vi.fn(() => false));
vi.mock("@tauri-apps/api/core", () => ({ invoke: async (command: string, args?: unknown) => {
  const view = resetViewMock();
  const generation = view.state.generation;
  if (command === "get_device_reset_state") return {generation,data:view};
  try { const data = await (args === undefined ? invokeMock(command) : invokeMock(command,args)); return {generation,data}; }
  catch (details) { throw {generation,code:"command_failed",details}; }
}, isTauri: isTauriMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
const listeners = new Map<string, (event: unknown) => void>();

const ownerState: SharingState = {
  phase: "shared", user_id: "owner", sync_status: "synced", pending: 0, last_synced_at: null,
  world: { id: "world-1", name: "Together", timezone: "Asia/Seoul", is_owner: true,
    member_count: 2 },
  planet_members: [],
};
const localSnapshot: WorldSnapshot = {
  generation:0, planet_ordinal:{status:"verified",current:1},
  usage: {
    codex: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: 42, coverage: "complete" },
    claude_code: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    codex_source: "ready", claude_code_source: "usage_unavailable", confirmed_subtotal: 42, complete_total: null, scanned_at_utc: "2026-09-26T00:00:00Z",
  },
  growth_credit: 0, stage: 0, progress_to_next: 0, incomplete: true,
  planet: {
    version: 1, profile: { nickname: "Orbit", avatar: "masculine" }, timezone: "Asia/Seoul", current_cycle_id: "cycle-1", cycle_started_at_utc: "2026-09-26T00:00:00Z", last_reset_at_utc: null,
    wallet_balance: 0, wallet_credits: [], current_planet_tokens: 42, lifetime_tokens: 42, growth_credit: 0, stage: 0, progress_to_next: 0, incomplete: true,
    can_reset: true, reset_available_at_utc: null, objects: [], removed_natural_keys: [],
  },
};

function canonicalShopState(balance = 12_000_000, stateRevision = 1, instances: ShopState["landscape_instances"] = [], placements: ShopState["placements"] = []): ShopState {
  return {
    account_id: "account:owner",
    current_cycle_id: "cycle-1",
    catalog_revision: 1,
    state_revision: stateRevision,
    available_balance: balance,
    products: [{
      sku: "land_pond", category: "landscape", display_name: "연못", price: 5_000_000,
      catalog_revision: 1, purchasable: true, placement_zone: "ground", avatar_slot: null,
      effect_type: "token_earning", effect_value: 100,
    }, {
      sku: "avatar_explorer_hat", category: "avatar", display_name: "탐험가 모자", price: 1_000_000,
      catalog_revision: 1, purchasable: true, placement_zone: null, avatar_slot: "head",
      effect_type: null, effect_value: 0,
    }],
    landscape_instances: instances,
    placements,
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
    reward_state: {
      reward_timezone: "Asia/Seoul",
      settled_cycle_tokens: 0,
      era_reward_tokens: 0,
      streak_reward_tokens: 0,
    },
    action_unavailable_reason: null,
    guest_import_pending: false,
    guest_import_error: null,
  };
}

function growthJournal(generation = 1, deletedAt: string | null = null, hasRecord = true, tokens = 123_456): GrowthJournalData {
  const today = new Date().toISOString().slice(0, 10);
  return {
    generation,
    deleted_at_utc: deletedAt,
    timezone: "UTC",
    cycles: hasRecord ? [{
      cycle_id: "cycle-1", started_at_utc: `${today}T00:00:00Z`, ended_at_utc: null,
      wallet_credit: null, wallet_credit_at_utc: null,
    }] : [],
    entries: hasRecord ? [{
      device_id: "device-1", cycle_id: "cycle-1", bucket_date: today, agent: "codex",
      revision: 1, generation, present: true, confirmed_tokens: tokens, coverage: "complete", payload_hash: "fixture",
    }] : [],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

it("starts anonymous sharing without email sign-in", async () => {
  let started = false;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return {
      ...ownerState, phase: started ? "signed_in" : "signed_out", world: null, sync_status: started ? "synced" : "local",
    };
    if (command === "start_anonymous_session") {
      started = true;
      return { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
    }
    if (command === "current_usage") return structuredClone(localSnapshot);
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("tab", { name: "그룹" }));
  expect(screen.queryByLabelText("이메일")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "공유 시작하기" }));
  expect(await screen.findByLabelText("공동 행성에서 사용할 닉네임")).toHaveValue("Orbit");
});

beforeEach(() => {
  resetLocalLifecycleForTests();
  window.history.replaceState({}, "", "/");
  invokeMock.mockReset();
  resetViewMock.mockReturnValue({state:{generation:0,phase:"idle",request_id:null},actions_blocked:false,storage_completed:false});
  listenMock.mockReset();
  isTauriMock.mockReturnValue(false);
  listeners.clear();
  listenMock.mockImplementation(async (event: string, handler: (event: unknown) => void) => {
    listeners.set(event, (incoming: unknown) => {
      const item = incoming as {payload:unknown};
      if (event === "usage-updated") {
        const payload = item.payload as WorldSnapshot;
        handler({payload:{generation:payload.generation,data:payload}});
      } else handler(incoming);
    });
    return () => {};
  });
  Object.defineProperty(navigator, "onLine", { configurable: true, value: true });
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "list_world_members") return [{ user_id: "owner", role: "owner" }, { user_id: "member", role: "member" }];
    if (command === "list_world_invites") return [];
    return null;
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("opens the cosmetic shop in the current window and restores exploration on return", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
    { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
  ];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "get_shop_state") return canonicalShopState();
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  const initialViewBox = container.querySelector(".planet-landscape-svg")!.getAttribute("viewBox");
  expect(container.querySelector(".planet-landscape")).toHaveAttribute("data-camera-zoom", "1");
  const landscapeBounds = planetLandscapeBounds(snapshot.planet.objects);
  const [viewX, viewY, viewWidth, viewHeight] = initialViewBox!.split(" ").map(Number);
  expect(viewX).toBeLessThanOrEqual(landscapeBounds.x);
  expect(viewY).toBeLessThanOrEqual(landscapeBounds.y);
  expect(viewX + viewWidth).toBeGreaterThanOrEqual(landscapeBounds.x + landscapeBounds.width);
  expect(viewY + viewHeight).toBeGreaterThanOrEqual(landscapeBounds.y + landscapeBounds.height);
  const viewport = screen.getByRole("group", { name: "행성 풍경 탐사" });
  viewport.focus();
  fireEvent.keyDown(viewport, { key: "ArrowDown" });
  expect(container.querySelector(".planet-landscape-svg")!.getAttribute("viewBox")).toBe(initialViewBox);
  fireEvent.click(container.querySelector('[data-landscape-object-id="1-3"]')!);
  const originalViewBox = container.querySelector(".planet-landscape-svg")?.getAttribute("viewBox");

  fireEvent.click(screen.getByRole("button", { name: "행성 상점" }));

  expect(openWindow).not.toHaveBeenCalled();
  expect(await screen.findByRole("heading", { name: "행성 상점", level: 1 })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));

  expect(await screen.findByRole("region", { name: "행성 풍경" })).toBeInTheDocument();
  expect(container.querySelector(".planet-landscape-svg")).toHaveAttribute("viewBox", originalViewBox);
  expect(container.querySelector('[data-landscape-object-id="1-3"]')).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "행성 상점" })).toHaveFocus();
});

it("opens the growth journal in the current window and returns to personal detail", async () => {
  const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));

  expect(openWindow).not.toHaveBeenCalled();
  expect(await screen.findByRole("heading", { name: "성장 일지", level: 1 })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));

  expect(await screen.findByRole("region", { name: "행성 풍경" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "성장 일지 보기" })).toHaveFocus();
});

it("opens legacy feature URLs in detail mode and returns to personal detail", async () => {
  window.history.replaceState({}, "", "/?window=cosmetic-shop");
  const { container } = render(<App />);

  expect(await screen.findByRole("heading", { name: "행성 상점", level: 1 })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));

  expect(await screen.findByRole("tab", { name: "내 행성" })).toHaveAttribute("aria-selected", "true");
  expect(container.querySelector(".planet-landscape")).toBeInTheDocument();
  expect(screen.queryByRole("main", { name: "행성 팝오버" })).not.toBeInTheDocument();
});

it("selects the just purchased instance via result card then confirms placement and retrieves it", async () => {
  let canonical = canonicalShopState();
  const instance = {
    instance_id: "pond-instance-placed", sku: "land_pond", variation_index: 2,
    seed: "seed-pond-placed", variation_version: 1, placement_version: 0,
  };
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(canonical);
    if (command === "quote_shop_action") {
      if (!args?.target) throw new Error("missing quote target");
      return { target: structuredClone(args.target), catalog_revision: 1, effect_revision: 0, price: 5_000_000 } satisfies ShopQuote;
    }
    if (command === "apply_shop_action") {
      const request = args?.request;
      if (!request) throw new Error("missing canonical request");
      applied.push(request);
      if (request.kind === "purchase") {
        canonical = canonicalShopState(7_000_000, 2, [instance]);
        return { status: "purchased", request_id: request.request_id, confirmed_quote: null, state: structuredClone(canonical) } satisfies ShopActionResult;
      }
      if (request.kind === "place") {
        const placed = { ...instance, placement_version: 1 };
        canonical = canonicalShopState(7_000_000, 3, [placed], [{ instance_id: instance.instance_id, cycle_id: "cycle-1", x: request.x, y: request.y, version: 1 }]);
        return { status: "placed", request_id: request.request_id, confirmed_quote: null, state: structuredClone(canonical) } satisfies ShopActionResult;
      }
      if (request.kind === "retrieve") {
        const retrieved = { ...instance, placement_version: 2 };
        canonical = canonicalShopState(7_000_000, 4, [retrieved], []);
        return { status: "retrieved", request_id: request.request_id, confirmed_quote: null, state: structuredClone(canonical) } satisfies ShopActionResult;
      }
      throw new Error(`unexpected request kind: ${request.kind}`);
    }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  const shop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(within(shop).getByRole("button", { name: "연못 구매" }));
  fireEvent.click(await screen.findByRole("button", { name: "구매 확정" }));
  expect(await within(shop).findByText("연못 구매 완료")).toBeInTheDocument();
  fireEvent.click(await within(shop).findByRole("button", { name: "행성에 배치하기" }));

  const viewport = await screen.findByRole("group", { name: "행성 풍경 탐사" });
  const svg = container.querySelector<SVGSVGElement>(".planet-landscape-svg")!;
  const rect = { left: 37, top: 51, width: 900, height: 420 };
  Object.defineProperty(viewport, "getBoundingClientRect", { configurable: true, value: () => rect });
  const [viewX, viewY, viewWidth, viewHeight] = svg.getAttribute("viewBox")!.split(" ").map(Number);
  const scale = Math.min(rect.width / viewWidth, rect.height / viewHeight);
  const point = (x: number, y: number) => ({
    clientX: rect.left + (rect.width - viewWidth * scale) / 2 + (x - viewX) * scale,
    clientY: rect.top + (rect.height - viewHeight * scale) / 2 + (y - viewY) * scale,
  });
  const from = point(760, 160);
  const drop = point(832, 232);
  await act(async () => {
    fireEvent.pointerDown(viewport, { pointerId: 33, button: 0, ...from });
    fireEvent.pointerMove(viewport, { pointerId: 33, ...drop });
    fireEvent.pointerUp(viewport, { pointerId: 33, ...drop });
    await Promise.resolve();
  });

  expect(applied.map((request) => request.kind)).toEqual(["purchase"]);
  fireEvent.click(screen.getByRole("button", { name: "배치 확정" }));
  await waitFor(() => expect(container.querySelector('[data-shop-instance-id="pond-instance-placed"]')).toHaveAttribute("transform", "translate(800 200)"));
  expect(applied.map((request) => request.kind)).toEqual(["purchase", "place"]);
  expect(applied[1]).toMatchObject({ kind: "place", instance_id: instance.instance_id, cycle_id: "cycle-1", expected_version: 0, x: 800, y: 200 });

  fireEvent.click(screen.getByRole("button", { name: "연못, 설치된 장식" }));
  fireEvent.click(screen.getByRole("button", { name: "보관함으로" }));
  await waitFor(() => expect(container.querySelector('[data-shop-instance-id="pond-instance-placed"]')).not.toBeInTheDocument());
  expect(applied[2]).toMatchObject({ kind: "retrieve", instance_id: instance.instance_id, cycle_id: "cycle-1", expected_version: 1 });
  fireEvent.click(screen.getByRole("button", { name: "행성 상점 열기" }));
  const returnedShop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(within(returnedShop).getByRole("tab", { name: "보유함" }));
  expect(await within(returnedShop).findByText("변형 3 · 보관함")).toBeInTheDocument();
});

it("previews avatar equipment temporarily and renders only confirmed equipment on the planet", async () => {
  let canonical = canonicalShopState();
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(canonical);
    if (command === "quote_shop_action") {
      if (!args?.target) throw new Error("missing quote target");
      return { target: structuredClone(args.target), catalog_revision: 1, effect_revision: 0, price: 1_000_000 } satisfies ShopQuote;
    }
    if (command === "apply_shop_action") {
      const request = args?.request;
      if (!request) throw new Error("missing canonical request");
      applied.push(request);
      if (request.kind === "purchase") {
        canonical = { ...canonicalShopState(11_000_000, 2), avatar_owned_skus: ["avatar_explorer_hat"] };
        return { status: "purchased", request_id: request.request_id, confirmed_quote: null, state: structuredClone(canonical) } satisfies ShopActionResult;
      }
      if (request.kind === "equip_avatar") {
        canonical = {
          ...canonicalShopState(11_000_000, 3),
          avatar_owned_skus: ["avatar_explorer_hat"],
          avatar_equipment: { ...canonical.avatar_equipment, head: { sku: request.sku, version: 1 } },
        };
        return { status: "equipped", request_id: request.request_id, confirmed_quote: null, state: structuredClone(canonical) } satisfies ShopActionResult;
      }
      throw new Error(`unexpected request kind: ${request.kind}`);
    }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  const shop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(within(shop).getByRole("tab", { name: "아바타" }));
  fireEvent.click(within(shop).getByRole("button", { name: "탐험가 모자 미리보기" }));
  expect(container.querySelector('.shop-planet-preview [data-avatar-equipment="avatar_explorer_hat"]')).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "미리보기 해제" }));
  expect(container.querySelector('.shop-planet-preview [data-avatar-equipment="avatar_explorer_hat"]')).not.toBeInTheDocument();

  fireEvent.click(within(shop).getByRole("button", { name: "탐험가 모자 구매" }));
  fireEvent.click(await screen.findByRole("button", { name: "구매 확정" }));
  fireEvent.click(within(shop).getByRole("tab", { name: "보유함" }));
  fireEvent.click(await within(shop).findByRole("button", { name: "탐험가 모자 장착" }));
  await waitFor(() => expect(container.querySelector('.shop-planet-preview [data-avatar-equipment="avatar_explorer_hat"]')).toBeInTheDocument());
  expect(within(shop).getByText("장착 중")).toBeInTheDocument();
  expect(applied.map((request) => request.kind)).toEqual(["purchase", "equip_avatar"]);

  fireEvent.click(screen.getByRole("button", { name: "← 행성으로 돌아가기" }));
  expect(await screen.findByRole("region", { name: "행성 풍경" })).toBeInTheDocument();
  expect(container.querySelector('.planet-landscape-avatar-sprite [data-avatar-equipment="avatar_explorer_hat"]')).toBeInTheDocument();
});

it("uses the canonical shop panel and keeps a confirmed landscape purchase in inventory", async () => {
  let canonical = canonicalShopState();
  const instance = {
    instance_id: "pond-instance-1", sku: "land_pond", variation_index: 2,
    seed: "seed-pond-1", variation_version: 1, placement_version: 0,
  };
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(canonical);
    if (command === "quote_shop_action") {
      if (!args?.target) throw new Error("missing quote target");
      return {
        target: structuredClone(args.target), catalog_revision: 1, effect_revision: 0, price: 5_000_000,
      } satisfies ShopQuote;
    }
    if (command === "apply_shop_action") {
      const request = args?.request;
      if (!request) throw new Error("missing canonical request");
      canonical = canonicalShopState(7_000_000, 2, [instance]);
      return {
        status: "purchased", request_id: request.request_id, confirmed_quote: null, state: structuredClone(canonical),
      } satisfies ShopActionResult;
    }
    if (command === "get_shop_state") return canonicalShopState();
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));

  const shop = await screen.findByRole("region", { name: "행성 상점" });
  expect(within(shop).getByRole("tab", { name: "조경" })).toHaveAttribute("aria-selected", "true");
  fireEvent.click(within(shop).getByRole("button", { name: "연못 구매" }));
  fireEvent.click(await screen.findByRole("button", { name: "구매 확정" }));

  await waitFor(() => expect(screen.getByText("7M 토큰")).toBeInTheDocument());
  fireEvent.click(within(shop).getByRole("tab", { name: "보유함" }));
  expect(await within(shop).findByText("연못")).toBeInTheDocument();
  expect(within(shop).getByText("변형 3 · 보관함")).toBeInTheDocument();
});

it("uses the flat landscape only in personal detail and keeps exploration when switching tabs", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
    { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
  ];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  expect(container.querySelector(".planet-landscape")).not.toBeInTheDocument();
  expect(container.querySelector(".planet-svg")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(container.querySelector('[data-landscape-object-id="1-3"]')!);
  const selected = screen.getByRole("region", { name: "선택한 오브젝트" });
  expect(selected).toHaveTextContent("정착·농경");
  expect(within(selected).getByText("나무")).toBeInTheDocument();
  expect(selected).toHaveTextContent("뿌리를 내리고 자라는 나무입니다.");
  const selectedViewBox = container.querySelector(".planet-landscape-svg")?.getAttribute("viewBox");

  fireEvent.click(screen.getByRole("tab", { name: "그룹" }));
  expect(container.querySelector(".planet-landscape")).not.toBeInTheDocument();
  fireEvent.click(await screen.findByRole("tab", { name: "내 행성" }));

  expect(container.querySelector('[data-landscape-object-id="1-3"]')).toHaveAttribute("aria-pressed", "true");
  expect(container.querySelector(".planet-landscape-svg")).toHaveAttribute("viewBox", selectedViewBox);
});

it.each(["account", "world", "cycle"] as const)("resets exploration and keeps the shop open when %s changes", async (changedContext) => {
  let activeShared = structuredClone(ownerState);
  let activeSnapshot = structuredClone(localSnapshot);
  activeSnapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
    { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
  ];
  const currentShop = () => ({
    ...canonicalShopState(),
    account_id: `account:${activeShared.user_id}`,
    current_cycle_id: activeSnapshot.planet.current_cycle_id,
  });
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(activeShared);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(activeSnapshot);
    if (command === "get_shop_state") return structuredClone(currentShop());
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(container.querySelector(".planet-landscape")).toHaveAttribute("data-camera-zoom", "1");
  const initialViewBox = container.querySelector(".planet-landscape-svg")!.getAttribute("viewBox");
  const viewport = screen.getByRole("group", { name: "행성 풍경 탐사" });
  viewport.focus();
  fireEvent.keyDown(viewport, { key: "ArrowDown" });
  expect(container.querySelector(".planet-landscape-svg")!.getAttribute("viewBox")).toBe(initialViewBox);
  fireEvent.click(container.querySelector('[data-landscape-object-id="1-3"]')!);
  expect(screen.getByRole("region", { name: "선택한 오브젝트" })).toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  const previewButton = await screen.findByRole("button", { name: "연못 미리보기" });
  fireEvent.click(previewButton);

  if (changedContext === "account") activeShared = { ...activeShared, user_id: "member-next", phase: "signed_in", world: null };
  if (changedContext === "world") activeShared = { ...activeShared, world: { ...activeShared.world!, id: "world-2" } };
  if (changedContext === "cycle") activeSnapshot.planet.current_cycle_id = "cycle-2";
  await act(async () => { listeners.get("world-context-changing")?.({ payload: "cosmetic-shop" }); });

  await waitFor(() => expect(screen.getByRole("button", { name: "행성 상점" })).toBeDisabled());
  expect(screen.getByRole("heading", { name: "행성 상점", level: 1 })).toBeInTheDocument();
  await act(async () => { listeners.get("world-state-updated")?.({ payload: null }); });
  await waitFor(() => expect(screen.getByRole("button", { name: "연못 미리보기" })).toBeEnabled());
  expect(screen.getByRole("heading", { name: "행성 상점", level: 1 })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));

  await screen.findByRole("region", { name: "행성 풍경" });
  expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
  expect(container.querySelector(".planet-landscape")).toHaveAttribute("data-camera-zoom", "1");
  expect(container.querySelector(".planet-landscape-svg")).toHaveAttribute("viewBox", initialViewBox);
  expect(container.querySelector('[data-landscape-object-id="1-3"]')).toHaveAttribute("aria-pressed", "false");
});

it("clears exploration immediately while a world context is locked", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
  ];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(container.querySelector(".planet-landscape")).toHaveAttribute("data-camera-zoom", "1");
  const initialViewBox = container.querySelector(".planet-landscape-svg")!.getAttribute("viewBox");
  const viewport = screen.getByRole("group", { name: "행성 풍경 탐사" });
  viewport.focus();
  fireEvent.keyDown(viewport, { key: "ArrowDown" });
  expect(container.querySelector(".planet-landscape-svg")!.getAttribute("viewBox")).toBe(initialViewBox);
  fireEvent.click(container.querySelector('[data-landscape-object-id="0-0"]')!);
  expect(screen.getByRole("region", { name: "선택한 오브젝트" })).toBeInTheDocument();

  await act(async () => { listeners.get("world-context-changing")?.({ payload: "cosmetic-shop" }); });

  expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
  expect(container.querySelector(".planet-landscape")).toHaveAttribute("data-camera-zoom", "1");
  expect(container.querySelector(".planet-landscape-svg")).toHaveAttribute("viewBox", initialViewBox);
  fireEvent.click(container.querySelector('[data-landscape-object-id="0-0"]')!);
  expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
});

it("keeps a canonical shop load error visible when leaving and reopening the shop", async () => {
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") throw new Error("상점 상태를 불러오지 못했습니다.");
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_shop_state", {context:{generation:0}}));
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("상점 상태를 불러오지 못했습니다.");

  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("상점 상태를 불러오지 못했습니다.");
  expect(screen.getByRole("button", { name: "상점 다시 확인" })).toBeInTheDocument();
});

it("ignores a late journal failure after leaving and reloads on reentry", async () => {
  const oldRead = deferred<unknown>();
  const currentRead = deferred<unknown>();
  let journalReads = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_growth_journal") return journalReads++ === 0 ? oldRead.promise : currentRead.promise;
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  await screen.findByRole("heading", { name: "성장 일지", level: 1 });
  await waitFor(() => expect(journalReads).toBe(1));

  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  await act(async () => { oldRead.reject(new Error("stale journal failure")); });
  expect(screen.queryByText("성장 일지를 불러오지 못했습니다.")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  await waitFor(() => expect(journalReads).toBe(2));
  expect(await screen.findByText("기록 동기화를 기다리며 성장 일지를 불러오고 있습니다.")).toBeInTheDocument();
  await act(async () => {
    currentRead.resolve({ generation: 1, deleted_at_utc: null, timezone: "UTC", cycles: [], entries: [] });
  });
  expect(await screen.findByText("확인된 행성 주기가 없습니다.")).toBeInTheDocument();
});

it("does not offer personal journal deletion in a signed-out local phase", async () => {
  const localState: SharingState = {
    ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local",
  };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(localState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_growth_journal") return growthJournal();
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  const journalRegion = await screen.findByRole("region", { name: "성장 일지" });

  expect(journalRegion.querySelector(".growth-journal-total")?.textContent).toContain("123,456");
  expect(within(journalRegion).queryByRole("button", { name: "개인 일지 삭제" })).not.toBeInTheDocument();
  expect(invokeMock).not.toHaveBeenCalledWith("delete_growth_journal", {context:{generation:0}});
});

it.each(["shared", "signed_in"] as const)("requires confirmation and then applies journal deletion for %s users", async (phase) => {
  const state: SharingState = phase === "shared"
    ? structuredClone(ownerState)
    : { ...ownerState, phase: "signed_in", world: null };
  const deletedJournal = growthJournal(2, new Date().toISOString(), false);
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(state);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_growth_journal") return growthJournal();
    if (command === "delete_growth_journal") return structuredClone(deletedJournal);
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  const journalRegion = await screen.findByRole("region", { name: "성장 일지" });
  const deleteButton = within(journalRegion).getByRole("button", { name: "개인 일지 삭제" });
  expect(journalRegion.querySelector(".growth-journal-total")?.textContent).toContain("123,456");

  fireEvent.click(deleteButton);
  expect(confirm).toHaveBeenCalled();
  expect(invokeMock).not.toHaveBeenCalledWith("delete_growth_journal", {context:{generation:0}});
  expect(journalRegion.querySelector(".growth-journal-total")?.textContent).toContain("123,456");

  confirm.mockReturnValue(true);
  fireEvent.click(deleteButton);

  expect(await within(journalRegion).findByText("확인된 행성 주기가 없습니다.")).toBeInTheDocument();
  expect(journalRegion.querySelector(".growth-journal-total")).toBeNull();
  expect(invokeMock).toHaveBeenCalledWith("delete_growth_journal", {context:{generation:0}});
});

it("shows a retryable journal error after delete fails", async () => {
  let journalReads = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_growth_journal") return journalReads++ === 0 ? growthJournal() : growthJournal(2, null, true, 654_321);
    if (command === "delete_growth_journal") throw new Error("temporary delete failure");
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  const journalRegion = await screen.findByRole("region", { name: "성장 일지" });
  await waitFor(() => expect(journalRegion.querySelector(".growth-journal-total")?.textContent).toContain("123,456"));
  fireEvent.click(within(journalRegion).getByRole("button", { name: "개인 일지 삭제" }));

  expect(await within(journalRegion).findByRole("alert")).toHaveTextContent("개인 일지를 삭제하지 못했습니다.");
  const reloadButton = within(journalRegion).getByRole("button", { name: "성장 일지 새로고침" });
  expect(reloadButton).toBeEnabled();
  const previousJournalReads = journalReads;
  fireEvent.click(reloadButton);

  await waitFor(() => expect(journalReads).toBeGreaterThan(previousJournalReads));
  await waitFor(() => expect(screen.getByRole("region", { name: "성장 일지" }).querySelector(".growth-journal-total")?.textContent).toContain("654,321"));
  expect(within(screen.getByRole("region", { name: "성장 일지" })).queryByRole("alert")).not.toBeInTheDocument();
});

it("refreshes owner candidates even when the member count stays the same", async () => {
  let roster = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "list_world_members") {
      roster += 1;
      return [{ user_id: "owner", role: "owner" }, { user_id: roster === 1 ? "member-b" : "member-c", role: "member" }];
    }
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "list_world_members")).toHaveLength(1));
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("tab", { name: "그룹" }));
  await screen.findByRole("option", { name: /member-b/ });
  fireEvent.change(screen.getByLabelText("소유권을 넘길 참여자"), { target: { value: "member-b" } });
  fireEvent.click(screen.getByRole("button", { name: "사용량 새로고침" }));
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "list_world_members")).toHaveLength(2));
  await screen.findByRole("option", { name: /member-c/ });
  expect(screen.getByLabelText("소유권을 넘길 참여자")).toHaveValue("");
});

it("refreshes the confirmed canonical shop state after sync finishes", async () => {
  let synced = false;
  let reads = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") {
      reads += 1;
      return canonicalShopState(synced ? 100_000 : 0, synced ? 2 : 1);
    }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  const shop = await screen.findByRole("region", { name: "행성 상점" });
  await within(shop).findByText("0 토큰", { exact: true });

  synced = true;
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });

  await waitFor(() => expect(within(shop).getByText("100K 토큰", { exact: true })).toBeInTheDocument());
  expect(reads).toBeGreaterThan(1);
});

it("retries an unavailable canonical shop state when the user asks to refresh", async () => {
  let reads = 0;
  let unavailable = true;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") {
      reads += 1;
      if (unavailable) throw new Error("상점 상태를 불러오지 못했습니다.");
      return canonicalShopState(300_000, 2);
    }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("상점 상태를 불러오지 못했습니다.");
  expect(screen.getByRole("button", { name: "상점 다시 확인" })).toBeEnabled();

  unavailable = false;
  fireEvent.click(screen.getByRole("button", { name: "상점 다시 확인" }));
  const shop = await screen.findByRole("region", { name: "행성 상점" });
  await within(shop).findByText("300K 토큰", { exact: true });
  expect(reads).toBe(2);
});

it("coalesces shop state loading while opening the canonical shop screen", async () => {
  const heldRead = deferred<ShopState>();
  let reads = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") { reads += 1; return heldRead.promise; }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  await waitFor(() => expect(reads).toBe(1));
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  await screen.findByText("상점을 불러오고 있습니다.");
  expect(reads).toBe(1);

  await act(async () => { heldRead.resolve(canonicalShopState(200_000)); });
  const shop = await screen.findByRole("region", { name: "행성 상점" });
  await within(shop).findByText("200K 토큰", { exact: true });
});

it("filters context-changing events by the actual main window while the shop is open", async () => {
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return canonicalShopState(500_000);
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  const previewButton = await screen.findByRole("button", { name: "연못 미리보기" });

  await act(async () => { listeners.get("world-context-changing")?.({ payload: "main" }); });
  expect(previewButton).toBeEnabled();

  await act(async () => { listeners.get("world-context-changing")?.({ payload: "cosmetic-shop" }); });
  expect(screen.getByRole("heading", { name: "행성 상점", level: 1 })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "행성 상점" })).toBeDisabled();
  await act(async () => { listeners.get("world-state-updated")?.({ payload: null }); });
  await waitFor(() => expect(screen.getByRole("button", { name: "행성 상점" })).toBeEnabled());
  expect(screen.getByRole("heading", { name: "행성 상점", level: 1 })).toBeInTheDocument();
});

it("retries a failed canonical purchase with the same request after returning to the shop", async () => {
  const instance = {
    instance_id: "pond-instance-retried", sku: "land_pond", variation_index: 1,
    seed: "seed-pond-retried", variation_version: 1, placement_version: 0,
  };
  let canonical = canonicalShopState();
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(canonical);
    if (command === "quote_shop_action") {
      if (!args?.target) throw new Error("missing quote target");
      return { target: structuredClone(args.target), catalog_revision: 1, effect_revision: 0, price: 5_000_000 } satisfies ShopQuote;
    }
    if (command === "apply_shop_action") {
      const request = args?.request;
      if (!request) throw new Error("missing canonical request");
      applied.push(request);
      if (applied.length === 1) throw new Error("network disconnected");
      canonical = canonicalShopState(7_000_000, 2, [instance]);
      return { status: "purchased", request_id: request.request_id, confirmed_quote: null, state: structuredClone(canonical) } satisfies ShopActionResult;
    }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  const shop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(within(shop).getByRole("button", { name: "연못 구매" }));
  fireEvent.click(await screen.findByRole("button", { name: "구매 확정" }));
  expect(await screen.findByText("요청 결과를 확인하지 못했습니다. 같은 요청으로 다시 시도해 주세요.")).toBeInTheDocument();
  expect(await screen.findByRole("button", { name: "같은 요청 다시 시도" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(screen.getByRole("button", { name: "행성 상점 열기" }));
  const reopenedShop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(await within(reopenedShop).findByRole("button", { name: "같은 요청 다시 시도" }));

  expect(await screen.findByText("구매를 완료했습니다. 보유함에서 상품을 확인할 수 있습니다.")).toBeInTheDocument();
  expect(applied).toHaveLength(2);
  expect(applied[1].request_id).toBe(applied[0].request_id);
  expect(applied[1]).toEqual(applied[0]);
  fireEvent.click(within(reopenedShop).getByRole("tab", { name: "보유함" }));
  expect(await within(reopenedShop).findByText("변형 2 · 보관함")).toBeInTheDocument();
});

it("ignores a pending purchase result after the account changes", async () => {
  const pendingApply = deferred<ShopActionResult>();
  const instance = {
    instance_id: "pond-instance-stale", sku: "land_pond", variation_index: 0,
    seed: "seed-pond-stale", variation_version: 1, placement_version: 0,
  };
  let currentAccount = structuredClone(ownerState);
  const pendingRequests: ShopRequest[] = [];
  const stateForCurrentAccount = () => ({
    ...canonicalShopState(currentAccount.user_id === "owner" ? 12_000_000 : 9_000_000),
    account_id: `account:${currentAccount.user_id}`,
  });
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(stateForCurrentAccount());
    if (command === "quote_shop_action") {
      if (!args?.target) throw new Error("missing quote target");
      return { target: structuredClone(args.target), catalog_revision: 1, effect_revision: 0, price: 5_000_000 } satisfies ShopQuote;
    }
    if (command === "apply_shop_action") {
      if (!args?.request) throw new Error("missing canonical request");
      pendingRequests.push(args.request);
      return pendingApply.promise;
    }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  let shop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(within(shop).getByRole("button", { name: "연못 구매" }));
  fireEvent.click(await screen.findByRole("button", { name: "구매 확정" }));
  await waitFor(() => expect(pendingRequests).toHaveLength(1));

  currentAccount = { ...currentAccount, user_id: "next-account", phase: "signed_in", world: null };
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await waitFor(() => expect(within(screen.getByRole("region", { name: "행성 상점" })).getByText("9M 토큰", { exact: true })).toBeInTheDocument());
  shop = screen.getByRole("region", { name: "행성 상점" });

  await act(async () => {
    pendingApply.resolve({
      status: "purchased", request_id: pendingRequests[0].request_id, confirmed_quote: null,
      state: canonicalShopState(1_000_000, 2, [instance]),
    });
    await Promise.resolve();
  });

  expect(within(shop).getByText("9M 토큰", { exact: true })).toBeInTheDocument();
  expect(within(shop).getByRole("button", { name: "연못 구매" })).toBeInTheDocument();
});

it("applies a confirmed avatar equipment result after returning to the planet", async () => {
  const pendingApply = deferred<ShopActionResult>();
  const ownedState = { ...canonicalShopState(), avatar_owned_skus: ["avatar_explorer_hat"] };
  const equippedState: ShopState = {
    ...ownedState,
    state_revision: 2,
    avatar_equipment: { ...ownedState.avatar_equipment, head: { sku: "avatar_explorer_hat", version: 1 } },
  };
  let request: ShopRequest | null = null;
  invokeMock.mockImplementation(async (command: string, args?: { request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(ownedState);
    if (command === "apply_shop_action") {
      request = args?.request ?? null;
      return pendingApply.promise;
    }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  const shop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(within(shop).getByRole("tab", { name: "아바타" }));
  fireEvent.click(within(shop).getByRole("tab", { name: "보유함" }));
  fireEvent.click(await within(shop).findByRole("button", { name: "탐험가 모자 장착" }));
  await waitFor(() => expect(request).toMatchObject({ kind: "equip_avatar", slot: "head", sku: "avatar_explorer_hat", expected_version: 0 }));

  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(container.querySelector('.planet-landscape-avatar-sprite [data-avatar-equipment="avatar_explorer_hat"]')).not.toBeInTheDocument();
  await act(async () => {
    pendingApply.resolve({ status: "equipped", request_id: request!.request_id, confirmed_quote: null, state: equippedState });
    await Promise.resolve();
  });
  await waitFor(() => expect(container.querySelector('.planet-landscape-avatar-sprite [data-avatar-equipment="avatar_explorer_hat"]')).toBeInTheDocument());
});

it("ignores pending avatar equipment after the account changes", async () => {
  const pendingApply = deferred<ShopActionResult>();
  const ownedState = { ...canonicalShopState(), avatar_owned_skus: ["avatar_explorer_hat"] };
  const equippedState: ShopState = {
    ...ownedState,
    state_revision: 2,
    avatar_equipment: { ...ownedState.avatar_equipment, head: { sku: "avatar_explorer_hat", version: 1 } },
  };
  let currentAccount = structuredClone(ownerState);
  let request: ShopRequest | null = null;
  const stateForCurrentAccount = () => currentAccount.user_id === "owner"
    ? ownedState
    : { ...canonicalShopState(9_000_000), account_id: `account:${currentAccount.user_id}` };
  invokeMock.mockImplementation(async (command: string, args?: { request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(stateForCurrentAccount());
    if (command === "apply_shop_action") { request = args?.request ?? null; return pendingApply.promise; }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  let shop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(within(shop).getByRole("tab", { name: "아바타" }));
  fireEvent.click(within(shop).getByRole("tab", { name: "보유함" }));
  fireEvent.click(await within(shop).findByRole("button", { name: "탐험가 모자 장착" }));
  await waitFor(() => expect(request).toMatchObject({ kind: "equip_avatar" }));

  currentAccount = { ...currentAccount, user_id: "next-account", phase: "signed_in", world: null };
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await waitFor(() => expect(within(screen.getByRole("region", { name: "행성 상점" })).getByText("9M 토큰", { exact: true })).toBeInTheDocument());
  shop = screen.getByRole("region", { name: "행성 상점" });
  expect(within(shop).queryByRole("button", { name: "탐험가 모자 장착" })).not.toBeInTheDocument();

  await act(async () => {
    pendingApply.resolve({ status: "equipped", request_id: request!.request_id, confirmed_quote: null, state: equippedState });
    await Promise.resolve();
  });
  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(container.querySelector('.planet-landscape-avatar-sprite [data-avatar-equipment="avatar_explorer_hat"]')).not.toBeInTheDocument();
});

it("does not show the previous account shop state when the next account lookup fails", async () => {
  let currentAccount: SharingState = ownerState;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") {
      if (currentAccount.user_id === "next-account") throw new Error("서버에서 상점 상태를 불러오지 못했습니다.");
      return canonicalShopState(600_000);
    }
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  let shop = await screen.findByRole("region", { name: "행성 상점" });
  await within(shop).findByText("600K 토큰", { exact: true });

  currentAccount = { ...ownerState, user_id: "next-account", phase: "signed_in", world: null };
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });

  await waitFor(() => expect(screen.queryByText("600K 토큰", { exact: true })).not.toBeInTheDocument());
  expect(await screen.findByRole("alert")).toHaveTextContent("서버에서 상점 상태를 불러오지 못했습니다.");
});

it("returns_to_popup_then_reopens_personal_detail", async () => {
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  screen.getByRole("main", { name: "행성 팝오버" });
  const summary = screen.getByRole("region", { name: "행성 요약" });
  expect(summary).toHaveTextContent("현재 시대");
  expect(summary).toHaveTextContent("이번 행성 토큰");
  expect(summary.querySelector('[role="progressbar"][aria-label="다음 시대 진행도"]')).not.toBeNull();
  const scene = container.querySelector<HTMLElement>(".world-visual")!;
  const detailButton = screen.getByRole("button", { name: "행성·그룹 자세히 보기" });
  const usage = screen.getByRole("region", { name: "전체 사용량 (과거 포함)" });
  expect(scene.compareDocumentPosition(summary) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(summary.compareDocumentPosition(detailButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(detailButton.compareDocumentPosition(usage) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

  fireEvent.click(detailButton);
  const personal = await screen.findByRole("tab", { name: "내 행성" });
  const group = screen.getByRole("tab", { name: "그룹" });
  expect(personal).toHaveAttribute("aria-selected", "true");
  personal.focus();
  fireEvent.keyDown(personal, { key: "ArrowRight" });
  expect(group).toHaveFocus();
  expect(group).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  expect(screen.getByRole("main", { name: "행성 팝오버" })).toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByRole("tab", { name: "내 행성" })).toHaveAttribute("aria-selected", "true");
});

it("shows_popover_summary_in_order", async () => {
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  const scene = container.querySelector<HTMLElement>(".world-visual")!;
  const token = screen.getByText("이번 행성 토큰");
  const progress = screen.getByRole("progressbar", { name: "다음 시대 진행도" });
  const detail = screen.getByRole("button", { name: "행성·그룹 자세히 보기" });
  const sources = screen.getByRole("region", { name: "수집 상태" });
  const sync = screen.getByRole("status");
  expect(screen.getByRole("main", { name: "행성 팝오버" })).toContainElement(scene);
  expect(scene.compareDocumentPosition(token) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(token.compareDocumentPosition(progress) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(progress.compareDocumentPosition(detail) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(detail.compareDocumentPosition(sources) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(sources.compareDocumentPosition(sync) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it("retains_view_when_native_transition_fails", async () => {
  isTauriMock.mockReturnValue(true);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage") return structuredClone(localSnapshot);
    if (command === "set_detail_view") throw "window size unavailable";
    if (command === "get_shop_state") return canonicalShopState();
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("화면을 전환하지 못했습니다");
  expect(screen.getByRole("main", { name: "행성 팝오버" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "행성·그룹 자세히 보기" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeInTheDocument();
  isTauriMock.mockReturnValue(false);
});

it("escape_hides_only_popup", async () => {
  isTauriMock.mockReturnValue(true);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.keyDown(document, { key: "Escape" });
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("hide_popover"));
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  invokeMock.mockClear();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(invokeMock).not.toHaveBeenCalledWith("hide_popover");
  isTauriMock.mockReturnValue(false);
});

it("opens_popover_after_profile_setup", async () => {
  const unprofiled = structuredClone(localSnapshot);
  unprofiled.planet.profile = null;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage") return structuredClone(unprofiled);
    if (command === "set_planet_profile") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return canonicalShopState();
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);
  const nickname = await screen.findByLabelText("행성에서 사용할 닉네임");
  expect(screen.queryByRole("main", { name: "행성 팝오버" })).not.toBeInTheDocument();
  fireEvent.change(nickname, { target: { value: "Orbit" } });
  fireEvent.click(screen.getByRole("button", { name: "행성 시작하기" }));
  expect(await screen.findByRole("main", { name: "행성 팝오버" })).toBeInTheDocument();
  expect(screen.getByText("Orbit의 행성")).toBeInTheDocument();
});

it("keeps_unknown_usage_distinct_from_zero", async () => {
  const unknown = structuredClone(localSnapshot);
  unknown.usage.confirmed_subtotal = null;
  unknown.usage.codex.total_tokens = null;
  unknown.usage.claude_code.total_tokens = null;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage") return unknown;
    if (command === "get_shop_state") return canonicalShopState();
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  render(<App />);
  await screen.findByText("아직 확인된 사용량 없음");
  expect(screen.getByRole("main", { name: "행성 팝오버" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "수집 상태" })).toHaveTextContent("—");
  expect(screen.getByRole("region", { name: "수집 상태" })).not.toHaveTextContent("0 토큰");
});

it("clears the canonical avatar preview when leaving the shop and personal detail", async () => {
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return canonicalShopState(600_000);
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  let shop = await screen.findByRole("region", { name: "행성 상점" });
  fireEvent.click(within(shop).getByRole("tab", { name: "아바타" }));
  fireEvent.click(within(shop).getByRole("button", { name: "탐험가 모자 미리보기" }));
  expect(container.querySelector('.shop-planet-preview [data-avatar-equipment="avatar_explorer_hat"]')).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(container.querySelector('.planet-landscape-avatar-sprite [data-avatar-equipment="avatar_explorer_hat"]')).not.toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button", { name: "행성 상점" }));
  shop = await screen.findByRole("region", { name: "행성 상점" });
  expect(container.querySelector('.shop-planet-preview [data-avatar-equipment="avatar_explorer_hat"]')).not.toBeInTheDocument();
  expect(within(shop).queryByText("탐험가 모자 미리보기 중")).not.toBeInTheDocument();
});


it("keeps the three token totals tied to their original fields", async () => {
  const distinct = structuredClone(localSnapshot);
  distinct.planet.current_planet_tokens = 7;
  distinct.planet.lifetime_tokens = 11;
  distinct.usage.confirmed_subtotal = 19;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "current_usage") return distinct;
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "list_world_members") return [];
    if (command === "list_world_invites") return [];
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

it("allows reset for a signed account with a matching canonical shop state", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  const resetSnapshot = structuredClone(localSnapshot);
  resetSnapshot.planet.current_cycle_id = "cycle-2";
  resetSnapshot.planet.profile = { nickname: "다시 시작", avatar: "masculine" };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return canonicalShopState();
    if (command === "reset_planet") return structuredClone(resetSnapshot);
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("reset_planet", expect.objectContaining({context:{generation:0},expected:expect.objectContaining({generation:0,current_cycle_id:"cycle-1"})})));
  await screen.findByText("다시 시작의 행성");
  confirm.mockRestore();
});

it("reports a confirmed signed reset as complete and offers refresh without another reset", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  let currentSnapshot = structuredClone(localSnapshot);
  const refreshedSnapshot = structuredClone(localSnapshot);
  refreshedSnapshot.planet.current_cycle_id = "cycle-2";
  refreshedSnapshot.planet.profile = { nickname: "새 행성", avatar: "masculine" };
  const resetResult = {
    kind: "confirmed_reset_view_unavailable",
    account_id: "owner",
    request_id: "90000000-0000-4000-8000-000000000052",
    expected_old_cycle_id: "cycle-1",
    new_cycle_id: "cycle-2",
  };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
    if (command === "current_usage") return structuredClone(currentSnapshot);
    if (command === "refresh_usage") {
      currentSnapshot = structuredClone(refreshedSnapshot);
      return structuredClone(currentSnapshot);
    }
    if (command === "get_shop_state") return { ...canonicalShopState(), current_cycle_id: currentSnapshot.planet.current_cycle_id };
    if (command === "reset_planet") throw resetResult;
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));

  const confirmedNotice = await screen.findByText("서버에서 행성 이동이 완료됐습니다. 화면 갱신만 필요합니다.");
  expect(confirmedNotice.closest('[role="status"]')).toHaveTextContent("서버에서 행성 이동이 완료됐습니다");
  expect(screen.getByRole("button", { name: "이동 완료" })).toBeDisabled();
  expect(screen.queryByText("행성을 초기화하지 못했습니다.")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "이동 완료" }));
  expect(invokeMock.mock.calls.filter(([command]) => command === "reset_planet")).toHaveLength(1);

  fireEvent.click(screen.getByRole("button", { name: "완료된 이동 상태 새로고침" }));
  await screen.findByText("새 행성의 행성");
  expect(invokeMock).toHaveBeenCalledWith("refresh_usage", {context:{generation:0}});
  expect(invokeMock.mock.calls.filter(([command]) => command === "reset_planet")).toHaveLength(1);
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  confirm.mockRestore();
});

it.each(["account", "cycle", "request"] as const)("treats mismatched confirmed-reset %s metadata as an ordinary retryable error", async (field) => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  const resetResult = {
    kind: "confirmed_reset_view_unavailable",
    account_id: field === "account" ? "another-user" : "owner",
    request_id: field === "request" ? "bad-id" : "90000000-0000-4000-8000-000000000053",
    expected_old_cycle_id: field === "cycle" ? "another-cycle" : "cycle-1",
    new_cycle_id: "cycle-2",
  };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return canonicalShopState();
    if (command === "reset_planet") throw resetResult;
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));

  expect(await screen.findByText("행성을 초기화하지 못했습니다.")).toBeInTheDocument();
  expect(screen.queryByText("서버에서 행성 이동이 완료됐습니다. 화면 갱신만 필요합니다.")).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  expect(invokeMock.mock.calls.filter(([command]) => command === "reset_planet")).toHaveLength(1);
  confirm.mockRestore();
});

it.each(["cooldown", "account", "cycle", "paused", "guest-import"] as const)("blocks signed reset when canonical eligibility has a %s mismatch", async (mismatch) => {
  const signedSnapshot = structuredClone(localSnapshot);
  if (mismatch === "cooldown") {
    signedSnapshot.planet.can_reset = false;
    signedSnapshot.planet.reset_available_at_utc = "2026-09-26T00:00:00Z";
  }
  const canonical = canonicalShopState();
  if (mismatch === "account") canonical.account_id = "account:other";
  if (mismatch === "cycle") canonical.current_cycle_id = "cycle-other";
  if (mismatch === "guest-import") canonical.guest_import_pending = true;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_in", world: null, sync_status: mismatch === "paused" ? "paused" : "synced" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(signedSnapshot);
    if (command === "get_shop_state") return structuredClone(canonical);
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  if (mismatch !== "paused") await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_shop_state", {context:{generation:0}}));
  const resetButton = screen.getByRole("button", { name: "다음 행성으로" });
  expect(resetButton).toBeDisabled();
  fireEvent.click(resetButton);
  expect(invokeMock).not.toHaveBeenCalledWith("reset_planet", expect.objectContaining({context:{generation:0},expected:expect.objectContaining({generation:0,current_cycle_id:"cycle-1"})}));
});

it("allows a signed retry after an ordinary held or uncertain reset response", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  let resetCalls = 0;
  const resetSnapshot = structuredClone(localSnapshot);
  resetSnapshot.planet.current_cycle_id = "cycle-2";
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return canonicalShopState();
    if (command === "reset_planet") {
      resetCalls += 1;
      if (resetCalls === 1) throw "초기화 결과를 확인할 수 없습니다. 같은 요청으로 재시도합니다.";
      return structuredClone(resetSnapshot);
    }
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  expect(await screen.findByText("초기화 결과를 확인할 수 없습니다. 같은 요청으로 재시도합니다.")).toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await screen.findByText("Orbit의 행성");
  await waitFor(() => expect(resetCalls).toBe(2));
  expect(invokeMock.mock.calls.filter(([command]) => command === "reset_planet")).toEqual([
    ["reset_planet", expect.any(Object)], ["reset_planet", expect.any(Object)],
  ]);
  confirm.mockRestore();
});

it("ignores a late signed reset success after the account changes", async () => {
  const pendingReset = deferred<WorldSnapshot>();
  let currentAccount: SharingState = { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
  let currentSnapshot = structuredClone(localSnapshot);
  const nextSnapshot = structuredClone(localSnapshot);
  nextSnapshot.planet.profile = { nickname: "새 계정", avatar: "masculine" };
  const staleSnapshot = structuredClone(localSnapshot);
  staleSnapshot.planet.profile = { nickname: "이전 초기화", avatar: "masculine" };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(currentSnapshot);
    if (command === "get_shop_state") return {
      ...canonicalShopState(),
      account_id: `account:${currentAccount.user_id}`,
      current_cycle_id: currentSnapshot.planet.current_cycle_id,
    };
    if (command === "reset_planet") return pendingReset.promise;
    return null;
  });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("reset_planet", expect.objectContaining({context:{generation:0},expected:expect.objectContaining({generation:0,current_cycle_id:"cycle-1"})})));

  currentAccount = { ...ownerState, user_id: "next-account", phase: "signed_in", world: null, sync_status: "synced" };
  currentSnapshot = nextSnapshot;
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await screen.findByText("새 계정의 행성");
  await act(async () => { pendingReset.resolve(staleSnapshot); });

  expect(screen.getByText("새 계정의 행성")).toBeInTheDocument();
  expect(screen.queryByText("이전 초기화의 행성")).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("ignores a late signed reset rejection after the cycle changes", async () => {
  const pendingReset = deferred<WorldSnapshot>();
  const currentAccount: SharingState = { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
  let currentSnapshot = structuredClone(localSnapshot);
  const nextSnapshot = structuredClone(localSnapshot);
  nextSnapshot.planet.current_cycle_id = "cycle-2";
  nextSnapshot.planet.profile = { nickname: "새 주기", avatar: "masculine" };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(currentSnapshot);
    if (command === "get_shop_state") return {
      ...canonicalShopState(),
      current_cycle_id: currentSnapshot.planet.current_cycle_id,
    };
    if (command === "reset_planet") return pendingReset.promise;
    return null;
  });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("reset_planet", expect.objectContaining({context:{generation:0},expected:expect.objectContaining({generation:0,current_cycle_id:"cycle-1"})})));

  currentSnapshot = nextSnapshot;
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await screen.findByText("새 주기의 행성");
  await act(async () => { pendingReset.reject(new Error("오래된 초기화 실패")); });

  expect(screen.getByText("새 주기의 행성")).toBeInTheDocument();
  expect(screen.queryByText("이전 초기화 실패")).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("keeps the local reset action for an eligible guest", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" };
    if (command === "current_usage" || command === "refresh_usage" || command === "reset_planet") return structuredClone(localSnapshot);
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("reset_planet", expect.objectContaining({context:{generation:0},expected:expect.objectContaining({generation:0,current_cycle_id:"cycle-1"})})));
  confirm.mockRestore();
});

it("drops a guest reset click when account context starts changing during confirmation", async () => {
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    return null;
  });
  const confirm = vi.spyOn(window, "confirm").mockImplementation(() => {
    listeners.get("world-context-changing")?.({ payload: "account" });
    return true;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  expect(confirm).toHaveBeenCalledTimes(1);
  expect(invokeMock).not.toHaveBeenCalledWith("reset_planet", expect.objectContaining({context:{generation:0},expected:expect.objectContaining({generation:0,current_cycle_id:"cycle-1"})}));
});

it("does not replace a new account snapshot with a pending old guest reset result", async () => {
  const pendingReset = deferred<WorldSnapshot>();
  let currentAccount: SharingState = { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" };
  let currentSnapshot = structuredClone(localSnapshot);
  const nextSnapshot = structuredClone(localSnapshot);
  nextSnapshot.planet.profile = { nickname: "Next", avatar: "masculine" };
  const staleSnapshot = structuredClone(localSnapshot);
  staleSnapshot.planet.profile = { nickname: "Stale", avatar: "masculine" };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(currentSnapshot);
    if (command === "get_shop_state") return {
      ...canonicalShopState(),
      account_id: currentAccount.user_id ? `account:${currentAccount.user_id}` : "local",
      current_cycle_id: currentSnapshot.planet.current_cycle_id,
    };
    if (command === "reset_planet") return pendingReset.promise;
    return null;
  });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("reset_planet", expect.objectContaining({context:{generation:0},expected:expect.objectContaining({generation:0,current_cycle_id:"cycle-1"})})));

  currentAccount = { ...ownerState, user_id: "next-account", phase: "signed_in", world: null, sync_status: "synced" };
  currentSnapshot = nextSnapshot;
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await screen.findByText("Next의 행성");
  await act(async () => { pendingReset.resolve(staleSnapshot); });

  expect(screen.getByText("Next의 행성")).toBeInTheDocument();
  expect(screen.queryByText("Stale의 행성")).not.toBeInTheDocument();
});

it("does not show a rejected old guest reset on the new account", async () => {
  const pendingReset = deferred<WorldSnapshot>();
  let currentAccount: SharingState = { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" };
  let currentSnapshot = structuredClone(localSnapshot);
  const nextSnapshot = structuredClone(localSnapshot);
  nextSnapshot.planet.profile = { nickname: "Next", avatar: "masculine" };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(currentSnapshot);
    if (command === "get_shop_state") return {
      ...canonicalShopState(),
      account_id: currentAccount.user_id ? `account:${currentAccount.user_id}` : "local",
      current_cycle_id: currentSnapshot.planet.current_cycle_id,
    };
    if (command === "reset_planet") return pendingReset.promise;
    return null;
  });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("reset_planet", expect.objectContaining({context:{generation:0},expected:expect.objectContaining({generation:0,current_cycle_id:"cycle-1"})})));

  currentAccount = { ...ownerState, user_id: "next-account", phase: "signed_in", world: null, sync_status: "synced" };
  currentSnapshot = nextSnapshot;
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await screen.findByText("Next의 행성");
  await act(async () => { pendingReset.reject(new Error("이전 계정 초기화 실패")); });

  expect(screen.getByText("Next의 행성")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("does not let an old reset completion clear the new cycle reset pending state", async () => {
  const firstReset = deferred<WorldSnapshot>();
  const secondReset = deferred<WorldSnapshot>();
  let resetCalls = 0;
  let currentSnapshot = structuredClone(localSnapshot);
  const staleSnapshot = structuredClone(localSnapshot);
  staleSnapshot.planet.profile = { nickname: "Stale", avatar: "masculine" };
  const nextSnapshot = structuredClone(localSnapshot);
  nextSnapshot.planet.current_cycle_id = "cycle-2";
  nextSnapshot.planet.profile = { nickname: "새 주기", avatar: "masculine" };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(currentSnapshot);
    if (command === "get_shop_state") return { ...canonicalShopState(), account_id: "local", current_cycle_id: currentSnapshot.planet.current_cycle_id };
    if (command === "reset_planet") return resetCalls++ === 0 ? firstReset.promise : secondReset.promise;
    return null;
  });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await waitFor(() => expect(resetCalls).toBe(1));

  currentSnapshot = nextSnapshot;
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await screen.findByText("새 주기의 행성");
  await waitFor(() => expect(screen.getByRole("button", { name: "다음 행성으로" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
  await waitFor(() => expect(resetCalls).toBe(2));

  await act(async () => { firstReset.resolve(staleSnapshot); });
  expect(screen.getByText("새 주기의 행성")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "처리 중" })).toBeDisabled();

  const completedSnapshot = structuredClone(nextSnapshot);
  completedSnapshot.planet.current_cycle_id = "cycle-3";
  completedSnapshot.planet.profile = { nickname: "완료", avatar: "masculine" };
  await act(async () => { secondReset.resolve(completedSnapshot); });
  await screen.findByText("완료의 행성");
});

it("explains reset effects and group visibility before the user acts", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  fireEvent.click(screen.getByRole("button", { name: "다음 행성으로" }));
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

it("offers removal for a selected generated object in the local guest planet", async () => {
  const guestState: SharingState = {
    ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local",
  };
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
    { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
  ];
  const naturalKey = { cycle_id: "cycle-1", stage: 0, ordinal: 0 } as const;
  let shopState = { ...canonicalShopState(), account_id: "local" };
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(guestState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "get_shop_state") return structuredClone(shopState);
    if (command === "quote_shop_action") return {
      target: { kind: "remove_natural", key: naturalKey },
      catalog_revision: 1,
      effect_revision: 0,
      price: 100_000,
    };
    if (command === "apply_shop_action") {
      const request = args?.request;
      if (!request) throw new Error("missing removal request");
      applied.push(request);
      shopState = { ...shopState, state_revision: 2, available_balance: 11_900_000, removed_natural_keys: [naturalKey] };
      return {
        status: "removed", request_id: request.request_id, confirmed_quote: null, state: structuredClone(shopState),
      } satisfies ShopActionResult;
    }
    if (command === "list_world_members") return [];
    return null;
  });

  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(container.querySelector('[data-landscape-object-id="0-0"]')!);

  const selectedObject = screen.getByRole("region", { name: "선택한 오브젝트" });
  fireEvent.click(within(selectedObject).getByRole("button", { name: "자연물 제거" }));

  const dialog = await screen.findByRole("dialog", { name: "바위 제거 확인" });
  expect(dialog).toHaveTextContent("최종 제거 비용 100K 토큰");
  expect(invokeMock).toHaveBeenCalledWith("quote_shop_action", {
    context:{generation:0},
    target: { kind: "remove_natural", key: naturalKey },
  });
  expect(applied).toHaveLength(0);

  const originalViewBox = container.querySelector(".planet-landscape-svg")?.getAttribute("viewBox");
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));

  await waitFor(() => expect(applied).toHaveLength(1));
  expect(applied[0]).toMatchObject({
    kind: "remove_natural",
    key: naturalKey,
    expected_version: 0,
    quote: { target: { kind: "remove_natural", key: naturalKey }, price: 100_000 },
  });
  expect(applied[0].request_id).toEqual(expect.any(String));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  expect(container.querySelector('[data-landscape-object-id="0-0"]')).not.toBeInTheDocument();
  expect(container.querySelector('[data-landscape-object-id="1-3"]')).toBeInTheDocument();
  expect(container.querySelector(".planet-landscape-svg")).toHaveAttribute("viewBox", originalViewBox);
  expect(snapshot.planet.objects).toHaveLength(2);
});

const naturalRemovalKey = { cycle_id: "cycle-1", stage: 0, ordinal: 0 } as const;

function naturalRemovalResult(
  request: ShopRequest,
  state: ShopState,
  status: ShopActionResult["status"] = "removed",
  confirmedQuote: ShopQuote | null = null,
  overrides: Partial<ShopState> = {},
): ShopActionResult {
  if (request.kind !== "remove_natural") throw new Error("expected natural removal request");
  const nextState: ShopState = {
    ...state,
    state_revision: state.state_revision + 1,
    available_balance: Math.max(0, state.available_balance - request.quote.price),
    removed_natural_keys: status === "removed"
      ? [...state.removed_natural_keys, request.key]
      : state.removed_natural_keys,
    ...overrides,
  };
  return { status, request_id: request.request_id, confirmed_quote: confirmedQuote, state: nextState };
}

function setupGuestNaturalRemoval(options: {
  quote?: (target: ShopQuote["target"]) => Promise<ShopQuote>;
  apply?: (request: ShopRequest, state: ShopState) => Promise<ShopActionResult>;
} = {}) {
  const guestState: SharingState = {
    ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local",
  };
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
    { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
  ];
  let state: ShopState = { ...canonicalShopState(), account_id: "local" };
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(guestState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "get_shop_state") return structuredClone(state);
    if (command === "quote_shop_action") {
      const target = args?.target ?? { kind: "remove_natural", key: naturalRemovalKey };
      return options.quote
        ? options.quote(target)
        : { target, catalog_revision: 1, effect_revision: 0, price: 100_000 } satisfies ShopQuote;
    }
    if (command === "apply_shop_action") {
      const request = args?.request;
      if (!request) throw new Error("missing removal request");
      applied.push(request);
      const result = options.apply
        ? await options.apply(request, structuredClone(state))
        : naturalRemovalResult(request, state);
      state = structuredClone(result.state);
      return result;
    }
    if (command === "list_world_members") return [];
    return null;
  });
  return {
    snapshot,
    applied,
    setBalance(balance: number) { state = { ...state, available_balance: balance, state_revision: state.state_revision + 1 }; },
    get state() { return state; },
  };
}

function setupSignedNaturalRemoval(options: {
  quote?: (target: ShopQuote["target"]) => Promise<ShopQuote>;
  apply?: (request: ShopRequest, state: ShopState) => Promise<ShopActionResult>;
  initialState?: ShopState;
} = {}) {
  let activeShared: SharingState = { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
  let activeSnapshot = structuredClone(localSnapshot);
  activeSnapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
  ];
  let state: ShopState = options.initialState ?? { ...canonicalShopState(100_000), state_revision: 1 };
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(activeShared);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(activeSnapshot);
    if (command === "get_shop_state") return structuredClone(state);
    if (command === "quote_shop_action") {
      const target = args?.target ?? { kind: "remove_natural", key: naturalRemovalKey };
      return options.quote
        ? options.quote(target)
        : { target, catalog_revision: 1, effect_revision: 0, price: 100_000 } satisfies ShopQuote;
    }
    if (command === "apply_shop_action") {
      const request = args?.request;
      if (!request) throw new Error("missing removal request");
      applied.push(request);
      const result = options.apply
        ? await options.apply(request, structuredClone(state))
        : naturalRemovalResult(request, state);
      state = structuredClone(result.state);
      return result;
    }
    if (command === "list_world_members") return [];
    return null;
  });
  return {
    applied,
    get state() { return state; },
    get snapshot() { return activeSnapshot; },
    setBalance(balance: number) { state = { ...state, available_balance: balance, state_revision: state.state_revision + 1 }; },
    setCanonicalState(next: ShopState) { state = structuredClone(next); },
    setSnapshot(next: WorldSnapshot) { activeSnapshot = structuredClone(next); },
    setShared(next: SharingState) { activeShared = structuredClone(next); },
  };
}

async function openNaturalRemoval(container: HTMLElement) {
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(container.querySelector('[data-landscape-object-id="0-0"]')!);
  const selected = screen.getByRole("region", { name: "선택한 오브젝트" });
  fireEvent.click(within(selected).getByRole("button", { name: "자연물 제거" }));
  return screen.findByRole("dialog", { name: "바위 제거 확인" });
}

async function openGuestNaturalRemoval(container: HTMLElement) {
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(container.querySelector('[data-landscape-object-id="0-0"]')!);
  const selected = screen.getByRole("region", { name: "선택한 오브젝트" });
  fireEvent.click(within(selected).getByRole("button", { name: "자연물 제거" }));
  return screen.findByRole("dialog", { name: "바위 제거 확인" });
}

it("does not apply a natural removal when its quote is canceled", async () => {
  const fixture = setupGuestNaturalRemoval();
  const { container } = render(<App />);
  const dialog = await openGuestNaturalRemoval(container);

  fireEvent.click(within(dialog).getByRole("button", { name: "취소" }));

  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  expect(fixture.applied).toHaveLength(0);
  expect(container.querySelector('[data-landscape-object-id="0-0"]')).toBeInTheDocument();
});

it("keeps the natural object visible when the canonical removal result is insufficient", async () => {
  const fixture = setupGuestNaturalRemoval({
    apply: async (request, state) => naturalRemovalResult(
      request, state, "insufficient_balance", null, { available_balance: 50_000 },
    ),
  });
  const { container } = render(<App />);
  const dialog = await openGuestNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("잔액이 부족해 자연물을 제거하지 못했습니다.");
  expect(within(screen.getByRole("dialog", { name: "바위 제거 확인" })).getByRole("status"))
    .toHaveTextContent("잔액이 부족합니다.");
  expect(container.querySelector('[data-landscape-object-id="0-0"]')).toBeInTheDocument();
  expect(fixture.state.removed_natural_keys).toEqual([]);
});

it("requires a new confirmation and request ID after the removal quote changes", async () => {
  let attempts = 0;
  const revisedQuote: ShopQuote = {
    target: { kind: "remove_natural", key: naturalRemovalKey },
    catalog_revision: 1, effect_revision: 1, price: 80_000,
  };
  const fixture = setupGuestNaturalRemoval({
    apply: async (request, state) => {
      attempts += 1;
      return attempts === 1
        ? naturalRemovalResult(request, state, "quote_changed", revisedQuote)
        : naturalRemovalResult(request, state);
    },
  });
  const { container } = render(<App />);
  let dialog = await openGuestNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("제거 비용이 변경되었습니다.");
  dialog = screen.getByRole("dialog", { name: "바위 제거 확인" });
  expect(dialog).toHaveTextContent("최종 제거 비용 80K 토큰");
  expect(fixture.applied).toHaveLength(1);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));

  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  expect(fixture.applied).toHaveLength(2);
  expect(fixture.applied[0].request_id).not.toBe(fixture.applied[1].request_id);
  expect(fixture.applied[1]).toMatchObject({ quote: revisedQuote, key: naturalRemovalKey });
});

it("retries an uncertain natural removal with the exact same request ID and payload", async () => {
  let attempts = 0;
  const fixture = setupGuestNaturalRemoval({
    apply: async (request, state) => {
      attempts += 1;
      if (attempts === 1) throw new Error("전송 결과를 확인할 수 없습니다.");
      return naturalRemovalResult(request, state);
    },
  });
  const { container } = render(<App />);
  let dialog = await openGuestNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("같은 요청 ID로 다시 확인합니다.");
  dialog = screen.getByRole("dialog", { name: "바위 제거 확인" });
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));

  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  expect(fixture.applied).toHaveLength(2);
  expect(fixture.applied[1]).toEqual(fixture.applied[0]);
  expect(fixture.applied[1].request_id).toEqual(expect.any(String));
});

it("keeps the same-ID receipt retry available after a refresh shows a lower balance", async () => {
  let attempts = 0;
  const fixture = setupGuestNaturalRemoval({
    apply: async (request, state) => {
      attempts += 1;
      if (attempts === 1) throw new Error("전송 결과를 확인할 수 없습니다.");
      return naturalRemovalResult(request, state);
    },
  });
  const { container } = render(<App />);
  let dialog = await openGuestNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("같은 요청 ID로 다시 확인합니다.");

  fixture.setBalance(50_000);
  await act(async () => { listeners.get("cosmetic-shop-updated")?.({ payload: null }); });
  dialog = screen.getByRole("dialog", { name: "바위 제거 확인" });
  await waitFor(() => expect(dialog).toHaveTextContent("현재 잔액 50K 토큰"));
  const retry = within(dialog).getByRole("button", { name: "제거 확인" });
  expect(retry).toBeEnabled();
  fireEvent.click(retry);

  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  expect(fixture.applied).toHaveLength(2);
  expect(fixture.applied[1]).toEqual(fixture.applied[0]);
});

it("blocks duplicate removal confirms while the canonical action is pending", async () => {
  const response = deferred<ShopActionResult>();
  const fixture = setupGuestNaturalRemoval({ apply: async () => response.promise });
  const { container } = render(<App />);
  const dialog = await openGuestNaturalRemoval(container);
  const confirmButton = within(dialog).getByRole("button", { name: "제거 확인" });
  fireEvent.click(confirmButton);
  fireEvent.click(confirmButton);

  await waitFor(() => expect(fixture.applied).toHaveLength(1));
  expect(screen.getByRole("button", { name: "제거 중" })).toBeDisabled();
  const result = naturalRemovalResult(fixture.applied[0], fixture.state);
  await act(async () => { response.resolve(result); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  expect(fixture.applied).toHaveLength(1);
});

it("discards a late removal quote when the user leaves the detail route", async () => {
  const pendingQuote = deferred<ShopQuote>();
  const fixture = setupGuestNaturalRemoval({ quote: () => pendingQuote.promise });
  const { container } = render(<App />);
  await openGuestNaturalRemoval(container);
  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());

  await act(async () => {
    pendingQuote.resolve({
      target: { kind: "remove_natural", key: naturalRemovalKey },
      catalog_revision: 1, effect_revision: 0, price: 100_000,
    });
  });

  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();
  expect(fixture.applied).toHaveLength(0);
});

it("invalidates a removal quote across a world lock even when the same context unlocks", async () => {
  const pendingQuote = deferred<ShopQuote>();
  const fixture = setupGuestNaturalRemoval({ quote: () => pendingQuote.promise });
  const { container } = render(<App />);
  await openGuestNaturalRemoval(container);

  await act(async () => { listeners.get("world-context-changing")?.({ payload: "account" }); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  await act(async () => { listeners.get("world-state-updated")?.({ payload: null }); });
  await screen.findByText("Orbit의 행성");
  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();

  await act(async () => {
    pendingQuote.resolve({
      target: { kind: "remove_natural", key: naturalRemovalKey },
      catalog_revision: 1, effect_revision: 0, price: 100_000,
    });
  });

  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();
  expect(fixture.applied).toHaveLength(0);
});

it("does not restore a removal dialog or late error after a same-context world lock", async () => {
  const pendingApply = deferred<ShopActionResult>();
  const fixture = setupGuestNaturalRemoval({ apply: async () => pendingApply.promise });
  const { container } = render(<App />);
  const dialog = await openGuestNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));
  await waitFor(() => expect(fixture.applied).toHaveLength(1));

  await act(async () => { listeners.get("world-context-changing")?.({ payload: "world" }); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  await act(async () => { listeners.get("world-state-updated")?.({ payload: null }); });
  await screen.findByText("Orbit의 행성");
  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();
  await act(async () => { pendingApply.reject(new Error("stale world action error")); });

  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("keeps a confirmed natural removal canonical when its result arrives after leaving detail", async () => {
  const pendingApply = deferred<ShopActionResult>();
  const fixture = setupGuestNaturalRemoval({ apply: async () => pendingApply.promise });
  const { container } = render(<App />);
  const dialog = await openGuestNaturalRemoval(container);
  const originalViewBox = container.querySelector(".planet-landscape-svg")?.getAttribute("viewBox");
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));
  await waitFor(() => expect(fixture.applied).toHaveLength(1));
  fireEvent.click(screen.getByRole("button", { name: /행성으로 돌아가기/ }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());

  await act(async () => {
    pendingApply.resolve(naturalRemovalResult(fixture.applied[0], fixture.state));
  });
  fireEvent.click(await screen.findByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });

  expect(container.querySelector('[data-landscape-object-id="0-0"]')).not.toBeInTheDocument();
  expect(container.querySelector('[data-landscape-object-id="1-3"]')).toBeInTheDocument();
  expect(container.querySelector(".planet-landscape-svg")).toHaveAttribute("viewBox", originalViewBox);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("discards a late removal quote after the guest cycle changes", async () => {
  const pendingQuote = deferred<ShopQuote>();
  let activeSnapshot = structuredClone(localSnapshot);
  activeSnapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
  ];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(activeSnapshot);
    if (command === "get_shop_state") return {
      ...canonicalShopState(), account_id: "local", current_cycle_id: activeSnapshot.planet.current_cycle_id,
    };
    if (command === "quote_shop_action") return pendingQuote.promise;
    if (command === "apply_shop_action") throw new Error("stale quote must not be applied");
    if (command === "list_world_members") return [];
    return null;
  });
  const { container } = render(<App />);
  await openGuestNaturalRemoval(container);
  const nextSnapshot = structuredClone(activeSnapshot);
  nextSnapshot.planet.current_cycle_id = "cycle-2";
  await act(async () => { listeners.get("usage-updated")?.({ payload: nextSnapshot }); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());

  await act(async () => {
    pendingQuote.resolve({
      target: { kind: "remove_natural", key: naturalRemovalKey },
      catalog_revision: 1, effect_revision: 0, price: 100_000,
    });
  });

  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();
  expect(invokeMock).not.toHaveBeenCalledWith("apply_shop_action", expect.anything());
});

it("ignores an old-cycle removal success after the guest has moved to a new cycle", async () => {
  const pendingApply = deferred<ShopActionResult>();
  let activeSnapshot = structuredClone(localSnapshot);
  activeSnapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
  ];
  let state: ShopState = { ...canonicalShopState(), account_id: "local" };
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { request?: ShopRequest }) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" };
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(activeSnapshot);
    if (command === "get_shop_state") return { ...structuredClone(state), current_cycle_id: activeSnapshot.planet.current_cycle_id };
    if (command === "quote_shop_action") return {
      target: { kind: "remove_natural", key: naturalRemovalKey },
      catalog_revision: 1, effect_revision: 0, price: 100_000,
    } satisfies ShopQuote;
    if (command === "apply_shop_action") {
      if (!args?.request) throw new Error("missing removal request");
      applied.push(args.request);
      return pendingApply.promise;
    }
    if (command === "list_world_members") return [];
    return null;
  });
  const originalState = structuredClone(state);
  const { container } = render(<App />);
  const dialog = await openGuestNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));
  await waitFor(() => expect(applied).toHaveLength(1));

  activeSnapshot = structuredClone(activeSnapshot);
  activeSnapshot.planet.current_cycle_id = "cycle-2";
  state = { ...state, current_cycle_id: "cycle-2", removed_natural_keys: [], state_revision: 2 };
  await act(async () => { listeners.get("usage-updated")?.({ payload: structuredClone(activeSnapshot) }); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  await act(async () => {
    pendingApply.resolve(naturalRemovalResult(applied[0], originalState));
  });

  expect(container.querySelector(".planet-landscape")).toHaveAttribute("data-cycle-id", "cycle-2");
  expect(container.querySelector('[data-landscape-object-id="0-0"]')).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("discards a late removal quote while the app switches from the guest to another account", async () => {
  const pendingQuote = deferred<ShopQuote>();
  let activeShared: SharingState = {
    ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local",
  };
  let activeSnapshot = structuredClone(localSnapshot);
  activeSnapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
  ];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(activeShared);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(activeSnapshot);
    if (command === "get_shop_state") return {
      ...canonicalShopState(),
      account_id: activeShared.user_id ? `account:${activeShared.user_id}` : "local",
      current_cycle_id: activeSnapshot.planet.current_cycle_id,
    };
    if (command === "quote_shop_action") return pendingQuote.promise;
    if (command === "apply_shop_action") throw new Error("stale quote must not be applied");
    if (command === "list_world_members") return [];
    return null;
  });
  const { container } = render(<App />);
  await openGuestNaturalRemoval(container);

  activeShared = { ...ownerState, phase: "signed_in", user_id: "next-account", world: null, sync_status: "synced" };
  activeSnapshot = structuredClone(activeSnapshot);
  activeSnapshot.planet.profile = { nickname: "Next", avatar: "masculine" };
  await act(async () => { listeners.get("world-context-changing")?.({ payload: "account" }); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  await act(async () => { listeners.get("world-state-updated")?.({ payload: null }); });
  await screen.findByText("Next의 행성");

  await act(async () => {
    pendingQuote.resolve({
      target: { kind: "remove_natural", key: naturalRemovalKey },
      catalog_revision: 1, effect_revision: 0, price: 100_000,
    });
  });

  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();
  expect(invokeMock).not.toHaveBeenCalledWith("apply_shop_action", expect.anything());
});

it("does not surface an old guest removal error after switching accounts", async () => {
  const pendingApply = deferred<ShopActionResult>();
  let activeShared: SharingState = {
    ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local",
  };
  let activeSnapshot = structuredClone(localSnapshot);
  activeSnapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
  ];
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(activeShared);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(activeSnapshot);
    if (command === "get_shop_state") return {
      ...canonicalShopState(),
      account_id: activeShared.user_id ? `account:${activeShared.user_id}` : "local",
      current_cycle_id: activeSnapshot.planet.current_cycle_id,
    };
    if (command === "quote_shop_action") return {
      target: args?.target ?? { kind: "remove_natural", key: naturalRemovalKey },
      catalog_revision: 1, effect_revision: 0, price: 100_000,
    } satisfies ShopQuote;
    if (command === "apply_shop_action") {
      if (!args?.request) throw new Error("missing removal request");
      applied.push(args.request);
      return pendingApply.promise;
    }
    if (command === "list_world_members") return [];
    return null;
  });
  const { container } = render(<App />);
  const dialog = await openGuestNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));
  await waitFor(() => expect(applied).toHaveLength(1));

  activeShared = { ...ownerState, phase: "signed_in", user_id: "next-account", world: null, sync_status: "synced" };
  activeSnapshot = structuredClone(activeSnapshot);
  activeSnapshot.planet.profile = { nickname: "Next", avatar: "masculine" };
  await act(async () => { listeners.get("world-context-changing")?.({ payload: "account" }); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  await act(async () => { listeners.get("world-state-updated")?.({ payload: null }); });
  await screen.findByText("Next의 행성");
  await act(async () => { pendingApply.reject(new Error("old account transport error")); });

  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "자연물 제거" })).not.toBeInTheDocument();
});

it("quotes and removes an authenticated user's natural object using canonical shop state", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [{ stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 }];
  const key = { cycle_id: "cycle-1", stage: 0, ordinal: 0 } as const;
  let state: ShopState = { ...canonicalShopState(100_000), state_revision: 1 };
  const applied: ShopRequest[] = [];
  invokeMock.mockImplementation(async (command: string, args?: { target?: ShopQuote["target"]; request?: ShopRequest }) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "get_shop_state") return structuredClone(state);
    if (command === "quote_shop_action") return {
      target: args?.target ?? { kind: "remove_natural", key },
      catalog_revision: 1, effect_revision: 0, price: 100_000,
    } satisfies ShopQuote;
    if (command === "apply_shop_action") {
      const request = args?.request;
      if (!request) throw new Error("missing removal request");
      applied.push(request);
      state = { ...state, state_revision: 2, available_balance: 0, removed_natural_keys: [key] };
      return { status: "removed", request_id: request.request_id, confirmed_quote: null, state: structuredClone(state) } satisfies ShopActionResult;
    }
    if (command === "list_world_members") return [];
    return null;
  });
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(container.querySelector('[data-landscape-object-id="0-0"]')!);

  const selection = screen.getByRole("region", { name: "선택한 오브젝트" });
  fireEvent.click(within(selection).getByRole("button", { name: "자연물 제거" }));
  const dialog = await screen.findByRole("dialog", { name: "바위 제거 확인" });
  expect(dialog).toHaveTextContent("현재 잔액 100K 토큰");
  expect(dialog).toHaveTextContent("최종 제거 비용 100K 토큰");
  expect(invokeMock).toHaveBeenCalledWith("quote_shop_action", {
    context:{generation:0},
    target: { kind: "remove_natural", key },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));

  await waitFor(() => expect(applied).toHaveLength(1));
  expect(applied[0]).toEqual({
    kind: "remove_natural", request_id: expect.any(String), key, expected_version: 0,
    quote: { target: { kind: "remove_natural", key }, catalog_revision: 1, effect_revision: 0, price: 100_000 },
  });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  expect(container.querySelector('[data-landscape-object-id="0-0"]')).not.toBeInTheDocument();
  expect(snapshot.planet.objects).toHaveLength(1);
  expect(snapshot.planet.removed_natural_keys).toEqual([]);
});

it("keeps signed receipt retry enabled after canonical refresh lowers the balance", async () => {
  let attempts = 0;
  const fixture = setupSignedNaturalRemoval({
    apply: async (request, state) => {
      attempts += 1;
      if (attempts === 1) throw new Error("전송 결과를 확인할 수 없습니다.");
      return naturalRemovalResult(request, state);
    },
  });
  const { container } = render(<App />);
  let dialog = await openNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("같은 요청 ID로 다시 확인합니다.");
  const originalRequest = fixture.applied[0];

  fixture.setBalance(50_000);
  await act(async () => { listeners.get("cosmetic-shop-updated")?.({ payload: null }); });
  dialog = screen.getByRole("dialog", { name: "바위 제거 확인" });
  await waitFor(() => expect(dialog).toHaveTextContent("현재 잔액 50K 토큰"));
  const retry = within(dialog).getByRole("button", { name: "제거 확인" });
  expect(retry).toBeEnabled();
  fireEvent.click(retry);

  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  expect(fixture.applied).toHaveLength(2);
  expect(fixture.applied[1]).toBe(originalRequest);
  expect(fixture.applied[1].request_id).toBe(fixture.applied[0].request_id);
  expect(container.querySelector('[data-landscape-object-id="0-0"]')).not.toBeInTheDocument();
});

it.each([
  ["another account", { ...canonicalShopState(100_000), account_id: "account:other" }],
  ["another cycle", { ...canonicalShopState(100_000), current_cycle_id: "cycle-old" }],
] as const)("does not expose signed natural removal when canonical state belongs to %s", async (_label, state) => {
  const fixture = setupSignedNaturalRemoval({ initialState: state });
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(container.querySelector('[data-landscape-object-id="0-0"]')!);

  const selection = screen.getByRole("region", { name: "선택한 오브젝트" });
  expect(within(selection).queryByRole("button", { name: "자연물 제거" })).not.toBeInTheDocument();
  expect(invokeMock).not.toHaveBeenCalledWith("quote_shop_action", expect.anything());
  expect(fixture.applied).toHaveLength(0);
});

it("keeps a signed natural object visible and reports a canonical insufficient-balance result", async () => {
  const fixture = setupSignedNaturalRemoval({
    apply: async (request, state) => naturalRemovalResult(
      request, state, "insufficient_balance", null, { available_balance: 50_000 },
    ),
  });
  const { container } = render(<App />);
  const dialog = await openNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("잔액이 부족해 자연물을 제거하지 못했습니다.");
  expect(screen.getByRole("dialog", { name: "바위 제거 확인" })).toHaveTextContent("현재 잔액 50K 토큰");
  expect(container.querySelector('[data-landscape-object-id="0-0"]')).toBeInTheDocument();
  expect(fixture.state.removed_natural_keys).toEqual([]);
});

it("discards a signed removal quote after switching to another account", async () => {
  const pendingQuote = deferred<ShopQuote>();
  const fixture = setupSignedNaturalRemoval({ quote: () => pendingQuote.promise });
  const { container } = render(<App />);
  await openNaturalRemoval(container);

  const nextShared: SharingState = {
    ...ownerState, phase: "signed_in", user_id: "next-account", world: null, sync_status: "synced",
  };
  const nextSnapshot = structuredClone(fixture.snapshot);
  nextSnapshot.planet.profile = { nickname: "Next", avatar: "masculine" };
  fixture.setSnapshot(nextSnapshot);
  fixture.setShared(nextShared);
  await act(async () => { listeners.get("world-context-changing")?.({ payload: "account" }); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());
  await act(async () => { listeners.get("world-state-updated")?.({ payload: null }); });
  await screen.findByText("Next의 행성");

  await act(async () => {
    pendingQuote.resolve({
      target: { kind: "remove_natural", key: naturalRemovalKey },
      catalog_revision: 1, effect_revision: 0, price: 100_000,
    });
  });

  expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument();
  expect(invokeMock).not.toHaveBeenCalledWith("apply_shop_action", expect.anything());
});

it("ignores a signed removal result after the same account moves to a new cycle", async () => {
  const pendingApply = deferred<ShopActionResult>();
  const fixture = setupSignedNaturalRemoval({ apply: async () => pendingApply.promise });
  const { container } = render(<App />);
  const dialog = await openNaturalRemoval(container);
  fireEvent.click(within(dialog).getByRole("button", { name: "제거 확인" }));
  await waitFor(() => expect(fixture.applied).toHaveLength(1));

  const nextSnapshot = structuredClone(fixture.snapshot);
  nextSnapshot.planet.current_cycle_id = "cycle-2";
  fixture.setSnapshot(nextSnapshot);
  fixture.setCanonicalState({
    ...fixture.state, current_cycle_id: "cycle-2", state_revision: fixture.state.state_revision + 1,
    removed_natural_keys: [],
  });
  await act(async () => { listeners.get("usage-updated")?.({ payload: nextSnapshot }); });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "바위 제거 확인" })).not.toBeInTheDocument());

  await act(async () => {
    pendingApply.resolve(naturalRemovalResult(fixture.applied[0], {
      ...canonicalShopState(100_000), state_revision: 1,
    }));
  });

  expect(container.querySelector(".planet-landscape")).toHaveAttribute("data-cycle-id", "cycle-2");
  expect(container.querySelector('[data-landscape-object-id="0-0"]')).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

function shortMacPopup() {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
  vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener() {}, removeEventListener() {} }));
}

it("shows_only_core_summary_in_short_popup", async () => {
  shortMacPopup();
  render(<App />);
  await screen.findByText("Orbit의 행성");
  expect(screen.getByText("이번 행성 토큰")).toBeInTheDocument();
  expect(screen.getByText("현재 시대")).toBeInTheDocument();
  expect(screen.getByRole("progressbar", { name: "다음 시대 진행도" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "행성·그룹 자세히 보기" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "수집 상태" })).not.toBeInTheDocument();
  expect(document.querySelector(".usage-summary")).toBeNull();
  expect(document.querySelector(".sync-status")).toBeNull();
});

it("short_popup_keeps_transition_retry", async () => {
  shortMacPopup(); isTauriMock.mockReturnValue(true);
  const previous = invokeMock.getMockImplementation()!;
  invokeMock.mockImplementation(async (command: string, ...args: unknown[]) => {
    if (command === "set_detail_view") throw "window size unavailable";
    return previous(command, ...args);
  });
  render(<App />); await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("화면을 전환하지 못했습니다");
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeInTheDocument();
  expect(document.querySelector(".app-shell--compact-popover")).not.toBeNull();
});

it("short_popup_opens_unchanged_detail_with_secondary_information", async () => {
  shortMacPopup(); render(<App />); await screen.findByText("Orbit의 행성");
  expect(document.querySelector(".usage-summary")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  expect(document.querySelector(".app-shell--detail")).not.toBeNull();
  expect(document.querySelector(".app-shell--macos-popover")).toBeNull();
  expect(document.querySelector(".usage-summary")).not.toBeNull();
  expect(document.querySelector(".source-list")).not.toBeNull();
  expect(document.querySelector(".sync-status")).not.toBeNull();
});


it("compacts planet and usage tokens while preserving growth credit precision", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.current_planet_tokens = 13200;
  snapshot.planet.lifetime_tokens = 239824;
  snapshot.planet.growth_credit = 1234.125;
  snapshot.usage.codex.total_tokens = 1000000;
  snapshot.usage.confirmed_subtotal = 1000000;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return snapshot;
    if (command === "list_world_members") return [];
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByText("13.2K")).toBeInTheDocument();
  expect(screen.getByText("239.82K")).toBeInTheDocument();
  expect(screen.getByText("1,234.125")).toBeInTheDocument();
  expect(screen.getAllByText("1M").length).toBeGreaterThan(0);
});


it("shows the latest visible growth object instead of a hidden recent kind", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "tree", x: 30, y: 40, seed: 1 },
    ...["road", "path", "rail", "fern"].map((kind, ordinal) => ({ stage: 3, ordinal, kind, x: 50, y: 40, seed: 2 })),
  ];
  const previous = invokeMock.getMockImplementation()!;
  invokeMock.mockImplementation(async (command: string, ...args: unknown[]) => {
    if (command === "current_usage" || command === "refresh_usage") return snapshot;
    return previous(command, ...args);
  });
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByText("최근 생성: 나무")).toBeInTheDocument();
  expect(container.querySelector(".recent-object")).not.toHaveTextContent("양치식물");
});


it("uses owner invitations and clears the code when the group panel closes", async () => {
  const base = invokeMock.getMockImplementation()!;
  invokeMock.mockImplementation(async (command: string, args?: unknown) => {
    if (command === "create_world_invite") return {invite_id: "one", code: "a".repeat(64), created_at: "2026-10-08T00:00:00Z", expires_at: "2026-10-15T00:00:00Z"};
    return base(command, args);
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", {name: "행성·그룹 자세히 보기"}));
  fireEvent.click(await screen.findByRole("tab", {name: "그룹"}));
  fireEvent.click(await screen.findByRole("button", {name: "초대 발급"}));
  await screen.findByText("a".repeat(64));
  fireEvent.click(screen.getByRole("tab", {name: "내 행성"}));
  fireEvent.click(screen.getByRole("tab", {name: "그룹"}));
  await screen.findByRole("button", {name: "초대 발급"});
  expect(screen.queryByText("a".repeat(64))).not.toBeInTheDocument();
  expect(invokeMock.mock.calls.some(([name]) => ["get_my_member_code","rotate_my_member_code"].includes(name))).toBe(false);
});


it("shows a join failure and permits the same invitation retry without legacy fallback", async () => {
  const base = invokeMock.getMockImplementation()!;
  let attempts = 0;
  invokeMock.mockImplementation(async (command: string, args?: unknown) => {
    if (command === "get_sharing_state") return { ...ownerState, phase: "signed_in", world: null };
    if (command === "join_world") {
      attempts++;
      if (attempts === 1) throw "가입 응답을 확인하지 못했습니다. 같은 초대 코드로 다시 시도하세요";
      return ownerState;
    }
    return base(command, args);
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", {name: "행성·그룹 자세히 보기"}));
  fireEvent.click(await screen.findByRole("tab", {name: "그룹"}));
  fireEvent.change(await screen.findByLabelText("소유자가 알려준 64자리 초대 코드"), {target: {value: "a".repeat(64)}});
  fireEvent.click(screen.getByRole("button", {name: "코드로 참여"}));
  expect(await screen.findByText(/가입 응답을 확인하지 못했습니다/)).toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", {name: "코드로 참여"})).not.toBeDisabled());
  fireEvent.click(screen.getByRole("button", {name: "코드로 참여"}));
  await screen.findByRole("button", {name: "초대 발급"});
  expect(attempts).toBe(2);
  expect(invokeMock.mock.calls.some(([name]) => ["get_my_member_code","rotate_my_member_code"].includes(name))).toBe(false);
});

it("shows recorded current and next planet numbers before moving", async () => {
  const snapshot = {...structuredClone(localSnapshot), generation:0, planet_ordinal:{status:"verified",current:4}};
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "current_usage") return snapshot;
    if (command === "get_sharing_state") return {...ownerState,phase:"signed_out",user_id:null,world:null,sync_status:"local"};
    return null;
  });
  const confirm = vi.spyOn(window,"confirm").mockReturnValue(false);
  render(<App />);
  fireEvent.click(await screen.findByRole("button",{name:"행성·그룹 자세히 보기"}));
  fireEvent.click(await screen.findByRole("button",{name:"다음 행성으로"}));
  expect(confirm).toHaveBeenCalledWith(expect.stringMatching(/4번째.*5번째/));
  expect(invokeMock.mock.calls.some(([command]) => command === "reset_planet")).toBe(false);
});

it("rechecks a missed reset on focus and ignores old usage events", async () => {
  let generation = 0;
  const fresh = {...structuredClone(localSnapshot),generation:1,planet:{...localSnapshot.planet,profile:{nickname:"Fresh",avatar:"masculine"}}};
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "current_usage") return generation === 0 ? localSnapshot : fresh;
    if (command === "get_sharing_state") return {...ownerState,phase:"signed_out",user_id:null,world:null,sync_status:"local"};
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  generation = 1;
  resetViewMock.mockReturnValue({state:{generation:1,phase:"completed",request_id:"reset"},actions_blocked:false,storage_completed:true});
  fireEvent(window,new Event("focus"));
  await screen.findByText("Fresh의 행성");
  act(() => listeners.get("usage-updated")?.({payload:localSnapshot}));
  expect(screen.queryByText("Orbit의 행성")).not.toBeInTheDocument();
  expect(screen.getByText("Fresh의 행성")).toBeInTheDocument();
});
it("offers device reset before profile creation", async () => {
  const snapshot = {...structuredClone(localSnapshot),planet:{...localSnapshot.planet,profile:null}};
  invokeMock.mockImplementation(async (command: string) => command === "current_usage" ? snapshot : command === "get_sharing_state" ? {...ownerState,phase:"signed_out",user_id:null,world:null,sync_status:"local"} : null);
  render(<App />);
  await screen.findByLabelText("행성에서 사용할 닉네임");
  fireEvent.click(screen.getByRole("button",{name:"기기 데이터 전체 초기화"}));
  expect(screen.getByRole("dialog",{name:"기기 데이터 전체 초기화 확인"})).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button",{name:"취소"}));
  expect(invokeMock.mock.calls.some(([command]) => command === "reset_device_data")).toBe(false);
});

it("shows an explicit unknown ordinal instead of guessing from balance", async () => {
  const snapshot = {...structuredClone(localSnapshot),planet_ordinal:{status:"unknown",current:null}};
  invokeMock.mockImplementation(async (command: string) => command === "current_usage" ? snapshot : command === "get_sharing_state" ? {...ownerState,phase:"signed_out",user_id:null,world:null,sync_status:"local"} : null);
  const confirm = vi.spyOn(window,"confirm").mockReturnValue(false);
  render(<App />);
  fireEvent.click(await screen.findByRole("button",{name:"행성·그룹 자세히 보기"}));
  fireEvent.click(await screen.findByRole("button",{name:"다음 행성으로"}));
  expect(confirm).toHaveBeenCalledWith(expect.stringContaining("현재 행성 순서를 확인할 수 없습니다"));
});

for (const mode of ["signed-in", "usage-error"] as const) {
  it(`offers device reset on the ${mode} screen`, async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "current_usage") {
        if (mode === "usage-error") throw new Error("isolated ledger failure");
        return structuredClone(localSnapshot);
      }
      if (command === "get_sharing_state") return { ...ownerState, phase: "signed_in", world: null };
      return null;
    });
    render(<App />);
    if (mode === "usage-error") await screen.findByText("기기 안의 사용량 원장을 열지 못했습니다.");
    else await screen.findByText("Orbit의 행성");
    fireEvent.click(screen.getByRole("button", { name: "기기 데이터 전체 초기화" }));
    expect(screen.getByRole("dialog", { name: "기기 데이터 전체 초기화 확인" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    expect(invokeMock.mock.calls.some(([command]) => command === "reset_device_data")).toBe(false);
  });
}

it("keeps the completed reset screen when older phases arrive late", async () => {
  const fresh = { ...structuredClone(localSnapshot), generation: 1 };
  invokeMock.mockImplementation(async (command: string) => command === "current_usage" ? fresh : command === "get_sharing_state" ? { ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local" } : null);
  resetViewMock.mockReturnValue({ state: { generation: 1, phase: "completed", request_id: "reset" }, actions_blocked: false, storage_completed: true });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  for (const phase of ["pending", "local_committed"]) {
    await act(async () => listeners.get("device-reset-updated")?.({ payload: { generation: 1, data: { state: { generation: 1, phase, request_id: "reset" }, actions_blocked: true, storage_completed: phase === "local_committed" } } }));
    expect(screen.getByRole("button", { name: "기기 데이터 전체 초기화" })).not.toBeDisabled();
    expect(screen.queryByRole("button", { name: "진행 중인 초기화 복구" })).not.toBeInTheDocument();
    expect(screen.getByText("Orbit의 행성")).toBeInTheDocument();
  }
});
