import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "../App";
import type { SharingState } from "../lib/sharing";
import type { WorldSnapshot } from "../types/usage";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const ownerState: SharingState = {
  phase: "shared", email: "owner@example.test", sync_status: "synced", pending: 0, last_synced_at: null,
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

afterEach(cleanup);

it.each([false, true])("shows the new account's profile setup after sign-in (group lookup fails: %s)", async (groupLookupFails) => {
  let verified = false;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state" || command === "sign_out") return {
      ...ownerState, phase: "signed_out", world: null, sync_status: "local",
    };
    if (command === "verify_email_code") {
      verified = true;
      if (groupLookupFails) throw "공동 세계를 불러올 수 없습니다";
      return { ...ownerState, phase: "signed_in", world: null, sync_status: "local" };
    }
    if (command === "current_usage") return verified
      ? { ...structuredClone(localSnapshot), planet: { ...localSnapshot.planet, profile: null, lifetime_tokens: 0 } }
      : structuredClone(localSnapshot);
    return null;
  });
  render(<App />);
  await screen.findByText("Orbit의 행성");
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  fireEvent.change(await screen.findByLabelText("이메일"), { target: { value: "bob@example.test" } });
  fireEvent.click(screen.getByRole("button", { name: "인증코드 받기" }));
  fireEvent.change(await screen.findByLabelText("이메일 인증코드"), { target: { value: "123456" } });
  fireEvent.click(screen.getByRole("button", { name: "코드 확인" }));
  await screen.findByRole("heading", { name: "행성의 첫 주민을 골라주세요" });
  expect(screen.queryByText("Orbit의 행성")).not.toBeInTheDocument();
});

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_sharing_state") return structuredClone(ownerState);
    if (command === "current_usage" || command === "refresh_usage") return structuredClone(localSnapshot);
    if (command === "list_world_members") return [{ user_id: "owner", role: "owner" }, { user_id: "member", role: "member" }];
    if (command === "list_invites") return [];
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
    if (command === "list_invites") return [];
    return null;
  });
  render(<App />);
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "list_world_members")).toHaveLength(1));
  fireEvent.click(screen.getByRole("button", { name: "행성·그룹 자세히 보기" }));
  await screen.findByRole("option", { name: /member-b/ });
  fireEvent.change(screen.getByLabelText("소유권을 넘길 참여자"), { target: { value: "member-b" } });
  fireEvent.click(screen.getByRole("button", { name: "사용량 새로고침" }));
  await waitFor(() => expect(invokeMock.mock.calls.filter(([command]) => command === "list_world_members")).toHaveLength(2));
  await screen.findByRole("option", { name: /member-c/ });
  expect(screen.getByLabelText("소유권을 넘길 참여자")).toHaveValue("");
});
