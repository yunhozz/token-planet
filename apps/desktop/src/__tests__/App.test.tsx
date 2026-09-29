import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import App from "../App";
import type { SharingState } from "../lib/sharing";
import type { CosmeticShopState, WorldSnapshot } from "../types/usage";

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
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
  invokeMock.mockReset();
  listenMock.mockReset();
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

it("shows the planet summary first and reopens details on the personal tab", async () => {
  const { container } = render(<App />);
  await screen.findByText("Orbit의 행성");
  const summary = screen.getByRole("region", { name: "행성 요약" });
  expect(summary).toHaveTextContent("현재 시대");
  expect(summary).toHaveTextContent("이번 행성 토큰");
  expect(summary.querySelector('[role="progressbar"][aria-label="다음 시대 진행도"]')).not.toBeNull();
  const scene = container.querySelector(".world-visual")!;
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
  fireEvent.click(await screen.findByRole("button", { name: "행성·그룹 자세히 보기" }));
  expect(await screen.findByRole("tab", { name: "내 행성" })).toHaveAttribute("aria-selected", "true");
});

it("clears a cosmetic preview when leaving the personal detail tab", async () => {
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
