import type { LandscapeBounds } from "./planetLandscapeLayout";

export type AvatarRoutePoint = { x: number; y: number };

function stops(origin: number, span: number, gap: number): number[] {
  const intervals = Math.ceil(span / gap);
  return intervals === 0 ? [origin] : Array.from({ length: intervals + 1 }, (_, index) => origin + span * index / intervals);
}

/** Positions are the top-left corners of the 24×30 avatar plus its maximum 3px downward walking bob. */
export function landscapeAvatarRoute(bounds: LandscapeBounds): AvatarRoutePoint[] {
  if (![bounds.x, bounds.y, bounds.width, bounds.height, bounds.x + bounds.width, bounds.y + bounds.height].every(Number.isFinite)
    || bounds.width < 24 || bounds.height < 33) return [];
  const xs = stops(bounds.x, bounds.width - 24, 48);
  const ys = stops(bounds.y, bounds.height - 33, 30);
  return ys.flatMap((y, row) => (row % 2 ? [...xs].reverse() : xs).map((x) => ({ x, y })));
}

/** Choose a neighboring grid point; do not retrace the last step if another neighbor exists. */
export function nextLandscapeAvatarPoint(
  points: readonly AvatarRoutePoint[],
  currentIndex: number,
  previousIndex: number | null,
  random: () => number = Math.random,
): number {
  if (!Number.isInteger(currentIndex) || !points[currentIndex]) return 0;
  const current = points[currentIndex];
  const neighbors = points.flatMap((point, index) => index !== currentIndex
    && Math.abs(point.x - current.x) <= 48 + 1e-9
    && Math.abs(point.y - current.y) <= 30 + 1e-9 ? [index] : []);
  const alternatives = neighbors.filter(index => index !== previousIndex);
  const candidates = alternatives.length ? alternatives : neighbors;
  if (!candidates.length) return currentIndex;
  const value = random();
  const fraction = Number.isFinite(value) ? Math.max(0, Math.min(1 - Number.EPSILON, value)) : 0;
  return candidates[Math.floor(fraction * candidates.length)];
}
