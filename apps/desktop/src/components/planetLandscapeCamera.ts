import type { LandscapeBounds } from "./planetLandscapeLayout";

export type LandscapeViewport = { width: number; height: number };
export type LandscapeCamera = { centerX: number; centerY: number; zoom: number };
export type ScreenPoint = { x: number; y: number };
export type LandscapePoint = { x: number; y: number };
export type LandscapeViewportRect = Pick<DOMRect, "left" | "top" | "width" | "height">;

const DEFAULT_BOUNDS: LandscapeBounds = { x: 0, y: 0, width: 600, height: 320 };
const DEFAULT_VIEWPORT: LandscapeViewport = { width: 1200, height: 420 };
const MIN_ZOOM = 1;
const SPRITE_FOOTPRINT = 36;
const MAX_SPRITE_VIEW_FRACTION = 0.65;
const FOCUS_TARGET_FRACTION = 0.45;

function finiteOr(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback;
}

function normalizeBounds(bounds: LandscapeBounds, fallback: LandscapeBounds = DEFAULT_BOUNDS): LandscapeBounds {
  const width = Number.isFinite(bounds.width) && bounds.width > 0 ? bounds.width : fallback.width;
  const height = Number.isFinite(bounds.height) && bounds.height > 0 ? bounds.height : fallback.height;
  return {
    x: finiteOr(bounds.x, fallback.x),
    y: finiteOr(bounds.y, fallback.y),
    width,
    height,
  };
}

function normalizeViewport(viewport: LandscapeViewport): LandscapeViewport {
  return {
    width: Number.isFinite(viewport.width) && viewport.width > 0 ? viewport.width : DEFAULT_VIEWPORT.width,
    height: Number.isFinite(viewport.height) && viewport.height > 0 ? viewport.height : DEFAULT_VIEWPORT.height,
  };
}

function fitSize(bounds: LandscapeBounds, viewport: LandscapeViewport) {
  const aspectRatio = viewport.width / viewport.height;
  const width = Math.max(bounds.width, bounds.height * aspectRatio);
  const height = width / aspectRatio;
  return { width, height };
}

function maximumZoom(bounds: LandscapeBounds, viewport: LandscapeViewport) {
  const fit = fitSize(bounds, viewport);
  const pixelsPerWorldUnit = viewport.width / fit.width;
  const largestSpritePixelsAtFit = SPRITE_FOOTPRINT * pixelsPerWorldUnit;
  return Math.max(
    MIN_ZOOM,
    (Math.min(viewport.width, viewport.height) * MAX_SPRITE_VIEW_FRACTION) / largestSpritePixelsAtFit,
  );
}

function normalizedCamera(camera: LandscapeCamera, bounds: LandscapeBounds): LandscapeCamera {
  return {
    centerX: finiteOr(camera.centerX, bounds.x + bounds.width / 2),
    centerY: finiteOr(camera.centerY, bounds.y + bounds.height / 2),
    zoom: Number.isFinite(camera.zoom) && camera.zoom > 0 ? camera.zoom : MIN_ZOOM,
  };
}

function clampCenter(center: number, minimum: number, size: number, viewSize: number): number {
  if (viewSize >= size) return minimum + size / 2;
  return Math.max(minimum + viewSize / 2, Math.min(minimum + size - viewSize / 2, center));
}

export function screenToLandscape(
  point: ScreenPoint,
  rect: LandscapeViewportRect,
  viewBox: LandscapeBounds,
): LandscapePoint | null {
  const values = [
    point.x,
    point.y,
    rect.left,
    rect.top,
    rect.width,
    rect.height,
    viewBox.x,
    viewBox.y,
    viewBox.width,
    viewBox.height,
  ];

  if (!values.every(Number.isFinite) || rect.width <= 0 || rect.height <= 0 || viewBox.width <= 0 || viewBox.height <= 0) {
    return null;
  }

  const scale = Math.min(rect.width / viewBox.width, rect.height / viewBox.height);
  if (!Number.isFinite(scale) || scale <= 0) return null;

  const renderedWidth = viewBox.width * scale;
  const renderedHeight = viewBox.height * scale;
  const contentLeft = rect.left + (rect.width - renderedWidth) / 2;
  const contentTop = rect.top + (rect.height - renderedHeight) / 2;
  const landscapePoint = {
    x: viewBox.x + (point.x - contentLeft) / scale,
    y: viewBox.y + (point.y - contentTop) / scale,
  };

  return Number.isFinite(landscapePoint.x) && Number.isFinite(landscapePoint.y) ? landscapePoint : null;
}

export function fitLandscape(bounds: LandscapeBounds): LandscapeCamera {
  const safeBounds = normalizeBounds(bounds);
  return {
    centerX: safeBounds.x + safeBounds.width / 2,
    centerY: safeBounds.y + safeBounds.height / 2,
    zoom: MIN_ZOOM,
  };
}

export function initialLandscapeCamera(bounds: LandscapeBounds): LandscapeCamera {
  const safeBounds = normalizeBounds(bounds);
  const fit = fitSize(safeBounds, DEFAULT_VIEWPORT);
  const readableViewHeight = 500;
  return clampLandscapeCamera(safeBounds, DEFAULT_VIEWPORT, {
    centerX: safeBounds.x + safeBounds.width / 2,
    centerY: Math.min(safeBounds.y + safeBounds.height / 2, 185),
    zoom: Math.max(MIN_ZOOM, fit.height / readableViewHeight),
  });
}

export function clampLandscapeCamera(
  bounds: LandscapeBounds,
  viewport: LandscapeViewport,
  camera: LandscapeCamera,
): LandscapeCamera {
  const safeBounds = normalizeBounds(bounds);
  const safeViewport = normalizeViewport(viewport);
  const current = normalizedCamera(camera, safeBounds);
  const zoom = Math.max(MIN_ZOOM, Math.min(maximumZoom(safeBounds, safeViewport), current.zoom));
  const fit = fitSize(safeBounds, safeViewport);
  const viewWidth = fit.width / zoom;
  const viewHeight = fit.height / zoom;

  return {
    centerX: clampCenter(current.centerX, safeBounds.x, safeBounds.width, viewWidth),
    centerY: clampCenter(current.centerY, safeBounds.y, safeBounds.height, viewHeight),
    zoom,
  };
}

export function landscapeViewBox(
  bounds: LandscapeBounds,
  viewport: LandscapeViewport,
  camera: LandscapeCamera,
): LandscapeBounds {
  const safeBounds = normalizeBounds(bounds);
  const safeViewport = normalizeViewport(viewport);
  const clampedCamera = clampLandscapeCamera(safeBounds, safeViewport, camera);
  const fit = fitSize(safeBounds, safeViewport);
  const width = fit.width / clampedCamera.zoom;
  const height = fit.height / clampedCamera.zoom;

  return {
    x: clampedCamera.centerX - width / 2,
    y: clampedCamera.centerY - height / 2,
    width,
    height,
  };
}

export function zoomLandscape(
  bounds: LandscapeBounds,
  viewport: LandscapeViewport,
  camera: LandscapeCamera,
  factor: number,
): LandscapeCamera {
  const current = normalizedCamera(camera, normalizeBounds(bounds));
  const safeFactor = Number.isFinite(factor) && factor > 0 ? factor : 1;
  return clampLandscapeCamera(bounds, viewport, { ...current, zoom: current.zoom * safeFactor });
}

export function focusLandscape(
  bounds: LandscapeBounds,
  viewport: LandscapeViewport,
  camera: LandscapeCamera,
  target: LandscapeBounds,
): LandscapeCamera {
  const safeBounds = normalizeBounds(bounds);
  const safeViewport = normalizeViewport(viewport);
  const safeTarget = normalizeBounds(target, safeBounds);
  const fit = fitSize(safeBounds, safeViewport);
  const zoom = Math.min(
    (fit.width * FOCUS_TARGET_FRACTION) / safeTarget.width,
    (fit.height * FOCUS_TARGET_FRACTION) / safeTarget.height,
  );

  return clampLandscapeCamera(safeBounds, safeViewport, {
    ...normalizedCamera(camera, safeBounds),
    centerX: safeTarget.x + safeTarget.width / 2,
    centerY: safeTarget.y + safeTarget.height / 2,
    zoom: Math.max(MIN_ZOOM, zoom),
  });
}
