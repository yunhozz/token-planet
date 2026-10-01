import type { PlanetObject } from "../types/usage";

export type LandscapeBounds = { x: number; y: number; width: number; height: number };
export type LandscapePlacement = {
  id: string;
  object: PlanetObject;
  x: number;
  y: number;
  bounds: LandscapeBounds;
};
export type LandscapeLayout = { bounds: LandscapeBounds; objects: LandscapePlacement[] };

export function cosmeticLandscapeBounds(terrain: LandscapeBounds, slotId: string, styleId: string): LandscapeBounds {
  if (slotId === "sky" && styleId === "meteor_shower") {
    return {
      x: terrain.x + terrain.width * 0.52,
      y: terrain.y - 201,
      width: terrain.width * 0.21,
      height: 112,
    };
  }
  if (slotId === "sky") {
    return {
      x: terrain.x + terrain.width * 0.18,
      y: terrain.y - (styleId === "star_cluster" ? 190 : 127),
      width: styleId === "star_cluster" ? 145 : 270,
      height: styleId === "star_cluster" ? 110 : 43,
    };
  }
  if (slotId === "ring") {
    const centerX = terrain.x + terrain.width - 82;
    return { x: centerX - 55, y: terrain.y - 190, width: 110, height: 112 };
  }
  if (slotId === "surface") {
    return { x: terrain.x + terrain.width - 140, y: terrain.y + 45, width: 92, height: 112 };
  }
  return { x: terrain.x + terrain.width - 98, y: terrain.y + 165, width: 130, height: 120 };
}

const EMPTY_TERRAIN_WIDTH = 1420;
const EMPTY_TERRAIN_HEIGHT = 548;
const TERRAIN_PADDING = 24;
export const LANDSCAPE_CELL_WIDTH = 56;
export const LANDSCAPE_CELL_HEIGHT = 50;
export const LANDSCAPE_CELL_X_ORIGIN = TERRAIN_PADDING + 2;
export const LANDSCAPE_CELL_Y_ORIGIN = TERRAIN_PADDING + 9;
const INITIAL_COLUMNS = 24;
const INITIAL_ROWS = 10;
const COSMETIC_RESERVED_COLUMNS = 3;
export const LANDSCAPE_WALKWAY_ROWS = [2, 8] as const;
export const LANDSCAPE_WALKWAY_END_COLUMNS = [1, 19] as const;
export const LANDSCAPE_WALKWAY_X_OFFSET = 17;
const WALKWAY_ROWS = new Set<number>(LANDSCAPE_WALKWAY_ROWS);
const WALKWAY_CONNECTOR_COLUMNS = new Set([0, 1, 19, 20]);
const SPRITE_LEFT = -4;
const SPRITE_TOP = -10;
const SPRITE_WIDTH = 36;
const SPRITE_HEIGHT = 36;
const PREFERRED_CELL_ROWS = 3;

export function landscapeObjectId(object: PlanetObject): string {
  return `${object.stage}-${object.ordinal}`;
}

function positiveModulo(value: number, divisor: number): number {
  return ((value % divisor) + divisor) % divisor;
}

function normalizedCoordinate(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(100, Math.max(0, value));
}

function preferredCell(object: PlanetObject): number {
  const x = normalizedCoordinate(object.x);
  const y = normalizedCoordinate(object.y);
  const seed = Number.isFinite(object.seed) ? Math.trunc(object.seed) : 0;
  const usableColumns = INITIAL_COLUMNS - COSMETIC_RESERVED_COLUMNS;
  const sourceColumn = Math.min(usableColumns - 1, Math.floor((x / 100) * usableColumns));
  const sourceRow = Math.min(INITIAL_ROWS - 1, Math.floor((y / 100) * INITIAL_ROWS));
  const stage = Number.isFinite(object.stage) ? Math.trunc(object.stage) : 0;
  const ordinal = Number.isFinite(object.ordinal) ? Math.max(0, Math.trunc(object.ordinal)) : 0;
  const balancedColumn = positiveModulo(ordinal * 13 + stage * 7, usableColumns);
  const balancedRow = positiveModulo(ordinal * 3 + stage * 2, INITIAL_ROWS);
  const columnOffset = positiveModulo(sourceColumn + positiveModulo(seed, PREFERRED_CELL_ROWS), PREFERRED_CELL_ROWS) - 1;
  const rowOffset = positiveModulo(sourceRow + positiveModulo(Math.trunc(seed / PREFERRED_CELL_ROWS), PREFERRED_CELL_ROWS), PREFERRED_CELL_ROWS) - 1;
  const column = positiveModulo(balancedColumn + columnOffset, usableColumns);
  const row = positiveModulo(balancedRow + rowOffset, INITIAL_ROWS);
  return row * INITIAL_COLUMNS + column;
}

function reservedCell(cell: number): boolean {
  const column = cell % INITIAL_COLUMNS;
  const row = Math.floor(cell / INITIAL_COLUMNS);
  const connectorRow = row > LANDSCAPE_WALKWAY_ROWS[0] && row < LANDSCAPE_WALKWAY_ROWS[1];
  const characterConnector = connectorRow && WALKWAY_CONNECTOR_COLUMNS.has(column);
  return WALKWAY_ROWS.has(row)
    || characterConnector
    || (column >= INITIAL_COLUMNS - COSMETIC_RESERVED_COLUMNS && row <= 6);
}

function compareObjects(left: PlanetObject, right: PlanetObject): number {
  const numericComparisons = [
    left.stage - right.stage,
    left.ordinal - right.ordinal,
    left.x - right.x,
    left.y - right.y,
    left.seed - right.seed,
  ];
  for (const comparison of numericComparisons) {
    if (Number.isFinite(comparison) && comparison !== 0) return comparison;
  }
  if (left.kind < right.kind) return -1;
  if (left.kind > right.kind) return 1;
  return 0;
}

export function layoutLandscape(objects: readonly PlanetObject[]): LandscapeLayout {
  const ordered = [...objects].sort(compareObjects);
  const occupiedCells = new Set<number>();
  const placements: LandscapePlacement[] = [];
  let maxRight = 0;
  let maxBottom = 0;

  for (const object of ordered) {
    let cell = preferredCell(object);
    while (occupiedCells.has(cell) || reservedCell(cell)) cell += 1;
    occupiedCells.add(cell);

    const column = cell % INITIAL_COLUMNS;
    const row = Math.floor(cell / INITIAL_COLUMNS);
    const x = LANDSCAPE_CELL_X_ORIGIN + column * LANDSCAPE_CELL_WIDTH;
    const y = LANDSCAPE_CELL_Y_ORIGIN + row * LANDSCAPE_CELL_HEIGHT;
    const bounds = {
      x: x + SPRITE_LEFT,
      y: y + SPRITE_TOP,
      width: SPRITE_WIDTH,
      height: SPRITE_HEIGHT,
    };
    maxRight = Math.max(maxRight, bounds.x + bounds.width);
    maxBottom = Math.max(maxBottom, bounds.y + bounds.height);
    placements.push({ id: landscapeObjectId(object), object, x, y, bounds });
  }

  return {
    bounds: {
      x: 0,
      y: 0,
      width: Math.max(EMPTY_TERRAIN_WIDTH, maxRight + TERRAIN_PADDING),
      height: Math.max(EMPTY_TERRAIN_HEIGHT, maxBottom + TERRAIN_PADDING),
    },
    objects: placements,
  };
}
