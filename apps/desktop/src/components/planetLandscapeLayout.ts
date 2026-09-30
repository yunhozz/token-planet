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

const EMPTY_TERRAIN_WIDTH = 600;
const EMPTY_TERRAIN_HEIGHT = 320;
const TERRAIN_PADDING = 24;
const CELL_WIDTH = 36;
const CELL_HEIGHT = 36;
const INITIAL_COLUMNS = 12;
const INITIAL_ROWS = 8;
const SPRITE_LEFT = -2;
const SPRITE_TOP = -6;
const SPRITE_WIDTH = 24;
const SPRITE_HEIGHT = 24;
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
  const sourceColumn = Math.min(INITIAL_COLUMNS - 1, Math.floor((x / 100) * INITIAL_COLUMNS));
  const sourceRow = Math.min(INITIAL_ROWS - 1, Math.floor((y / 100) * INITIAL_ROWS));
  const seedColumnOffset = positiveModulo(seed, PREFERRED_CELL_ROWS) - 1;
  const seedRowOffset = positiveModulo(Math.trunc(seed / PREFERRED_CELL_ROWS), PREFERRED_CELL_ROWS) - 1;
  const column = positiveModulo(sourceColumn + seedColumnOffset, INITIAL_COLUMNS);
  const row = positiveModulo(sourceRow + seedRowOffset, INITIAL_ROWS);
  return row * INITIAL_COLUMNS + column;
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
    while (occupiedCells.has(cell)) cell += 1;
    occupiedCells.add(cell);

    const column = cell % INITIAL_COLUMNS;
    const row = Math.floor(cell / INITIAL_COLUMNS);
    const x = TERRAIN_PADDING + 2 + column * CELL_WIDTH;
    const y = TERRAIN_PADDING + 6 + row * CELL_HEIGHT;
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
