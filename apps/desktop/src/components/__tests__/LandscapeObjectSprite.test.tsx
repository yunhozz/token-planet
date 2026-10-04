import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  LANDSCAPE_OBJECT_INTRINSIC_BOUNDS,
  LandscapeObjectSprite,
} from "../LandscapeObjectSprite";
import type { LandscapeInstance, ShopProduct } from "../../types/usage";

const LANDSCAPE_SKUS = [
  "land_pond", "land_well", "land_greenhouse", "land_reservoir",
  "land_crystal", "land_school", "land_observatory", "land_laboratory",
  "land_market", "land_trading_post", "land_freight", "land_bazaar",
  "land_rover", "land_clocktower", "land_launchpad", "land_portal",
  "land_toolbox", "land_excavator", "land_cutter", "land_recycler",
  "land_flag", "land_thin_ring", "land_double_ring", "land_moonlets",
  "land_lantern", "land_stars", "land_aurora", "land_meteors",
  "land_garden", "land_tree", "land_bench", "land_fountain",
] as const;

const SKY_SKUS = new Set([
  "land_stars", "land_aurora", "land_meteors",
  "land_thin_ring", "land_double_ring", "land_moonlets",
]);

function markupFor(sku: string, variationIndex: number) {
  const product: ShopProduct = {
    sku,
    category: "landscape",
    display_name: sku,
    price: 1,
    catalog_revision: 1,
    purchasable: true,
    placement_zone: SKY_SKUS.has(sku as typeof LANDSCAPE_SKUS[number]) ? "sky" : "ground",
    avatar_slot: null,
    effect_type: null,
    effect_value: 0,
  };
  const instance: LandscapeInstance = {
    instance_id: `${sku}-${variationIndex}`,
    sku,
    variation_index: variationIndex,
    seed: `stable-${variationIndex}`,
    variation_version: 1,
    placement_version: 1,
  };
  return renderToStaticMarkup(createElement("svg", null,
    createElement(LandscapeObjectSprite, { instance, product }),
  ));
}

function artMarkup(markup: string, sku: string) {
  const start = markup.indexOf(`data-landscape-art="${sku}"`);
  expect(start).toBeGreaterThanOrEqual(0);
  const openEnd = markup.indexOf(">", start);
  const closeStart = markup.indexOf("</g>", openEnd);
  return markup.slice(openEnd + 1, closeStart);
}

function geometrySignature(markup: string, sku: string) {
  const start = markup.indexOf(`data-landscape-art="${sku}"`);
  const openEnd = markup.indexOf(">", start);
  const closeStart = markup.indexOf("</g>", openEnd);
  return markup.slice(start, closeStart + 4)
    .replace(/\sdata-landscape-art="[^"]*"/g, "")
    .replace(/\sdata-landscape-zone="[^"]*"/g, "")
    .replace(/\sdata-landscape-variation="[^"]*"/g, "")
    .replace(/\sfill="[^"]*"/g, "")
    .replace(/\sstroke="[^"]*"/g, "");
}

type SvgBounds = { minX: number; minY: number; maxX: number; maxY: number };

function emptyBounds(): SvgBounds {
  return { minX: Number.POSITIVE_INFINITY, minY: Number.POSITIVE_INFINITY, maxX: Number.NEGATIVE_INFINITY, maxY: Number.NEGATIVE_INFINITY };
}

function includePoint(bounds: SvgBounds, x: number, y: number) {
  bounds.minX = Math.min(bounds.minX, x);
  bounds.minY = Math.min(bounds.minY, y);
  bounds.maxX = Math.max(bounds.maxX, x);
  bounds.maxY = Math.max(bounds.maxY, y);
}

function pathBounds(d: string): SvgBounds {
  const tokens = d.match(/[A-Za-z]|[-+]?(?:\d*\.)?\d+(?:e[-+]?\d+)?/gi) ?? [];
  const bounds = emptyBounds();
  let index = 0;
  let command = "";
  let x = 0;
  let y = 0;
  let moveX = 0;
  let moveY = 0;
  const isCommand = (token: string | undefined) => Boolean(token && /^[A-Za-z]$/.test(token));
  const next = () => {
    const token = tokens[index++];
    if (!token || isCommand(token)) throw new Error(`Unsupported SVG path data: ${d}`);
    return Number(token);
  };

  while (index < tokens.length) {
    if (isCommand(tokens[index])) command = tokens[index++];
    if (command === "Z") {
      x = moveX;
      y = moveY;
      includePoint(bounds, x, y);
      command = "";
      continue;
    }
    if (command === "M" || command === "L") {
      let firstMove = command === "M";
      while (index < tokens.length && !isCommand(tokens[index])) {
        x = next();
        y = next();
        includePoint(bounds, x, y);
        if (firstMove) {
          moveX = x;
          moveY = y;
          firstMove = false;
        }
      }
      if (command === "M") command = "L";
      continue;
    }
    if (command === "H") {
      while (index < tokens.length && !isCommand(tokens[index])) {
        x = next();
        includePoint(bounds, x, y);
      }
      continue;
    }
    if (command === "V") {
      while (index < tokens.length && !isCommand(tokens[index])) {
        y = next();
        includePoint(bounds, x, y);
      }
      continue;
    }
    if (command === "A") {
      while (index < tokens.length && !isCommand(tokens[index])) {
        const rx = next();
        const ry = next();
        const rotation = next();
        const largeArc = next();
        const sweep = next();
        const endX = next();
        const endY = next();
        if (rotation !== 0 || largeArc !== 0 || y !== endY || Math.abs(Math.abs(endX - x) - 2 * rx) > 0.001) {
          throw new Error(`Bounds helper does not support this arc: ${d}`);
        }
        const half = Math.min(ry, Math.abs(endX - x) / 2);
        includePoint(bounds, x, y);
        includePoint(bounds, endX, endY);
        includePoint(bounds, (x + endX) / 2, y + (sweep === 1 ? half : -half));
        x = endX;
        y = endY;
      }
      continue;
    }
    throw new Error(`Bounds helper does not support SVG path command ${command}: ${d}`);
  }
  return bounds;
}

function transformedArtBounds(markup: string, sku: string, width: number): SvgBounds {
  const start = markup.indexOf(`data-landscape-art="${sku}"`);
  const openEnd = markup.indexOf(">", start);
  const closeStart = markup.indexOf("</g>", openEnd);
  const artMarkup = markup.slice(openEnd + 1, closeStart);
  const transform = markup.slice(start, openEnd).match(/transform="translate\(([-\d.]+) ([-\d.]+)\) scale\(([-\d.]+) ([-\d.]+)\)/);
  if (!transform) throw new Error(`Missing fixed variation transform for ${sku}`);
  const centerX = Number(transform[1]);
  const centerY = Number(transform[2]);
  const scaleX = Number(transform[3]);
  const scaleY = Number(transform[4]);
  const bounds = emptyBounds();
  const attr = (attributes: string, name: string) => attributes.match(new RegExp(`(?:^|\\s)${name}="([^"]*)"`))?.[1];
  const number = (attributes: string, name: string, fallback = 0) => Number(attr(attributes, name) ?? fallback);

  for (const [, tag, attributes] of artMarkup.matchAll(/<(rect|circle|ellipse|path|polygon|polyline)\b([^>]*?)\/?\s*>/g)) {
    let primitiveBounds: SvgBounds;
    if (tag === "rect") {
      const x = number(attributes, "x");
      const y = number(attributes, "y");
      primitiveBounds = { minX: x, minY: y, maxX: x + number(attributes, "width"), maxY: y + number(attributes, "height") };
    } else if (tag === "circle" || tag === "ellipse") {
      const cx = number(attributes, "cx");
      const cy = number(attributes, "cy");
      const rx = number(attributes, tag === "circle" ? "r" : "rx");
      const ry = number(attributes, tag === "circle" ? "r" : "ry");
      primitiveBounds = { minX: cx - rx, minY: cy - ry, maxX: cx + rx, maxY: cy + ry };
    } else if (tag === "path") {
      primitiveBounds = pathBounds(attr(attributes, "d") ?? "");
    } else {
      const points = (attr(attributes, "points") ?? "").trim().split(/[ ,]+/).map(Number);
      primitiveBounds = emptyBounds();
      for (let point = 0; point + 1 < points.length; point += 2) includePoint(primitiveBounds, points[point], points[point + 1]);
    }

    const stroke = attr(attributes, "stroke");
    const strokeWidth = stroke && stroke !== "none" ? number(attributes, "stroke-width", 1) / 2 : 0;
    const minX = primitiveBounds.minX - strokeWidth;
    const minY = primitiveBounds.minY - strokeWidth;
    const maxX = primitiveBounds.maxX + strokeWidth;
    const maxY = primitiveBounds.maxY + strokeWidth;
    const x0 = centerX + (minX - centerX) * scaleX;
    const x1 = centerX + (maxX - centerX) * scaleX;
    const y0 = centerY + (minY - centerY) * scaleY;
    const y1 = centerY + (maxY - centerY) * scaleY;
    bounds.minX = Math.min(bounds.minX, x0);
    bounds.minY = Math.min(bounds.minY, y0);
    bounds.maxX = Math.max(bounds.maxX, x1);
    bounds.maxY = Math.max(bounds.maxY, y1);
  }

  expect(bounds.minX).not.toBe(Number.POSITIVE_INFINITY);
  expect(centerX * 2).toBe(width);
  return bounds;
}

describe("LandscapeObjectSprite", () => {
  it("draws all 32 catalog SKUs as distinct pixel silhouettes inside their placement zone", () => {
    const silhouettes = new Set<string>();

    for (const sku of LANDSCAPE_SKUS) {
      const markup = markupFor(sku, 2);
      const art = artMarkup(markup, sku);
      const primitives = art.match(/<(?:rect|path|circle|ellipse|polygon|polyline)\b/g) ?? [];
      expect(primitives.length, `${sku} should contain visible vector art`).toBeGreaterThanOrEqual(3);
      silhouettes.add(geometrySignature(markup, sku));
    }

    expect(silhouettes.size).toBe(32);
    expect(LANDSCAPE_OBJECT_INTRINSIC_BOUNDS.ground).toEqual({ x: 0, y: 0, width: 64, height: 64 });
    expect(LANDSCAPE_OBJECT_INTRINSIC_BOUNDS.sky).toEqual({ x: 0, y: 0, width: 96, height: 64 });
    expect([...SKY_SKUS]).toHaveLength(6);
    expect(LANDSCAPE_SKUS.filter((sku) => !SKY_SKUS.has(sku))).toHaveLength(26);
  });

  it("keeps rendered primitives and transformed strokes inside each exact placement footprint", () => {
    for (const sku of LANDSCAPE_SKUS) {
      const zone = SKY_SKUS.has(sku) ? "sky" : "ground";
      const bounds = LANDSCAPE_OBJECT_INTRINSIC_BOUNDS[zone];
      for (const variationIndex of [0, 1, 2, 3, 4]) {
        const markup = markupFor(sku, variationIndex);
        expect(markup).toContain(`data-landscape-bounds="${bounds.width}x${bounds.height}"`);
        expect(markup).toContain(`data-landscape-zone="${zone}"`);
        const rendered = transformedArtBounds(markup, sku, bounds.width);
        expect(rendered.minX, `${sku} variation ${variationIndex} left`).toBeGreaterThanOrEqual(bounds.x);
        expect(rendered.minY, `${sku} variation ${variationIndex} top`).toBeGreaterThanOrEqual(bounds.y);
        expect(rendered.maxX, `${sku} variation ${variationIndex} right`).toBeLessThanOrEqual(bounds.x + bounds.width);
        expect(rendered.maxY, `${sku} variation ${variationIndex} bottom`).toBeLessThanOrEqual(bounds.y + bounds.height);
      }
    }
  });

  it.each(LANDSCAPE_SKUS)("keeps five saved variants of %s visibly distinct without randomness", (sku) => {
    const renders = [0, 1, 2, 3, 4].map((index) => markupFor(sku, index));
    const shapeVariants = new Set(renders.map((markup) => geometrySignature(markup, sku)));
    const palettes = new Set(renders.map((markup) => [...markup.matchAll(/\sfill="([^"]+)"/g)].map((match) => match[1]).join("|")));

    expect(shapeVariants.size).toBe(5);
    expect(palettes.size).toBe(5);
    expect(renders[0]).toContain('data-landscape-variation="0"');
    expect(renders[4]).toContain('data-landscape-variation="4"');
  });

  it("renders a stored variation identically when its seed is unchanged", () => {
    const first = markupFor("land_fountain", 3);
    const second = markupFor("land_fountain", 3);
    expect(first).toBe(second);
  });
});
