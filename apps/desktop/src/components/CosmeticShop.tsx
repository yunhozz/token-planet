import { useEffect, useMemo, useRef, useState } from "react";
import { FormattedNumber } from "./FormattedNumber";
import { legacyEquivalent, SUPPORTED_SKUS, SUPPORTED_SLOTS } from "./cosmeticStyles";
import type {
  CosmeticEquipAction,
  CosmeticProduct,
  CosmeticPurchaseAction,
  CosmeticShopState,
  EquippedCosmetic,
} from "../types/usage";

type CosmeticShopProps = {
  state: CosmeticShopState;
  onPurchase: (sku: string) => Promise<CosmeticPurchaseAction>;
  onEquip: (slotId: string, sku: string | null, cycleId: string, expectedVersion: number) => Promise<CosmeticEquipAction>;
  onPreviewChange: (equipped: EquippedCosmetic[] | null) => void;
  onPreviewSelectionChange?: (product: CosmeticProduct | null) => void;
  onRefresh: () => Promise<CosmeticShopState>;
  initiallyOpen?: boolean;
  actionsDisabled?: boolean;
  showCollapseButton?: boolean;
  supportedSlots?: string[];
  supportedSkus?: string[];
  initialTab?: "shop" | "inventory";
  initialFocusSku?: string | null;
  previewResetRevision?: number;
};

function formatTokens(value: number) {
  return value.toLocaleString("ko-KR");
}

function purchaseMessage(action: CosmeticPurchaseAction, product: CosmeticProduct) {
  if (action.unavailable_reason) return action.unavailable_reason;
  switch (action.result?.status) {
    case "purchased": return `${product.display_name} 구매 완료 · 잔액 ${formatTokens(action.result.available_balance)} 토큰`;
    case "already_owned": return "이미 보유한 장식입니다. 잔액은 차감되지 않았습니다.";
    case "insufficient_balance": return `${formatTokens(action.result.price)} 토큰이 필요합니다. 현재 잔액은 ${formatTokens(action.result.available_balance)} 토큰입니다.`;
    case "catalog_mismatch": return "상품 정보가 바뀌었습니다. 상점 상태를 새로 불러오세요.";
    case "request_conflict": return "구매 요청을 확인할 수 없습니다. 상점 상태를 새로 불러오세요.";
    default: return "구매 결과를 확인할 수 없습니다. 같은 요청으로 다시 시도하세요.";
  }
}

function equipMessage(action: CosmeticEquipAction) {
  if (action.unavailable_reason) return action.unavailable_reason;
  switch (action.result?.status) {
    case "equipped": return "행성에 장식을 장착했습니다.";
    case "unequipped": return "장식을 해제했습니다.";
    case "version_conflict": return "다른 기기에서 장착이 바뀌었습니다. 최신 상태를 확인하세요.";
    case "cycle_mismatch": return "행성이 바뀌었습니다. 현재 행성의 장착 상태를 확인하세요.";
    case "not_owned": return "보유한 장식만 장착할 수 있습니다.";
    case "catalog_mismatch": return "이 장식은 현재 앱에서 사용할 수 없습니다.";
    default: return "장착 결과를 확인할 수 없습니다.";
  }
}

export function CosmeticShop({
  state,
  onPurchase,
  onEquip,
  onPreviewChange,
  onPreviewSelectionChange,
  onRefresh,
  initiallyOpen = false,
  actionsDisabled = false,
  showCollapseButton = true,
  supportedSlots = SUPPORTED_SLOTS,
  supportedSkus = SUPPORTED_SKUS,
  initialTab = "shop",
  initialFocusSku = null,
  previewResetRevision = 0,
}: CosmeticShopProps) {
  const [open, setOpen] = useState(initiallyOpen);
  const [tab, setTab] = useState<"shop" | "inventory">(initialTab);
  const [confirmSku, setConfirmSku] = useState<string | null>(null);
  const [busySku, setBusySku] = useState<string | null>(null);
  const [busySlot, setBusySlot] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [notice, setNotice] = useState("");
  const [previewBySlot, setPreviewBySlot] = useState<Record<string, EquippedCosmetic>>({});
  const dialogRef = useRef<HTMLDialogElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const slots = useMemo(
    () => state.slots.filter((slot) => supportedSlots.includes(slot.slot_id)),
    [state.slots, supportedSlots],
  );
  const products = useMemo(
    () => state.products.filter((product) => supportedSkus.includes(product.sku)
      && supportedSlots.includes(product.slot_id)),
    [state.products, supportedSkus, supportedSlots],
  );
  const owned = new Set(state.owned_skus);
  const saleProducts = products.filter((product) => product.purchasable && !owned.has(product.sku)
    && !(legacyEquivalent(product.sku) && owned.has(legacyEquivalent(product.sku)!)));
  const availableBalance = state.available_balance;
  const slotUnavailableReason = state.action_unavailable_reason
    ?? (state.guest_import_pending ? state.guest_import_error : null);
  const equipmentSignature = JSON.stringify([
    state.current_cycle_id,
    state.equipped.map(({ slot_id, sku, version }) => [slot_id, sku, version]),
    state.slot_versions,
  ]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (confirmSku && !dialog.open) {
      if (typeof dialog.showModal === "function") dialog.showModal();
      else dialog.setAttribute("open", "");
    } else if (!confirmSku && dialog.open) {
      if (typeof dialog.close === "function") dialog.close();
      else dialog.removeAttribute("open");
    }
  }, [confirmSku]);

  useEffect(() => {
    setPreviewBySlot({});
    onPreviewChange(null);
    onPreviewSelectionChange?.(null);
  }, [equipmentSignature]);

  useEffect(() => {
    setPreviewBySlot({});
  }, [previewResetRevision]);

  useEffect(() => {
    if (!initialFocusSku || tab !== "inventory") return;
    const target = listRef.current?.querySelector<HTMLElement>(`[data-cosmetic-sku="${CSS.escape(initialFocusSku)}"]`);
    target?.scrollIntoView?.({ block: "center" });
  }, [initialFocusSku, tab]);

  function preview(product: CosmeticProduct) {
    if (actionsDisabled) return;
    const item = {
      slot_id: product.slot_id,
      sku: product.sku,
      version: state.slot_versions[product.slot_id] ?? 0,
    };
    const next = { ...previewBySlot, [product.slot_id]: item };
    setPreviewBySlot(next);
    onPreviewChange([
      ...state.equipped.filter((equipped) => !(equipped.slot_id in next)),
      ...Object.values(next),
    ]);
    onPreviewSelectionChange?.(product);
  }

  function clearPreview() {
    setPreviewBySlot({});
    onPreviewChange(null);
    onPreviewSelectionChange?.(null);
  }

  async function confirmPurchase(product: CosmeticProduct) {
    if (actionsDisabled) return;
    setBusySku(product.sku);
    setConfirmSku(null);
    setNotice("");
    try {
      const action = await onPurchase(product.sku);
      setNotice(purchaseMessage(action, product));
    } catch (cause) {
      setNotice(typeof cause === "string" ? cause : "구매 결과를 확인할 수 없습니다. 같은 요청으로 다시 시도하세요.");
    } finally {
      setBusySku(null);
    }
  }

  async function equip(slotId: string, sku: string | null) {
    if (actionsDisabled) return;
    setBusySlot(slotId);
    setNotice("");
    try {
      setNotice(equipMessage(await onEquip(
        slotId,
        sku,
        state.current_cycle_id,
        state.slot_versions[slotId] ?? 0,
      )));
    } catch (cause) {
      setNotice(typeof cause === "string" ? cause : "장착을 변경하지 못했습니다.");
    } finally {
      setBusySlot(null);
    }
  }

  async function refreshShop() {
    if (actionsDisabled) return;
    setRefreshing(true);
    try {
      await onRefresh();
      setPreviewBySlot({});
      onPreviewChange(null);
      onPreviewSelectionChange?.(null);
      setNotice("");
    } catch (cause) {
      setNotice(typeof cause === "string" ? cause : "상점 상태를 새로 확인하지 못했습니다.");
    } finally {
      setRefreshing(false);
    }
  }

  if (!open) {
    return <button className="cosmetic-entry" type="button" disabled={actionsDisabled} onClick={() => { setOpen(true); void refreshShop(); }}>행성 꾸미기</button>;
  }

  return (
    <section className="cosmetic-shop" aria-label="행성 꾸미기 상점">
      <div className="cosmetic-shop-heading">
        <div>
          <p className="cosmetic-shop-kicker">외형 장식</p>
          <h2>행성 꾸미기</h2>
        </div>
        {showCollapseButton && <button className="cosmetic-close" type="button" onClick={() => { clearPreview(); setOpen(false); }} aria-label="상점 닫기">닫기</button>}
      </div>

      <div className="cosmetic-balance">
        <span>사용 가능 잔액</span>
        <strong><FormattedNumber value={availableBalance} /> <small>토큰</small></strong>
      </div>

      <div className="cosmetic-tabs" role="tablist" aria-label="장식 보기">
        <button type="button" role="tab" aria-selected={tab === "shop"} onClick={() => setTab("shop")}>상점</button>
        <button type="button" role="tab" aria-selected={tab === "inventory"} onClick={() => setTab("inventory")}>보관함 ({state.owned_skus.length})</button>
      </div>

      <div ref={listRef} className="cosmetic-list" role="tabpanel" aria-label={tab === "shop" ? "상점 상품" : "보유 장식"}>
        {slots.map((slot) => {
          const slotProducts = products.filter((product) => product.slot_id === slot.slot_id);
          const visibleProducts = tab === "shop"
            ? saleProducts.filter((product) => product.slot_id === slot.slot_id)
            : slotProducts.filter((product) => owned.has(product.sku));
          if (visibleProducts.length === 0) return null;
          return (
            <section className="cosmetic-slot" key={slot.slot_id} aria-label={`${slot.display_name} 장식`}>
              <h3>{slot.display_name}</h3>
              <ul>
                {visibleProducts.map((product) => {
                  const equipped = state.equipped.some((item) => item.slot_id === slot.slot_id && item.sku === product.sku);
                  const canBuy = product.purchasable && !owned.has(product.sku)
                    && availableBalance >= product.price && !slotUnavailableReason && !busySku && !actionsDisabled;
                  return (
                    <li className="cosmetic-item" key={product.sku} data-cosmetic-sku={product.sku} data-cosmetic-previewing={previewBySlot[slot.slot_id]?.sku === product.sku || undefined}>
                      <div className="cosmetic-item-copy">
                        <strong>{product.display_name}{equipped && <span className="cosmetic-equipped">장착 중</span>}</strong>
                        <span><FormattedNumber value={product.price} /> 토큰</span>
                      </div>
                      <div className="cosmetic-item-actions">
                        <button className="cosmetic-secondary" type="button" disabled={actionsDisabled} aria-pressed={previewBySlot[slot.slot_id]?.sku === product.sku} onClick={() => preview(product)} aria-label={`${product.display_name} 미리보기`}>{previewBySlot[slot.slot_id]?.sku === product.sku ? "미리보기 중" : "미리보기"}</button>
                        {tab === "shop" ? (
                          owned.has(product.sku)
                            ? <span className="cosmetic-owned">보유 중</span>
                          : <button
                              className="cosmetic-primary"
                              type="button"
                              disabled={!canBuy}
                              title={slotUnavailableReason ?? (!product.purchasable ? "판매 중인 상품이 아닙니다" : availableBalance < product.price ? "잔액이 부족합니다" : undefined)}
                              onClick={() => setConfirmSku(product.sku)}
                              aria-label={`${product.display_name} 구매`}
                            >{busySku === product.sku ? "확인 중" : "구매"}</button>
                        ) : (
                          <button
                            className={equipped ? "cosmetic-secondary" : "cosmetic-primary"}
                            type="button"
                            disabled={actionsDisabled || Boolean(slotUnavailableReason) || busySlot === slot.slot_id}
                            onClick={() => void equip(slot.slot_id, equipped ? null : product.sku)}
                            aria-label={`${product.display_name} ${equipped ? "해제" : "장착"}`}
                          >{equipped ? "해제" : busySlot === slot.slot_id ? "확인 중" : "장착"}</button>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
              {tab === "shop" && saleProducts.some((product) => product.slot_id === slot.slot_id && availableBalance < product.price)
                && <p className="cosmetic-shortfall">잔액이 부족한 상품이 있습니다. 현재 잔액과 상품 가격을 확인하세요.</p>}
            </section>
          );
        })}
        {tab === "inventory" && state.owned_skus.length === 0 && (
          <p className="cosmetic-empty">아직 보유한 장식이 없습니다. 미리보기 후 마음에 드는 장식을 구매하세요.</p>
        )}
        {tab === "inventory" && state.owned_skus.length > 0 && products.every((product) => !owned.has(product.sku)) && (
          <p className="cosmetic-empty">현재 앱에서 표시할 수 있는 보유 장식이 없습니다.</p>
        )}
      </div>

      <div className="cosmetic-footer">
        {slotUnavailableReason && <>
          <p className="cosmetic-unavailable" role="status">{slotUnavailableReason}</p>
          <button className="cosmetic-reset-preview" type="button" disabled={actionsDisabled || refreshing} onClick={() => void refreshShop()}>{refreshing ? "확인 중" : "상점 다시 확인"}</button>
        </>}
        {notice && <p className="cosmetic-notice" role="status">{notice}</p>}
        {Object.keys(previewBySlot).length > 0 && <button className="cosmetic-reset-preview" type="button" disabled={actionsDisabled} onClick={clearPreview}>현재 장착으로 되돌리기</button>}
      </div>

      {confirmSku && (() => {
        const product = products.find((item) => item.sku === confirmSku);
        if (!product) return null;
        const afterPurchase = Math.max(0, availableBalance - product.price);
        return (
            <dialog
              ref={dialogRef}
              className="cosmetic-confirmation"
              aria-labelledby="cosmetic-confirm-title"
              onCancel={(event) => { event.preventDefault(); setConfirmSku(null); }}
              onClick={(event) => { if (event.target === event.currentTarget) setConfirmSku(null); }}
            >
              <p className="cosmetic-shop-kicker">구매는 장착과 별개입니다</p>
              <h3 id="cosmetic-confirm-title">{product.display_name} 구매 확인</h3>
              <p><FormattedNumber value={product.price} /> 토큰 차감</p>
              <p>구매 후 잔액 <FormattedNumber value={afterPurchase} /> 토큰</p>
              <div className="cosmetic-confirm-actions">
                <button type="button" className="cosmetic-secondary" onClick={() => setConfirmSku(null)}>취소</button>
                <button type="button" className="cosmetic-primary" disabled={actionsDisabled || Boolean(slotUnavailableReason) || availableBalance < product.price} onClick={() => void confirmPurchase(product)}>구매 확인</button>
              </div>
            </dialog>
        );
      })()}
    </section>
  );
}
