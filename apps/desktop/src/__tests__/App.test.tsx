import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "../App";
import type { SharingState } from "../lib/sharing";
import type { CosmeticShopState, GrowthJournal as GrowthJournalData, WorldSnapshot } from "../types/usage";

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());
const isTauriMock = vi.hoisted(() => vi.fn(() => false));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock, isTauri: isTauriMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
const listeners = new Map<string, (event: unknown) => void>();

const ownerState: SharingState = {
  phase: "shared", user_id: "owner", sync_status: "synced", pending: 0, last_synced_at: null,
  world: { id: "world-1", name: "Together", timezone: "Asia/Seoul", is_owner: true,
    member_count: 2 },
  planet_members: [],
};
const localSnapshot: WorldSnapshot = {
  usage: {
    codex: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: 42, coverage: "complete" },
    claude_code: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    codex_source: "ready", claude_code_source: "usage_unavailable", confirmed_subtotal: 42, complete_total: null, scanned_at_utc: "2026-09-26T00:00:00Z",
  },
  growth_credit: 0, stage: 0, progress_to_next: 0, incomplete: true,
  planet: {
    version: 1, profile: { nickname: "Orbit", avatar: "masculine" }, timezone: "Asia/Seoul", current_cycle_id: "cycle-1", cycle_started_at_utc: "2026-09-26T00:00:00Z", last_reset_at_utc: null,
    wallet_balance: 0, wallet_credits: [], current_planet_tokens: 42, lifetime_tokens: 42, growth_credit: 0, stage: 0, progress_to_next: 0, incomplete: true,
    can_reset: true, reset_available_at_utc: null, objects: [],
  },
};

function shopState(balance: number): CosmeticShopState {
  return {
    slots: [], products: [], current_cycle_id: "cycle-1", available_balance: balance,
    owned_skus: [], equipped: [], slot_versions: {}, actions_require_online: true,
    action_unavailable_reason: null, guest_import_pending: false, guest_import_error: null,
  };
}

function catalogShopState(balance: number, owned: string[] = []): CosmeticShopState {
  return {
    ...shopState(balance),
    slots: [{ slot_id: "sky", display_name: "하늘" }],
    products: [{ sku: "star_cluster", slot_id: "sky", display_name: "별무리", price: 100_000, catalog_revision: 1, purchasable: true }],
    owned_skus: owned,
    slot_versions: { sky: 0 },
  };
}

function growthJournal(generation = 1, deletedAt: string | null = null, hasRecord = true, tokens = 123_456): GrowthJournalData {
  const today = new Date().toISOString().slice(0, 10);
  return {
    generation,
    deleted_at_utc: deletedAt,
    timezone: "UTC",
    cycles: hasRecord ? [{
      cycle_id: "cycle-1", started_at_utc: `${today}T00:00:00Z`, ended_at_utc: null,
      wallet_credit: null, wallet_credit_at_utc: null,
    }] : [],
    entries: hasRecord ? [{
      device_id: "device-1", cycle_id: "cycle-1", bucket_date: today, agent: "codex",
      revision: 1, generation, present: true, confirmed_tokens: tokens, coverage: "complete", payload_hash: "fixture",
    }] : [],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

it("starts anonymous sharing without email sign-in", async () => {
  let started = false;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return {
      ...ownerState, phase: started ? "signed_in" : "signed_out", world: null, sync_status: started ? "synced" : "local",
    };
    if (command === "start_anonymous_session") {
      started = true;
      return { ...ownerState, phase: "signed_in", world: null, sync_status: "synced" };
    }
    if (command === "current_usage") return structuredClone(localSnapshot);
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("tab", { name: "그룹" }));
  expect(screen.queryByLabelText("이메일")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "공유 시작하기" }));
  expect(await screen.findByLabelText("공동 행성에서 사용할 닉네임")).toHaveValue("Orbit");
});

beforeEach(() => {
  window.history.replaceState({}, "", "/");
  invokeMock.mockReset();
  listenMock.mockReset();
  isTauriMock.mockReturnValue(false);
  listeners.clear();
  listenMock.mockImplementation(async (event: string, handler: (event: unknown) => void) => {
    listeners.set(event, handler);
    return () => {};
  });
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "list_world_members") return [{ user_id: "owner", role: "owner" }, { user_id: "member", role: "member" }];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

it("opens the cosmetic shop in the current window and restores exploration on return", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
    { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
  ];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "get_shop_state") return shopState(0);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(screen.getByRole("button", { name: "확대" }));
  fireEvent.click(container.querySelector('[data-object-list-id="1-3"]')!);
  const originalViewBox = container.querySelector(".planet-landscape-svg")?.getAttribute("viewBox");

  fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));

  expect(openWindow).not.toHaveBeenCalled();
  expect(await screen.findByRole("heading", { name: "행성 꾸미기", level: 1 })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));

  expect(await screen.findByRole("region", { name: "행성 풍경" })).toBeInTheDocument();
  expect(container.querySelector(".planet-landscape-svg")).toHaveAttribute("viewBox", originalViewBox);
  expect(container.querySelector('[data-object-list-id="1-3"]')).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "행성 꾸미기" })).toHaveFocus();
});

it("opens the growth journal in the current window and returns to personal detail", async () => {
  const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));

  expect(openWindow).not.toHaveBeenCalled();
  expect(await screen.findByRole("heading", { name: "성장 일지", level: 1 })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));

  expect(await screen.findByRole("region", { name: "행성 풍경" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "성장 일지 보기" })).toHaveFocus();
});

it("opens legacy feature URLs in detail mode and returns to personal detail", async () => {
  window.history.replaceState({}, "", "/?window=cosmetic-shop");
  const { container } = render(<App />);

  expect(await screen.findByRole("heading", { name: "행성 꾸미기", level: 1 })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));

  expect(await screen.findByRole("tab", { name: "내 행성" })).toHaveAttribute("aria-selected", "true");
  expect(container.querySelector(".planet-landscape")).toBeInTheDocument();
  expect(screen.queryByRole("main", { name: "행성 팝오버" })).not.toBeInTheDocument();
});

it("uses the flat landscape only in personal detail and keeps exploration when switching tabs", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
    { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
  ];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  expect(container.querySelector(".planet-landscape")).not.toBeInTheDocument();
  expect(container.querySelector(".planet-svg")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(container.querySelector('[data-object-list-id="1-3"]')!);
  const selected = screen.getByRole("region", { name: "선택한 오브젝트" });
  expect(selected).toHaveTextContent("정착·농경");
  expect(selected).toHaveTextContent("4번째");
  const selectedViewBox = container.querySelector(".planet-landscape-svg")?.getAttribute("viewBox");

  fireEvent.click(screen.getByRole("tab", { name: "그룹" }));
  expect(container.querySelector(".planet-landscape")).not.toBeInTheDocument();
  fireEvent.click(await screen.findByRole("tab", { name: "내 행성" }));

  expect(container.querySelector('[data-object-list-id="1-3"]')).toHaveAttribute("aria-pressed", "true");
  expect(container.querySelector(".planet-landscape-svg")).toHaveAttribute("viewBox", selectedViewBox);
});

it.each(["account", "world", "cycle"] as const)("resets exploration and keeps the shop open when %s changes", async (changedContext) => {
  let activeShared = structuredClone(ownerState);
  let activeSnapshot = structuredClone(localSnapshot);
  activeSnapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
    { stage: 1, ordinal: 3, kind: "tree", x: 63, y: 20, seed: 3 },
  ];
  const heldRefresh = deferred<WorldSnapshot>();
  let holdRefresh = false;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(activeShared);
    if (command === "current_usage") return structuredClone(activeSnapshot);
    if (command === "refresh_usage") return holdRefresh ? heldRefresh.promise : structuredClone(activeSnapshot);
    if (command === "get_shop_state") return {
      ...catalogShopState(500_000), current_cycle_id: activeSnapshot.planet.current_cycle_id,
    };
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(screen.getByRole("button", { name: "확대" }));
  fireEvent.click(container.querySelector('[data-object-list-id="1-3"]')!);
  expect(screen.getByRole("region", { name: "선택한 오브젝트" })).toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  await screen.findByRole("button", { name: "별무리 미리보기" });

  if (changedContext === "account") activeShared = { ...activeShared, user_id: "member-next", phase: "signed_in", world: null };
  if (changedContext === "world") activeShared = { ...activeShared, world: { ...activeShared.world!, id: "world-2" } };
  if (changedContext === "cycle") activeSnapshot.planet.current_cycle_id = "cycle-2";
  holdRefresh = true;
  fireEvent.click(screen.getByRole("button", { name: "사용량 새로고침" }));

  await screen.findByText("계정과 행성 상태를 확인하고 있습니다.");
  expect(screen.getByRole("heading", { name: "행성 꾸미기", level: 1 })).toBeInTheDocument();
  await act(async () => { heldRefresh.resolve(structuredClone(activeSnapshot)); });
  expect(await screen.findByRole("button", { name: "별무리 미리보기" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));

  await screen.findByRole("region", { name: "행성 풍경" });
  expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
  expect(Number(container.querySelector<HTMLElement>(".planet-landscape")!.dataset.cameraZoom)).toBe(1);
  expect(container.querySelector('[data-object-list-id="1-3"]')).toHaveAttribute("aria-pressed", "false");
});

it("clears exploration immediately while a world context is locked", async () => {
  const snapshot = structuredClone(localSnapshot);
  snapshot.planet.objects = [
    { stage: 0, ordinal: 0, kind: "rock", x: 35, y: 42, seed: 10 },
  ];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(snapshot);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("region", { name: "행성 풍경" });
  fireEvent.click(screen.getByRole("button", { name: "확대" }));
  fireEvent.click(container.querySelector('[data-object-list-id="0-0"]')!);
  expect(screen.getByRole("region", { name: "선택한 오브젝트" })).toBeInTheDocument();

  await act(async () => { listeners.get("world-context-changing")?.({ payload: "cosmetic-shop" }); });

  expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
  expect(Number(container.querySelector<HTMLElement>(".planet-landscape")!.dataset.cameraZoom)).toBe(1);
  fireEvent.click(container.querySelector('[data-object-list-id="0-0"]')!);
  expect(screen.queryByRole("region", { name: "선택한 오브젝트" })).not.toBeInTheDocument();
});

it("keeps shop load errors on the shop screen", async () => {
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") throw new Error("shop unavailable");
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_shop_state"));
  expect(screen.queryByText("상점 상태를 불러오지 못했습니다.")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  expect(await screen.findByText("상점 상태를 불러오지 못했습니다.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));
  expect(screen.queryByText("상점 상태를 불러오지 못했습니다.")).not.toBeInTheDocument();

  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  expect(await screen.findByText("상점 상태를 불러오지 못했습니다.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "다시 불러오기" })).toBeInTheDocument();
});

it("ignores a late journal failure after leaving and reloads on reentry", async () => {
  const oldRead = deferred<unknown>();
  const currentRead = deferred<unknown>();
  let journalReads = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_growth_journal") return journalReads++ === 0 ? oldRead.promise : currentRead.promise;
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  await screen.findByRole("heading", { name: "성장 일지", level: 1 });
  await waitFor(() => expect(journalReads).toBe(1));

  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));
  await act(async () => { oldRead.reject(new Error("stale journal failure")); });
  expect(screen.queryByText("성장 일지를 불러오지 못했습니다.")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  await waitFor(() => expect(journalReads).toBe(2));
  expect(await screen.findByText("기록 동기화를 기다리며 성장 일지를 불러오고 있습니다.")).toBeInTheDocument();
  await act(async () => {
    currentRead.resolve({ generation: 1, deleted_at_utc: null, timezone: "UTC", cycles: [], entries: [] });
  });
  expect(await screen.findByText("확인된 행성 주기가 없습니다.")).toBeInTheDocument();
});

it("does not offer personal journal deletion in a signed-out local phase", async () => {
  const localState: SharingState = {
    ...ownerState, phase: "signed_out", user_id: null, world: null, sync_status: "local",
  };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(localState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_growth_journal") return growthJournal();
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  const journalRegion = await screen.findByRole("region", { name: "성장 일지" });

  expect(journalRegion.querySelector(".growth-journal-total")?.textContent).toContain("123,456");
  expect(within(journalRegion).queryByRole("button", { name: "개인 일지 삭제" })).not.toBeInTheDocument();
  expect(invokeMock).not.toHaveBeenCalledWith("delete_growth_journal");
});

it.each(["shared", "signed_in"] as const)("requires confirmation and then applies journal deletion for %s users", async (phase) => {
  const state: SharingState = phase === "shared"
    ? structuredClone(ownerState)
    : { ...ownerState, phase: "signed_in", world: null };
  const deletedJournal = growthJournal(2, new Date().toISOString(), false);
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(state);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_growth_journal") return growthJournal();
    if (command === "delete_growth_journal") return structuredClone(deletedJournal);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  const journalRegion = await screen.findByRole("region", { name: "성장 일지" });
  const deleteButton = within(journalRegion).getByRole("button", { name: "개인 일지 삭제" });
  expect(journalRegion.querySelector(".growth-journal-total")?.textContent).toContain("123,456");

  fireEvent.click(deleteButton);
  expect(confirm).toHaveBeenCalled();
  expect(invokeMock).not.toHaveBeenCalledWith("delete_growth_journal");
  expect(journalRegion.querySelector(".growth-journal-total")?.textContent).toContain("123,456");

  confirm.mockReturnValue(true);
  fireEvent.click(deleteButton);

  expect(await within(journalRegion).findByText("확인된 행성 주기가 없습니다.")).toBeInTheDocument();
  expect(journalRegion.querySelector(".growth-journal-total")).toBeNull();
  expect(invokeMock).toHaveBeenCalledWith("delete_growth_journal");
});

it("shows a retryable journal error after delete fails", async () => {
  let journalReads = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_growth_journal") return journalReads++ === 0 ? growthJournal() : growthJournal(2, null, true, 654_321);
    if (command === "delete_growth_journal") throw new Error("temporary delete failure");
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(screen.getByRole("button", { name: "성장 일지 보기" }));
  const journalRegion = await screen.findByRole("region", { name: "성장 일지" });
  await waitFor(() => expect(journalRegion.querySelector(".growth-journal-total")?.textContent).toContain("123,456"));
  fireEvent.click(within(journalRegion).getByRole("button", { name: "개인 일지 삭제" }));

  expect(await within(journalRegion).findByRole("alert")).toHaveTextContent("개인 일지를 삭제하지 못했습니다.");
  const reloadButton = within(journalRegion).getByRole("button", { name: "성장 일지 새로고침" });
  expect(reloadButton).toBeEnabled();
  const previousJournalReads = journalReads;
  fireEvent.click(reloadButton);

  await waitFor(() => expect(journalReads).toBeGreaterThan(previousJournalReads));
  await waitFor(() => expect(screen.getByRole("region", { name: "성장 일지" }).querySelector(".growth-journal-total")?.textContent).toContain("654,321"));
  expect(within(screen.getByRole("region", { name: "성장 일지" })).queryByRole("alert")).not.toBeInTheDocument();
});

it("refreshes owner candidates even when the member count stays the same", async () => {
  let roster = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "list_world_members") {
      roster += 1;
      return [{ user_id: "owner", role: "owner" }, { user_id: roster === 1 ? "member-b" : "member-c", role: "member" }];
    }
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "list_world_members")).toHaveLength(1));
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("tab", { name: "그룹" }));
  await screen.findByRole("option", { name: /member-b/ });
  fireEvent.change(screen.getByLabelText("소유권을 넘길 참여자"), { target: { value: "member-b" } });
  fireEvent.click(screen.getByRole("button", { name: "사용량 새로고침" }));
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "list_world_members")).toHaveLength(2));
  await screen.findByRole("option", { name: /member-c/ });
  expect(screen.getByLabelText("소유권을 넘길 참여자")).toHaveValue("");
});

it("refreshes the confirmed shop state after sync finishes", async () => {
  let synced = false;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return shopState(synced ? 100_000 : 0);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("0 토큰"));

  synced = true;
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });

  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("100,000 토큰"));
});

it("retries an unavailable shop when navigating to the in-app shop screen", async () => {
  let shopReads = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") {
      shopReads += 1;
      if (shopReads === 1) throw new Error("temporary shop read failure");
      return catalogShopState(300_000);
    }
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  await waitFor(() => expect(shopReads).toBe(1));
  expect(screen.queryByText("상점 상태를 불러오지 못했습니다.")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));

  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("300,000 토큰"));
  expect(shopReads).toBe(2);
});

it("coalesces a shared shop read when opening the in-app shop screen", async () => {
  let latestShop = catalogShopState(200_000);
  let heldRead: ReturnType<typeof deferred<CosmeticShopState>> | null = null;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return heldRead?.promise ?? structuredClone(latestShop);
    if (command === "list_world_members") return [{ user_id: "owner", role: "owner" }, { user_id: "member", role: "member" }];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  const readsBefore = invokeMock.mock.calls.filter(([command]) => command === "get_shop_state").length;
  heldRead = deferred<CosmeticShopState>();
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "get_shop_state")).toHaveLength(readsBefore + 1));

  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  await waitFor(() => expect(document.querySelector(".cosmetic-shop")).toBeInTheDocument());
  expect(invokeMock.mock.calls.filter(([command]) => command === "get_shop_state")).toHaveLength(readsBefore + 1);

  await act(async () => { heldRead!.resolve(structuredClone(latestShop)); });
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("200,000 토큰"));
});

it("filters context-change events using the actual main window identity while in the shop", async () => {
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return catalogShopState(500_000);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  const previewButton = await screen.findByRole("button", { name: "별무리 미리보기" });

  await act(async () => { listeners.get("world-context-changing")?.({ payload: "main" }); });
  expect(previewButton).toBeEnabled();

  await act(async () => { listeners.get("world-context-changing")?.({ payload: "cosmetic-shop" }); });
  expect(screen.getByRole("heading", { name: "행성 꾸미기", level: 1 })).toBeInTheDocument();
  expect(screen.getByText("계정과 행성 상태를 확인하고 있습니다.")).toBeInTheDocument();
  await act(async () => { listeners.get("world-state-updated")?.({ payload: null }); });
  await waitFor(() => expect(screen.getByRole("button", { name: "별무리 미리보기" })).toBeEnabled());
  expect(screen.getByRole("heading", { name: "행성 꾸미기", level: 1 })).toBeInTheDocument();
});

it("keeps a confirmed purchase without equipping it automatically", async () => {
  const originalShop = catalogShopState(200_000);
  const purchasedShop = catalogShopState(100_000, ["star_cluster"]);
  let heldRead: ReturnType<typeof deferred<CosmeticShopState>> | null = null;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return heldRead?.promise ?? structuredClone(originalShop);
    if (command === "purchase_cosmetic") return {
      result: { purchase_id: "purchase-1", sku: "star_cluster", status: "purchased", price: 100_000, available_balance: 100_000 },
      state: structuredClone(purchasedShop), unavailable_reason: null,
    };
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("200,000 토큰"));

  const readsBefore = invokeMock.mock.calls.filter(([command]) => command === "get_shop_state").length;
  heldRead = deferred<CosmeticShopState>();
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "get_shop_state")).toHaveLength(readsBefore + 1));

  fireEvent.click(screen.getByRole("button", { name: "별무리 구매" }));
  fireEvent.click(await screen.findByRole("button", { name: "구매 확인" }));
  expect(await screen.findByText("별무리 구매 완료 · 잔액 100,000 토큰")).toBeInTheDocument();
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("100,000 토큰"));

  await act(async () => { heldRead!.resolve(structuredClone(originalShop)); });
  fireEvent.click(screen.getByRole("tab", { name: "보관함 (1)" }));
  expect(await screen.findByRole("button", { name: "별무리 장착" })).toBeInTheDocument();
  expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("100,000 토큰");

  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(container.querySelector('[data-cosmetic="star_cluster"]')).not.toBeInTheDocument();
});

it("does not apply a pending purchase after the account changes", async () => {
  const pendingPurchase = deferred<unknown>();
  let currentAccount = structuredClone(ownerState);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return catalogShopState(currentAccount.user_id === "owner" ? 200_000 : 700_000);
    if (command === "purchase_cosmetic") return pendingPurchase.promise;
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("200,000 토큰"));
  fireEvent.click(screen.getByRole("button", { name: "별무리 구매" }));
  fireEvent.click(await screen.findByRole("button", { name: "구매 확인" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("purchase_cosmetic", { sku: "star_cluster" }));

  currentAccount = { ...currentAccount, user_id: "next-account", phase: "signed_in", world: null };
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("700,000 토큰"));
  expect(screen.getByRole("heading", { name: "행성 꾸미기", level: 1 })).toBeInTheDocument();

  await act(async () => {
    pendingPurchase.resolve({
      result: { purchase_id: "stale-purchase", sku: "star_cluster", status: "purchased", price: 100_000, available_balance: 100_000 },
      state: catalogShopState(100_000, ["star_cluster"]), unavailable_reason: null,
    });
    await Promise.resolve();
  });

  expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("700,000 토큰");
  expect(screen.getByRole("button", { name: "별무리 구매" })).toBeInTheDocument();
});

it.each(["purchase", "equip"] as const)("shows a failed %s in the shop after returning to it", async (actionKind) => {
  const pendingAction = deferred<unknown>();
  const actionError = "요청 결과를 확인할 수 없습니다.";
  let shopReads = 0;
  let returnedToShop = false;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") {
      shopReads += 1;
      return catalogShopState(returnedToShop ? 175_000 : 200_000, actionKind === "equip" ? ["star_cluster"] : []);
    }
    if (command === (actionKind === "purchase" ? "purchase_cosmetic" : "equip_cosmetic")) return pendingAction.promise;
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("200,000 토큰"));
  if (actionKind === "purchase") {
    fireEvent.click(screen.getByRole("button", { name: "별무리 구매" }));
    fireEvent.click(await screen.findByRole("button", { name: "구매 확인" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("purchase_cosmetic", { sku: "star_cluster" }));
  } else {
    fireEvent.click(screen.getByRole("tab", { name: "보관함 (1)" }));
    fireEvent.click(await screen.findByRole("button", { name: "별무리 장착" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("equip_cosmetic", {
      slotId: "sky", sku: "star_cluster", cycleId: "cycle-1", expectedVersion: 0,
    }));
  }

  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  await act(async () => {
    pendingAction.reject(actionError);
    await Promise.resolve();
  });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();

  const readsBeforeReentry = shopReads;
  returnedToShop = true;
  fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(actionError);
  expect(screen.getByRole("button", { name: "상점 다시 확인" })).toBeEnabled();
  await waitFor(() => expect(shopReads).toBeGreaterThan(readsBeforeReentry));
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("175,000 토큰"));
  expect(await screen.findByRole("alert")).toHaveTextContent(actionError);

  const readsBeforeRetry = shopReads;
  fireEvent.click(screen.getByRole("button", { name: "상점 다시 확인" }));
  await waitFor(() => expect(shopReads).toBeGreaterThan(readsBeforeRetry));
  expect(screen.queryByText(actionError)).not.toBeInTheDocument();
});

it.each(["purchase", "equip"] as const)("discards a pending %s failure after the account changes", async (actionKind) => {
  const pendingAction = deferred<unknown>();
  const actionError = "이전 계정 요청 실패";
  let currentAccount = structuredClone(ownerState);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return currentAccount.user_id === "owner"
      ? catalogShopState(200_000, actionKind === "equip" ? ["star_cluster"] : [])
      : catalogShopState(700_000);
    if (command === (actionKind === "purchase" ? "purchase_cosmetic" : "equip_cosmetic")) return pendingAction.promise;
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("200,000 토큰"));
  if (actionKind === "purchase") {
    fireEvent.click(screen.getByRole("button", { name: "별무리 구매" }));
    fireEvent.click(await screen.findByRole("button", { name: "구매 확인" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("purchase_cosmetic", { sku: "star_cluster" }));
  } else {
    fireEvent.click(screen.getByRole("tab", { name: "보관함 (1)" }));
    fireEvent.click(await screen.findByRole("button", { name: "별무리 장착" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("equip_cosmetic", {
      slotId: "sky", sku: "star_cluster", cycleId: "cycle-1", expectedVersion: 0,
    }));
  }

  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  currentAccount = { ...currentAccount, user_id: "next-account", phase: "signed_in", world: null };
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "get_shop_state").length).toBeGreaterThan(1));

  await act(async () => {
    pendingAction.reject(actionError);
    await Promise.resolve();
  });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "행성 꾸미기" }));
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("700,000 토큰"));
  expect(screen.queryByText(actionError)).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("shows confirmed equipment on the planet after returning from the shop", async () => {
  const ownedShop = catalogShopState(500_000, ["star_cluster"]);
  const equippedShop = {
    ...ownedShop,
    equipped: [{ slot_id: "sky", sku: "star_cluster", version: 1 }],
    slot_versions: { sky: 1 },
  };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(ownedShop);
    if (command === "equip_cosmetic") return {
      result: { status: "equipped", cycle_id: "cycle-1", slot_id: "sky", sku: "star_cluster", version: 1 },
      state: structuredClone(equippedShop), unavailable_reason: null,
    };
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  fireEvent.click(screen.getByRole("tab", { name: "보관함 (1)" }));
  fireEvent.click(await screen.findByRole("button", { name: "별무리 장착" }));
  expect(await screen.findByText("행성에 장식을 장착했습니다.")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(container.querySelector('[data-cosmetic="star_cluster"]')).toBeInTheDocument();
});

it("applies a pending same-context equip after returning to the planet", async () => {
  const pendingEquip = deferred<unknown>();
  const ownedShop = catalogShopState(500_000, ["star_cluster"]);
  const equippedShop = {
    ...ownedShop,
    equipped: [{ slot_id: "sky", sku: "star_cluster", version: 1 }],
    slot_versions: { sky: 1 },
  };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(ownedShop);
    if (command === "equip_cosmetic") return pendingEquip.promise;
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  fireEvent.click(screen.getByRole("tab", { name: "보관함 (1)" }));
  fireEvent.click(await screen.findByRole("button", { name: "별무리 장착" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("equip_cosmetic", {
    slotId: "sky", sku: "star_cluster", cycleId: "cycle-1", expectedVersion: 0,
  }));

  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(container.querySelector('[data-cosmetic="star_cluster"]')).not.toBeInTheDocument();
  await act(async () => {
    pendingEquip.resolve({
      result: { status: "equipped", cycle_id: "cycle-1", slot_id: "sky", sku: "star_cluster", version: 1 },
      state: equippedShop, unavailable_reason: null,
    });
    await Promise.resolve();
  });

  await waitFor(() => expect(container.querySelector('[data-cosmetic="star_cluster"]')).toBeInTheDocument());
});

it("discards a pending equip when the account changes", async () => {
  const pendingEquip = deferred<unknown>();
  const ownedShop = catalogShopState(500_000, ["star_cluster"]);
  const equippedShop = {
    ...ownedShop,
    equipped: [{ slot_id: "sky", sku: "star_cluster", version: 1 }],
    slot_versions: { sky: 1 },
  };
  let currentAccount = structuredClone(ownerState);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return currentAccount.user_id === "owner" ? structuredClone(ownedShop) : catalogShopState(700_000);
    if (command === "equip_cosmetic") return pendingEquip.promise;
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  const { container } = render(<App />);

  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  fireEvent.click(screen.getByRole("tab", { name: "보관함 (1)" }));
  fireEvent.click(await screen.findByRole("button", { name: "별무리 장착" }));
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("equip_cosmetic", {
    slotId: "sky", sku: "star_cluster", cycleId: "cycle-1", expectedVersion: 0,
  }));

  currentAccount = { ...currentAccount, user_id: "next-account", phase: "signed_in", world: null };
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("700,000 토큰"));
  expect(screen.getByRole("button", { name: "별무리 구매" })).toBeInTheDocument();

  await act(async () => {
    pendingEquip.resolve({
      result: { status: "equipped", cycle_id: "cycle-1", slot_id: "sky", sku: "star_cluster", version: 1 },
      state: equippedShop, unavailable_reason: null,
    });
    await Promise.resolve();
  });

  expect(container.querySelector('[data-cosmetic="star_cluster"]')).not.toBeInTheDocument();
  expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("700,000 토큰");
});

it("does not show the previous account shop when the next account lookup fails", async () => {
  let currentAccount = ownerState;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(currentAccount);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") {
      if (currentAccount.user_id === "next-account") throw "서버에서 상점 상태를 불러오지 못했습니다";
      return shopState(600_000);
    }
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")?.textContent).toBe("600,000 토큰"));

  currentAccount = { ...ownerState, user_id: "next-account", phase: "signed_in", world: null };
  await act(async () => { listeners.get("sync-status-updated")?.({ payload: null }); });

  await waitFor(() => expect(document.querySelector(".cosmetic-balance strong")).toBeNull());
  expect(await screen.findByText("서버에서 상점 상태를 불러오지 못했습니다")).toBeInTheDocument();
});

it("returns_to_popup_then_reopens_personal_detail", async () => {
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  screen.getByRole("main", { name: "행성 팝오버" });
  const summary = screen.getByRole("region", { name: "행성 요약" });
  expect(summary).toHaveTextContent("현재 시대");
  expect(summary).toHaveTextContent("이번 행성 토큰");
  expect(summary.querySelector('[role="progressbar"][aria-label="다음 시대 진행도"]')).not.toBeNull();
  const scene = container.querySelector<HTMLElement>(".world-visual")!;
  const detailButton = screen.getByRole("button", { name: "행성·그룹 자세히 보기" });
  const usage = screen.getByRole("region", { name: "전체 사용량 (과거 포함)" });
  expect(scene.compareDocumentPosition(summary) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(summary.compareDocumentPosition(detailButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(detailButton.compareDocumentPosition(usage) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

  fireEvent.click(detailButton);
  const personal = await screen.findByRole("tab", { name: "내 행성" });
  const group = screen.getByRole("tab", { name: "그룹" });
  expect(personal).toHaveAttribute("aria-selected", "true");
  personal.focus();
  fireEvent.keyDown(personal, { key: "ArrowRight" });
  expect(group).toHaveFocus();
  expect(group).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("button", { name: "행성으로 돌아가기" }));
  expect(screen.getByRole("main", { name: "행성 팝오버" })).toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByRole("tab", { name: "내 행성" })).toHaveAttribute("aria-selected", "true");
});

it("shows_popover_summary_in_order", async () => {
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  const scene = container.querySelector<HTMLElement>(".world-visual")!;
  const token = screen.getByText("이번 행성 토큰");
  const progress = screen.getByRole("progressbar", { name: "다음 시대 진행도" });
  const detail = screen.getByRole("button", { name: "행성·그룹 자세히 보기" });
  const sources = screen.getByRole("region", { name: "수집 상태" });
  const sync = screen.getByRole("status");
  expect(screen.getByRole("main", { name: "행성 팝오버" })).toContainElement(scene);
  expect(scene.compareDocumentPosition(token) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(token.compareDocumentPosition(progress) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(progress.compareDocumentPosition(detail) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(detail.compareDocumentPosition(sources) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(sources.compareDocumentPosition(sync) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it("retains_view_when_native_transition_fails", async () => {
  isTauriMock.mockReturnValue(true);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage") return structuredClone(localSnapshot);
    if (command === "set_detail_view") throw "window size unavailable";
    if (command === "get_shop_state") return shopState(0);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("화면을 전환하지 못했습니다");
  expect(screen.getByRole("main", { name: "행성 팝오버" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "행성·그룹 자세히 보기" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeInTheDocument();
  isTauriMock.mockReturnValue(false);
});

it("escape_hides_only_popup", async () => {
  isTauriMock.mockReturnValue(true);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.keyDown(document, { key: "Escape" });
  await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("hide_popover"));
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  invokeMock.mockClear();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(invokeMock).not.toHaveBeenCalledWith("hide_popover");
  isTauriMock.mockReturnValue(false);
});

it("opens_popover_after_profile_setup", async () => {
  const unprofiled = structuredClone(localSnapshot);
  unprofiled.planet.profile = null;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage") return structuredClone(unprofiled);
    if (command === "set_planet_profile") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return shopState(0);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  const nickname = await screen.findByLabelText("행성에서 사용할 닉네임");
  expect(screen.queryByRole("main", { name: "행성 팝오버" })).not.toBeInTheDocument();
  fireEvent.change(nickname, { target: { value: "Orbit" } });
  fireEvent.click(screen.getByRole("button", { name: "행성 시작하기" }));
  expect(await screen.findByRole("main", { name: "행성 팝오버" })).toBeInTheDocument();
  expect(screen.getByText("Orbit의 행성")).toBeInTheDocument();
});

it("keeps_unknown_usage_distinct_from_zero", async () => {
  const unknown = structuredClone(localSnapshot);
  unknown.usage.confirmed_subtotal = null;
  unknown.usage.codex.total_tokens = null;
  unknown.usage.claude_code.total_tokens = null;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage") return unknown;
    if (command === "get_shop_state") return shopState(0);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  await screen.findByText("아직 확인된 사용량 없음");
  expect(screen.getByRole("main", { name: "행성 팝오버" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "수집 상태" })).toHaveTextContent("—");
  expect(screen.getByRole("region", { name: "수집 상태" })).not.toHaveTextContent("0 토큰");
});

it("clears a cosmetic preview when leaving the shop and personal detail", async () => {
  const previewShop: CosmeticShopState = {
    ...shopState(600_000),
    slots: [{ slot_id: "sky", display_name: "하늘" }],
    products: [{ sku: "star_cluster", slot_id: "sky", display_name: "별무리", price: 100_000, catalog_revision: 1, purchasable: true }],
    slot_versions: { sky: 0 },
  };
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "get_shop_state") return structuredClone(previewShop);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("button", { name: "행성 꾸미기" }));
  fireEvent.click(await screen.findByRole("button", { name: "별무리 미리보기" }));
  expect(document.querySelector('[data-cosmetic="star_cluster"]')).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /내 행성으로 돌아가기/ }));
  await screen.findByRole("region", { name: "행성 풍경" });
  expect(document.querySelector('[data-cosmetic="star_cluster"]')).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("tab", { name: "그룹" }));
  fireEvent.click(screen.getByRole("tab", { name: "내 행성" }));

  expect(document.querySelector('[data-cosmetic="star_cluster"]')).not.toBeInTheDocument();
});

it("keeps the three token totals tied to their original fields", async () => {
  const distinct = structuredClone(localSnapshot);
  distinct.planet.current_planet_tokens = 7;
  distinct.planet.lifetime_tokens = 11;
  distinct.usage.confirmed_subtotal = 19;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "current_usage") return distinct;
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "list_world_members") return [];
    if (command === "get_my_member_code") return "AB12CD34EF";
    return null;
  });
  render(<App />);
  const summary = await screen.findByRole("region", { name: "행성 요약" });
  expect(summary).toHaveTextContent("7");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  expect(screen.getByText("누적 토큰 (개편 후)").parentElement).toHaveTextContent("11");
  expect(screen.getByRole("region", { name: "전체 사용량 (과거 포함)" })).toHaveTextContent("19");
});

it("explains reset effects and group visibility before the user acts", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("tab", { name: "내 행성" });
  fireEvent.click(screen.getByRole("button", { name: "행성 초기화" }));
  expect(confirm).toHaveBeenCalledWith(expect.stringContaining("지갑"));
  expect(confirm).toHaveBeenCalledWith(expect.stringContaining("자연 생태계"));
  fireEvent.click(screen.getByRole("tab", { name: "그룹" }));
  expect(screen.getByText("그룹에는 행성 모습과 집계값만 공유됩니다")).toBeInTheDocument();
  confirm.mockRestore();
});

it("shows the group's syncing state without the personal scene", async () => {
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.click(await screen.findByRole("tab", { name: "그룹" }));
  expect(screen.getByText("멤버의 행성을 동기화하고 있습니다.")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "나의 행성" })).not.toBeInTheDocument();
});
