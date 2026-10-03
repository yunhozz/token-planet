import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useShopActions } from "../useShopActions";
import type { ShopActionsContext, ShopActionsTransport } from "../useShopActions";
import type { ShopActionResult, ShopQuote, ShopRequest, ShopState } from "../../types/usage";

const BASE_CONTEXT: ShopActionsContext = {
  account_id: "local",
  current_cycle_id: "cycle-1",
  is_guest: true,
  online: false,
  actions_available: true,
};

function context(overrides: Partial<ShopActionsContext> = {}): ShopActionsContext {
  return { ...BASE_CONTEXT, ...overrides };
}

function shopState(
  account_id = BASE_CONTEXT.account_id,
  current_cycle_id = BASE_CONTEXT.current_cycle_id,
  available_balance = 100,
): ShopState {
  return {
    account_id,
    current_cycle_id,
    catalog_revision: 1,
    state_revision: 1,
    available_balance,
    products: [],
    landscape_instances: [],
    placements: [],
    removed_natural_keys: [],
    avatar_owned_skus: [],
    avatar_equipment: {
      head: { sku: null, version: 0 },
      outfit: { sku: null, version: 0 },
      face: { sku: null, version: 0 },
      back: { sku: null, version: 0 },
    },
    effects: {
      token_earning_bps: 0,
      civilization_growth_bps: 0,
      shop_discount_bps: 0,
      reset_cooldown_bps: 0,
      natural_removal_discount_bps: 0,
      era_reward_tokens: 0,
      streak_reward_tokens: 0,
    },
    reward_state: {
      reward_timezone: "UTC",
      settled_cycle_tokens: 0,
      era_reward_tokens: 0,
      streak_reward_tokens: 0,
    },
    action_unavailable_reason: null,
    guest_import_pending: false,
    guest_import_error: null,
  };
}

const oldQuote: ShopQuote = {
  target: { kind: "purchase", sku: "land_pond" },
  catalog_revision: 1,
  effect_revision: 1,
  price: 25,
};

function purchaseRequest(request_id = "request-1", quote = oldQuote): ShopRequest {
  return { kind: "purchase", request_id, quote };
}

function actionResult(
  request_id: string,
  state = shopState(),
  status: ShopActionResult["status"] = "purchased",
  confirmed_quote: ShopQuote | null = null,
): ShopActionResult {
  return { status, request_id, confirmed_quote, state };
}

function transport(overrides: Partial<ShopActionsTransport> = {}): ShopActionsTransport {
  return {
    refresh: vi.fn(async () => shopState()),
    quote: vi.fn(async () => oldQuote),
    apply: vi.fn(async (request) => actionResult(request.request_id)),
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("useShopActions", () => {
  it("loads matching canonical state and keeps the last state when refresh fails", async () => {
    const saved = shopState("local", "cycle-1", 123);
    const refresh = vi.fn()
      .mockResolvedValueOnce(saved)
      .mockRejectedValueOnce(new Error("offline"));
    const { result } = renderHook(() => useShopActions(context(), transport({ refresh })));

    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.state).toEqual(saved);

    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.state).toEqual(saved);
    expect(result.current.error).toContain("다시 시도");
  });

  it.each([
    ["account", context({ account_id: "account:other", is_guest: false, online: true })],
    ["cycle", context({ current_cycle_id: "cycle-2" })],
  ])("ignores a late refresh after the %s context changes", async (_change, nextContext) => {
    const pending = deferred<ShopState>();
    const api = transport({ refresh: vi.fn(() => pending.promise) });
    const { result, rerender } = renderHook(
      ({ currentContext }) => useShopActions(currentContext, api),
      { initialProps: { currentContext: context() } },
    );
    const oldPreviewKey = result.current.previewResetKey;
    let refreshPromise!: Promise<ShopState | null>;
    act(() => {
      refreshPromise = result.current.refresh();
    });

    rerender({ currentContext: nextContext });
    expect(result.current.state).toBeNull();
    expect(result.current.previewResetKey).not.toBe(oldPreviewKey);

    await act(async () => {
      pending.resolve(shopState("local", "cycle-1", 999));
      await refreshPromise;
    });
    expect(result.current.state).toBeNull();
  });

  it("does not update state after unmount while refresh is pending", async () => {
    const pending = deferred<ShopState>();
    const api = transport({ refresh: vi.fn(() => pending.promise) });
    const { result, unmount } = renderHook(() => useShopActions(context(), api));
    let refreshPromise!: Promise<ShopState | null>;
    act(() => {
      refreshPromise = result.current.refresh();
    });

    unmount();
    await act(async () => {
      pending.resolve(shopState("local", "cycle-1", 999));
      await refreshPromise;
    });
    expect(api.refresh).toHaveBeenCalledTimes(1);
  });

  it("returns a server-cycle mismatch for an explicit context decision without adopting it", async () => {
    const newerCycle = shopState("local", "cycle-2", 300);
    const api = transport({ refresh: vi.fn(async () => newerCycle) });
    const { result } = renderHook(() => useShopActions(context(), api));
    let returned!: ShopState | null;

    await act(async () => {
      returned = await result.current.refresh();
    });

    expect(returned).toEqual(newerCycle);
    expect(result.current.state).toBeNull();
    expect(result.current.error).toContain("주기");
  });

  it("does not return foreign-account refresh data and preserves the last matching cache", async () => {
    const cached = shopState("local", "cycle-1", 123);
    const foreign = shopState("account:other", "cycle-1", 999);
    const refresh = vi.fn()
      .mockResolvedValueOnce(cached)
      .mockResolvedValueOnce(foreign);
    const { result } = renderHook(() => useShopActions(context(), transport({ refresh })));

    await act(async () => {
      await result.current.refresh();
    });
    let returned!: ShopState | null;
    await act(async () => {
      returned = await result.current.refresh();
    });

    expect(returned).toBeNull();
    expect(result.current.state).toEqual(cached);
    expect(result.current.error).toContain("계정");
  });

  it("ignores a refresh started before a completed apply, even at an equal state revision", async () => {
    const cached = { ...shopState("local", "cycle-1", 123), state_revision: 7 };
    const stale = deferred<ShopState>();
    const refresh = vi.fn()
      .mockResolvedValueOnce(cached)
      .mockImplementationOnce(() => stale.promise);
    const api = transport({
      refresh,
      apply: vi.fn(async (request) => actionResult(
        request.request_id,
        { ...shopState("local", "cycle-1", 70), state_revision: 8 },
      )),
    });
    const { result } = renderHook(() => useShopActions(context(), api));

    await act(async () => {
      await result.current.refresh();
    });
    let staleRefresh!: Promise<ShopState | null>;
    act(() => {
      staleRefresh = result.current.refresh();
    });
    await act(async () => {
      await result.current.apply(purchaseRequest("apply-after-refresh"));
    });
    const confirmed = result.current.state;

    await act(async () => {
      stale.resolve({ ...shopState("local", "cycle-1", 999), state_revision: 8 });
      await expect(staleRefresh).resolves.toBeNull();
    });

    expect(confirmed?.available_balance).toBe(70);
    expect(result.current.state).toEqual(confirmed);
  });

  it("ignores an older refresh that resolves after the latest refresh, even with a higher revision", async () => {
    const olderRequest = deferred<ShopState>();
    const latestRequest = deferred<ShopState>();
    const refresh = vi.fn()
      .mockImplementationOnce(() => olderRequest.promise)
      .mockImplementationOnce(() => latestRequest.promise);
    const { result } = renderHook(() => useShopActions(context(), transport({ refresh })));
    let olderRefresh!: Promise<ShopState | null>;
    let latestRefresh!: Promise<ShopState | null>;
    act(() => {
      olderRefresh = result.current.refresh();
      latestRefresh = result.current.refresh();
    });

    await act(async () => {
      latestRequest.resolve({ ...shopState("local", "cycle-1", 20), state_revision: 10 });
      await expect(latestRefresh).resolves.toMatchObject({ state_revision: 10 });
    });
    await act(async () => {
      olderRequest.resolve({ ...shopState("local", "cycle-1", 999), state_revision: 11 });
      await expect(olderRefresh).resolves.toBeNull();
    });

    expect(result.current.state?.state_revision).toBe(10);
    expect(result.current.state?.available_balance).toBe(20);
  });

  it("rejects a lower-revision refresh and preserves the canonical cache", async () => {
    const current = { ...shopState("local", "cycle-1", 123), state_revision: 8 };
    const old = { ...shopState("local", "cycle-1", 1), state_revision: 7 };
    const refresh = vi.fn()
      .mockResolvedValueOnce(current)
      .mockResolvedValueOnce(old);
    const { result } = renderHook(() => useShopActions(context(), transport({ refresh })));

    await act(async () => {
      await result.current.refresh();
    });
    let returned!: ShopState | null;
    await act(async () => {
      returned = await result.current.refresh();
    });

    expect(returned).toBeNull();
    expect(result.current.state).toEqual(current);
    expect(result.current.error).toContain("이전");
  });

  it("blocks authenticated offline actions even if the caller labels that account as a guest", async () => {
    const api = transport();
    const authenticatedOffline = context({ account_id: "account:user-1", is_guest: true, online: false });
    const { result } = renderHook(() => useShopActions(authenticatedOffline, api));

    await expect(result.current.quote(oldQuote.target)).rejects.toThrow(/온라인/i);
    await expect(result.current.apply(purchaseRequest())).rejects.toThrow(/온라인/i);
    expect(api.quote).not.toHaveBeenCalled();
    expect(api.apply).not.toHaveBeenCalled();
  });

  it("allows the local guest to quote and apply while offline", async () => {
    const api = transport();
    const { result } = renderHook(() => useShopActions(context({ online: false }), api));

    await act(async () => {
      await result.current.quote(oldQuote.target);
      await result.current.apply(purchaseRequest());
    });

    expect(api.quote).toHaveBeenCalledWith(oldQuote.target);
    expect(api.apply).toHaveBeenCalledTimes(1);
    expect(result.current.state?.available_balance).toBe(100);
  });

  it("discards a quote when canonical state refreshes before the quote returns", async () => {
    const pendingQuote = deferred<ShopQuote>();
    const api = transport({ quote: vi.fn(() => pendingQuote.promise) });
    const { result } = renderHook(() => useShopActions(context(), api));
    let quotePromise!: Promise<ShopQuote | null>;
    act(() => {
      quotePromise = result.current.quote(oldQuote.target);
    });

    await act(async () => {
      await result.current.refresh();
    });
    await act(async () => {
      pendingQuote.resolve(oldQuote);
      await expect(quotePromise).resolves.toBeNull();
    });

    expect(result.current.currentQuote).toBeNull();
    expect(result.current.quotePending).toBe(false);
  });

  it("does not restore a dismissed quote when its request resolves later", async () => {
    const pendingQuote = deferred<ShopQuote>();
    const api = transport({ quote: vi.fn(() => pendingQuote.promise) });
    const { result } = renderHook(() => useShopActions(context(), api));
    let quotePromise!: Promise<ShopQuote | null>;
    act(() => {
      quotePromise = result.current.quote(oldQuote.target);
    });
    expect(result.current.quotePending).toBe(true);

    act(() => {
      result.current.clearQuote();
    });
    expect(result.current.currentQuote).toBeNull();
    expect(result.current.quotePending).toBe(false);

    await act(async () => {
      pendingQuote.resolve(oldQuote);
      await expect(quotePromise).resolves.toBeNull();
    });
    expect(result.current.currentQuote).toBeNull();
    expect(result.current.quotePending).toBe(false);
  });

  it("prevents duplicate applies while a request is in flight", async () => {
    const pending = deferred<ShopActionResult>();
    const api = transport({ apply: vi.fn(() => pending.promise) });
    const { result } = renderHook(() => useShopActions(context(), api));
    let firstAttempt!: Promise<ShopActionResult | null>;
    act(() => {
      firstAttempt = result.current.apply(purchaseRequest("request-1"));
    });

    expect(result.current.pending?.status).toBe("submitting");
    await expect(result.current.apply(purchaseRequest("request-2"))).rejects.toThrow(/진행 중/i);
    expect(api.apply).toHaveBeenCalledTimes(1);

    await act(async () => {
      pending.resolve(actionResult("request-1", shopState("local", "cycle-1", 75)));
      await firstAttempt;
    });
    expect(result.current.pending).toBeNull();
    expect(result.current.state?.available_balance).toBe(75);
  });

  it("retains the exact request as uncertain after a foreign-account apply response", async () => {
    const cached = shopState("local", "cycle-1", 123);
    const applyMock = vi.fn()
      .mockResolvedValueOnce(actionResult("foreign-response", shopState("account:other", "cycle-1", 1)))
      .mockResolvedValueOnce(actionResult("foreign-response", shopState("local", "cycle-1", 100)));
    const api = transport({
      refresh: vi.fn(async () => cached),
      apply: applyMock,
    });
    const { result } = renderHook(() => useShopActions(context(), api));
    const request = purchaseRequest("foreign-response");

    await act(async () => {
      await result.current.refresh();
      await result.current.apply(request);
    });

    expect(result.current.pending?.status).toBe("uncertain");
    const immutableRequest = result.current.pending?.request;
    expect(immutableRequest).toEqual(request);
    expect(Object.isFrozen(immutableRequest)).toBe(true);
    expect(result.current.state).toEqual(cached);
    expect(result.current.error).toContain("계정");
    await expect(result.current.apply(purchaseRequest("new-id"))).rejects.toThrow(/진행 중/);
    expect(applyMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      await result.current.retryPending();
    });
    expect(applyMock).toHaveBeenCalledTimes(2);
    expect(applyMock.mock.calls[0][0]).toBe(immutableRequest);
    expect(applyMock.mock.calls[1][0]).toBe(immutableRequest);
    expect(result.current.pending).toBeNull();
  });

  it("retains the exact request as uncertain when an apply response is for another cycle", async () => {
    const cached = shopState("local", "cycle-1", 123);
    const applyMock = vi.fn(async () => actionResult(
      "cycle-response",
      shopState("local", "cycle-2", 1),
    ));
    const api = transport({
      refresh: vi.fn(async () => cached),
      apply: applyMock,
    });
    const { result } = renderHook(() => useShopActions(context(), api));
    const request = purchaseRequest("cycle-response");

    await act(async () => {
      await result.current.refresh();
      await result.current.apply(request);
    });

    expect(result.current.pending?.status).toBe("uncertain");
    expect(result.current.pending?.request).toEqual(request);
    expect(result.current.state).toEqual(cached);
    expect(result.current.error).toContain("주기");
    await expect(result.current.apply(purchaseRequest("new-id"))).rejects.toThrow(/진행 중/);
    expect(applyMock).toHaveBeenCalledTimes(1);
  });

  it("returns a confirmed reset cycle as a handoff without adopting it under the old context", async () => {
    const resetResult = actionResult(
      "reset-request",
      shopState("local", "cycle-2", 0),
      "reset",
    );
    const api = transport({ apply: vi.fn(async () => resetResult) });
    const { result } = renderHook(() => useShopActions(context(), api));
    const request: ShopRequest = {
      kind: "reset_planet",
      request_id: "reset-request",
      cycle_id: "cycle-1",
    };
    let returned!: ShopActionResult | null;

    await act(async () => {
      returned = await result.current.apply(request);
    });

    expect(returned).toEqual(resetResult);
    expect(result.current.state).toBeNull();
    expect(result.current.pending).toBeNull();
    expect(result.current.error).toBeNull();
    expect(api.apply).toHaveBeenCalledTimes(1);
  });

  it("retains the original request when the action response request ID differs", async () => {
    const applyMock = vi.fn(async () => actionResult("different-id"));
    const api = transport({ apply: applyMock });
    const { result } = renderHook(() => useShopActions(context(), api));
    const request = purchaseRequest("original-id");

    await act(async () => {
      await expect(result.current.apply(request)).rejects.toThrow(/요청 ID/);
    });

    expect(result.current.pending?.status).toBe("uncertain");
    expect(result.current.pending?.request).toEqual(request);
    await expect(result.current.apply(purchaseRequest("new-id"))).rejects.toThrow(/진행 중/);
    expect(applyMock).toHaveBeenCalledTimes(1);
  });

  it("ignores an apply result after the same account advances to another cycle", async () => {
    const pending = deferred<ShopActionResult>();
    const api = transport({ apply: vi.fn(() => pending.promise) });
    const { result, rerender } = renderHook(
      ({ currentContext }) => useShopActions(currentContext, api),
      { initialProps: { currentContext: context() } },
    );
    let firstAttempt!: Promise<ShopActionResult | null>;
    act(() => {
      firstAttempt = result.current.apply(purchaseRequest("request-old-cycle"));
    });

    rerender({ currentContext: context({ current_cycle_id: "cycle-2" }) });
    expect(result.current.pending).toBeNull();
    await act(async () => {
      pending.resolve(actionResult("request-old-cycle", shopState("local", "cycle-1", 1)));
      await expect(firstAttempt).resolves.toBeNull();
    });

    expect(result.current.state).toBeNull();
    expect(result.current.pending).toBeNull();
    expect(api.apply).toHaveBeenCalledTimes(1);
  });

  it("clears an uncertain retry when the account or cycle context changes", async () => {
    const api = transport({ apply: vi.fn().mockRejectedValue(new Error("unknown outcome")) });
    const { result, rerender } = renderHook(
      ({ currentContext }) => useShopActions(currentContext, api),
      { initialProps: { currentContext: context() } },
    );
    await act(async () => {
      await expect(result.current.apply(purchaseRequest("ambiguous"))).rejects.toThrow(/다시 시도/);
    });
    expect(result.current.pending?.status).toBe("uncertain");

    rerender({ currentContext: context({ account_id: "account:user-2", is_guest: false, online: true }) });
    expect(result.current.pending).toBeNull();
    await expect(result.current.retryPending()).rejects.toThrow(/재시도할 불확실한/);
    expect(api.apply).toHaveBeenCalledTimes(1);
  });

  it("retains an immutable ambiguous request and only retries it after an explicit call", async () => {
    const applyMock = vi.fn()
      .mockRejectedValueOnce(new Error("transport closed"))
      .mockResolvedValueOnce(actionResult("same-id", shopState("local", "cycle-1", 70)));
    const api = transport({ apply: applyMock });
    const { result } = renderHook(() => useShopActions(context(), api));
    const originalRequest = purchaseRequest("same-id", { ...oldQuote, target: { ...oldQuote.target } });

    await act(async () => {
      await expect(result.current.apply(originalRequest)).rejects.toThrow(/다시 시도/);
    });
    const pendingRequest = result.current.pending?.request;
    expect(result.current.pending?.status).toBe("uncertain");
    expect(pendingRequest).toEqual(originalRequest);
    expect(pendingRequest).not.toBe(originalRequest);
    expect(Object.isFrozen(pendingRequest)).toBe(true);
    expect(Object.isFrozen(pendingRequest?.kind === "purchase" ? pendingRequest.quote : null)).toBe(true);
    expect(api.apply).toHaveBeenCalledTimes(1);

    await act(async () => {
      await result.current.retryPending();
    });
    expect(api.apply).toHaveBeenCalledTimes(2);
    expect(applyMock.mock.calls[0][0]).toBe(pendingRequest);
    expect(applyMock.mock.calls[1][0]).toBe(pendingRequest);
    expect(result.current.pending).toBeNull();
  });

  it("adopts quote_changed state and exposes the new quote for reconfirmation without charging again", async () => {
    const revisedQuote = { ...oldQuote, price: 40, effect_revision: 2 };
    const api = transport({
      apply: vi.fn(async () => actionResult(
        "request-1",
        shopState("local", "cycle-1", 80),
        "quote_changed",
        revisedQuote,
      )),
    });
    const { result } = renderHook(() => useShopActions(context(), api));

    await act(async () => {
      await result.current.apply(purchaseRequest());
    });

    expect(result.current.state?.available_balance).toBe(80);
    expect(result.current.currentQuote).toEqual(revisedQuote);
    expect(result.current.reconfirmationRequired).toBe(true);
    expect(api.apply).toHaveBeenCalledTimes(1);
  });
});
