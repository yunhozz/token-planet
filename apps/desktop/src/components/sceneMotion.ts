export const WALK_POINTS = [
  { x: 94, y: 170 }, { x: 105, y: 169 }, { x: 120, y: 169 },
  { x: 135, y: 167 }, { x: 150, y: 166 }, { x: 165, y: 167 },
  { x: 180, y: 166 }, { x: 195, y: 167 }, { x: 210, y: 169 },
  { x: 225, y: 170 }, { x: 240, y: 171 },
];

function sample(random: () => number) {
  return Math.max(0, Math.min(1 - Number.EPSILON, random()));
}

export function planWalk(startIndex: number, previousDestination: number | null, random: () => number): number[] {
  const truncatedStart = Number.isFinite(startIndex) ? Math.trunc(startIndex) : 0;
  const start = Math.max(0, Math.min(WALK_POINTS.length - 1, truncatedStart));
  const directions = [-1, 1].filter((direction) => start + direction >= 0 && start + direction < WALK_POINTS.length);
  let direction = directions[Math.floor(sample(random) * directions.length)];
  let available = direction < 0 ? start : WALK_POINTS.length - 1 - start;
  let steps = 1 + Math.floor(sample(random) * Math.min(5, available));

  if (start + direction * steps === previousDestination) {
    const alternateDirections = directions.filter((candidate) => candidate !== direction);
    if (alternateDirections.length > 0) {
      direction = alternateDirections[Math.floor(sample(random) * alternateDirections.length)];
      available = direction < 0 ? start : WALK_POINTS.length - 1 - start;
      steps = Math.min(steps, available);
    } else {
      const maxSteps = Math.min(5, available);
      const alternateSteps = Array.from({ length: maxSteps }, (_, index) => index + 1)
        .filter((candidate) => candidate !== steps && start + direction * candidate !== previousDestination);
      if (alternateSteps.length > 0) {
        steps = alternateSteps[Math.floor(sample(random) * alternateSteps.length)];
      }
    }
  }

  return Array.from({ length: steps }, (_, index) => start + direction * (index + 1));
}

export function restDuration(random: () => number): number {
  return 1500 + Math.floor(sample(random) * 3501);
}

export function stepDuration(random: () => number): number {
  return 300 + Math.floor(sample(random) * 251);
}
