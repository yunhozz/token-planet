import { describe, expect, it } from "vitest";
import * as layout from "../planetLandscapeLayout";

type Bounds = { x: number; y: number; width: number; height: number };
type Product = { placement_zone: "ground" | "sky" };
type Point = { x: number; y: number };

const validatePlacement = (layout as unknown as {
  validatePlacement?: (product: Product, point: Point, terrain: Bounds) => boolean;
}).validatePlacement;

describe("landscape placement validation", () => {
  it("accepts only finite footprints fully inside their zone and outside walkways", () => {
    expect(validatePlacement).toBeTypeOf("function");
    if (!validatePlacement) return;

    const terrain = { x: 0, y: 0, width: 1420, height: 548 };
    expect(validatePlacement({ placement_zone: "ground" }, { x: 160, y: 200 }, terrain)).toBe(true);
    expect(validatePlacement({ placement_zone: "ground" }, { x: 1390, y: 500 }, terrain)).toBe(false);
    expect(validatePlacement({ placement_zone: "ground" }, { x: Number.NaN, y: 200 }, terrain)).toBe(false);
    expect(validatePlacement({ placement_zone: "ground" }, { x: 160, y: 140 }, terrain)).toBe(false);
    expect(validatePlacement({ placement_zone: "ground" }, { x: 50, y: 200 }, terrain)).toBe(false);
    expect(validatePlacement({ placement_zone: "sky" }, { x: 100, y: -100 }, terrain)).toBe(true);
    expect(validatePlacement({ placement_zone: "sky" }, { x: 100, y: -40 }, terrain)).toBe(false);
  });
});

import * as editing from "../landscapeEditing";
it("explains invalid input, zone and reserved walkway failures", () => {
  const failure = (editing as { placementFailure?: (product: Product, point: Point, terrain: Bounds) => string | null }).placementFailure;
  expect(failure).toBeTypeOf("function");
  if (!failure) return;
  const terrain = { x: 0, y: 0, width: 1420, height: 548 };
  expect(failure({ placement_zone: "ground" }, { x: NaN, y: 200 }, terrain)).toBe("invalid_input");
  expect(failure({ placement_zone: "ground" }, { x: 1390, y: 500 }, terrain)).toBe("outside_zone");
  expect(failure({ placement_zone: "ground" }, { x: 160, y: 140 }, terrain)).toBe("reserved_walkway");
  expect(failure({ placement_zone: "ground" }, { x: 160, y: 200 }, terrain)).toBeNull();
});
