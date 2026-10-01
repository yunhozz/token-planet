import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { NaturalRemovalDialog } from "../NaturalRemovalDialog";
import type { NaturalObjectKey, ShopQuote } from "../../types/usage";

const target: NaturalObjectKey = { cycle_id: "cycle-8", stage: 2, ordinal: 3 };

function removalQuote(key: NaturalObjectKey = target, price = 375_000): ShopQuote {
  return {
    target: { kind: "remove_natural", key },
    catalog_revision: 4,
    effect_revision: 9,
    price,
  };
}

function renderDialog(overrides: Partial<ComponentProps<typeof NaturalRemovalDialog>> = {}) {
  const onConfirm = vi.fn();
  const onCancel = vi.fn();
  const props: ComponentProps<typeof NaturalRemovalDialog> = {
    target,
    quote: removalQuote(),
    pending: false,
    confirmedWalletBalance: 500_000,
    objectName: "작은 분수",
    onConfirm,
    onCancel,
    ...overrides,
  };

  const view = render(<NaturalRemovalDialog {...props} />);
  return { ...view, onConfirm, onCancel, props };
}

describe("NaturalRemovalDialog", () => {
  it.each([
    [0, "자연", 100_000],
    [1, "농경", 250_000],
    [2, "도시", 500_000],
    [3, "산업", 1_000_000],
    [4, "우주", 2_000_000],
  ])("shows the fixed base cost for generated stage %i", (stage, era, baseCost) => {
    const stageTarget = { ...target, stage };
    const quote = removalQuote(stageTarget, baseCost);
    const dialog = renderDialog({ target: stageTarget, quote });

    expect(screen.getByRole("dialog", { name: "작은 분수 제거 확인" })).toBeInTheDocument();
    expect(dialog.container).toHaveTextContent(`생성 시대: ${era} · 4번째 개체`);
    expect(dialog.container).toHaveTextContent(`기본 제거 비용 ${baseCost.toLocaleString("ko-KR")} 토큰`);
    expect(dialog.container).toHaveTextContent("현재 할인 0 토큰");
    expect(dialog.container).toHaveTextContent(`최종 제거 비용 ${baseCost.toLocaleString("ko-KR")} 토큰`);
  });

  it("derives the displayed discount from the matching canonical quote", () => {
    const { container } = renderDialog({ quote: removalQuote(target, 375_000) });

    expect(container).toHaveTextContent("기본 제거 비용 500,000 토큰");
    expect(container).toHaveTextContent("현재 할인 125,000 토큰");
    expect(container).toHaveTextContent("최종 제거 비용 375,000 토큰");
  });

  it("renders a refreshed quote without confirming it automatically", () => {
    const { rerender, onConfirm, props, container } = renderDialog({ quote: removalQuote(target, 400_000) });

    rerender(<NaturalRemovalDialog {...props} quote={removalQuote(target, 350_000)} />);

    expect(container).toHaveTextContent("현재 할인 150,000 토큰");
    expect(container).toHaveTextContent("최종 제거 비용 350,000 토큰");
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("keeps confirmation disabled while a quote is unavailable", () => {
    renderDialog({ quote: null });

    expect(screen.getByRole("button", { name: "제거 확인" })).toBeDisabled();
  });

  it.each([
    ["purchase quote", { kind: "purchase", sku: "pond" }],
    ["different cycle", { kind: "remove_natural", key: { ...target, cycle_id: "cycle-9" } }],
    ["different stage", { kind: "remove_natural", key: { ...target, stage: 1 } }],
    ["different ordinal", { kind: "remove_natural", key: { ...target, ordinal: 4 } }],
  ] as const)("disables confirmation for a quote with a %s target", (_label, quoteTarget) => {
    const quote: ShopQuote = { target: quoteTarget, catalog_revision: 4, effect_revision: 9, price: 200_000 };

    renderDialog({ quote });

    expect(screen.getByRole("button", { name: "제거 확인" })).toBeDisabled();
  });

  it.each([-1, 250_000.5, 500_001])("disables confirmation for invalid quoted price %s", (price) => {
    renderDialog({ quote: removalQuote(target, price) });

    expect(screen.getByRole("button", { name: "제거 확인" })).toBeDisabled();
  });

  it.each([
    [0, 100_000, 70_000],
    [1, 250_000, 175_000],
    [2, 500_000, 350_000],
    [3, 1_000_000, 700_000],
    [4, 2_000_000, 1_400_000],
  ])("enforces the 30 percent discount floor for generated stage %i", (stage, _baseCost, minimumPrice) => {
    const stageTarget = { ...target, stage };
    const { rerender, props } = renderDialog({
      target: stageTarget,
      quote: removalQuote(stageTarget, minimumPrice - 1),
      confirmedWalletBalance: minimumPrice,
    });

    expect(screen.getByRole("button", { name: "제거 확인" })).toBeDisabled();

    rerender(<NaturalRemovalDialog {...props} quote={removalQuote(stageTarget, minimumPrice)} />);

    expect(screen.getByRole("button", { name: "제거 확인" })).toBeEnabled();
  });

  it("disables confirmation when the confirmed wallet balance is below the quote", () => {
    renderDialog({ quote: removalQuote(target, 375_000), confirmedWalletBalance: 374_999 });

    expect(screen.getByRole("button", { name: "제거 확인" })).toBeDisabled();
  });

  it("enables confirmation when the confirmed balance exactly covers the quote", () => {
    renderDialog({ quote: removalQuote(target, 375_000), confirmedWalletBalance: 375_000 });

    expect(screen.getByRole("button", { name: "제거 확인" })).toBeEnabled();
  });

  it("blocks confirmation, cancel, and Escape while pending", () => {
    const { onConfirm, onCancel } = renderDialog({ pending: true });
    const dialog = screen.getByRole("dialog", { name: "작은 분수 제거 확인" });
    const confirm = within(dialog).getByRole("button", { name: "제거 중" });
    const cancel = within(dialog).getByRole("button", { name: "취소" });
    const escape = new Event("cancel", { cancelable: true });

    expect(confirm).toBeDisabled();
    expect(cancel).toBeDisabled();
    fireEvent.click(confirm);
    fireEvent.click(cancel);
    dialog.dispatchEvent(escape);

    expect(escape.defaultPrevented).toBe(true);
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("confirms one valid matching removal request", () => {
    const { onConfirm } = renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "제거 확인" }));

    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("cancels without invoking confirmation", () => {
    const { onConfirm, onCancel } = renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "취소" }));

    expect(onCancel).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("lets Escape cancel the dialog when not pending", () => {
    const { onCancel } = renderDialog();
    const dialog = screen.getByRole("dialog", { name: "작은 분수 제거 확인" });
    const escape = new Event("cancel", { cancelable: true });

    dialog.dispatchEvent(escape);

    expect(escape.defaultPrevented).toBe(true);
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("opens as a named native modal and places focus on cancel", () => {
    renderDialog();
    const dialog = screen.getByRole("dialog", { name: "작은 분수 제거 확인" });

    expect(dialog).toHaveAttribute("open");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(within(dialog).getByRole("button", { name: "취소" })).toHaveFocus();
  });

  it("shows the optional error and uses a fallback name", () => {
    renderDialog({ objectName: undefined, error: "삭제를 저장하지 못했습니다" });

    expect(screen.getByRole("dialog", { name: "자연 오브젝트 제거 확인" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("삭제를 저장하지 못했습니다");
  });
});
