import { useEffect, useId, useRef } from "react";
import { FormattedTokens } from "./FormattedTokens";
import type { NaturalObjectKey, ShopQuote } from "../types/usage";

type NaturalRemovalDialogProps = {
  target: NaturalObjectKey;
  quote: ShopQuote | null;
  pending: boolean;
  allowUnaffordableRetry?: boolean;
  confirmedWalletBalance: number;
  onConfirm: () => void;
  onCancel: () => void;
  objectName?: string;
  error?: string | null;
};

const NATURAL_REMOVAL_BASE_COSTS = [100_000, 250_000, 500_000, 1_000_000, 2_000_000];
const ERA_NAMES = ["자연", "농경", "도시", "산업", "우주"];

function isNaturalObjectKey(key: NaturalObjectKey): boolean {
  return typeof key.cycle_id === "string"
    && key.cycle_id.length > 0
    && Number.isInteger(key.stage)
    && key.stage >= 0
    && key.stage < NATURAL_REMOVAL_BASE_COSTS.length
    && Number.isInteger(key.ordinal)
    && key.ordinal >= 0;
}

export function NaturalRemovalDialog({
  target,
  quote,
  pending,
  allowUnaffordableRetry = false,
  confirmedWalletBalance,
  onConfirm,
  onCancel,
  objectName,
  error,
}: NaturalRemovalDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const cancelButtonRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const itemName = objectName?.trim() || "자연 오브젝트";
  const validTarget = isNaturalObjectKey(target);
  const baseCost = validTarget ? NATURAL_REMOVAL_BASE_COSTS[target.stage] ?? null : null;
  const matchingQuote = quote?.target.kind === "remove_natural"
    && quote.target.key.cycle_id === target.cycle_id
    && quote.target.key.stage === target.stage
    && quote.target.key.ordinal === target.ordinal;
  const finalPrice = validTarget
    && matchingQuote
    && baseCost !== null
    && quote !== null
    && Number.isInteger(quote.price)
    && quote.price >= 0
    && quote.price >= Math.ceil((baseCost * 7_000) / 10_000)
    && quote.price <= baseCost
    ? quote.price
    : null;
  const discount = baseCost !== null && finalPrice !== null ? baseCost - finalPrice : null;
  const hasConfirmedBalance = Number.isFinite(confirmedWalletBalance);
  const canAfford = finalPrice !== null && hasConfirmedBalance && confirmedWalletBalance >= finalPrice;
  const canConfirm = finalPrice !== null && (canAfford || allowUnaffordableRetry) && !pending;
  const eraName = validTarget ? ERA_NAMES[target.stage] : null;

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    if (typeof dialog.showModal === "function") {
      try {
        dialog.showModal();
      } catch {
        dialog.setAttribute("open", "");
      }
    } else {
      dialog.setAttribute("open", "");
    }
    cancelButtonRef.current?.focus();

    return () => {
      if (dialog.open && typeof dialog.close === "function") dialog.close();
      else dialog.removeAttribute("open");
    };
  }, []);

  return (
    <dialog
      ref={dialogRef}
      className="cosmetic-confirmation"
      aria-labelledby={titleId}
      aria-modal="true"
      onCancel={(event) => {
        event.preventDefault();
        if (!pending) onCancel();
      }}
      onClick={(event) => {
        if (event.target === event.currentTarget && !pending) onCancel();
      }}
    >
      <p className="cosmetic-shop-kicker">
        생성 시대: {eraName ?? "확인 필요"}
      </p>
      <h3 id={titleId}>{itemName} 제거 확인</h3>
      {baseCost !== null
        ? <p>기본 제거 비용 <FormattedTokens value={baseCost} /> 토큰</p>
        : <p role="status">생성 시대를 확인할 수 없습니다.</p>}
      {discount !== null && finalPrice !== null
        ? <>
            <p>현재 할인 <FormattedTokens value={discount} /> 토큰</p>
            <p>최종 제거 비용 <FormattedTokens value={finalPrice} /> 토큰</p>
          </>
        : <p role="status">최신 제거 견적을 확인해야 합니다.</p>}
      {hasConfirmedBalance
        ? <p>현재 잔액 <FormattedTokens value={confirmedWalletBalance} /> 토큰</p>
        : <p role="status">현재 잔액을 확인할 수 없습니다.</p>}
      {finalPrice !== null && hasConfirmedBalance && !canAfford && (
        <p className="cosmetic-shortfall" role="status">잔액이 부족합니다. 현재 잔액을 확인하세요.</p>
      )}
      {error && <p className="cosmetic-unavailable" role="alert">{error}</p>}
      <div className="cosmetic-confirm-actions">
        <button
          ref={cancelButtonRef}
          type="button"
          className="cosmetic-secondary"
          disabled={pending}
          onClick={onCancel}
        >취소</button>
        <button
          type="button"
          className="cosmetic-primary"
          disabled={!canConfirm}
          onClick={() => {
            if (canConfirm) onConfirm();
          }}
        >{pending ? "제거 중" : "제거 확인"}</button>
      </div>
    </dialog>
  );
}
