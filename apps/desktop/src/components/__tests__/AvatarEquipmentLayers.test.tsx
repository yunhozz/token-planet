import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AvatarSprite } from "../AvatarSprite";
import type { AvatarEquipment, AvatarSlot } from "../../types/usage";

const AVATAR_PRODUCTS: ReadonlyArray<{ sku: string; slot: AvatarSlot }> = [
  { sku: "avatar_explorer_hat", slot: "head" },
  { sku: "avatar_crown", slot: "head" },
  { sku: "avatar_space_helmet", slot: "head" },
  { sku: "avatar_halo", slot: "head" },
  { sku: "avatar_workwear", slot: "outfit" },
  { sku: "avatar_labwear", slot: "outfit" },
  { sku: "avatar_spacesuit", slot: "outfit" },
  { sku: "avatar_nebula_suit", slot: "outfit" },
  { sku: "avatar_glasses", slot: "face" },
  { sku: "avatar_sunglasses", slot: "face" },
  { sku: "avatar_goggles", slot: "face" },
  { sku: "avatar_hud", slot: "face" },
  { sku: "avatar_backpack", slot: "back" },
  { sku: "avatar_cape", slot: "back" },
  { sku: "avatar_jetpack", slot: "back" },
  { sku: "avatar_wings", slot: "back" },
];

function emptyEquipment(): AvatarEquipment {
  return {
    head: { sku: null, version: 0 },
    outfit: { sku: null, version: 0 },
    face: { sku: null, version: 0 },
    back: { sku: null, version: 0 },
  };
}

function equipmentWith(slot: AvatarSlot, sku: string): AvatarEquipment {
  return { ...emptyEquipment(), [slot]: { sku, version: 1 } };
}

function markupFor(avatar: "masculine" | "feminine", equipment?: AvatarEquipment, extras = {}) {
  return renderToStaticMarkup(createElement(AvatarSprite, {
    avatar,
    className: "test-avatar",
    label: "Test avatar",
    ...(equipment ? { equipment } : {}),
    ...extras,
  }));
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

function equipmentBounds(markup: string, sku: string): SvgBounds {
  const start = markup.indexOf(`data-avatar-equipment="${sku}"`);
  const openEnd = markup.indexOf(">", start);
  const closeStart = markup.indexOf("</g>", openEnd);
  const equipmentMarkup = markup.slice(openEnd + 1, closeStart);
  const bounds = emptyBounds();
  const attr = (attributes: string, name: string) => attributes.match(new RegExp(`(?:^|\\s)${name}="([^"]*)"`))?.[1];
  const number = (attributes: string, name: string, fallback = 0) => Number(attr(attributes, name) ?? fallback);

  for (const [, tag, attributes] of equipmentMarkup.matchAll(/<(rect|circle|ellipse|path|polygon|polyline)\b([^>]*?)\/?\s*>/g)) {
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

    const strokeWidth = attr(attributes, "stroke") && attr(attributes, "stroke") !== "none"
      ? number(attributes, "stroke-width", 1) / 2
      : 0;
    bounds.minX = Math.min(bounds.minX, primitiveBounds.minX - strokeWidth);
    bounds.minY = Math.min(bounds.minY, primitiveBounds.minY - strokeWidth);
    bounds.maxX = Math.max(bounds.maxX, primitiveBounds.maxX + strokeWidth);
    bounds.maxY = Math.max(bounds.maxY, primitiveBounds.maxY + strokeWidth);
  }

  if (markup.includes('transform="translate(16 0) scale(-1 1)"')) {
    return { ...bounds, minX: 16 - bounds.maxX, maxX: 16 - bounds.minX };
  }
  return bounds;
}

describe("AvatarEquipmentLayers", () => {
  it("renders every avatar SKU over both existing base appearances", () => {
    for (const avatar of ["masculine", "feminine"] as const) {
      for (const product of AVATAR_PRODUCTS) {
        const markup = markupFor(avatar, equipmentWith(product.slot, product.sku));
        expect(markup, `${product.sku} on ${avatar}`).toContain(`data-avatar-equipment="${product.sku}"`);
        const art = markup.match(new RegExp(`<g data-avatar-equipment="${product.sku}"[\\s\\S]*?</g>`))?.[0] ?? "";
        const visiblePrimitives = art.match(/<(?:rect|path|circle|ellipse|polygon|polyline)\b/g) ?? [];
        expect(visiblePrimitives.length, `${product.sku} should draw actual accessory geometry`).toBeGreaterThanOrEqual(2);
      }
    }
  });

  it.each(AVATAR_PRODUCTS)("keeps $sku geometry and strokes inside the 16×20 avatar viewport", ({ sku, slot }) => {
    for (const avatar of ["masculine", "feminine"] as const) {
      for (const facing of ["right", "left"] as const) {
        const markup = markupFor(avatar, equipmentWith(slot, sku), { facing });
        const bounds = equipmentBounds(markup, sku);
        expect(bounds.minX, `${sku} ${facing} left edge`).toBeGreaterThanOrEqual(0);
        expect(bounds.minY, `${sku} ${facing} top edge`).toBeGreaterThanOrEqual(0);
        expect(bounds.maxX, `${sku} ${facing} right edge`).toBeLessThanOrEqual(16);
        expect(bounds.maxY, `${sku} ${facing} bottom edge`).toBeLessThanOrEqual(20);
      }
    }
  });

  it("orders back equipment behind the base and keeps outfit, face, and head in front", () => {
    const allEquipment: AvatarEquipment = {
      head: { sku: "avatar_space_helmet", version: 1 },
      outfit: { sku: "avatar_spacesuit", version: 1 },
      face: { sku: "avatar_hud", version: 1 },
      back: { sku: "avatar_jetpack", version: 1 },
    };
    const markup = markupFor("masculine", allEquipment);
    const order = ["back", "base", "outfit", "face", "head"]
      .map((layer) => markup.indexOf(`data-avatar-layer="${layer}"`));

    expect(order.every((index) => index >= 0)).toBe(true);
    expect(order).toEqual([...order].sort((left, right) => left - right));
    expect(markup).toContain("viewBox=\"0 0 16 20\"");
  });

  it("keeps default rendering, blinking, and facing classes when no equipment is supplied", () => {
    const normal = markupFor("masculine");
    const walkingLeftWithClosedEyes = markupFor("feminine", undefined, {
      facing: "left",
      walking: true,
      eyesClosed: true,
    });

    expect(normal).toContain("avatar-sprite test-avatar");
    expect(normal).toContain('fill="#4c9b9a"');
    expect(normal).toContain('fill="#f3c995"');
    expect(normal).not.toContain("data-avatar-equipment");
    expect(walkingLeftWithClosedEyes).toContain("avatar-sprite--facing-left");
    expect(walkingLeftWithClosedEyes).toContain("avatar-sprite--walking");
    expect(walkingLeftWithClosedEyes).toContain('transform="translate(16 0) scale(-1 1)"');
    expect(walkingLeftWithClosedEyes).toContain('<rect x="5" y="8" width="2" height="1"');
  });

  it("composes head, face, outfit, and back items simultaneously while mirroring them", () => {
    const allEquipment: AvatarEquipment = {
      head: { sku: "avatar_crown", version: 1 },
      outfit: { sku: "avatar_workwear", version: 1 },
      face: { sku: "avatar_glasses", version: 1 },
      back: { sku: "avatar_backpack", version: 1 },
    };
    const markup = markupFor("feminine", allEquipment, { facing: "left" });

    for (const { sku } of AVATAR_PRODUCTS.filter(({ sku }) => [
      "avatar_crown", "avatar_workwear", "avatar_glasses", "avatar_backpack",
    ].includes(sku))) {
      expect(markup).toContain(`data-avatar-equipment="${sku}"`);
    }
    expect(markup).toContain('transform="translate(16 0) scale(-1 1)"');
  });
});
