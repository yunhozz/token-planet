import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CosmeticShop } from "../CosmeticShop";
import type { CosmeticShopState } from "../../types/usage";

const shopState: CosmeticShopState = {
  slots: [
    { slot_id: "sky", display_name: "하늘" },
    { slot_id: "ring", display_name: "고리" },
    { slot_id: "surface", display_name: "지표" },
  ],
  products: [
    { sku: "star_cluster", slot_id: "sky", display_name: "별무리", price: 100_000, catalog_revision: 1, purchasable: true },
    { sku: "aurora", slot_id: "sky", display_name: "오로라", price: 500_000, catalog_revision: 1, purchasable: true },
    { sku: "thin_ring", slot_id: "ring", display_name: "얇은 고리", price: 100_000, catalog_revision: 1, purchasable: true },
    { sku: "double_ring", slot_id: "ring", display_name: "이중 고리", price: 500_000, catalog_revision: 1, purchasable: true },
    { sku: "flag", slot_id: "surface", display_name: "깃발", price: 100_000, catalog_revision: 1, purchasable: true },
    { sku: "crystal_tower", slot_id: "surface", display_name: "수정탑", price: 500_000, catalog_revision: 1, purchasable: true },
  ],
  current_cycle_id: "cycle-1",
  available_balance: 600_000,
  owned_skus: [],
  equipped: [],
  slot_versions: { sky: 0, ring: 0, surface: 0 },
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
      { slot_id: "sky", sku: "star_cluster", version: 0 },
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
    const { onPurchase } = mountShop();
    fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));
    fireEvent.click(screen.getByRole("button", { name: "별무리 구매" }));

    const dialog = screen.getByRole("dialog", { name: /구매 확인/ });
    expect(within(dialog).getByText("100,000 토큰 차감")).toBeInTheDocument();
    expect(within(dialog).getByText("구매 후 잔액 500,000 토큰")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "구매 확인" }));
    expect(onPurchase).toHaveBeenCalledWith("star_cluster");
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
