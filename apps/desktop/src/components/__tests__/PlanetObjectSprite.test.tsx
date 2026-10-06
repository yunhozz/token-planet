import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PlanetObjectSprite } from "../PlanetObjectSprite";

describe("growth object artwork", () => {
  it("distinguishes nature, buildings and transport by geometry rather than palette", () => {
    const kinds = ["tree", "fern", "camp", "cottage", "house", "market", "well", "workshop", "plaza", "district", "power", "factory", "tower", "laboratory", "habitat", "solar", "satellite", "rocket", "crops", "path", "road", "rail"];
    const shapes = kinds.map(kind => renderToStaticMarkup(<svg><PlanetObjectSprite object={{kind, stage: 2, ordinal: 0, seed: 0, x: 0, y: 0}} x={0} y={0} scale={1}/></svg>).replace(/(?:fill|class|style)="[^"]*"/g, ""));
    expect(new Set(shapes).size).toBe(kinds.length);
  });
});

it.each(["tree", "rock", "water", "creature"])("shades %s with distinct material highlights and shadows", kind => {
  const markup = renderToStaticMarkup(<svg><PlanetObjectSprite object={{kind, stage: 0, ordinal: 0, seed: 0, x: 0, y: 0}} x={0} y={0} scale={1}/></svg>);
  const root = new DOMParser().parseFromString(markup, "image/svg+xml");
  const colors = new Set(Array.from(root.querySelectorAll("[fill]")).map(part => part.getAttribute("fill")));
  expect(colors.size).toBeGreaterThan(3);
});
