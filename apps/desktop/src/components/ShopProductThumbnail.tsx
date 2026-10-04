import { AvatarSprite } from "./AvatarSprite";
import { LANDSCAPE_OBJECT_INTRINSIC_BOUNDS, LandscapeObjectSprite } from "./LandscapeObjectSprite";
import type { AvatarEquipment, AvatarSlot, LandscapeInstance, PlanetAvatar, PlacementZone, ShopProduct } from "../types/usage";

type ShopProductThumbnailProps = {
  product: ShopProduct;
  instance?: LandscapeInstance;
  avatar?: PlanetAvatar;
  equipment?: AvatarEquipment;
  className?: string;
};

const LANDSCAPE_ZONE_BY_SKU: Record<string, PlacementZone> = {
  land_pond: "ground",
  land_well: "ground",
  land_greenhouse: "ground",
  land_reservoir: "ground",
  land_crystal: "ground",
  land_school: "ground",
  land_observatory: "ground",
  land_laboratory: "ground",
  land_market: "ground",
  land_trading_post: "ground",
  land_freight: "ground",
  land_bazaar: "ground",
  land_rover: "ground",
  land_clocktower: "ground",
  land_launchpad: "ground",
  land_portal: "ground",
  land_toolbox: "ground",
  land_excavator: "ground",
  land_cutter: "ground",
  land_recycler: "ground",
  land_flag: "ground",
  land_thin_ring: "sky",
  land_double_ring: "sky",
  land_moonlets: "sky",
  land_lantern: "ground",
  land_stars: "sky",
  land_aurora: "sky",
  land_meteors: "sky",
  land_garden: "ground",
  land_tree: "ground",
  land_bench: "ground",
  land_fountain: "ground",
};

const AVATAR_SLOT_BY_SKU: Record<string, AvatarSlot> = {
  avatar_explorer_hat: "head",
  avatar_crown: "head",
  avatar_space_helmet: "head",
  avatar_halo: "head",
  avatar_workwear: "outfit",
  avatar_labwear: "outfit",
  avatar_spacesuit: "outfit",
  avatar_nebula_suit: "outfit",
  avatar_glasses: "face",
  avatar_sunglasses: "face",
  avatar_goggles: "face",
  avatar_hud: "face",
  avatar_backpack: "back",
  avatar_cape: "back",
  avatar_jetpack: "back",
  avatar_wings: "back",
};

const EMPTY_AVATAR_EQUIPMENT: AvatarEquipment = {
  head: { sku: null, version: 0 },
  outfit: { sku: null, version: 0 },
  face: { sku: null, version: 0 },
  back: { sku: null, version: 0 },
};

function unavailablePreview(product: ShopProduct, className?: string) {
  return (
    <span
      className={className}
      data-thumbnail-unavailable="true"
      role="img"
      aria-label={`${product.display_name} 미리보기`}
    >미리보기를 제공할 수 없습니다.</span>
  );
}

export function ShopProductThumbnail({
  product,
  instance,
  avatar = "masculine",
  equipment,
  className,
}: ShopProductThumbnailProps) {
  if (product.category === "landscape") {
    const zone = LANDSCAPE_ZONE_BY_SKU[product.sku];
    if (!zone || product.placement_zone !== zone || (instance && instance.sku !== product.sku)) {
      return unavailablePreview(product, className);
    }

    const bounds = LANDSCAPE_OBJECT_INTRINSIC_BOUNDS[zone];
    const previewInstance: LandscapeInstance = instance ?? {
      instance_id: `catalog-preview:${product.sku}`,
      sku: product.sku,
      variation_index: 0,
      seed: `catalog-preview:${product.sku}`,
      variation_version: 0,
      placement_version: 0,
    };

    return (
      <svg
        className={className}
        data-shop-product-thumbnail="landscape"
        width={bounds.width}
        height={bounds.height}
        viewBox={`${bounds.x} ${bounds.y} ${bounds.width} ${bounds.height}`}
        role="img"
        aria-label={`${product.display_name} 미리보기`}
        shapeRendering="crispEdges"
      >
        <LandscapeObjectSprite instance={previewInstance} product={product} />
      </svg>
    );
  }

  const slot = AVATAR_SLOT_BY_SKU[product.sku];
  if (!slot || product.avatar_slot !== slot) return unavailablePreview(product, className);

  const confirmedEquipment = equipment ?? EMPTY_AVATAR_EQUIPMENT;
  const previewEquipment: AvatarEquipment = {
    ...confirmedEquipment,
    [slot]: { ...confirmedEquipment[slot], sku: product.sku },
  };

  return (
    <AvatarSprite
      avatar={avatar}
      className={className}
      label={`${product.display_name} 미리보기`}
      equipment={previewEquipment}
    />
  );
}
