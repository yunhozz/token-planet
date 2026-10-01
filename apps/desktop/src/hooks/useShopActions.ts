import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ShopActionResult, ShopQuote, ShopQuoteTarget, ShopRequest, ShopState } from "../types/usage";

export type ShopContext = Readonly<{
  /** Canonical account key returned by ShopState ("local" for the local guest). */
  account_id: string;
  current_cycle_id: string;
  is_guest: boolean;
  online: boolean;
  actions_available: boolean;
  unavailable_reason?: string | null;
}>;

export type ShopActionsContext = ShopContext;

export type ShopActionsTransport = Readonly<{
  refresh: () => Promise<ShopState>;
  quote: (target: ShopQuoteTarget) => Promise<ShopQuote>;
  apply: (request: ShopRequest) => Promise<ShopActionResult>;
}>;

export type PendingShopAction = Readonly<{
  request: ShopRequest;
  status: "submitting" | "uncertain";
  error: string | null;
}>;

export type ShopActions = Readonly<{
  state: ShopState | null;
  currentQuote: ShopQuote | null;
  pending: PendingShopAction | null;
  error: string | null;
  quotePending: boolean;
  reconfirmationRequired: boolean;
  canAct: boolean;
  unavailableReason: string | null;
  /** Changes on every account or cycle switch so scene previews can reset. */
  previewResetKey: string;
  refresh: () => Promise<ShopState | null>;
  quote: (target: ShopQuoteTarget) => Promise<ShopQuote | null>;
  apply: (request: ShopRequest) => Promise<ShopActionResult | null>;
  retryPending: () => Promise<ShopActionResult | null>;
  clearQuote: () => void;
}>;

type ContextToken = Readonly<{ key: string; generation: number }>;

type LocalView = {
  token: ContextToken;
  state: ShopState | null;
  currentQuote: ShopQuote | null;
  error: string | null;
  quotePending: boolean;
  reconfirmationRequired: boolean;
};

type PendingRecord = Readonly<{ token: ContextToken; action: PendingShopAction }>;

const defaultTransport: ShopActionsTransport = {
  refresh: () => invoke<ShopState>("get_shop_state"),
  quote: (target) => invoke<ShopQuote>("quote_shop_action", { target }),
  apply: (request) => invoke<ShopActionResult>("apply_shop_action", { request }),
};

function contextKey(context: ShopContext): string {
  return JSON.stringify([context.account_id, context.current_cycle_id]);
}

function sameToken(left: ContextToken | null | undefined, right: ContextToken): boolean {
  return Boolean(left && left.key === right.key && left.generation === right.generation);
}

function emptyView(token: ContextToken): LocalView {
  return {
    token,
    state: null,
    currentQuote: null,
    error: null,
    quotePending: false,
    reconfirmationRequired: false,
  };
}

function errorText(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error || "");
  if (/[\uac00-\ud7af]/.test(message)) return message;
  return "요청 결과를 확인하지 못했습니다. 연결 상태를 확인한 뒤 다시 시도해 주세요.";
}

function cloneAndFreeze<T>(value: T): T {
  const clone = structuredClone(value);
  const freeze = (node: unknown): void => {
    if (!node || typeof node !== "object" || Object.isFrozen(node)) return;
    Object.values(node as Record<string, unknown>).forEach(freeze);
    Object.freeze(node);
  };
  freeze(clone);
  return clone;
}

function requestCycleId(request: ShopRequest): string | null {
  switch (request.kind) {
    case "place":
    case "retrieve":
    case "reset_planet":
      return request.cycle_id;
    case "remove_natural":
      return request.key.cycle_id;
    case "purchase":
    case "equip_avatar":
      return null;
  }
}

function isVerifiedGuest(context: ShopContext): boolean {
  // The local guest account is the only guest account emitted by the current ledger.
  // A caller-provided guest flag cannot make an authenticated account offline-capable.
  return context.is_guest && context.account_id === "local";
}

function unavailableReasonFor(context: ShopContext, state: ShopState | null): string | null {
  const guest = isVerifiedGuest(context);
  if (!context.actions_available) {
    return context.unavailable_reason || state?.action_unavailable_reason || "현재 상점 작업을 사용할 수 없습니다.";
  }
  if (!context.online && !guest) {
    return context.unavailable_reason || "이 계정의 상점 작업에는 온라인 연결이 필요합니다.";
  }
  return context.unavailable_reason || state?.action_unavailable_reason || null;
}

export function useShopActions(
  context: ShopContext,
  transport: ShopActionsTransport = defaultTransport,
): ShopActions {
  const key = contextKey(context);
  const tokenRef = useRef<ContextToken>({ key, generation: 0 });
  if (tokenRef.current.key !== key) {
    tokenRef.current = { key, generation: tokenRef.current.generation + 1 };
  }
  const token = tokenRef.current;

  const [view, setView] = useState<LocalView>(() => emptyView(token));
  const [pendingRevision, setPendingRevision] = useState(0);
  const mountedRef = useRef(false);
  const pendingRef = useRef<PendingRecord | null>(null);
  const inFlightContextsRef = useRef(new Set<string>());
  const quoteSequenceRef = useRef(0);
  const refreshSequenceRef = useRef(0);
  const mutationGenerationRef = useRef(new Map<string, number>());

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      pendingRef.current = null;
      inFlightContextsRef.current.clear();
      mutationGenerationRef.current.clear();
    };
  }, []);

  useEffect(() => {
    // Pending retries belong to one account/cycle and must not cross a context switch.
    pendingRef.current = null;
    setPendingRevision((revision) => revision + 1);
    quoteSequenceRef.current += 1;
  }, [token.key, token.generation]);

  const isCurrent = (captured: ContextToken): boolean =>
    mountedRef.current && sameToken(tokenRef.current, captured);

  const updateView = (captured: ContextToken, patch: Partial<Omit<LocalView, "token">>): void => {
    if (!isCurrent(captured)) return;
    setView((previous) => {
      if (!isCurrent(captured)) return previous;
      const base = sameToken(previous.token, captured) ? previous : emptyView(captured);
      return { ...base, ...patch, token: captured };
    });
  };

  const retainPendingAsUncertain = (
    captured: ContextToken,
    request: ShopRequest,
    message: string,
    patch: Partial<Omit<LocalView, "token" | "error">> = {},
  ): void => {
    if (!isCurrent(captured)) return;
    pendingRef.current = {
      token: captured,
      action: { request, status: "uncertain", error: message },
    };
    setPendingRevision((revision) => revision + 1);
    updateView(captured, { ...patch, error: message });
  };

  const clearPendingRequest = (captured: ContextToken, request: ShopRequest): void => {
    const pendingRecord = pendingRef.current;
    if (pendingRecord && sameToken(pendingRecord.token, captured) && pendingRecord.action.request === request) {
      pendingRef.current = null;
      setPendingRevision((revision) => revision + 1);
    }
  };

  const mutationGenerationFor = (identity: string): number =>
    mutationGenerationRef.current.get(identity) ?? 0;

  const bumpMutationGeneration = (identity: string): void => {
    mutationGenerationRef.current.set(identity, mutationGenerationFor(identity) + 1);
  };

  const currentView = sameToken(view.token, token) ? view : emptyView(token);
  const unavailableReason = unavailableReasonFor(context, currentView.state);
  const canAct = unavailableReason === null;
  const pendingRecord = pendingRef.current;
  const pending = pendingRecord && sameToken(pendingRecord.token, token) ? pendingRecord.action : null;
  // Subscribe to ref changes so the retained request is visible after an ambiguous failure.
  void pendingRevision;

  const refresh = async (): Promise<ShopState | null> => {
    const captured = token;
    if (!isCurrent(captured)) return null;
    const requestGeneration = refreshSequenceRef.current + 1;
    refreshSequenceRef.current = requestGeneration;
    const mutationGeneration = mutationGenerationFor(captured.key);
    updateView(captured, { error: null });
    try {
      const state = await transport.refresh();
      if (!isCurrent(captured)) return null;
      if (requestGeneration !== refreshSequenceRef.current
        || mutationGeneration !== mutationGenerationFor(captured.key)) return null;
      if (state.account_id !== context.account_id) {
        quoteSequenceRef.current += 1;
        updateView(captured, {
          currentQuote: null,
          quotePending: false,
          reconfirmationRequired: false,
          error: "상점 상태가 현재 계정과 일치하지 않습니다.",
        });
        return null;
      }
      if (state.current_cycle_id !== context.current_cycle_id) {
        quoteSequenceRef.current += 1;
        updateView(captured, {
          state: null,
          currentQuote: null,
          quotePending: false,
          reconfirmationRequired: false,
          error: "상점 상태가 현재 주기와 다릅니다. 주기 정보를 갱신한 뒤 사용해 주세요.",
        });
        return state;
      }
      if (currentView.state && state.state_revision < currentView.state.state_revision) {
        updateView(captured, { error: "상점 상태가 현재 저장된 상태보다 이전입니다. 다시 새로고침해 주세요." });
        return null;
      }
      quoteSequenceRef.current += 1;
      updateView(captured, {
        state,
        currentQuote: null,
        quotePending: false,
        reconfirmationRequired: false,
        error: null,
      });
      return state;
    } catch (error) {
      if (isCurrent(captured)) updateView(captured, { error: errorText(error) });
      return null;
    }
  };

  const quote = async (target: ShopQuoteTarget): Promise<ShopQuote | null> => {
    const captured = token;
    const reason = unavailableReasonFor(context, currentView.state);
    if (reason) {
      updateView(captured, { error: reason });
      throw new Error(reason);
    }
    if (target.kind === "remove_natural" && target.key.cycle_id !== context.current_cycle_id) {
      const error = "자연물의 주기가 현재 주기와 다릅니다.";
      updateView(captured, { error });
      throw new Error(error);
    }

    const quoteSequence = quoteSequenceRef.current + 1;
    quoteSequenceRef.current = quoteSequence;
    updateView(captured, {
      currentQuote: null,
      quotePending: true,
      reconfirmationRequired: false,
      error: null,
    });
    try {
      const nextQuote = await transport.quote(target);
      if (!isCurrent(captured) || quoteSequenceRef.current !== quoteSequence) return null;
      if (currentView.state && nextQuote.catalog_revision !== currentView.state.catalog_revision) {
        updateView(captured, {
          quotePending: false,
          currentQuote: null,
          error: "상품 목록이 변경되었습니다. 상점 상태를 새로고침한 뒤 다시 조회해 주세요.",
        });
        return null;
      }
      updateView(captured, { currentQuote: nextQuote, quotePending: false, error: null });
      return nextQuote;
    } catch (error) {
      if (isCurrent(captured) && quoteSequenceRef.current === quoteSequence) {
        updateView(captured, { quotePending: false, error: errorText(error) });
      }
      throw new Error(errorText(error));
    }
  };

  const submit = async (
    captured: ContextToken,
    request: ShopRequest,
    isExplicitRetry = false,
  ): Promise<ShopActionResult | null> => {
    const identity = captured.key;
    if (!isCurrent(captured)) return null;
    const reason = unavailableReasonFor(context, currentView.state);
    if (reason) {
      updateView(captured, { error: reason });
      throw new Error(reason);
    }
    const cycleId = requestCycleId(request);
    if (cycleId !== null && cycleId !== context.current_cycle_id) {
      const error = "상점 작업의 주기가 현재 주기와 다릅니다.";
      updateView(captured, { error });
      throw new Error(error);
    }
    const existing = pendingRef.current;
    const matchingUncertainRequest = Boolean(existing && sameToken(existing.token, captured)
      && existing.action.status === "uncertain"
      && existing.action.request === request);
    if (inFlightContextsRef.current.has(identity)
      || (sameToken(existing?.token, captured) && (!isExplicitRetry || !matchingUncertainRequest))) {
      const error = "현재 계정과 주기의 상점 작업이 진행 중입니다.";
      updateView(captured, { error });
      throw new Error(error);
    }

    const immutableRequest = isExplicitRetry ? request : cloneAndFreeze(request);
    const pendingAction: PendingShopAction = {
      request: immutableRequest,
      status: "submitting",
      error: null,
    };
    pendingRef.current = { token: captured, action: pendingAction };
    setPendingRevision((revision) => revision + 1);
    inFlightContextsRef.current.add(identity);
    bumpMutationGeneration(identity);
    quoteSequenceRef.current += 1;
    updateView(captured, { currentQuote: null, quotePending: false, error: null });

    let mutationSettled = false;
    try {
      const result = await transport.apply(immutableRequest);
      inFlightContextsRef.current.delete(identity);
      bumpMutationGeneration(identity);
      mutationSettled = true;
      if (result.request_id !== immutableRequest.request_id) {
        throw new Error("상점 응답이 요청 ID와 일치하지 않습니다.");
      }
      if (!isCurrent(captured)) return null;

      if (result.state.account_id !== context.account_id) {
        quoteSequenceRef.current += 1;
        retainPendingAsUncertain(captured, immutableRequest, "상점 작업 결과가 현재 계정과 일치하지 않습니다.", {
          currentQuote: null,
          quotePending: false,
          reconfirmationRequired: false,
        });
        return null;
      }
      if (result.status === "reset" && result.state.current_cycle_id !== context.current_cycle_id) {
        quoteSequenceRef.current += 1;
        clearPendingRequest(captured, immutableRequest);
        updateView(captured, {
          state: null,
          currentQuote: null,
          quotePending: false,
          reconfirmationRequired: false,
          error: null,
        });
        return result;
      }
      if (result.state.current_cycle_id !== context.current_cycle_id) {
        quoteSequenceRef.current += 1;
        retainPendingAsUncertain(captured, immutableRequest,
          "상점 작업 결과가 현재 주기와 다릅니다. 주기 정보를 갱신한 뒤 사용해 주세요.", {
          currentQuote: null,
          quotePending: false,
          reconfirmationRequired: false,
        });
        return null;
      }

      clearPendingRequest(captured, immutableRequest);
      quoteSequenceRef.current += 1;
      if (result.status === "quote_changed") {
        updateView(captured, {
          state: result.state,
          currentQuote: result.confirmed_quote,
          quotePending: false,
          reconfirmationRequired: true,
          error: null,
        });
      } else {
        updateView(captured, {
          state: result.state,
          currentQuote: null,
          quotePending: false,
          reconfirmationRequired: false,
          error: null,
        });
      }
      return result;
    } catch (error) {
      inFlightContextsRef.current.delete(identity);
      if (!mutationSettled) bumpMutationGeneration(identity);
      const message = errorText(error);
      retainPendingAsUncertain(captured, immutableRequest, message);
      throw new Error(message);
    }
  };

  const apply = (request: ShopRequest): Promise<ShopActionResult | null> => submit(token, request);

  const retryPending = (): Promise<ShopActionResult | null> => {
    const current = pendingRef.current;
    if (!current || !sameToken(current.token, token) || current.action.status !== "uncertain") {
      return Promise.reject(new Error("재시도할 불확실한 상점 작업이 없습니다."));
    }
    return submit(token, current.action.request, true);
  };

  const clearQuote = (): void => {
    quoteSequenceRef.current += 1;
    updateView(token, {
      currentQuote: null,
      quotePending: false,
      reconfirmationRequired: false,
      error: null,
    });
  };

  return {
    state: currentView.state,
    currentQuote: currentView.currentQuote,
    pending,
    error: currentView.error,
    quotePending: currentView.quotePending,
    reconfirmationRequired: currentView.reconfirmationRequired,
    canAct,
    unavailableReason,
    previewResetKey: `${token.key}:${token.generation}`,
    refresh,
    quote,
    apply,
    retryPending,
    clearQuote,
  };
}
