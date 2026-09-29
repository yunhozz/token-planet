const LEGACY_BY_NEW: Record<string, string> = {
  star_cluster_v2: "star_cluster",
  aurora_v2: "aurora",
  thin_ring_v2: "thin_ring",
  double_ring_v2: "double_ring",
  flag_v2: "flag",
  crystal_tower_v2: "crystal_tower",
};

export const SUPPORTED_SLOTS = ["sky", "ring", "surface", "forecourt"];

export const SUPPORTED_SKUS = [
  "star_cluster", "aurora", "thin_ring", "double_ring", "flag", "crystal_tower",
  "star_cluster_v2", "aurora_v2", "thin_ring_v2", "double_ring_v2", "flag_v2", "crystal_tower_v2",
  "meteor_shower", "moonlets", "flower_garden", "observatory", "pond", "lantern", "rover", "greenhouse",
];

export function styleIdForSku(sku: string): string {
  return LEGACY_BY_NEW[sku] ?? sku;
}

export function legacyEquivalent(newSku: string): string | null {
  return LEGACY_BY_NEW[newSku] ?? null;
}
