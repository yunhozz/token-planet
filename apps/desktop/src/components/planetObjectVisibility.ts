import type { PlanetObject } from "../types/usage";

const HIDDEN_GROWTH_KINDS = new Set(["road", "path", "rail", "fern"]);

export function isPlanetObjectVisible(object: Pick<PlanetObject, "kind">): boolean {
  return !HIDDEN_GROWTH_KINDS.has(object.kind);
}
