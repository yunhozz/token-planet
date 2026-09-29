import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CosmeticShop } from "../CosmeticShop";
import { legacyEquivalent, styleIdForSku, SUPPORTED_SKUS, SUPPORTED_SLOTS } from "../cosmeticStyles";
import type { CosmeticShopState } from "../../types/usage";

const shopState: CosmeticShopState = {
  slots: [
    { slot_id: "sky", display_name: "하늘" },
    { slot_id: "ring", display_name: "고리" },
    { slot_id: "surface", display_name: "지표" },
    { slot_id: "forecourt", display_name: "앞마당" },
  ],
  products: [
    { sku: "star_cluster", slot_id: "sky", display_name: "별무리", price: 100_000, catalog_revision: 1, purchasable: false },
    { sku: "aurora", slot_id: "sky", display_name: "오로라", price: 500_000, catalog_revision: 1, purchasable: false },
    { sku: "thin_ring", slot_id: "ring", display_name: "얇은 고리", price: 100_000, catalog_revision: 1, purchasable: false },
    { sku: "double_ring", slot_id: "ring", display_name: "이중 고리", price: 500_000, catalog_revision: 1, purchasable: false },
    { sku: "flag", slot_id: "surface", display_name: "깃발", price: 100_000, catalog_revision: 1, purchasable: false },
    { sku: "crystal_tower", slot_id: "surface", display_name: "수정탑", price: 500_000, catalog_revision: 1, purchasable: false },
    { sku: "star_cluster_v2", slot_id: "sky", display_name: "별무리", price: 500_000, catalog_revision: 1, purchasable: true },
    { sku: "aurora_v2", slot_id: "sky", display_name: "오로라", price: 2_000_000, catalog_revision: 1, purchasable: true },
    { sku: "thin_ring_v2", slot_id: "ring", display_name: "얇은 고리", price: 500_000, catalog_revision: 1, purchasable: true },
    { sku: "double_ring_v2", slot_id: "ring", display_name: "이중 고리", price: 2_000_000, catalog_revision: 1, purchasable: true },
    { sku: "flag_v2", slot_id: "surface", display_name: "깃발", price: 500_000, catalog_revision: 1, purchasable: true },
    { sku: "crystal_tower_v2", slot_id: "surface", display_name: "수정탑", price: 2_000_000, catalog_revision: 1, purchasable: true },
    { sku: "meteor_shower", slot_id: "sky", display_name: "유성우", price: 1_000_000, catalog_revision: 1, purchasable: true },
    { sku: "moonlets", slot_id: "ring", display_name: "작은 위성들", price: 3_000_000, catalog_revision: 1, purchasable: true },
    { sku: "flower_garden", slot_id: "surface", display_name: "꽃 정원", price: 1_000_000, catalog_revision: 1, purchasable: true },
    { sku: "observatory", slot_id: "surface", display_name: "천문대", price: 5_000_000, catalog_revision: 1, purchasable: true },
    { sku: "pond", slot_id: "forecourt", display_name: "연못", price: 750_000, catalog_revision: 1, purchasable: true },
    { sku: "lantern", slot_id: "forecourt", display_name: "등불", price: 1_500_000, catalog_revision: 1, purchasable: true },
    { sku: "rover", slot_id: "forecourt", display_name: "탐사 로버", price: 3_000_000, catalog_revision: 1, purchasable: true },
    { sku: "greenhouse", slot_id: "forecourt", display_name: "온실", price: 5_000_000, catalog_revision: 1, purchasable: true },
  ],
  current_cycle_id: "cycle-1",
  available_balance: 600_000,
  owned_skus: [],
  equipped: [],
  slot_versions: { sky: 0, ring: 0, surface: 0, forecourt: 0 },
  actions_require_online: false,
  action_unavailable_reason: null,
  guest_import_pending: false,
  guest_import_error: null,
};

function mountShop(overrides: Partial<CosmeticShopState> = {}, support?: { slots: string[]; skus: string[] }) {
  const onPurchase = vi.fn().mockResolvedValue({ result: null, state: shopState, unavailable_reason: null });
  const onEquip = vi.fn().mockResolvedValue({ result: null, state: shopState, unavailable_reason: null });
  const onPreviewChange = vi.fn();
  const onRefresh = vi.fn().mockResolvedValue({ ...shopState, ...overrides });
  render(
    <CosmeticShop
      state={{ ...shopState, ...overrides }}
      onPurchase={onPurchase}
      onEquip={onEquip}
      onPreviewChange={onPreviewChange}
      onRefresh={onRefresh}
      supportedSlots={support?.slots}
      supportedSkus={support?.skus}
    />,
  );
  return { onPurchase, onEquip, onPreviewChange, onRefresh };
}

describe("CosmeticShop", () => {
  it("lists supported catalog items and previews without equipping", () => {
    const { onEquip, onPurchase, onPreviewChange } = mountShop();
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));

    expect(screen.getByRole("heading", { name: "하늘" })).toBeInTheDocument();
    expect(screen.getByText("별무리")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "별무리 미리보기" }));

    expect(onPreviewChange).toHaveBeenCalledWith([
      { slot_id: "sky", sku: "star_cluster_v2", version: 0 },
    ]);
    expect(onEquip).not.toHaveBeenCalled();
    expect(onPurchase).not.toHaveBeenCalled();
  });

  it("refreshes canonical state when the shop opens", async () => {
    const { onRefresh, onPreviewChange } = mountShop();
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));

    await waitFor(() => expect(onRefresh).toHaveBeenCalledOnce());
    expect(onPreviewChange).toHaveBeenLastCalledWith(null);
  });

  it("confirms a purchase with its price and resulting balance", () => {
    const { onPurchase } = mountShop({ available_balance: 6_000_000 });
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));
    fireEvent.click(screen.getByRole("button", { name: "천문대 구매" }));

    const dialog = screen.getByRole("dialog", { name: /구매 확인/ });
    expect(within(dialog).getByText("5,000,000 토큰 차감")).toBeInTheDocument();
    expect(within(dialog).getByText("구매 후 잔액 1,000,000 토큰")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "구매 확인" }));
    expect(onPurchase).toHaveBeenCalledWith("observatory");
  });

  it("shows all four slots and exactly fourteen sale rows for a new user", () => {
    mountShop();
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));

    expect(screen.getByRole("heading", { name: "앞마당" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /구매$/ })).toHaveLength(14);
    expect(screen.getAllByRole("button", { name: "별무리 구매" })).toHaveLength(1);
  });

  it("shows one legacy owned appearance and hides its higher-priced replacement sale row", () => {
    mountShop({ owned_skus: ["star_cluster"] });
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));

    expect(screen.queryByRole("button", { name: "별무리 구매" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: /보관함/ }));
    expect(screen.getAllByText("별무리")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "별무리 장착" })).toBeInTheDocument();
  });

  it("maps replacement SKUs to their original art and lists every new renderer SKU", () => {
    expect(styleIdForSku("star_cluster_v2")).toBe("star_cluster");
    expect(styleIdForSku("pond")).toBe("pond");
    expect(legacyEquivalent("double_ring_v2")).toBe("double_ring");
    expect(legacyEquivalent("meteor_shower")).toBeNull();
    expect(SUPPORTED_SLOTS).toEqual(["sky", "ring", "surface", "forecourt"]);
    expect(SUPPORTED_SKUS).toEqual([
      "star_cluster", "aurora", "thin_ring", "double_ring", "flag", "crystal_tower",
      "star_cluster_v2", "aurora_v2", "thin_ring_v2", "double_ring_v2", "flag_v2", "crystal_tower_v2",
      "meteor_shower", "moonlets", "flower_garden", "observatory", "pond", "lantern", "rover", "greenhouse",
    ]);
  });

  it("previews each of the eight newly drawn products without equipping it", () => {
    const { onPreviewChange, onEquip } = mountShop();
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));

    for (const [name, sku, slotId] of [
      ["유성우", "meteor_shower", "sky"], ["작은 위성들", "moonlets", "ring"],
      ["꽃 정원", "flower_garden", "surface"], ["천문대", "observatory", "surface"],
      ["연못", "pond", "forecourt"], ["등불", "lantern", "forecourt"],
      ["탐사 로버", "rover", "forecourt"], ["온실", "greenhouse", "forecourt"],
    ]) {
      fireEvent.click(screen.getByRole("button", { name: `${name} 미리보기` }));
      expect(onPreviewChange).toHaveBeenLastCalledWith(expect.arrayContaining([
        expect.objectContaining({ slot_id: slotId, sku }),
      ]));
    }
    expect(onEquip).not.toHaveBeenCalled();
  });

  it("equips owned inventory items and offers free unequip", async () => {
    const { onEquip } = mountShop({
      owned_skus: ["star_cluster", "aurora"],
      equipped: [{ slot_id: "sky", sku: "star_cluster", version: 2 }],
      slot_versions: { sky: 2, ring: 0, surface: 0 },
    });
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));
    fireEvent.click(screen.getByRole("tab", { name: /보관함/ }));

    await userEvent.click(screen.getByRole("button", { name: "별무리 해제" }));
    await userEvent.click(screen.getByRole("button", { name: "오로라 장착" }));
    expect(onEquip).toHaveBeenNthCalledWith(1, "sky", null, "cycle-1", 2);
    expect(onEquip).toHaveBeenNthCalledWith(2, "sky", "aurora", "cycle-1", 2);
  });

  it("sends the displayed cycle and slot version with an equip request", async () => {
    const { onEquip } = mountShop({
      owned_skus: ["aurora"],
      slot_versions: { sky: 7, ring: 0, surface: 0 },
    });
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));
    fireEvent.click(screen.getByRole("tab", { name: /보관함/ }));
    await userEvent.click(screen.getByRole("button", { name: "오로라 장착" }));

    expect(onEquip).toHaveBeenCalledWith("sky", "aurora", "cycle-1", 7);
  });

  it("clears the preview overlay when the shop closes", () => {
    const { onPreviewChange } = mountShop();
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));
    fireEvent.click(screen.getByRole("button", { name: "별무리 미리보기" }));
    fireEvent.click(screen.getByRole("button", { name: "상점 닫기" }));

    expect(onPreviewChange).toHaveBeenLastCalledWith(null);
  });

  it("renders added slots and products when the local renderer supports them", () => {
    const slots = [...shopState.slots, { slot_id: "moon", display_name: "달" }];
    const products = [...shopState.products, {
      sku: "moon_bloom",
      slot_id: "moon",
      display_name: "월화",
      price: 100_000,
      catalog_revision: 2,
      purchasable: true,
    }];
    mountShop({ slots, products }, {
      slots: ["sky", "ring", "surface", "moon"],
      skus: ["star_cluster", "aurora", "thin_ring", "double_ring", "flag", "crystal_tower", "moon_bloom"],
    });
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));

    expect(screen.getByRole("heading", { name: "달" })).toBeInTheDocument();
    expect(screen.getByText("월화")).toBeInTheDocument();
  });

  it("preserves but does not render unknown slots or SKUs", () => {
    mountShop({
      slots: [...shopState.slots, { slot_id: "unknown_slot", display_name: "비밀 슬롯" }],
      products: [...shopState.products, {
        sku: "unknown_item",
        slot_id: "unknown_slot",
        display_name: "알 수 없는 장식",
        price: 10,
        catalog_revision: 99,
        purchasable: true,
      }],
      owned_skus: ["unknown_item"],
      equipped: [{ slot_id: "unknown_slot", sku: "unknown_item", version: 1 }],
    });
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));

    expect(screen.queryByText("비밀 슬롯")).not.toBeInTheDocument();
    expect(screen.queryByText("알 수 없는 장식")).not.toBeInTheDocument();
  });

  it("keeps cached inventory and preview available while disabling offline actions", async () => {
    const unavailable = "서버에 연결할 수 없습니다";
    const { onEquip, onPreviewChange, onRefresh } = mountShop({
      owned_skus: ["star_cluster"],
      equipped: [{ slot_id: "sky", sku: "star_cluster", version: 1 }],
      action_unavailable_reason: unavailable,
    });
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));
    fireEvent.click(screen.getByRole("tab", { name: /보관함/ }));

    expect(screen.getByText("별무리")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "별무리 해제" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "별무리 미리보기" }));
    fireEvent.click(screen.getByRole("tab", { name: "상점" }));
    expect(screen.getByRole("button", { name: "오로라 구매" })).toBeDisabled();
    expect(screen.getByText(unavailable)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "상점 다시 확인" }));

    expect(onRefresh).toHaveBeenCalledTimes(2);
    expect(onPreviewChange).toHaveBeenCalled();
    expect(onEquip).not.toHaveBeenCalled();
  });
});
