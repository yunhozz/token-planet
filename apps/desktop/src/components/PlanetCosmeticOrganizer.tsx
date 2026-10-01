import type { CosmeticProduct, CosmeticShopState, EquippedCosmetic } from "../types/usage";
import { CosmeticThumbnail } from "./CosmeticThumbnail";

export type PlanetCosmeticOrganizerView = {
  open: boolean;
  category: string;
  query: string;
};

type Props = {
  state: CosmeticShopState | null;
  loading: boolean;
  error: string;
  selectedSku: string | null;
  view: PlanetCosmeticOrganizerView;
  equipped: EquippedCosmetic[];
  onViewChange: (view: PlanetCosmeticOrganizerView) => void;
  onSelect: (product: CosmeticProduct) => void;
  onOpenShop: (sku?: string) => void;
};

const CATEGORIES = [
  { id: "all", label: "전체" },
  { id: "sky", label: "하늘" },
  { id: "ring", label: "고리" },
  { id: "surface", label: "지표" },
  { id: "forecourt", label: "앞마당" },
];

const CATEGORY_NAMES = Object.fromEntries(CATEGORIES.map(({ id, label }) => [id, label]));

export function PlanetCosmeticOrganizer({
  state,
  loading,
  error,
  selectedSku,
  view,
  equipped,
  onViewChange,
  onSelect,
  onOpenShop,
}: Props) {
  const ownedSkus = state?.owned_skus ?? [];
  const products = (state?.products ?? []).filter((product) => ownedSkus.includes(product.sku));
  const selectedProduct = products.find((product) => product.sku === selectedSku) ?? null;
  const equippedSkus = new Set(equipped.map((item) => item.sku));
  const query = view.query.trim().toLocaleLowerCase("ko-KR");
  const visibleProducts = products.filter((product) =>
    (view.category === "all" || product.slot_id === view.category)
    && (!query || product.display_name.toLocaleLowerCase("ko-KR").includes(query))
  );

  function updateView(patch: Partial<PlanetCosmeticOrganizerView>) {
    onViewChange({ ...view, ...patch });
  }

  return (
    <section className="planet-cosmetic-organizer" aria-label="보유 장식 보관함">
      <button
        className="planet-cosmetic-toggle"
        type="button"
        aria-expanded={view.open}
        aria-controls="planet-cosmetic-content"
        onClick={() => updateView({ open: !view.open })}
      >
        <span><strong>보유 장식</strong><span className="planet-cosmetic-count">{state ? `${products.length}개` : loading ? "불러오는 중" : "확인 필요"}</span></span>
        <span className="planet-cosmetic-toggle-copy">{view.open ? "보관함 닫기" : "둘러보기"} <span aria-hidden="true">{view.open ? "⌃" : "⌄"}</span></span>
      </button>
      {view.open && <div id="planet-cosmetic-content" className="planet-cosmetic-content">
        {loading && !state && <p className="planet-cosmetic-status" role="status">구매한 장식을 불러오고 있습니다.</p>}
        {!loading && error && !state && <p className="planet-cosmetic-status" role="alert">보유 장식 상태를 확인하지 못했습니다. {error}</p>}
        {state && <>
          <div className="planet-cosmetic-tools">
            <label className="planet-cosmetic-search">
              <span>장식 이름 검색</span>
              <input type="search" value={view.query} onChange={(event) => updateView({ query: event.target.value })} placeholder="예: 오로라" />
            </label>
            <div className="planet-cosmetic-categories" role="group" aria-label="장식 분류">
              {CATEGORIES.map(({ id, label }) => <button
                key={id}
                type="button"
                aria-pressed={view.category === id}
                onClick={() => updateView({ category: id })}
              >{label}</button>)}
            </div>
          </div>
          {products.length === 0
            ? <div className="planet-cosmetic-empty"><p>{ownedSkus.length === 0 ? "아직 구매한 장식이 없습니다." : "현재 표시할 수 있는 보유 장식이 없습니다."}</p><button type="button" onClick={() => onOpenShop()}>상점 둘러보기</button></div>
            : visibleProducts.length === 0
              ? <p className="planet-cosmetic-status" role="status">조건에 맞는 장식이 없습니다.</p>
              : <ul className="planet-cosmetic-grid" aria-label="구매한 장식">
                {visibleProducts.map((product) => {
                  const isEquipped = equippedSkus.has(product.sku);
                  const isSelected = selectedSku === product.sku;
                  return <li key={product.sku}>
                    <button
                      className={`planet-cosmetic-tile${isSelected ? " is-selected" : ""}`}
                      type="button"
                      aria-label={`${product.display_name}, ${isEquipped ? "장착 중" : "미장착"}, ${CATEGORY_NAMES[product.slot_id] ?? "장식"}`}
                      aria-pressed={isSelected}
                      onClick={() => onSelect(product)}
                    >
                      <CosmeticThumbnail sku={product.sku} slotId={product.slot_id} />
                      <span className="planet-cosmetic-tile-copy"><strong>{product.display_name}</strong><small>{CATEGORY_NAMES[product.slot_id] ?? "장식"}</small></span>
                      <span className={isEquipped ? "planet-cosmetic-equipped" : "planet-cosmetic-unequipped"}>{isEquipped ? "장착 중" : "미장착"}</span>
                    </button>
                  </li>;
                })}
              </ul>}
          {selectedProduct && <div className="planet-cosmetic-selection" aria-live="polite">
            <CosmeticThumbnail sku={selectedProduct.sku} slotId={selectedProduct.slot_id} className="planet-cosmetic-selection-art" />
            <div><p className="planet-cosmetic-selection-kicker">{equippedSkus.has(selectedProduct.sku) ? "행성에서 강조 중" : "보유한 장식"}</p><strong>{selectedProduct.display_name}</strong><p>{equippedSkus.has(selectedProduct.sku) ? "풍경에서 장착 위치를 비추고 있습니다." : "행성에 놓으려면 장착하세요."}</p></div>
            {!equippedSkus.has(selectedProduct.sku) && <button type="button" onClick={() => onOpenShop(selectedProduct.sku)}>행성 꾸미기에서 장착</button>}
          </div>}
        </>}
      </div>}
    </section>
  );
}
