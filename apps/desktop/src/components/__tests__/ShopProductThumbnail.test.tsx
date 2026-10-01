import { render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it } from "vitest";
import { ShopProductThumbnail } from "../ShopProductThumbnail";
import type { AvatarEquipment, AvatarSlot, LandscapeInstance, PlacementZone, PlanetAvatar, ShopProduct } from "../../types/usage";

const LANDSCAPE_PRODUCTS: Array<[string, string, PlacementZone]> = [
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

const AVATAR_PRODUCTS: Array<[string, string, AvatarSlot]> = [
  ["avatar_explorer_hat", "탐험가 모자", "head"], ["avatar_crown", "왕관", "head"],
  ["avatar_space_helmet", "우주 헬멧", "head"], ["avatar_halo", "홀로그램 관", "head"],
  ["avatar_workwear", "작업복", "outfit"], ["avatar_labwear", "연구복", "outfit"],
  ["avatar_spacesuit", "우주복", "outfit"], ["avatar_nebula_suit", "성운 의상", "outfit"],
  ["avatar_glasses", "안경", "face"], ["avatar_sunglasses", "선글라스", "face"],
  ["avatar_goggles", "고글", "face"], ["avatar_hud", "HUD 바이저", "face"],
  ["avatar_backpack", "배낭", "back"], ["avatar_cape", "망토", "back"],
  ["avatar_jetpack", "제트팩", "back"], ["avatar_wings", "에너지 날개", "back"],
];

const LANDSCAPE_EFFECTS = [
  "token_earning", "civilization_growth", "shop_discount", "reset_cooldown",
  "natural_removal_discount", "era_reward", "streak_reward", "civilization_growth",
] as const;
const LANDSCAPE_PRICES = [5_000_000, 15_000_000, 40_000_000, 100_000_000];
const AVATAR_PRICES = [100_000_000, 200_000_000, 350_000_000, 500_000_000];

function landscapeProduct([sku, display_name, placement_zone]: typeof LANDSCAPE_PRODUCTS[number], index: number): ShopProduct {
  return {
    sku,
    category: "landscape",
    display_name,
    price: LANDSCAPE_PRICES[index % 4],
    catalog_revision: 1,
    purchasable: true,
    placement_zone,
    avatar_slot: null,
    effect_type: LANDSCAPE_EFFECTS[Math.floor(index / 4)],
    effect_value: 100,
  };
}

function avatarProduct([sku, display_name, avatar_slot]: typeof AVATAR_PRODUCTS[number], index: number): ShopProduct {
  return {
    sku,
    category: "avatar",
    display_name,
    price: AVATAR_PRICES[index % 4],
    catalog_revision: 1,
    purchasable: true,
    placement_zone: null,
    avatar_slot,
    effect_type: null,
    effect_value: 0,
  };
}

const LANDSCAPE_CATALOG = LANDSCAPE_PRODUCTS.map(landscapeProduct);
const AVATAR_CATALOG = AVATAR_PRODUCTS.map(avatarProduct);
const CONFIRMED_EQUIPMENT: AvatarEquipment = {
  head: { sku: "avatar_crown", version: 2 },
  outfit: { sku: "avatar_workwear", version: 4 },
  face: { sku: "avatar_glasses", version: 6 },
  back: { sku: "avatar_backpack", version: 8 },
};

function thumbnail(product: ShopProduct, props: Partial<ComponentProps<typeof ShopProductThumbnail>> = {}) {
  return render(<ShopProductThumbnail product={product} {...props} />);
}

describe("ShopProductThumbnail", () => {
  it("renders distinct art for all 32 landscape products in their canonical zones", () => {
    const { container } = render(
      <div>{LANDSCAPE_CATALOG.map((product) => <ShopProductThumbnail key={product.sku} product={product} />)}</div>,
    );
    const images = screen.getAllByRole("img");
    const landscapeArt = Array.from(container.querySelectorAll("[data-landscape-art]"));
    const thumbnails = Array.from(container.querySelectorAll("svg"));

    expect(images).toHaveLength(32);
    expect(images.map((image) => image.getAttribute("aria-label"))).toEqual(
      LANDSCAPE_CATALOG.map((product) => `${product.display_name} 미리보기`),
    );
    expect(landscapeArt.map((art) => art.getAttribute("data-landscape-art"))).toEqual(
      LANDSCAPE_CATALOG.map((product) => product.sku),
    );
    expect(landscapeArt.map((art) => art.getAttribute("data-landscape-zone"))).toEqual(
      LANDSCAPE_PRODUCTS.map(([, , zone]) => zone),
    );
    expect(thumbnails.map((svg) => svg.getAttribute("viewBox"))).toEqual(
      LANDSCAPE_PRODUCTS.map(([, , zone]) => zone === "sky" ? "0 0 96 64" : "0 0 64 64"),
    );
    expect(thumbnails.every((svg) => svg.getAttribute("shape-rendering") === "crispEdges")).toBe(true);
    expect(container.querySelectorAll("[data-thumbnail-unavailable]")).toHaveLength(0);
  });

  it("uses a saved instance seed and variation instead of catalog defaults", () => {
    const product = LANDSCAPE_CATALOG[28];
    const instance: LandscapeInstance = {
      instance_id: "landscape-instance-28",
      sku: product.sku,
      variation_index: 4,
      seed: "persistent-seed-28",
      variation_version: 3,
      placement_version: 7,
    };
    const { container } = thumbnail(product, { instance });
    const art = container.querySelector("[data-landscape-art]");
    const renderedInstance = container.querySelector("[data-landscape-object]");

    expect(art).toHaveAttribute("data-landscape-variation", "4");
    expect(renderedInstance).toHaveAttribute("data-landscape-seed", "persistent-seed-28");
    expect(renderedInstance).toHaveAttribute("data-landscape-variation-version", "3");
  });

  it("uses stable variation zero and a deterministic string seed for catalog previews", () => {
    const product = LANDSCAPE_CATALOG[20];
    const { container, rerender } = thumbnail(product);
    const firstRender = container.innerHTML;
    const renderedInstance = container.querySelector("[data-landscape-object]");

    expect(container.querySelector("[data-landscape-art]")).toHaveAttribute("data-landscape-variation", "0");
    expect(renderedInstance).toHaveAttribute("data-landscape-seed", `catalog-preview:${product.sku}`);

    rerender(<ShopProductThumbnail product={product} />);

    expect(container.innerHTML).toBe(firstRender);
  });

  it.each(["masculine", "feminine"] as const)("previews every avatar product on the %s base without mutating equipment", (avatar: PlanetAvatar) => {
    const initialEquipment = structuredClone(CONFIRMED_EQUIPMENT);

    for (const product of AVATAR_CATALOG) {
      const { container, unmount } = thumbnail(product, { avatar, equipment: CONFIRMED_EQUIPMENT });
      const image = screen.getByRole("img", { name: `${product.display_name} 미리보기` });
      const svg = container.querySelector("svg");
      const previewedEquipment = product.avatar_slot
        ? svg?.querySelector(`[data-avatar-layer="${product.avatar_slot}"] [data-avatar-equipment]`)
        : null;

      expect(image).toBeInTheDocument();
      expect(svg).toHaveAttribute("viewBox", "0 0 16 20");
      expect(svg).toHaveAttribute("shape-rendering", "crispEdges");
      expect(previewedEquipment).toHaveAttribute("data-avatar-equipment", product.sku);
      expect(previewedEquipment).toHaveAttribute("data-avatar-slot", product.avatar_slot);
      expect(svg?.querySelector("[data-avatar-style]")).toHaveAttribute("data-avatar-style", avatar);
      expect(CONFIRMED_EQUIPMENT).toEqual(initialEquipment);
      unmount();
    }
  });

  it("defaults to the existing masculine avatar base", () => {
    const { container } = thumbnail(AVATAR_CATALOG[0]);

    expect(container.querySelector("[data-avatar-style]")).toHaveAttribute("data-avatar-style", "masculine");
  });

  it("preserves the confirmed items in the other avatar slots when previewing one item", () => {
    const product = AVATAR_CATALOG[2];
    const { container } = thumbnail(product, { equipment: CONFIRMED_EQUIPMENT });
    const svg = container.querySelector("svg");

    expect(svg?.querySelector('[data-avatar-layer="head"] [data-avatar-equipment="avatar_space_helmet"]')).toBeInTheDocument();
    expect(svg?.querySelector('[data-avatar-layer="outfit"] [data-avatar-equipment="avatar_workwear"]')).toBeInTheDocument();
    expect(svg?.querySelector('[data-avatar-layer="face"] [data-avatar-equipment="avatar_glasses"]')).toBeInTheDocument();
    expect(svg?.querySelector('[data-avatar-layer="back"] [data-avatar-equipment="avatar_backpack"]')).toBeInTheDocument();
  });

  it("shows an accessible unavailable preview for unknown products", () => {
    const unknownProduct: ShopProduct = {
      sku: "unknown_landscape",
      category: "landscape",
      display_name: "미등록 장식",
      price: 1_000_000,
      catalog_revision: 9,
      purchasable: false,
      placement_zone: "ground",
      avatar_slot: null,
      effect_type: null,
      effect_value: 0,
    };
    const { container } = thumbnail(unknownProduct, { className: "catalog-preview" });

    expect(screen.getByRole("img", { name: "미등록 장식 미리보기" })).toHaveTextContent("미리보기를 제공할 수 없습니다.");
    expect(container.querySelector("svg")).not.toBeInTheDocument();
    expect(container.querySelector("[data-thumbnail-unavailable]")).toHaveClass("catalog-preview");
  });
});
