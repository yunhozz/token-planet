import type { LandscapeBounds } from "./planetLandscapeLayout";

export type AvatarRoutePoint = { x: number; y: number };
export type AvatarRouteProgress = { index: number; direction: 1 | -1 };

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

export function advanceLandscapeAvatarRoute(length: number, progress: AvatarRouteProgress): AvatarRouteProgress {
  if (length <= 1) return { index: 0, direction: progress.direction };
  const direction = progress.index === 0 ? 1 : progress.index === length - 1 ? -1 : progress.direction;
  return { index: progress.index + direction, direction };
}
