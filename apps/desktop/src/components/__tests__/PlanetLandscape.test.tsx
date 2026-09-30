import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PlanetLandscape } from "../PlanetLandscape";
import { PlanetLandscapeDecorations } from "../PlanetLandscapeDecorations";

const objects = [
  { stage: 0, ordinal: 0, kind: "water", x: 34, y: 29, seed: 14 },
  { stage: 2, ordinal: 0, kind: "house", x: 52, y: 45, seed: 39 },
] as const;

describe("planet landscape artwork", () => {
  it("shows a flat sky and ground without stretching or hiding saved objects", () => {
    const { container } = render(
      <PlanetLandscape stage={2} avatar="masculine" objects={[...objects]} equippedCosmetics={[]} />,
    );
    const svg = container.querySelector(".planet-landscape-svg");

    expect(svg).toHaveAttribute("preserveAspectRatio", "xMidYMid meet");
    expect(svg).toHaveAttribute("shape-rendering", "crispEdges");
    expect(container.querySelector("clipPath")).not.toBeInTheDocument();
    expect(container.querySelector(".planet-landscape-sky")).toBeInTheDocument();
    expect(container.querySelector(".planet-landscape-ground")).toBeInTheDocument();
    expect([...container.querySelectorAll("[data-landscape-object-id]")].map((node) => node.getAttribute("data-landscape-object-id")))
      .toEqual(["0-0", "2-0"]);
  });

  it("keeps decorative era scenery separate from actual objects on an empty planet", () => {
    const { container } = render(
      <PlanetLandscape stage={0} avatar="feminine" objects={[]} equippedCosmetics={[]} />,
    );

    expect(container.querySelector("[data-landscape-decoration='natural-stream']")).toBeInTheDocument();
    expect(container.querySelector("[data-landscape-object-id]")).not.toBeInTheDocument();
    expect(container.querySelector(".planet-landscape-avatar")).toBeInTheDocument();
  });

  it("draws each recognized equipped cosmetic in its matching landscape layer", () => {
    const { container } = render(
      <PlanetLandscape
        stage={4}
        avatar="masculine"
        objects={[{ stage: 0, ordinal: 0, kind: "rock", x: 12, y: 40, seed: 8 }]}
        equippedCosmetics={[
          { slot_id: "sky", sku: "star_cluster_v2", version: 1 },
          { slot_id: "sky", sku: "aurora_v2", version: 1 },
          { slot_id: "sky", sku: "meteor_shower", version: 1 },
          { slot_id: "ring", sku: "thin_ring_v2", version: 1 },
          { slot_id: "ring", sku: "double_ring_v2", version: 1 },
          { slot_id: "ring", sku: "moonlets", version: 1 },
          { slot_id: "surface", sku: "flag_v2", version: 1 },
          { slot_id: "surface", sku: "crystal_tower_v2", version: 1 },
          { slot_id: "surface", sku: "flower_garden", version: 1 },
          { slot_id: "surface", sku: "observatory", version: 1 },
          { slot_id: "forecourt", sku: "pond", version: 1 },
          { slot_id: "forecourt", sku: "lantern", version: 1 },
          { slot_id: "forecourt", sku: "rover", version: 1 },
          { slot_id: "forecourt", sku: "greenhouse", version: 1 },
        ]}
      />,
    );

    const cosmetics = [...container.querySelectorAll("[data-cosmetic]")];
    expect(cosmetics.map((node) => node.getAttribute("data-cosmetic"))).toEqual([
      "star_cluster", "aurora", "meteor_shower", "thin_ring", "double_ring", "moonlets",
      "flag", "crystal_tower", "flower_garden", "observatory", "pond", "lantern", "rover", "greenhouse",
    ]);
    expect(container.querySelectorAll("[data-landscape-object-id]")).toHaveLength(1);
    expect(container.querySelector("[data-cosmetic='star_cluster']")?.closest("[data-landscape-slot='sky']"))
      .not.toBeNull();
    expect(container.querySelector("[data-cosmetic='thin_ring']")?.closest("[data-landscape-slot='ring']"))
      .not.toBeNull();
    expect(container.querySelector("[data-cosmetic='pond']")?.closest("[data-landscape-slot='forecourt']"))
      .not.toBeNull();
  });

  it("keeps stars, grass, and distant silhouettes fixed in world space while the camera pans", () => {
    const bounds = { x: 0, y: 0, width: 600, height: 320 };
    const renderDecorations = (x: number) => render(
      <svg>
        <PlanetLandscapeDecorations
          stage={2}
          bounds={bounds}
          viewBox={{ x, y: -220, width: 800, height: 540 }}
          equippedCosmetics={[]}
        />
      </svg>,
    );
    const first = renderDecorations(0);
    const second = renderDecorations(145);

    const expectSharedGeometry = (selector: string, attributeNames: string[]) => {
      const geometry = (container: HTMLElement) => new Map(
        [...container.querySelectorAll(selector)].map((node) => {
          const key = node.getAttribute("x") ?? node.getAttribute("d") ?? "";
          return [key, attributeNames.map((name) => node.getAttribute(name))];
        }),
      );
      const firstGeometry = geometry(first.container);
      const secondGeometry = geometry(second.container);
      const sharedKeys = [...firstGeometry.keys()].filter((key) => secondGeometry.has(key));

      expect(sharedKeys.length).toBeGreaterThan(0);
      for (const key of sharedKeys) {
        expect(secondGeometry.get(key)).toEqual(firstGeometry.get(key));
      }
    };

    expectSharedGeometry(".planet-landscape-stars rect", ["x", "y", "width", "height"]);
    expectSharedGeometry(".planet-landscape-ground-dressing rect", ["x", "y", "width", "height"]);
    expectSharedGeometry(".planet-landscape-distant-ground path", ["d"]);

    first.unmount();
    second.unmount();
  });
});
