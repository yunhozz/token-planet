import type { LandscapeBounds } from "./planetLandscapeLayout";
import type { PlacementZone, ShopProduct } from "../types/usage";

export type LandscapePoint = { x: number; y: number };

const FOOTPRINT_WIDTH: Record<PlacementZone, number> = { ground: 64, sky: 96 };
const FOOTPRINT_HEIGHT = 64;
const SKY_HEIGHT = 220;
function finiteBounds(bounds: LandscapeBounds): boolean {
  return [bounds.x, bounds.y, bounds.width, bounds.height].every(Number.isFinite)
    && bounds.width > 0
    && bounds.height > 0
    && Number.isFinite(bounds.x + bounds.width)
    && Number.isFinite(bounds.y + bounds.height);
}

/** Validate the complete fixed sprite footprint; point is its top-left corner. */
export type PlacementFailure = "invalid_input" | "outside_zone";

export function placementFailure(
  product: Pick<ShopProduct, "placement_zone">,
  point: LandscapePoint,
  terrain: LandscapeBounds,
): PlacementFailure | null {
  const zone = product.placement_zone;
  if (!zone || !finiteBounds(terrain) || !Number.isFinite(point.x) || !Number.isFinite(point.y)) return "invalid_input";

  const footprint = {
    x: point.x,
    y: point.y,
    width: FOOTPRINT_WIDTH[zone],
    height: FOOTPRINT_HEIGHT,
  };
  const right = terrain.x + terrain.width;
  const bottom = terrain.y + terrain.height;
  const inHorizontalBounds = footprint.x >= terrain.x && footprint.x + footprint.width <= right;
  const inZone = zone === "ground"
    ? inHorizontalBounds && footprint.y >= terrain.y && footprint.y + footprint.height <= bottom
    : inHorizontalBounds
      && footprint.y >= terrain.y - SKY_HEIGHT
      && footprint.y + footprint.height <= terrain.y;

  if (!inZone) return "outside_zone";
  return null;
}

export function validatePlacement(
  product: Pick<ShopProduct, "placement_zone">,
  point: LandscapePoint,
  terrain: LandscapeBounds,
): boolean {
  return placementFailure(product, point, terrain) === null;
}
