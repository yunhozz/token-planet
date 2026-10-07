import type { LandscapeBounds } from "./planetLandscapeLayout";
import type { PlacementZone, ShopProduct } from "../types/usage";

export type LandscapePoint = { x: number; y: number };

const FOOTPRINT_WIDTH: Record<PlacementZone, number> = { ground: 64, sky: 96 };
const FOOTPRINT_HEIGHT = 64;
const SKY_HEIGHT = 220;
const CELL_WIDTH = 56;
const CELL_HEIGHT = 50;
const CELL_X_ORIGIN = 26;
const CELL_Y_ORIGIN = 33;

function finiteBounds(bounds: LandscapeBounds): boolean {
  return [bounds.x, bounds.y, bounds.width, bounds.height].every(Number.isFinite)
    && bounds.width > 0
    && bounds.height > 0
    && Number.isFinite(bounds.x + bounds.width)
    && Number.isFinite(bounds.y + bounds.height);
}

function intersects(
  left: { x: number; y: number; width: number; height: number },
  right: { x: number; y: number; width: number; height: number },
): boolean {
  return left.x < right.x + right.width
    && left.x + left.width > right.x
    && left.y < right.y + right.height
    && left.y + left.height > right.y;
}

function reservedCell(row: number, column: number): boolean {
  return row === 2 || row === 8
    || ((row >= 3 && row < 8) && [0, 1, 19, 20].includes(column));
}

function crossesReservedWalkway(footprint: { x: number; y: number; width: number; height: number }): boolean {
  for (let row = 0; row < 10; row += 1) {
    for (let column = 0; column < 24; column += 1) {
      if (!reservedCell(row, column)) continue;
      const cell = {
        x: CELL_X_ORIGIN + column * CELL_WIDTH,
        y: CELL_Y_ORIGIN + row * CELL_HEIGHT,
        width: CELL_WIDTH,
        height: CELL_HEIGHT,
      };
      if (intersects(footprint, cell)) return true;
    }
  }
  return false;
}

/** Validate the complete fixed sprite footprint; point is its top-left corner. */
export type PlacementFailure = "invalid_input" | "outside_zone" | "reserved_walkway";

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
  return zone === "ground" && crossesReservedWalkway(footprint) ? "reserved_walkway" : null;
}

export function validatePlacement(
  product: Pick<ShopProduct, "placement_zone">,
  point: LandscapePoint,
  terrain: LandscapeBounds,
): boolean {
  return placementFailure(product, point, terrain) === null;
}
