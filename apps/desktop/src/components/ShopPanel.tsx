import { formatTokenAmount } from "../lib/tokenFormatting";
import { useEffect, useMemo, useRef, useState } from "react";
import { ShopProductThumbnail } from "./ShopProductThumbnail";
import type { PendingShopAction } from "../hooks/useShopActions";
import type {
  ActiveEffects,
  AvatarEquipment,
  AvatarSlot,
  LandscapeInstance,
  PlanetAvatar,
  ShopActionResult,
  ShopProduct,
  ShopQuote,
  ShopQuoteTarget,
  ShopRequest,
  ShopState,
} from "../types/usage";

export type ShopPanelPreview =
  | { kind: "landscape"; product: ShopProduct; instance?: LandscapeInstance }
  | { kind: "avatar"; product: ShopProduct; equipment: AvatarEquipment; avatar: PlanetAvatar };

export type ShopPanelProps = {
  state: ShopState;
  quote: (target: ShopQuoteTarget) => Promise<ShopQuote | null>;
  apply: (request: ShopRequest) => Promise<ShopActionResult | null>;
  retryPending: () => Promise<ShopActionResult | null>;
  pending?: PendingShopAction | null;
  disabledReason?: string | null;
  avatar?: PlanetAvatar;
  open?: boolean;
  onPreviewChange: (preview: ShopPanelPreview | null) => void;
  onSelectLandscapeInstance: (instanceId: string) => void;
  onClose?: () => void;
  createRequestId?: () => string;
};

type PanelView = "products" | "inventory";
type ActivePurchaseQuote = { product: ShopProduct; quote: ShopQuote; changed: boolean };

const AVATAR_SLOTS: AvatarSlot[] = ["head", "outfit", "face", "back"];
const AVATAR_SLOT_LABELS: Record<AvatarSlot, string> = {
  head: "머리",
  outfit: "의상",
  face: "얼굴",
  back: "등 장식",
};
const EFFECT_LABELS: Record<keyof ActiveEffects, string> = {
  token_earning_bps: "토큰 획득",
  civilization_growth_bps: "문명 성장",
  shop_discount_bps: "상점 할인",
  reset_cooldown_bps: "초기화 대기시간",
  natural_removal_discount_bps: "자연물 제거 할인",
  era_reward_tokens: "시대 보상",
  streak_reward_tokens: "연속 보상",
};
const PRODUCT_EFFECT_LABELS: Record<NonNullable<ShopProduct["effect_type"]>, string> = {
  token_earning: "토큰 획득 효과",
  civilization_growth: "문명 성장 효과",
  shop_discount: "상점 할인 효과",
  reset_cooldown: "초기화 대기시간 효과",
  natural_removal_discount: "자연물 제거 할인 효과",
  era_reward: "시대 보상 효과",
  streak_reward: "연속 보상 효과",
};

function formatTokens(value: number): string {
  return `${formatTokenAmount(Math.max(0, Math.floor(value)))} 토큰`;
}

function estimatedPrice(product: ShopProduct, effects: ActiveEffects): number {
  const discount = Math.min(1500, Math.max(0, Math.floor(effects.shop_discount_bps)));
  return Math.ceil((product.price * (10_000 - discount)) / 10_000);
}

function percent(value: number): string {
  const bounded = Math.max(0, Math.min(10_000, Math.floor(value)));
  return `${Number((bounded / 100).toFixed(2)).toLocaleString("ko-KR")}%`;
}

function effectValue(label: keyof ActiveEffects, value: number): string {
  return label === "era_reward_tokens" || label === "streak_reward_tokens"
    ? formatTokens(value)
    : percent(value);
}

function requestId(): string {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  const bytes = new Uint8Array(16);
  if (globalThis.crypto?.getRandomValues) globalThis.crypto.getRandomValues(bytes);
  else for (let index = 0; index < bytes.length; index += 1) bytes[index] = Math.floor(Math.random() * 256);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function actionMessage(result: ShopActionResult): string {
  switch (result.status) {
    case "purchased": return "구매를 완료했습니다. 보유함에서 상품을 확인할 수 있습니다.";
    case "equipped": return "장착을 완료했습니다.";
    case "unequipped": return "장착을 해제했습니다.";
    case "quote_changed": return "가격 정보가 바뀌었습니다. 새 가격을 확인한 뒤 다시 확정해 주세요.";
    case "limit_reached": return "이 상품은 보관함과 행성에 합쳐 최대 5개까지 둘 수 있습니다.";
    case "already_owned": return "이미 보유한 아바타 상품입니다.";
    case "insufficient_balance": return "사용 가능 잔액이 부족합니다.";
    case "version_conflict": return "장착 상태가 바뀌었습니다. 최신 상태를 확인해 주세요.";
    case "cycle_mismatch": return "현재 행성이 바뀌었습니다. 최신 상태를 확인해 주세요.";
    case "not_owned": return "보유한 상품만 장착할 수 있습니다.";
    case "catalog_mismatch": return "상품 정보가 바뀌었습니다. 상점 상태를 새로 확인해 주세요.";
    case "request_conflict": return "요청 결과가 일치하지 않습니다. 같은 요청을 다시 확인해 주세요.";
    case "unavailable": return "현재 상점 작업을 사용할 수 없습니다.";
    default: return "상점 상태를 새로 확인해 주세요.";
  }
}

function cloneEquipmentWith(equipment: AvatarEquipment, slot: AvatarSlot, sku: string): AvatarEquipment {
  return {
    head: { ...equipment.head },
    outfit: { ...equipment.outfit },
    face: { ...equipment.face },
    back: { ...equipment.back },
    [slot]: { ...equipment[slot], sku },
  };
}

function previewKey(preview: ShopPanelPreview): string {
  return preview.kind === "landscape"
    ? `landscape:${preview.product.sku}:${preview.instance?.instance_id ?? "catalog"}`
    : `avatar:${preview.product.sku}`;
}

export function ShopPanel({
  state,
  quote,
  apply,
  retryPending,
  pending = null,
  disabledReason = null,
  avatar = "masculine",
  open,
  onPreviewChange,
  onSelectLandscapeInstance,
  onClose,
  createRequestId = requestId,
}: ShopPanelProps) {
  const [panelOpen, setPanelOpen] = useState(true);
  const [view, setView] = useState<PanelView>("products");
  const [category, setCategory] = useState<"landscape" | "avatar">("landscape");
  const [activeQuote, setActiveQuote] = useState<ActivePurchaseQuote | null>(null);
  const [notice, setNotice] = useState("");
  const [previewingKey, setPreviewingKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const contextKey = JSON.stringify([state.account_id, state.current_cycle_id]);
  const canonicalEquipmentKey = JSON.stringify(state.avatar_equipment);
  const contextRef = useRef(contextKey);
  const equipmentRef = useRef(canonicalEquipmentKey);
  const contextChangedPendingEffect = useRef(false);
  const openRef = useRef(open ?? panelOpen);
  const closedPendingEffect = useRef(false);
  const operationSequence = useRef(0);
  const mountedRef = useRef(false);
  const previewCallbackRef = useRef(onPreviewChange);
  previewCallbackRef.current = onPreviewChange;

  const isOpen = open ?? panelOpen;
  if (contextRef.current !== contextKey) {
    contextRef.current = contextKey;
    contextChangedPendingEffect.current = true;
    operationSequence.current += 1;
  }
  if (openRef.current !== isOpen) {
    openRef.current = isOpen;
    closedPendingEffect.current = !isOpen;
    if (!isOpen) operationSequence.current += 1;
  }
  const products = useMemo(
    () => state.products.filter((product) => product.category === category),
    [category, state.products],
  );
  const productsBySku = useMemo(() => new Map(state.products.map((product) => [product.sku, product])), [state.products]);
  const placementIds = useMemo(
    () => new Set(state.placements.filter((placement) => placement.cycle_id === state.current_cycle_id).map((placement) => placement.instance_id)),
    [state.current_cycle_id, state.placements],
  );
  const unavailableReason = disabledReason || state.action_unavailable_reason;
  const actionDisabled = Boolean(unavailableReason || busy || pending);
  const ownedAvatarSkus = useMemo(() => new Set(state.avatar_owned_skus), [state.avatar_owned_skus]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      operationSequence.current += 1;
      previewCallbackRef.current(null);
    };
  }, []);

  useEffect(() => {
    if (!contextChangedPendingEffect.current) return;
    contextChangedPendingEffect.current = false;
    setActiveQuote(null);
    setNotice("");
    setBusy(false);
    clearPreview();
  // clearPreview intentionally sends the latest callback ref, and this effect is keyed to account/cycle only.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [contextKey]);

  useEffect(() => {
    if (equipmentRef.current === canonicalEquipmentKey) return;
    equipmentRef.current = canonicalEquipmentKey;
    clearPreview();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [canonicalEquipmentKey]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (activeQuote && !dialog.open) {
      if (typeof dialog.showModal === "function") dialog.showModal();
      else dialog.setAttribute("open", "");
    } else if (!activeQuote && dialog.open) {
      if (typeof dialog.close === "function") dialog.close();
      else dialog.removeAttribute("open");
    }
  }, [activeQuote]);

  useEffect(() => {
    if (!closedPendingEffect.current) return;
    closedPendingEffect.current = false;
    setActiveQuote(null);
    setBusy(false);
    clearPreview();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen]);

  function clearPreview() {
    setPreviewingKey(null);
    onPreviewChange(null);
  }

  function clearForNavigation() {
    operationSequence.current += 1;
    setActiveQuote(null);
    setBusy(false);
    clearPreview();
  }

  function selectCategory(next: "landscape" | "avatar") {
    if (next === category) return;
    clearForNavigation();
    setCategory(next);
    setNotice("");
  }

  function selectView(next: PanelView) {
    if (next === view) return;
    clearForNavigation();
    setView(next);
    setNotice("");
  }

  function closePanel() {
    clearForNavigation();
    if (open === undefined) setPanelOpen(false);
    onClose?.();
  }

  function handleActionResult(result: ShopActionResult, expectedRequest: ShopRequest) {
    if (result.request_id !== expectedRequest.request_id) {
      setNotice("요청 결과가 원래 요청과 일치하지 않습니다. 같은 요청 상태를 다시 확인해 주세요.");
      return;
    }
    if (result.status === "quote_changed") {
      const expectedSku = expectedRequest.kind === "purchase" && expectedRequest.quote.target.kind === "purchase"
        ? expectedRequest.quote.target.sku
        : activeQuote?.product.sku;
      const revisedQuote = result.confirmed_quote;
      const product = expectedSku ? productsBySku.get(expectedSku) : undefined;
      if (product?.category === "landscape" || product?.category === "avatar") {
        if (revisedQuote?.target.kind === "purchase" && revisedQuote.target.sku === product.sku) {
          setActiveQuote({ product, quote: revisedQuote, changed: true });
        } else if (expectedRequest.kind === "purchase") {
          setActiveQuote(null);
        }
      } else if (expectedRequest.kind === "purchase") {
        setActiveQuote(null);
      }
      setNotice(actionMessage(result));
      return;
    }
    setActiveQuote(null);
    setNotice(actionMessage(result));
  }

  function previewProduct(product: ShopProduct, instance?: LandscapeInstance) {
    if (product.category === "landscape") {
      const preview: ShopPanelPreview = { kind: "landscape", product, ...(instance ? { instance } : {}) };
      setPreviewingKey(previewKey(preview));
      onPreviewChange(preview);
      return;
    }
    if (!product.avatar_slot) return;
    const preview: ShopPanelPreview = {
      kind: "avatar",
      product,
      avatar,
      equipment: cloneEquipmentWith(state.avatar_equipment, product.avatar_slot, product.sku),
    };
    setPreviewingKey(previewKey(preview));
    onPreviewChange(preview);
  }

  async function beginPurchase(product: ShopProduct) {
    if (actionDisabled || !product.purchasable) return;
    if (product.category === "avatar" && ownedAvatarSkus.has(product.sku)) return;
    if (product.category === "landscape" && state.landscape_instances.filter((instance) => instance.sku === product.sku).length >= 5) return;

    const capturedContext = contextRef.current;
    const sequence = ++operationSequence.current;
    setBusy(true);
    setNotice("");
    try {
      const nextQuote = await quote({ kind: "purchase", sku: product.sku });
      if (!mountedRef.current || capturedContext !== contextRef.current || sequence !== operationSequence.current) return;
      if (!nextQuote || nextQuote.target.kind !== "purchase" || nextQuote.target.sku !== product.sku) {
        setNotice("가격 정보를 확인하지 못했습니다. 다시 시도해 주세요.");
        return;
      }
      setActiveQuote({ product, quote: nextQuote, changed: false });
    } catch (cause) {
      if (mountedRef.current && capturedContext === contextRef.current && sequence === operationSequence.current) {
        setNotice(typeof cause === "string" ? cause : "가격 정보를 불러오지 못했습니다. 다시 시도해 주세요.");
      }
    } finally {
      if (mountedRef.current && capturedContext === contextRef.current && sequence === operationSequence.current) setBusy(false);
    }
  }

  async function confirmPurchase() {
    if (!activeQuote || actionDisabled || busy) return;
    const purchase = activeQuote;
    const capturedContext = contextRef.current;
    const sequence = ++operationSequence.current;
    const request: ShopRequest = {
      kind: "purchase",
      request_id: createRequestId(),
      quote: purchase.quote,
    };
    setBusy(true);
    setNotice("");
    try {
      const result = await apply(request);
      if (!mountedRef.current || capturedContext !== contextRef.current || sequence !== operationSequence.current) return;
      if (!result || result.state.account_id !== state.account_id || result.state.current_cycle_id !== state.current_cycle_id) {
        setNotice("상점 응답이 현재 계정이나 행성과 일치하지 않습니다.");
        return;
      }
      handleActionResult(result, request);
    } catch {
      if (mountedRef.current && capturedContext === contextRef.current && sequence === operationSequence.current) {
        setNotice("요청 결과를 확인하지 못했습니다. 같은 요청으로 다시 시도해 주세요.");
      }
    } finally {
      if (mountedRef.current && capturedContext === contextRef.current && sequence === operationSequence.current) setBusy(false);
    }
  }

  async function retryUncertainAction() {
    if (!pending || pending.status !== "uncertain" || busy || unavailableReason) return;
    const capturedContext = contextRef.current;
    const sequence = ++operationSequence.current;
    setBusy(true);
    setNotice("");
    try {
      const result = await retryPending();
      if (!mountedRef.current || capturedContext !== contextRef.current || sequence !== operationSequence.current) return;
      if (result && result.state.account_id === state.account_id && result.state.current_cycle_id === state.current_cycle_id) {
        handleActionResult(result, pending.request);
      } else if (result) {
        setNotice("상점 응답이 현재 계정이나 행성과 일치하지 않습니다.");
      }
    } catch {
      if (mountedRef.current && capturedContext === contextRef.current && sequence === operationSequence.current) {
        setNotice("요청 결과를 확인하지 못했습니다. 같은 요청으로 다시 시도해 주세요.");
      }
    } finally {
      if (mountedRef.current && capturedContext === contextRef.current && sequence === operationSequence.current) setBusy(false);
    }
  }

  async function setAvatarEquipment(product: ShopProduct) {
    const slot = product.avatar_slot;
    if (!slot || !ownedAvatarSkus.has(product.sku) || actionDisabled) return;
    const current = state.avatar_equipment[slot];
    const nextSku = current.sku === product.sku ? null : product.sku;
    const request: ShopRequest = {
      kind: "equip_avatar",
      request_id: createRequestId(),
      slot,
      sku: nextSku,
      expected_version: current.version,
    };
    const capturedContext = contextRef.current;
    const sequence = ++operationSequence.current;
    setBusy(true);
    setNotice("");
    try {
      const result = await apply(request);
      if (!mountedRef.current || capturedContext !== contextRef.current || sequence !== operationSequence.current) return;
      if (!result || result.state.account_id !== state.account_id || result.state.current_cycle_id !== state.current_cycle_id) {
        setNotice("상점 응답이 현재 계정이나 행성과 일치하지 않습니다.");
      } else {
        handleActionResult(result, request);
      }
    } catch {
      if (mountedRef.current && capturedContext === contextRef.current && sequence === operationSequence.current) {
        setNotice("장착 상태를 확인하지 못했습니다. 최신 상태를 새로 불러와 주세요.");
      }
    } finally {
      if (mountedRef.current && capturedContext === contextRef.current && sequence === operationSequence.current) setBusy(false);
    }
  }

  if (!isOpen) return null;

  const categoryProducts = products;
  const activePending = pending;
  const discount = Math.min(1500, Math.max(0, Math.floor(state.effects.shop_discount_bps)));
  const activeQuoteUnaffordable = Boolean(activeQuote && activeQuote.quote.price > state.available_balance);

  return (
    <section className="cosmetic-shop" aria-label="행성 상점">
      <div className="cosmetic-shop-heading">
        <div>
          <p className="cosmetic-shop-kicker">행성에 쌓이는 외형과 장식</p>
          <h2>행성 상점</h2>
        </div>
        {onClose && <button className="cosmetic-close" type="button" onClick={closePanel} aria-label="상점 닫기">닫기</button>}
      </div>

      <div className="cosmetic-balance">
        <span>사용 가능 잔액</span>
        <strong>{formatTokens(state.available_balance)}</strong>
      </div>

      <div className="cosmetic-tabs" role="tablist" aria-label="상품 종류">
        <button type="button" role="tab" aria-selected={category === "landscape"} onClick={() => selectCategory("landscape")}>조경</button>
        <button type="button" role="tab" aria-selected={category === "avatar"} onClick={() => selectCategory("avatar")}>아바타</button>
      </div>
      <div className="cosmetic-tabs" role="tablist" aria-label="상점 보기">
        <button type="button" role="tab" aria-selected={view === "products"} onClick={() => selectView("products")}>상품</button>
        <button type="button" role="tab" aria-selected={view === "inventory"} onClick={() => selectView("inventory")}>보유함</button>
      </div>

      {unavailableReason && <p className="cosmetic-unavailable" role="status">{unavailableReason}</p>}
      {activePending && (
        <div className="cosmetic-notice" role="status">
          {activePending.status === "uncertain" ? "요청 결과를 확인하지 못했습니다. 같은 요청을 다시 확인할 수 있습니다." : "상점 요청을 처리하고 있습니다."}
          {activePending.status === "uncertain" && (
            <button className="cosmetic-secondary" type="button" disabled={Boolean(unavailableReason || busy)} onClick={() => { void retryUncertainAction(); }}>같은 요청 다시 시도</button>
          )}
        </div>
      )}

      {view === "products" ? (
        <div className="cosmetic-list" aria-label={category === "landscape" ? "조경 상품" : "아바타 상품"}>
          {categoryProducts.map((product) => {
            const owned = product.category === "avatar" && ownedAvatarSkus.has(product.sku);
            const landscapeCount = product.category === "landscape"
              ? state.landscape_instances.filter((instance) => instance.sku === product.sku).length
              : 0;
            const atLimit = product.category === "landscape" && landscapeCount >= 5;
            const unavailableProduct = !product.purchasable || owned || atLimit;
            const estimated = estimatedPrice(product, state.effects);
            const productEffect = product.effect_type
              ? `${PRODUCT_EFFECT_LABELS[product.effect_type]} · ${product.effect_type === "era_reward" || product.effect_type === "streak_reward" ? formatTokens(product.effect_value) : percent(product.effect_value)}`
              : null;
            const key = product.category === "landscape" ? `landscape:${product.sku}:catalog` : `avatar:${product.sku}`;
            return (
              <article className="cosmetic-item" key={product.sku} data-cosmetic-previewing={previewingKey === key ? "true" : undefined} data-shop-sku={product.sku}>
                <div className="cosmetic-item-copy">
                  <ShopProductThumbnail product={product} avatar={avatar} equipment={state.avatar_equipment} />
                  <strong>{product.display_name}</strong>
                  <span>{product.category === "landscape" ? `행성 장식 · ${product.placement_zone === "sky" ? "하늘" : "지면"}` : `아바타 · ${AVATAR_SLOT_LABELS[product.avatar_slot ?? "head"]} 슬롯`}</span>
                  {productEffect && <small>{productEffect}</small>}
                  {product.category === "landscape" && <small>{`${landscapeCount} / 5 보유`}</small>}
                  {product.category === "avatar" && owned && <span className="cosmetic-owned">보유 중</span>}
                </div>
                <div className="cosmetic-item-actions">
                  <button className="cosmetic-secondary" type="button" onClick={() => previewProduct(product)}>{product.display_name} 미리보기</button>
                  {product.category === "landscape" || !owned ? (
                    <button
                      className="cosmetic-primary"
                      type="button"
                      aria-label={`${product.display_name} 구매`}
                      disabled={Boolean(actionDisabled || unavailableProduct)}
                      onClick={() => { void beginPurchase(product); }}
                    >구매 · {formatTokens(estimated)}</button>
                  ) : null}
                </div>
              </article>
            );
          })}
          {categoryProducts.length === 0 && <p className="cosmetic-empty">현재 표시할 상품이 없습니다.</p>}
        </div>
      ) : category === "landscape" ? (
        <div className="cosmetic-list" aria-label="보유한 조경 상품">
          {state.landscape_instances.map((instance) => {
            const product = productsBySku.get(instance.sku);
            if (!product || product.category !== "landscape") return null;
            const isPlaced = placementIds.has(instance.instance_id);
            const key = `landscape:${instance.sku}:${instance.instance_id}`;
            return (
              <article className="cosmetic-item" key={instance.instance_id} data-shop-instance={instance.instance_id} data-cosmetic-previewing={previewingKey === key ? "true" : undefined}>
                <div className="cosmetic-item-copy">
                  <ShopProductThumbnail product={product} instance={instance} avatar={avatar} />
                  <strong>{product.display_name}</strong>
                  <span>변형 {instance.variation_index + 1} · {isPlaced ? "배치됨" : "보관함"}</span>
                  <small>인스턴스 {instance.instance_id}</small>
                </div>
                <div className="cosmetic-item-actions">
                  <button className="cosmetic-secondary" type="button" onClick={() => previewProduct(product, instance)}>{product.display_name} 미리보기</button>
                  <button className="cosmetic-primary" type="button" onClick={() => onSelectLandscapeInstance(instance.instance_id)}>배치 선택</button>
                </div>
              </article>
            );
          })}
          {state.landscape_instances.length === 0 && <p className="cosmetic-empty">보유한 조경 상품이 없습니다.</p>}
        </div>
      ) : (
        <div className="cosmetic-list" aria-label="보유한 아바타 상품">
          {AVATAR_SLOTS.map((slot) => {
            const ownedForSlot = state.avatar_owned_skus
              .map((sku) => productsBySku.get(sku))
              .filter((product): product is ShopProduct => product?.category === "avatar" && product.avatar_slot === slot);
            return (
              <section className="cosmetic-slot" key={slot} aria-label={`${AVATAR_SLOT_LABELS[slot]} 슬롯`}>
                <h3>{AVATAR_SLOT_LABELS[slot]}</h3>
                <ul>
                  {ownedForSlot.map((product) => {
                    const equipped = state.avatar_equipment[slot].sku === product.sku;
                    const key = `avatar:${product.sku}`;
                    return (
                      <li className="cosmetic-item" key={product.sku} data-shop-avatar-sku={product.sku} data-cosmetic-previewing={previewingKey === key ? "true" : undefined}>
                        <div className="cosmetic-item-copy">
                          <ShopProductThumbnail product={product} avatar={avatar} equipment={state.avatar_equipment} />
                          <strong>{product.display_name}{equipped && <span className="cosmetic-equipped">장착 중</span>}</strong>
                          <span>{AVATAR_SLOT_LABELS[slot]} 슬롯</span>
                          <small>외형만 변경하며 게임 효과는 없습니다</small>
                        </div>
                        <div className="cosmetic-item-actions">
                          <button className="cosmetic-secondary" type="button" onClick={() => previewProduct(product)}>{product.display_name} 미리보기</button>
                          <button className="cosmetic-primary" type="button" disabled={Boolean(actionDisabled)} onClick={() => { void setAvatarEquipment(product); }}>{equipped ? "장착 해제" : `${product.display_name} 장착`}</button>
                        </div>
                      </li>
                    );
                  })}
                  {ownedForSlot.length === 0 && <li className="cosmetic-empty">보유한 상품이 없습니다.</li>}
                </ul>
              </section>
            );
          })}
        </div>
      )}

      <section className="cosmetic-footer" aria-label="현재 상점 효과">
        <p className="cosmetic-notice">현재 효과 · 상점 할인 {percent(discount)}</p>
        <ul>
          {(Object.keys(EFFECT_LABELS) as Array<keyof ActiveEffects>).map((key) => (
            <li key={key}>{EFFECT_LABELS[key]}: {effectValue(key, state.effects[key])}</li>
          ))}
        </ul>
      </section>
      {notice && <p className="cosmetic-notice" role="status">{notice}</p>}

      <dialog
        ref={dialogRef}
        className="cosmetic-confirmation"
        aria-labelledby="shop-purchase-confirm-title"
        onCancel={(event) => { event.preventDefault(); setActiveQuote(null); }}
        onClick={(event) => { if (event.target === event.currentTarget) setActiveQuote(null); }}
      >
        {activeQuote && (
          <>
            <p className="cosmetic-shop-kicker">구매는 장착과 별개입니다</p>
            <h3 id="shop-purchase-confirm-title">{activeQuote.product.display_name} 구매 확인</h3>
            {activeQuote.changed && <p className="cosmetic-shortfall">가격이 바뀌었습니다. 새 금액을 확인한 뒤 다시 확정해 주세요.</p>}
            <p>확정 가격 <strong>{formatTokens(activeQuote.quote.price)}</strong></p>
            <p>확정 후 잔액 {formatTokens(Math.max(0, state.available_balance - activeQuote.quote.price))}</p>
            {activeQuoteUnaffordable && <p className="cosmetic-shortfall">잔액이 부족합니다.</p>}
            <div className="cosmetic-confirm-actions">
              <button className="cosmetic-secondary" type="button" disabled={busy} onClick={() => setActiveQuote(null)}>취소</button>
              <button className="cosmetic-primary" type="button" disabled={Boolean(actionDisabled || busy || activeQuoteUnaffordable)} onClick={() => { void confirmPurchase(); }}>구매 확정</button>
            </div>
          </>
        )}
      </dialog>
    </section>
  );
}
