import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ShopPanel } from "../ShopPanel";
import type { ShopPanelPreview, ShopPanelProps } from "../ShopPanel";
import type { ShopActionResult, ShopProduct, ShopQuote, ShopQuoteTarget, ShopRequest, ShopState } from "../../types/usage";

const LANDSCAPE_PRODUCTS: Array<[string, string, "ground" | "sky"]> = [
  ["land_pond", "연못", "ground"], ["land_well", "우물", "ground"],
  ["land_greenhouse", "온실", "ground"], ["land_reservoir", "저수지", "ground"],
  ["land_crystal", "수정탑", "ground"], ["land_school", "학교", "ground"],
  ["land_observatory", "천문대", "ground"], ["land_laboratory", "연구소", "ground"],
  ["land_market", "시장", "ground"], ["land_trading_post", "교역소", "ground"],
  ["land_freight", "화물 터미널", "ground"], ["land_bazaar", "대형 상가", "ground"],
  ["land_rover", "탐사 로버", "ground"], ["land_clocktower", "시계탑", "ground"],
  ["land_launchpad", "발사대", "ground"], ["land_portal", "포털", "ground"],
  ["land_toolbox", "정리 도구함", "ground"], ["land_excavator", "굴착기", "ground"],
  ["land_cutter", "암석 절단기", "ground"], ["land_recycler", "재활용 로봇", "ground"],
  ["land_flag", "깃발", "ground"], ["land_thin_ring", "얇은 고리", "sky"],
  ["land_double_ring", "이중 고리", "sky"], ["land_moonlets", "작은 위성들", "sky"],
  ["land_lantern", "등불", "ground"], ["land_stars", "별무리", "sky"],
  ["land_aurora", "오로라", "sky"], ["land_meteors", "유성우", "sky"],
  ["land_garden", "꽃 정원", "ground"], ["land_tree", "장식 나무", "ground"],
  ["land_bench", "벤치", "ground"], ["land_fountain", "분수", "ground"],
];

const AVATAR_PRODUCTS: Array<[string, string, "head" | "outfit" | "face" | "back"]> = [
  ["avatar_explorer_hat", "탐험가 모자", "head"], ["avatar_crown", "왕관", "head"],
  ["avatar_space_helmet", "우주 헬멧", "head"], ["avatar_halo", "홀로그램 관", "head"],
  ["avatar_workwear", "작업복", "outfit"], ["avatar_labwear", "연구복", "outfit"],
  ["avatar_spacesuit", "우주복", "outfit"], ["avatar_nebula_suit", "성운 의상", "outfit"],
  ["avatar_glasses", "안경", "face"], ["avatar_sunglasses", "선글라스", "face"],
  ["avatar_goggles", "고글", "face"], ["avatar_hud", "HUD 바이저", "face"],
  ["avatar_backpack", "배낭", "back"], ["avatar_cape", "망토", "back"],
  ["avatar_jetpack", "제트팩", "back"], ["avatar_wings", "에너지 날개", "back"],
];

const EFFECT_TYPES = [
  "token_earning", "civilization_growth", "shop_discount", "reset_cooldown",
  "natural_removal_discount", "era_reward", "streak_reward",
] as const;

function catalog(): ShopProduct[] {
  return [
    ...LANDSCAPE_PRODUCTS.map(([sku, display_name, placement_zone], index) => ({
      sku,
      category: "landscape" as const,
      display_name,
      price: sku === "land_pond" ? 101 : 200 + index,
      catalog_revision: 1,
      purchasable: true,
      placement_zone,
      avatar_slot: null,
      effect_type: EFFECT_TYPES[index % EFFECT_TYPES.length],
      effect_value: 100,
    })),
    ...AVATAR_PRODUCTS.map(([sku, display_name, avatar_slot], index) => ({
      sku,
      category: "avatar" as const,
      display_name,
      price: sku === "avatar_crown" ? 101 : 300 + index,
      catalog_revision: 1,
      purchasable: true,
      placement_zone: null,
      avatar_slot,
      effect_type: null,
      effect_value: 0,
    })),
  ];
}

function shopState(overrides: Partial<ShopState> = {}): ShopState {
  return {
    account_id: "local",
    current_cycle_id: "cycle-1",
    catalog_revision: 1,
    state_revision: 1,
    available_balance: 100_000_000,
    products: catalog(),
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
    reward_state: {
      reward_timezone: "UTC",
      settled_cycle_tokens: 0,
      era_reward_tokens: 0,
      streak_reward_tokens: 0,
    },
    action_unavailable_reason: null,
    guest_import_pending: false,
    guest_import_error: null,
    ...overrides,
  };
}

function mountPanel(state = shopState(), overrides: Partial<ShopPanelProps> = {}) {
  const quote = vi.fn(async (target: ShopQuoteTarget): Promise<ShopQuote> => ({
    target,
    catalog_revision: state.catalog_revision,
    effect_revision: state.state_revision,
    price: state.products.find((product) => product.sku === (target.kind === "purchase" ? target.sku : ""))?.price ?? 1,
  }));
  const apply = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
    status: "purchased" as const,
    request_id: request.request_id,
    confirmed_quote: null,
    state,
  }));
  const retryPending = vi.fn(async () => null);
  const onPreviewChange = vi.fn();
  const onSelectLandscapeInstance = vi.fn();
  const createRequestId = vi.fn(() => "00000000-0000-4000-8000-000000000001");
  const props: ShopPanelProps = {
    state,
    quote,
    apply,
    retryPending,
    onPreviewChange,
    onSelectLandscapeInstance,
    createRequestId,
    ...overrides,
  };
  const view = render(<ShopPanel {...props} />);
  return {
    view,
    quote: props.quote as ReturnType<typeof vi.fn>,
    apply: props.apply as ReturnType<typeof vi.fn>,
    retryPending: props.retryPending as ReturnType<typeof vi.fn>,
    onPreviewChange: props.onPreviewChange as ReturnType<typeof vi.fn>,
    onSelectLandscapeInstance: props.onSelectLandscapeInstance as ReturnType<typeof vi.fn>,
    createRequestId: props.createRequestId as ReturnType<typeof vi.fn>,
  };
}

describe("ShopPanel", () => {
  it.each([
    [-13.2, "0 토큰"],
    [999.99, "999 토큰"],
  ])("normalizes balance %s before compact display", (balance, expected) => {
    const { view } = mountPanel(shopState({ available_balance: balance }));
    expect(view.container.querySelector(".cosmetic-balance strong")).toHaveTextContent(expected);
  });

  it("displays a compact quote while submitting its exact integer price", async () => {
    const state = shopState();
    state.products[0] = { ...state.products[0], price: 239824 };
    const { apply } = mountPanel(state);
    fireEvent.click(screen.getByRole("button", { name: "연못 구매" }));
    await screen.findByRole("dialog", { name: "연못 구매 확인" });
    expect(screen.getByText("239.82K 토큰", { selector: "strong" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "구매 확정" }));
    expect(apply.mock.calls[0][0].quote.price).toBe(239824);
  });

  it("renders each canonical SKU once across the landscape and avatar categories", () => {
    const { view } = mountPanel();

    const landscapeArt = Array.from(view.container.querySelectorAll("[data-shop-product-thumbnail='landscape']"));
    expect(landscapeArt).toHaveLength(32);
    expect(new Set(landscapeArt.map((svg) => svg.querySelector("[data-landscape-art]")?.getAttribute("data-landscape-art"))).size).toBe(32);

    fireEvent.click(screen.getByRole("tab", { name: "아바타" }));
    const avatarArt = Array.from(view.container.querySelectorAll("[data-avatar-style]"));
    expect(avatarArt).toHaveLength(16);
    expect(screen.getAllByRole("img")).toHaveLength(16);
  });

  it("caps purchases at five total stored and placed landscape instances per SKU", () => {
    const instances = Array.from({ length: 5 }, (_, variation_index) => ({
      instance_id: `pond-${variation_index}`,
      sku: "land_pond",
      variation_index,
      seed: `seed-${variation_index}`,
      variation_version: 1,
      placement_version: 1,
    }));
    const { view } = mountPanel(shopState({
      landscape_instances: instances,
      placements: [
        { instance_id: "pond-0", cycle_id: "cycle-1", x: 0, y: 0, version: 1 },
        { instance_id: "pond-1", cycle_id: "cycle-1", x: 64, y: 0, version: 1 },
      ],
    }));

    expect(screen.getByText("5 / 5 보유")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "연못 구매" })).toBeDisabled();
    fireEvent.click(screen.getByRole("tab", { name: "보유함" }));
    expect(view.container.querySelectorAll("[data-shop-instance]")).toHaveLength(5);
    expect(screen.getAllByText(/보관함|배치됨/)).toHaveLength(5);
  });

  it("quotes before opening purchase confirmation and submits that exact quote only after confirmation", async () => {
    const { quote, apply } = mountPanel();

    fireEvent.click(screen.getByRole("button", { name: "연못 구매" }));
    expect(quote).toHaveBeenCalledWith({ kind: "purchase", sku: "land_pond" });
    expect(await screen.findByRole("dialog", { name: "연못 구매 확인" })).toBeInTheDocument();
    expect(apply).not.toHaveBeenCalled();
    expect(screen.getByText("101 토큰", { selector: "strong" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "구매 확정" }));
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply.mock.calls[0][0]).toMatchObject({
      kind: "purchase",
      request_id: "00000000-0000-4000-8000-000000000001",
      quote: await quote.mock.results[0].value,
    });
  });

  it("requires a second human confirmation after a revised quote and submits the revised quote with a new id", async () => {
    const initialQuote: ShopQuote = {
      target: { kind: "purchase", sku: "land_pond" },
      catalog_revision: 1,
      effect_revision: 1,
      price: 101,
    };
    const revisedQuote: ShopQuote = { ...initialQuote, effect_revision: 2, price: 115 };
    const state = shopState();
    const apply = vi.fn()
      .mockResolvedValueOnce({ status: "quote_changed", request_id: "id-1", confirmed_quote: revisedQuote, state })
      .mockResolvedValueOnce({ status: "purchased", request_id: "id-2", confirmed_quote: revisedQuote, state });
    const ids = vi.fn().mockReturnValueOnce("id-1").mockReturnValueOnce("id-2");
    mountPanel(state, { quote: vi.fn(async () => initialQuote), apply, createRequestId: ids });

    fireEvent.click(screen.getByRole("button", { name: "연못 구매" }));
    await screen.findByRole("dialog", { name: "연못 구매 확인" });
    fireEvent.click(screen.getByRole("button", { name: "구매 확정" }));
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(1));
    expect(screen.getByText("가격이 바뀌었습니다. 새 금액을 확인한 뒤 다시 확정해 주세요.")).toBeInTheDocument();
    expect(screen.getByText("115 토큰", { selector: "strong" })).toBeInTheDocument();
    expect(apply).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "구매 확정" }));
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(2));
    expect(apply.mock.calls[0][0]).toMatchObject({ request_id: "id-1", quote: initialQuote });
    expect(apply.mock.calls[1][0]).toMatchObject({ request_id: "id-2", quote: revisedQuote });
    expect(ids).toHaveBeenCalledTimes(2);
  });

  it("closes the old purchase confirmation when retrying an uncertain request confirms it", async () => {
    const state = shopState();
    const quoteValue: ShopQuote = {
      target: { kind: "purchase", sku: "land_pond" },
      catalog_revision: 1,
      effect_revision: 1,
      price: 101,
    };
    const request: ShopRequest = { kind: "purchase", request_id: "same-id", quote: quoteValue };
    const pending = { request, status: "uncertain" as const, error: "응답 대기 중" };
    const quote = vi.fn(async () => quoteValue);
    const apply = vi.fn(async (_request: ShopRequest): Promise<ShopActionResult> => { throw new Error("응답 대기 중"); });
    const retryPending = vi.fn(async (): Promise<ShopActionResult> => ({
      status: "purchased", request_id: "same-id", confirmed_quote: quoteValue, state,
    }));
    const createRequestId = vi.fn(() => "same-id");
    const props = {
      state,
      quote,
      apply,
      retryPending,
      onPreviewChange: vi.fn(),
      onSelectLandscapeInstance: vi.fn(),
      createRequestId,
    };
    const view = render(<ShopPanel {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "연못 구매" }));
    await screen.findByRole("dialog", { name: "연못 구매 확인" });
    fireEvent.click(screen.getByRole("button", { name: "구매 확정" }));
    await screen.findByText("요청 결과를 확인하지 못했습니다. 같은 요청으로 다시 시도해 주세요.");
    view.rerender(<ShopPanel {...props} pending={pending} />);

    fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 시도" }));
    await waitFor(() => expect(retryPending).toHaveBeenCalledTimes(1));
    view.rerender(<ShopPanel {...props} pending={null} />);

    expect(screen.queryByRole("dialog", { name: "연못 구매 확인" })).not.toBeInTheDocument();
    expect(createRequestId).toHaveBeenCalledTimes(1);
    expect(quote).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledTimes(1);
  });

  it("uses a retry's changed quote for a new explicit confirmation without auto-submitting", async () => {
    const state = shopState();
    const initialQuote: ShopQuote = {
      target: { kind: "purchase", sku: "land_pond" },
      catalog_revision: 1,
      effect_revision: 1,
      price: 101,
    };
    const revisedQuote: ShopQuote = { ...initialQuote, effect_revision: 2, price: 115 };
    const request: ShopRequest = { kind: "purchase", request_id: "first-id", quote: initialQuote };
    const pending = { request, status: "uncertain" as const, error: "응답 대기 중" };
    const quote = vi.fn(async () => initialQuote);
    const apply = vi.fn(async (_request: ShopRequest): Promise<ShopActionResult> => { throw new Error("응답 대기 중"); });
    const retryPending = vi.fn(async (): Promise<ShopActionResult> => ({
      status: "quote_changed", request_id: "first-id", confirmed_quote: revisedQuote, state,
    }));
    const ids = vi.fn().mockReturnValueOnce("first-id").mockReturnValueOnce("second-id");
    const props = {
      state,
      quote,
      apply,
      retryPending,
      onPreviewChange: vi.fn(),
      onSelectLandscapeInstance: vi.fn(),
      createRequestId: ids,
    };
    const view = render(<ShopPanel {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "연못 구매" }));
    await screen.findByRole("dialog", { name: "연못 구매 확인" });
    fireEvent.click(screen.getByRole("button", { name: "구매 확정" }));
    await screen.findByText("요청 결과를 확인하지 못했습니다. 같은 요청으로 다시 시도해 주세요.");
    view.rerender(<ShopPanel {...props} pending={pending} />);

    fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 시도" }));
    await waitFor(() => expect(retryPending).toHaveBeenCalledTimes(1));
    view.rerender(<ShopPanel {...props} pending={null} />);
    expect(screen.getByText("115 토큰", { selector: "strong" })).toBeInTheDocument();
    expect(apply).toHaveBeenCalledTimes(1);
    expect(ids).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "구매 확정" }));
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(2));
    expect(apply.mock.calls[1][0]).toMatchObject({ request_id: "second-id", quote: revisedQuote });
    expect(ids).toHaveBeenCalledTimes(2);
  });

  it("caps avatar estimates at a 15 percent discount for a 500 million token product", () => {
    const state = shopState({
      products: catalog().map((product) => product.sku === "avatar_crown"
        ? { ...product, price: 500_000_000 }
        : product.sku === "avatar_halo"
          ? { ...product, price: 500_000_001 }
          : product),
      effects: {
        token_earning_bps: 0,
        civilization_growth_bps: 0,
        shop_discount_bps: 2500,
        reset_cooldown_bps: 0,
        natural_removal_discount_bps: 0,
        era_reward_tokens: 0,
        streak_reward_tokens: 0,
      },
    });
    mountPanel(state);

    fireEvent.click(screen.getByRole("tab", { name: "아바타" }));
    expect(screen.getByRole("button", { name: "왕관 구매" })).toHaveTextContent("425M 토큰");
    expect(screen.getByRole("button", { name: "홀로그램 관 구매" })).toHaveTextContent("425M 토큰");
    expect(screen.getByText(/상점 할인 15%/)).toBeInTheDocument();
  });

  it("uses each canonical equipment slot version when unequipping all four avatar slots", async () => {
    const state = shopState({
      avatar_owned_skus: ["avatar_crown", "avatar_workwear", "avatar_glasses", "avatar_backpack"],
      avatar_equipment: {
        head: { sku: "avatar_crown", version: 31 },
        outfit: { sku: "avatar_workwear", version: 32 },
        face: { sku: "avatar_glasses", version: 33 },
        back: { sku: "avatar_backpack", version: 34 },
      },
    });
    const apply = vi.fn(async (request: ShopRequest): Promise<ShopActionResult> => ({
      status: "unequipped", request_id: request.request_id, confirmed_quote: null, state,
    }));
    const ids = vi.fn()
      .mockReturnValueOnce("head-id")
      .mockReturnValueOnce("outfit-id")
      .mockReturnValueOnce("face-id")
      .mockReturnValueOnce("back-id");
    const { view } = mountPanel(state, { apply, createRequestId: ids });
    fireEvent.click(screen.getByRole("tab", { name: "아바타" }));
    fireEvent.click(screen.getByRole("tab", { name: "보유함" }));

    const expected = [
      ["avatar_crown", "head", 31],
      ["avatar_workwear", "outfit", 32],
      ["avatar_glasses", "face", 33],
      ["avatar_backpack", "back", 34],
    ] as const;
    for (let index = 0; index < expected.length; index += 1) {
      const [sku] = expected[index];
      const row = view.container.querySelector(`[data-shop-avatar-sku="${sku}"]`);
      const button = row?.querySelector("button.cosmetic-primary");
      expect(button).toHaveTextContent("장착 해제");
      fireEvent.click(button!);
      await waitFor(() => expect(apply).toHaveBeenCalledTimes(index + 1));
      if (index < expected.length - 1) {
        await waitFor(() => expect(view.container.querySelector("button.cosmetic-primary:disabled")).toBeNull());
      }
    }
    expect(apply.mock.calls.map(([request]) => request)).toMatchObject(expected.map(([, slot, version], index) => ({
      kind: "equip_avatar",
      request_id: ["head-id", "outfit-id", "face-id", "back-id"][index],
      sku: null,
      slot,
      expected_version: version,
    })));
  });

  it("selects an individual stored or placed landscape instance for future placement", () => {
    const state = shopState({
      landscape_instances: [
        { instance_id: "pond-stored", sku: "land_pond", variation_index: 2, seed: "pond-seed-a", variation_version: 1, placement_version: 4 },
        { instance_id: "tree-placed", sku: "land_tree", variation_index: 4, seed: "tree-seed-b", variation_version: 1, placement_version: 9 },
      ],
      placements: [{ instance_id: "tree-placed", cycle_id: "cycle-1", x: 3, y: 4, version: 9 }],
    });
    const { view, onSelectLandscapeInstance } = mountPanel(state);
    fireEvent.click(screen.getByRole("tab", { name: "보유함" }));

    expect(view.container.querySelector("[data-shop-instance='pond-stored']")).toHaveTextContent("변형 3 · 보관함");
    expect(view.container.querySelector("[data-shop-instance='tree-placed']")).toHaveTextContent("변형 5 · 배치됨");
    fireEvent.click(view.container.querySelector("[data-shop-instance='pond-stored'] button.cosmetic-primary")!);
    expect(onSelectLandscapeInstance).toHaveBeenCalledWith("pond-stored");
  });

  it("clears avatar preview after canonical equipment changes without changing confirmed effects", async () => {
    const state = shopState({
      effects: {
        token_earning_bps: 100,
        civilization_growth_bps: 200,
        shop_discount_bps: 1250,
        reset_cooldown_bps: 300,
        natural_removal_discount_bps: 400,
        era_reward_tokens: 500,
        streak_reward_tokens: 50,
      },
    });
    const props = {
      state,
      quote: vi.fn(async (target: ShopQuoteTarget) => ({ target, catalog_revision: 1, effect_revision: 1, price: 1 })),
      apply: vi.fn(async () => null),
      retryPending: vi.fn(async () => null),
      onPreviewChange: vi.fn(),
      onSelectLandscapeInstance: vi.fn(),
    };
    const view = render(<ShopPanel {...props} />);
    const effectSummary = view.container.querySelector(".cosmetic-footer")?.textContent;
    fireEvent.click(screen.getByRole("tab", { name: "아바타" }));
    fireEvent.click(screen.getByRole("button", { name: "안경 미리보기" }));
    expect(props.onPreviewChange.mock.calls[props.onPreviewChange.mock.calls.length - 1]?.[0]).toMatchObject({ kind: "avatar" });

    const changedState = {
      ...state,
      avatar_equipment: { ...state.avatar_equipment, head: { sku: "avatar_crown", version: 1 } },
    };
    view.rerender(<ShopPanel {...props} state={changedState} />);
    await waitFor(() => expect(props.onPreviewChange).toHaveBeenLastCalledWith(null));
    expect(view.container.querySelector(".cosmetic-footer")?.textContent).toBe(effectSummary);
    expect(view.container.querySelector(".cosmetic-footer")).toHaveTextContent("상점 할인 12.5%");
  });

  it("ignores a quote response from the previous account context", async () => {
    let resolveQuote!: (value: ShopQuote) => void;
    const quote = vi.fn(() => new Promise<ShopQuote>((resolve) => { resolveQuote = resolve; }));
    const state = shopState();
    const props = {
      state,
      quote,
      apply: vi.fn(async () => null),
      retryPending: vi.fn(async () => null),
      onPreviewChange: vi.fn(),
      onSelectLandscapeInstance: vi.fn(),
      createRequestId: vi.fn(() => "unused"),
    };
    const view = render(<ShopPanel {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "연못 구매" }));
    expect(quote).toHaveBeenCalledTimes(1);

    view.rerender(<ShopPanel {...props} state={{ ...state, account_id: "account-2" }} />);
    await act(async () => {
      resolveQuote({ target: { kind: "purchase", sku: "land_pond" }, catalog_revision: 1, effect_revision: 1, price: 101 });
    });

    expect(screen.queryByRole("dialog", { name: "연못 구매 확인" })).not.toBeInTheDocument();
  });

  it("clears a selected preview when the panel closes or canonical cycle changes", async () => {
    const state = shopState();
    const onPreviewChange = vi.fn();
    const onClose = vi.fn();
    const props = {
      state,
      quote: vi.fn(async (target: ShopQuoteTarget) => ({ target, catalog_revision: 1, effect_revision: 1, price: 101 })),
      apply: vi.fn(async () => null),
      retryPending: vi.fn(async () => null),
      onPreviewChange,
      onSelectLandscapeInstance: vi.fn(),
      onClose,
    };
    const view = render(<ShopPanel {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "연못 미리보기" }));
    expect(onPreviewChange.mock.calls[onPreviewChange.mock.calls.length - 1]?.[0]).toMatchObject({ kind: "landscape", product: { sku: "land_pond" } });

    view.rerender(<ShopPanel {...props} state={{ ...state, current_cycle_id: "cycle-2" }} />);
    await waitFor(() => expect(onPreviewChange).toHaveBeenLastCalledWith(null));
    fireEvent.click(screen.getByRole("button", { name: "연못 미리보기" }));
    fireEvent.click(screen.getByRole("button", { name: "상점 닫기" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onPreviewChange).toHaveBeenLastCalledWith(null);
  });

  it("previews avatar equipment as a cloned cosmetic-only loadout and clears it on category change", () => {
    const state = shopState({
      avatar_equipment: {
        head: { sku: "avatar_crown", version: 3 },
        outfit: { sku: "avatar_workwear", version: 4 },
        face: { sku: null, version: 5 },
        back: { sku: null, version: 6 },
      },
    });
    const { onPreviewChange } = mountPanel(state);

    fireEvent.click(screen.getByRole("tab", { name: "아바타" }));
    fireEvent.click(screen.getByRole("button", { name: "안경 미리보기" }));
    const previewCalls = onPreviewChange.mock.calls as unknown as Array<[ShopPanelPreview | null]>;
    const preview = previewCalls.find(([value]) => value !== null)?.[0] as ShopPanelPreview;
    expect(preview.kind).toBe("avatar");
    if (preview.kind === "avatar") {
      expect(preview.equipment).toEqual({
        head: { sku: "avatar_crown", version: 3 },
        outfit: { sku: "avatar_workwear", version: 4 },
        face: { sku: "avatar_glasses", version: 5 },
        back: { sku: null, version: 6 },
      });
      expect(preview.equipment).not.toBe(state.avatar_equipment);
    }

    fireEvent.click(screen.getByRole("tab", { name: "조경" }));
    expect(onPreviewChange).toHaveBeenLastCalledWith(null);
  });

  it("uses canonical avatar slot versions for equip requests and shows confirmed effects", () => {
    const state = shopState({
      avatar_owned_skus: ["avatar_crown"],
      avatar_equipment: {
        head: { sku: null, version: 7 },
        outfit: { sku: null, version: 0 },
        face: { sku: null, version: 0 },
        back: { sku: null, version: 0 },
      },
      effects: {
        token_earning_bps: 250,
        civilization_growth_bps: 300,
        shop_discount_bps: 2500,
        reset_cooldown_bps: 100,
        natural_removal_discount_bps: 500,
        era_reward_tokens: 12,
        streak_reward_tokens: 3,
      },
    });
    const { apply } = mountPanel(state);

    expect(screen.getByText(/상점 할인 15%/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "연못 구매" })).toHaveTextContent("86 토큰");
    fireEvent.click(screen.getByRole("tab", { name: "아바타" }));
    fireEvent.click(screen.getByRole("tab", { name: "보유함" }));
    expect(screen.getByText("왕관")).toBeInTheDocument();
    expect(screen.getByText("외형만 변경하며 게임 효과는 없습니다")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "왕관 장착" }));

    expect(apply).toHaveBeenCalledWith({
      kind: "equip_avatar",
      request_id: "00000000-0000-4000-8000-000000000001",
      slot: "head",
      sku: "avatar_crown",
      expected_version: 7,
    });
  });

  it("keeps an uncertain purchase retry on its immutable request id", () => {
    const request: ShopRequest = {
      kind: "purchase",
      request_id: "00000000-0000-4000-8000-000000000009",
      quote: {
        target: { kind: "purchase", sku: "land_pond" },
        catalog_revision: 1,
        effect_revision: 1,
        price: 101,
      },
    };
    const pending = { request, status: "uncertain" as const, error: "응답 대기 중" };
    const retryPending = vi.fn(async () => null);
    const createRequestId = vi.fn(() => "unexpected-new-id");
    mountPanel(shopState(), { pending, retryPending, createRequestId });

    fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 시도" }));
    expect(retryPending).toHaveBeenCalledTimes(1);
    expect(createRequestId).not.toHaveBeenCalled();
  });
});
