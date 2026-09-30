import { describe, expect, it } from "vitest";
import {
  clampLandscapeCamera,
  fitLandscape,
  focusLandscape,
  landscapeViewBox,
  zoomLandscape,
} from "../planetLandscapeCamera";
import type { LandscapeBounds } from "../planetLandscapeLayout";

const bounds: LandscapeBounds = { x: 0, y: 0, width: 900, height: 350 };

function expectFinitePositiveBox(box: LandscapeBounds) {
  expect(Object.values(box).every(Number.isFinite)).toBe(true);
  expect(box.width).toBeGreaterThan(0);
  expect(box.height).toBeGreaterThan(0);
}

describe("planet landscape camera", () => {
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
    const zoomed = zoomLandscape(bounds, viewport, fitLandscape(bounds), 4);
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

  it("centers and enlarges a selected object while keeping its full footprint visible", () => {
    const viewport = { width: 1200, height: 400 };
    const target = { x: 330, y: 110, width: 24, height: 24 };
    const camera = focusLandscape(bounds, viewport, fitLandscape(bounds), target);
    const viewBox = landscapeViewBox(bounds, viewport, camera);

    expect(camera.zoom).toBeGreaterThan(1);
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
