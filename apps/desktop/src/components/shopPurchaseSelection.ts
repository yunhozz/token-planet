import type { LandscapeInstance, ShopActionResult } from "../types/usage";

export type LandscapePurchaseBaseline = Readonly<{
  requestId: string;
  accountId: string;
  cycleId: string;
  sku: string;
  instanceIds: readonly string[];
}>;

export function resolvePurchasedLandscapeInstance(
  baseline: LandscapePurchaseBaseline,
  result: ShopActionResult,
): LandscapeInstance | null {
  if (result.status !== "purchased" || result.request_id !== baseline.requestId
    || result.state.account_id !== baseline.accountId || result.state.current_cycle_id !== baseline.cycleId) return null;
  const existing = new Set(baseline.instanceIds);
  const candidates = result.state.landscape_instances.filter((instance) => instance.sku === baseline.sku && !existing.has(instance.instance_id));
  return candidates.length === 1 ? candidates[0] : null;
}
