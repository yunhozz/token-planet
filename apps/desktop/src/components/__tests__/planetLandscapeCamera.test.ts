import { describe, expect, it } from "vitest";
import {
  clampLandscapeCamera,
  fitLandscape,
  focusLandscape,
  initialLandscapeCamera,
  landscapeViewBox,
  screenToLandscape,
} from "../planetLandscapeCamera";
import type { LandscapePoint, LandscapeViewportRect, ScreenPoint } from "../planetLandscapeCamera";
import type { LandscapeBounds } from "../planetLandscapeLayout";

const bounds: LandscapeBounds = { x: 0, y: 0, width: 900, height: 350 };

function expectFinitePositiveBox(box: LandscapeBounds) {
  expect(Object.values(box).every(Number.isFinite)).toBe(true);
  expect(box.width).toBeGreaterThan(0);
  expect(box.height).toBeGreaterThan(0);
}

function clientPointForLandscapePoint(
  point: LandscapePoint,
  viewBox: LandscapeBounds,
  rect: LandscapeViewportRect,
): ScreenPoint {
  const scale = Math.min(rect.width / viewBox.width, rect.height / viewBox.height);
  const renderedWidth = viewBox.width * scale;
  const renderedHeight = viewBox.height * scale;
  return {
    x: rect.left + (rect.width - renderedWidth) / 2 + (point.x - viewBox.x) * scale,
    y: rect.top + (rect.height - renderedHeight) / 2 + (point.y - viewBox.y) * scale,
  };
}

describe("planet landscape camera", () => {
  it.each([
    { width: 1200, height: 420 },
    { width: 600, height: 600 },
    { width: 360, height: 500 },
  ])("initially shows the entire tall landscape in a $width×$height viewport", (viewport) => {
    const terrain = { x: 0, y: -220, width: 2400, height: 820 };
    const camera = initialLandscapeCamera(terrain);
    const viewBox = landscapeViewBox(terrain, viewport, camera);

    expect(camera).toEqual({ centerX: 1200, centerY: 190, zoom: 1 });
    expect(viewBox.x).toBeLessThanOrEqual(0);
    expect(viewBox.y).toBeLessThanOrEqual(-220);
    expect(viewBox.x + viewBox.width).toBeGreaterThanOrEqual(2400);
    expect(viewBox.y + viewBox.height).toBeGreaterThanOrEqual(600);
    expect(viewBox.width / viewBox.height).toBeCloseTo(viewport.width / viewport.height);
  });

  it("initializes invalid landscape bounds with finite default geometry", () => {
    expect(initialLandscapeCamera({ x: Number.NaN, y: Infinity, width: 0, height: Number.NaN }))
      .toEqual({ centerX: 300, centerY: 160, zoom: 1 });
  });

  it("maps pointer coordinates through the actual panned and zoomed viewBox", () => {
    const terrain = { x: 0, y: 0, width: 1200, height: 600 };
    const viewport = { width: 1200, height: 600 };
    const camera = clampLandscapeCamera(terrain, viewport, {
      centerX: 700,
      centerY: 300,
      zoom: 2,
    });
    const viewBox = landscapeViewBox(terrain, viewport, camera);
    const rect = { left: 30, top: 50, width: 1200, height: 600 };

    expect(viewBox).toEqual({ x: 400, y: 150, width: 600, height: 300 });
    expect(screenToLandscape({ x: 330, y: 200 }, rect, viewBox)).toEqual({ x: 550, y: 225 });
  });

  it("accounts for horizontal and vertical SVG letterboxing without clamping outside points", () => {
    const viewBox = { x: 0, y: 10, width: 600, height: 300 };
    const wideRect = { left: 10, top: 20, width: 1200, height: 400 };
    const horizontalBarPoint = screenToLandscape({ x: 110, y: 220 }, wideRect, viewBox);
    expect(horizontalBarPoint).toEqual({ x: -75, y: 160 });
    expect(horizontalBarPoint?.x).toBeLessThan(viewBox.x);

    const tallRect = { left: 10, top: 20, width: 400, height: 1200 };
    const verticalBarPoint = screenToLandscape({ x: 210, y: 470 }, tallRect, viewBox);
    expect(verticalBarPoint).toEqual({ x: 300, y: -65 });
    expect(verticalBarPoint?.y).toBeLessThan(viewBox.y);
  });

  it("uses the SVG DOMRect in CSS pixels regardless of device pixel ratio", () => {
    const viewBox = { x: 100, y: 50, width: 800, height: 400 };
    const rect = { left: 25, top: 35, width: 400, height: 200 };

    expect(screenToLandscape({ x: 225, y: 135 }, rect, viewBox)).toEqual({ x: 500, y: 250 });
  });

  it("preserves viewBox edges and maps pointer positions beyond the viewport", () => {
    const viewBox = { x: -5, y: 7, width: 100, height: 50 };
    const rect = { left: 100, top: 200, width: 200, height: 100 };

    expect(screenToLandscape({ x: 100, y: 200 }, rect, viewBox)).toEqual({ x: -5, y: 7 });
    expect(screenToLandscape({ x: 300, y: 300 }, rect, viewBox)).toEqual({ x: 95, y: 57 });
    expect(screenToLandscape({ x: 320, y: 340 }, rect, viewBox)).toEqual({ x: 105, y: 77 });
  });

  it("returns null when finite geometry overflows during conversion", () => {
    const point = { x: Number.MAX_VALUE, y: 0 };
    const rect = { left: 0, top: 0, width: Number.MIN_VALUE, height: Number.MIN_VALUE };
    const viewBox = { x: 0, y: 0, width: 1, height: 1 };

    expect([point.x, point.y, rect.left, rect.top, rect.width, rect.height, ...Object.values(viewBox)]
      .every(Number.isFinite)).toBe(true);
    expect(screenToLandscape(point, rect, viewBox)).toBeNull();
  });

  it("round trips landscape points through preserveAspectRatio meet geometry", () => {
    const viewBox = { x: -300, y: 80, width: 600, height: 400 };
    const rect = { left: 20, top: 70, width: 900, height: 500 };
    const points: LandscapePoint[] = [
      { x: -300, y: 80 },
      { x: 120, y: 260 },
      { x: 300, y: 480 },
    ];

    for (const point of points) {
      const clientPoint = clientPointForLandscapePoint(point, viewBox, rect);
      const roundTrip = screenToLandscape(clientPoint, rect, viewBox);

      expect(roundTrip?.x).toBeCloseTo(point.x, 10);
      expect(roundTrip?.y).toBeCloseTo(point.y, 10);
    }
  });

  it.each([
    ["client x", { x: Number.NaN, y: 1 }, { left: 0, top: 0, width: 100, height: 100 }, { x: 0, y: 0, width: 100, height: 100 }],
    ["client y", { x: 1, y: Number.POSITIVE_INFINITY }, { left: 0, top: 0, width: 100, height: 100 }, { x: 0, y: 0, width: 100, height: 100 }],
    ["rect left", { x: 1, y: 1 }, { left: Number.NaN, top: 0, width: 100, height: 100 }, { x: 0, y: 0, width: 100, height: 100 }],
    ["rect top", { x: 1, y: 1 }, { left: 0, top: Number.NEGATIVE_INFINITY, width: 100, height: 100 }, { x: 0, y: 0, width: 100, height: 100 }],
    ["rect width", { x: 1, y: 1 }, { left: 0, top: 0, width: 0, height: 100 }, { x: 0, y: 0, width: 100, height: 100 }],
    ["rect height", { x: 1, y: 1 }, { left: 0, top: 0, width: 100, height: -1 }, { x: 0, y: 0, width: 100, height: 100 }],
    ["viewBox x", { x: 1, y: 1 }, { left: 0, top: 0, width: 100, height: 100 }, { x: Number.POSITIVE_INFINITY, y: 0, width: 100, height: 100 }],
    ["viewBox y", { x: 1, y: 1 }, { left: 0, top: 0, width: 100, height: 100 }, { x: 0, y: Number.NaN, width: 100, height: 100 }],
    ["viewBox width", { x: 1, y: 1 }, { left: 0, top: 0, width: 100, height: 100 }, { x: 0, y: 0, width: 0, height: 100 }],
    ["viewBox height", { x: 1, y: 1 }, { left: 0, top: 0, width: 100, height: 100 }, { x: 0, y: 0, width: 100, height: -1 }],
  ] as Array<[string, ScreenPoint, LandscapeViewportRect, LandscapeBounds]>)
  ("returns null for invalid %s geometry", (_name, point, rect, viewBox) => {
    expect(screenToLandscape(point, rect, viewBox)).toBeNull();
  });

  it.each([
    { width: 1200, height: 400 },
    { width: 600, height: 600 },
    { width: 360, height: 500 },
  ])("fits the full terrain without stretching in a $width×$height viewport", (viewport) => {
    const viewBox = landscapeViewBox(bounds, viewport, fitLandscape(bounds));

    expect(viewBox.width / viewport.width).toBeCloseTo(viewBox.height / viewport.height);
    expect(viewBox.x).toBeLessThanOrEqual(bounds.x);
    expect(viewBox.y).toBeLessThanOrEqual(bounds.y);
    expect(viewBox.x + viewBox.width).toBeGreaterThanOrEqual(bounds.x + bounds.width);
    expect(viewBox.y + viewBox.height).toBeGreaterThanOrEqual(bounds.y + bounds.height);
  });

  it("clamps an explored camera inside the terrain and re-clamps it after resize", () => {
    const viewport = { width: 1200, height: 400 };
    const zoomed = clampLandscapeCamera(bounds, viewport, { ...fitLandscape(bounds), zoom: 4 });
    const resized = { width: 360, height: 500 };
    const camera = clampLandscapeCamera(bounds, resized, {
      ...zoomed,
      centerX: -500,
      centerY: 900,
    });
    const viewBox = landscapeViewBox(bounds, resized, camera);

    expect(viewBox.x).toBeGreaterThanOrEqual(bounds.x);
    expect(viewBox.y).toBeGreaterThanOrEqual(bounds.y);
    expect(viewBox.x + viewBox.width).toBeLessThanOrEqual(bounds.x + bounds.width);
    expect(viewBox.y + viewBox.height).toBeLessThanOrEqual(bounds.y + bounds.height);
  });

  it("keeps the largest sprite legible at the camera zoom limit", () => {
    const viewport = { width: 1200, height: 400 };
    const camera = clampLandscapeCamera(bounds, viewport, {
      centerX: 450,
      centerY: 175,
      zoom: Number.MAX_VALUE,
    });
    const viewBox = landscapeViewBox(bounds, viewport, camera);
    const pixelsPerWorldUnit = viewport.width / viewBox.width;

    expect(camera.zoom).toBeGreaterThan(1);
    expect(pixelsPerWorldUnit * 24).toBeLessThanOrEqual(Math.min(viewport.width, viewport.height) * 0.65 + 0.01);
  });

  it("centers a selected object while preserving the existing magnification", () => {
    const viewport = { width: 1200, height: 400 };
    const target = { x: 330, y: 110, width: 24, height: 24 };
    const camera = focusLandscape(bounds, viewport, { ...fitLandscape(bounds), zoom: 2 }, target);
    const viewBox = landscapeViewBox(bounds, viewport, camera);

    expect(camera.zoom).toBe(2);
    expect(camera.centerX).toBe(target.x + target.width / 2);
    expect(camera.centerY).toBe(target.y + target.height / 2);
    expect(target.x).toBeGreaterThan(viewBox.x);
    expect(target.y).toBeGreaterThan(viewBox.y);
    expect(target.x + target.width).toBeLessThan(viewBox.x + viewBox.width);
    expect(target.y + target.height).toBeLessThan(viewBox.y + viewBox.height);
  });

  it("uses finite camera geometry until a measured viewport is available", () => {
    const camera = clampLandscapeCamera(
      { x: Number.NaN, y: Number.POSITIVE_INFINITY, width: 0, height: Number.NaN },
      { width: 0, height: Number.NaN },
      { centerX: Number.NaN, centerY: Number.POSITIVE_INFINITY, zoom: Number.NaN },
    );
    const viewBox = landscapeViewBox(
      { x: Number.NaN, y: Number.POSITIVE_INFINITY, width: 0, height: Number.NaN },
      { width: 0, height: Number.NaN },
      camera,
    );

    expectFinitePositiveBox(viewBox);
    expect(Object.values(camera).every(Number.isFinite)).toBe(true);
  });
});
