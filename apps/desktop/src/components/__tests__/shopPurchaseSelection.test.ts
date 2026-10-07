import { describe, expect, it } from "vitest";
import * as selection from "../shopPurchaseSelection";
import type { ShopActionResult, LandscapeInstance } from "../../types/usage";
const instance = (id: string, sku = "land_pond"): LandscapeInstance => ({ instance_id: id, sku, variation_index: 2, variation_version: 1, seed: id, placement_version: 0 });
const baseline = { requestId: "request", accountId: "local", cycleId: "cycle", sku: "land_pond", instanceIds: ["old"] };
const result = (instances: LandscapeInstance[], overrides = {}): ShopActionResult => ({ status: "purchased", request_id: "request", confirmed_quote: null, state: { account_id: "local", current_cycle_id: "cycle", landscape_instances: instances }, ...overrides } as ShopActionResult);
const resolve = (selection as { resolvePurchasedLandscapeInstance?: (purchaseBaseline: typeof baseline, result: ShopActionResult) => LandscapeInstance | null }).resolvePurchasedLandscapeInstance;
describe("purchase instance selection", () => {
  it("selects exactly one new same SKU instance rather than ordering or variation", () => {
    expect(resolve).toBeTypeOf("function");
    if (!resolve) return;
    expect(resolve(baseline, result([instance("new"), instance("old"), instance("other", "land_tree")]))).toEqual(instance("new"));
  });
  it("rejects ambiguous candidates and mismatched request, account, cycle or status", () => {
    expect(resolve).toBeTypeOf("function");
    if (!resolve) return;
    for (const response of [result([instance("old")]), result([instance("a"), instance("b")]), result([instance("new", "land_tree")]), result([instance("new")], { request_id: "different" }), result([instance("new")], { status: "unavailable" }), result([instance("new")], { state: { account_id: "other", current_cycle_id: "cycle", landscape_instances: [instance("new")] } }), result([instance("new")], { state: { account_id: "local", current_cycle_id: "other", landscape_instances: [instance("new")] } })]) expect(resolve(baseline, response)).toBeNull();
  });
});
