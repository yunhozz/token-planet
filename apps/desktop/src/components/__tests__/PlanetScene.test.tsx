import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PlanetScene } from "../PlanetScene";

describe("planet cosmetics", () => {
  it("draws supported slot and SKU pairs at every planet stage", () => {
    for (const stage of [0, 1, 2, 3, 4]) {
      const { container, unmount } = render(
        <PlanetScene
          stage={stage}
          progress={0.5}
          equippedCosmetics={[
            { slot_id: "sky", sku: "star_cluster" },
            { slot_id: "sky", sku: "aurora" },
            { slot_id: "ring", sku: "thin_ring" },
            { slot_id: "ring", sku: "double_ring" },
            { slot_id: "surface", sku: "flag" },
            { slot_id: "surface", sku: "crystal_tower" },
            { slot_id: "sky", sku: "future_item" },
            { slot_id: "future_slot", sku: "star_cluster" },
          ]}
        />,
      );

      expect([...container.querySelectorAll("[data-cosmetic]")].map((node) => node.getAttribute("data-cosmetic")))
        .toEqual(["thin_ring", "double_ring", "star_cluster", "aurora", "flag", "crystal_tower"]);
      unmount();
    }
  });
});
