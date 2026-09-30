import { describe, expect, it } from "vitest";
import type { PlanetObject } from "../../types/usage";
import { layoutLandscape } from "../planetLandscapeLayout";

function collocatedObjects(count: number): PlanetObject[] {
  return Array.from({ length: count }, (_, ordinal) => ({
    stage: ordinal < 60 ? 0 : 1,
    ordinal: ordinal % 60,
    kind: "house",
    x: 50,
    y: 50,
    seed: ordinal,
  }));
}

describe("planet landscape layout", () => {
  it("gives an empty planet finite terrain bounds", () => {
    const result = layoutLandscape([]);

    expect(result.objects).toEqual([]);
    expect(result.bounds.width).toBeGreaterThan(0);
    expect(result.bounds.height).toBeGreaterThan(0);
    expect(Object.values(result.bounds).every(Number.isFinite)).toBe(true);
  });

  it("keeps every collocated object in separated bounds inside the terrain", () => {
    const objects = collocatedObjects(120);
    const result = layoutLandscape(objects);

    expect(result.objects).toHaveLength(120);
    expect(new Set(result.objects.map((placement) => placement.id)).size).toBe(120);

    for (let index = 0; index < result.objects.length; index += 1) {
      const current = result.objects[index].bounds;
      expect(current.x).toBeGreaterThanOrEqual(result.bounds.x);
      expect(current.y).toBeGreaterThanOrEqual(result.bounds.y);
      expect(current.x + current.width).toBeLessThanOrEqual(result.bounds.x + result.bounds.width);
      expect(current.y + current.height).toBeLessThanOrEqual(result.bounds.y + result.bounds.height);

      for (const next of result.objects.slice(index + 1)) {
        const other = next.bounds;
        const overlaps = current.x < other.x + other.width
          && current.x + current.width > other.x
          && current.y < other.y + other.height
          && current.y + current.height > other.y;
        expect(overlaps).toBe(false);
      }
    }
  });

  it("assigns stable IDs and placements independent of input order without mutating input", () => {
    const objects = [
      { stage: 1, ordinal: 0, kind: "tree", x: 22, y: 43, seed: 33 },
      { stage: 0, ordinal: 1, kind: "rock", x: 22, y: 43, seed: 22 },
      { stage: 0, ordinal: 0, kind: "water", x: 22, y: 43, seed: 11 },
    ] satisfies PlanetObject[];
    const before = JSON.stringify(objects);

    const result = layoutLandscape(objects);

    expect(result.objects.map((placement) => placement.id)).toEqual(["0-0", "0-1", "1-0"]);
    expect(layoutLandscape([...objects].reverse())).toEqual(result);
    expect(JSON.stringify(objects)).toBe(before);
  });

  it("keeps existing positions when a later generated object is appended", () => {
    const existing = [
      { stage: 0, ordinal: 0, kind: "rock", x: 50, y: 50, seed: 10 },
      { stage: 0, ordinal: 1, kind: "tree", x: 50, y: 50, seed: 11 },
      { stage: 1, ordinal: 0, kind: "cottage", x: 50, y: 50, seed: 20 },
    ] satisfies PlanetObject[];
    const before = layoutLandscape(existing);
    const after = layoutLandscape([
      ...existing,
      { stage: 2, ordinal: 0, kind: "house", x: 50, y: 50, seed: 30 },
    ]);

    for (const placement of before.objects) {
      expect(after.objects.find((candidate) => candidate.id === placement.id)).toEqual(placement);
    }
  });
});
