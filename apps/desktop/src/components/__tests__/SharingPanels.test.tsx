import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SharingSetup } from "../SharingSetup";
import { InvitePanel } from "../InvitePanel";
import { SyncStatus } from "../SyncStatus";
import { WorldCommunity } from "../WorldCommunity";
import { planetGrowth, type SharingState } from "../../lib/sharing";
import type { WorldPlanet } from "../../types/usage";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
const issued = { invite_id: "invite-one", code: "a".repeat(64), created_at: "2026-10-08T00:00:00Z", expires_at: "2026-10-15T00:00:00Z" };
beforeEach(() => {
  invokeMock.mockReset().mockImplementation(async (command: string) => {
    if (command === "list_world_invites") return [];
    if (command === "create_world_invite") return issued;
    if (command === "revoke_world_invite") return { status: "revoked" };
    throw new Error("unexpected command");
  });
});

const member = (nickname: string, rank = 1): WorldPlanet => ({
  nickname, avatar: "masculine", stage: 0, current_planet_tokens: rank * 10,
  lifetime_tokens: rank * 20, growth_credit: rank * 0.5, progress_to_next: 0.1,
  incomplete: false, objects: [], equipped_cosmetics: [],
  token_rank: rank, civilization_rank: rank,
});

it("opens the first member planet by default and selects another with the keyboard", () => {
  render(<WorldCommunity name="Together" members={[member("Nova", 2), member("Mira", 1)]} />);
  expect(screen.getByRole("region", { name: "Nova의 행성 자세히 보기" })).toBeInTheDocument();
  const mira = screen.getByRole("button", { name: "Mira의 행성 크게 보기" });
  fireEvent.keyDown(screen.getByRole("button", { name: "Nova의 행성 크게 보기" }), { key: "ArrowRight" });
  expect(mira).toHaveFocus();
  expect(mira).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("region", { name: "Mira의 행성 자세히 보기" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "누적 토큰 사용량" }).querySelector("li")?.textContent).toContain("Mira");
  expect(screen.getByRole("region", { name: "문명 발전" }).querySelector("li")?.textContent).toContain("Mira");
});

it("falls back to the first available member when the selected member disappears", () => {
  const { rerender } = render(<WorldCommunity name="Together" members={[member("Nova"), member("Mira")]} />);
  fireEvent.click(screen.getByRole("button", { name: "Mira의 행성 크게 보기" }));
  rerender(<WorldCommunity name="Together" members={[member("Nova")]} />);
  expect(screen.getByRole("region", { name: "Nova의 행성 자세히 보기" })).toBeInTheDocument();
});

describe("sharing setup", () => {
  it("offers device-bound anonymous sharing without collecting email", () => {
    render(<SharingSetup phase="signed_out" initialNickname="Orbit" onStartAnonymousSession={vi.fn()} onCreateWorld={vi.fn()} onJoinWorld={vi.fn()} />);
    expect(screen.getByText(/나만의 세계/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "공유 시작하기" })).toBeInTheDocument();
    expect(screen.queryByLabelText("이메일")).not.toBeInTheDocument();
  });

  it("prefills the nickname and normalizes a pasted invitation", () => {
    const join = vi.fn();
    render(<SharingSetup phase="signed_in" initialNickname="Orbit" onStartAnonymousSession={vi.fn()} onCreateWorld={vi.fn()} onJoinWorld={join} />);
    expect(screen.getByRole("button", { name: "세계 만들기" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "코드로 참여" })).toBeInTheDocument();
    expect(screen.getByLabelText("공동 행성에서 사용할 닉네임")).toHaveValue("Orbit");
    fireEvent.change(screen.getByLabelText("소유자가 알려준 64자리 초대 코드"), { target: { value: "  " + "A".repeat(64) + "  " } });
    fireEvent.click(screen.getByRole("button", { name: "코드로 참여" }));
    expect(join).toHaveBeenCalledWith("a".repeat(64), "Orbit");
  });
});

describe("shared world controls", () => {
  it("prevents issuing invitations when the group is full", () => {
    render(<InvitePanel memberCount={10} isOwner contextKey="owner" />);
    expect(screen.getByRole("button", {name: "초대 발급"})).toBeDisabled();
    expect(screen.getByText(/정원 10명/)).toBeInTheDocument();
  });

  it("prevents duplicate joins while preserving the code for a retry", async () => {
    let finish!: () => void;
    const join = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    render(<SharingSetup phase="signed_in" initialNickname="Orbit" onStartAnonymousSession={vi.fn()} onCreateWorld={vi.fn()} onJoinWorld={join} />);
    fireEvent.change(screen.getByLabelText("소유자가 알려준 64자리 초대 코드"), {target: {value: "a".repeat(64)}});
    const button=screen.getByRole("button", {name: "코드로 참여"});
    fireEvent.click(button); fireEvent.click(button);
    expect(join).toHaveBeenCalledOnce();
    expect(button).toBeDisabled();
    await act(async () => finish());
    expect(button).not.toBeDisabled();
    expect(screen.getByLabelText("소유자가 알려준 64자리 초대 코드")).toHaveValue("a".repeat(64));
  });

  it("passes only group stage and progress to the planet scene", () => {
    const state: SharingState = {
      phase: "shared", user_id: "member", sync_status: "synced", pending: 0, last_synced_at: null,
      world: { id: "world", name: "Together", timezone: "Asia/Seoul", is_owner: false,
        member_count: 2 },
      planet_members: [],
    };
    expect(planetGrowth({ stage: 0, progress_to_next: 0.1 }, state)).toEqual({ stage: 0, progress: 0.1 });
    expect(Object.keys(planetGrowth(null, state))).toEqual(["stage", "progress"]);
  });

  it("lets only the owner issue a one-time code and copy it", async () => {
    const clipboard = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: clipboard } });
    render(<InvitePanel memberCount={2} isOwner contextKey="owner-a" />);
    expect(screen.getByText(/2명/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "초대 발급" }));
    await screen.findByText(issued.code);
    fireEvent.click(screen.getByRole("button", { name: "초대 코드 복사" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("복사"));
    expect(clipboard).toHaveBeenCalledWith(issued.code);
    expect(invokeMock).toHaveBeenCalledWith("create_world_invite");
  });

  it("prevents duplicate invitation issuance while the request is pending", async () => {
    let finish!: (value: typeof issued) => void;
    invokeMock.mockImplementation(async (command: string) => command === "create_world_invite"
      ? new Promise((resolve) => { finish = resolve; }) : []);
    render(<InvitePanel memberCount={2} isOwner contextKey="owner" />);
    const button = screen.getByRole("button", { name: "초대 발급" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(button).toBeDisabled();
    expect(invokeMock.mock.calls.filter(([command]) => command === "create_world_invite")).toHaveLength(1);
    await act(async () => finish(issued));
    expect(screen.getByText(issued.code)).toBeInTheDocument();
    expect(button).not.toBeDisabled();
    expect(invokeMock.mock.calls.filter(([command]) => command === "create_world_invite")).toHaveLength(1);
  });

  it("clears the one-time code on visibilitychange and does not restore it on return", async () => {
    const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    try {
      render(<InvitePanel memberCount={2} isOwner contextKey="owner" />);
      fireEvent.click(screen.getByRole("button", { name: "초대 발급" }));
      await screen.findByText(issued.code);
      visibility.mockReturnValue("hidden");
      fireEvent(document, new Event("visibilitychange"));
      expect(screen.queryByText(issued.code)).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "초대 코드 복사" })).not.toBeInTheDocument();
      visibility.mockReturnValue("visible");
      fireEvent(document, new Event("visibilitychange"));
      expect(screen.queryByText(issued.code)).not.toBeInTheDocument();
    } finally { visibility.mockRestore(); }
  });

  it.each(["success", "failure"])("ignores a delayed initial list %s after the post-create list wins", async (mode) => {
    let initialResolve!: (value: unknown) => void;
    let initialReject!: (cause: unknown) => void;
    let latestResolve!: (value: unknown) => void;
    let requests = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "create_world_invite") return issued;
      if (command === "list_world_invites") {
        requests++;
        return requests === 1
          ? new Promise((resolve, reject) => { initialResolve = resolve; initialReject = reject; })
          : new Promise((resolve) => { latestResolve = resolve; });
      }
      throw new Error("unexpected command");
    });
    render(<InvitePanel memberCount={2} isOwner contextKey="owner" />);
    fireEvent.click(screen.getByRole("button", {name: "초대 발급"}));
    await waitFor(() => expect(requests).toBe(2));
    const record = {invite_id: issued.invite_id, created_at: issued.created_at, expires_at: issued.expires_at, revoked_at: null, used_at: "2026-10-08T01:00:00Z", status: "used"};
    await act(async () => latestResolve([record]));
    expect(screen.getByText("사용됨")).toBeInTheDocument();
    await act(async () => {
      if (mode === "success") initialResolve([{...record, used_at: null, status: "active"}]);
      else initialReject(new Error("stale-list-error"));
    });
    expect(screen.getByText("사용됨")).toBeInTheDocument();
    expect(screen.queryByText("사용 가능")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it.each(["success", "failure"])("preserves issuance recovery guidance after a delayed initial list %s", async (mode) => {
    let initialResolve!: (value: unknown) => void;
    let initialReject!: (cause: unknown) => void;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_world_invites") return new Promise((resolve, reject) => {
        initialResolve = resolve; initialReject = reject;
      });
      if (command === "create_world_invite") throw new Error("uncertain-create-response");
      throw new Error("unexpected command");
    });
    render(<InvitePanel memberCount={2} isOwner contextKey="owner" />);
    fireEvent.click(screen.getByRole("button", {name: "초대 발급"}));
    expect(await screen.findByRole("alert")).toHaveTextContent("초대를 철회한 뒤 다시 발급하세요");
    await act(async () => {
      if (mode === "success") initialResolve([]);
      else initialReject(new Error("stale-initial-list"));
    });
    expect(screen.getByRole("alert")).toHaveTextContent("초대를 철회한 뒤 다시 발급하세요");
  });

  it("keeps a pending issue locked across hide and show and discards its code", async () => {
    let finish!: (value: typeof issued) => void;
    invokeMock.mockImplementation(async (command: string) => command === "create_world_invite"
      ? new Promise((resolve) => { finish = resolve; }) : []);
    const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    try {
      render(<InvitePanel memberCount={2} isOwner contextKey="owner" />);
      const button = screen.getByRole("button", {name: "초대 발급"});
      fireEvent.click(button);
      visibility.mockReturnValue("hidden");
      fireEvent(document, new Event("visibilitychange"));
      visibility.mockReturnValue("visible");
      fireEvent(document, new Event("visibilitychange"));
      fireEvent.click(button);
      expect(invokeMock.mock.calls.filter(([command]) => command === "create_world_invite")).toHaveLength(1);
      expect(button).toBeDisabled();
      expect(screen.queryByText(issued.code)).not.toBeInTheDocument();
      await act(async () => finish(issued));
      expect(button).not.toBeDisabled();
      expect(screen.queryByText(issued.code)).not.toBeInTheDocument();
      expect(screen.queryByRole("button", {name: "초대 코드 복사"})).not.toBeInTheDocument();
    } finally { visibility.mockRestore(); }
  });

  it("hides invite management for ordinary members", () => {
    render(<InvitePanel memberCount={2} isOwner={false} contextKey="member" />);
    expect(screen.queryByRole("button", { name: "초대 발급" })).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("discards an issued code when owner context changes", async () => {
    let resolve!: (value: typeof issued) => void;
    invokeMock.mockImplementation(async (command: string) => command === "create_world_invite"
      ? new Promise((r) => { resolve = r; }) : []);
    const { rerender } = render(<InvitePanel memberCount={2} isOwner contextKey="owner-a" />);
    fireEvent.click(screen.getByRole("button", { name: "초대 발급" }));
    rerender(<InvitePanel memberCount={2} isOwner contextKey="owner-b" />);
    await act(async () => resolve(issued));
    expect(screen.queryByText(issued.code)).not.toBeInTheDocument();
  });

  it("keeps a new owner request pending when an old request finishes", async () => {
    const resolves: Array<(value: typeof issued) => void> = [];
    invokeMock.mockImplementation(async (command: string) => command === "create_world_invite"
      ? new Promise((r) => { resolves.push(r); }) : []);
    const { rerender } = render(<InvitePanel memberCount={2} isOwner contextKey="first" />);
    fireEvent.click(screen.getByRole("button", {name: "초대 발급"}));
    rerender(<InvitePanel memberCount={2} isOwner contextKey="second" />);
    fireEvent.click(screen.getByRole("button", {name: "초대 발급"}));
    await act(async () => resolves[0](issued));
    expect(screen.getByRole("button", {name: "초대 발급"})).toBeDisabled();
    await act(async () => resolves[1](issued));
  });

  it("reports clipboard failure without exposing the failing payload", async () => {
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: vi.fn().mockRejectedValue(new Error("secret-payload")) } });
    render(<InvitePanel memberCount={2} isOwner contextKey="owner" />);
    fireEvent.click(screen.getByRole("button", { name: "초대 발급" }));
    await screen.findByText(issued.code);
    fireEvent.click(screen.getByRole("button", { name: "초대 코드 복사" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("복사하지 못했습니다");
    expect(screen.queryByText(/secret-payload/)).not.toBeInTheDocument();
  });

  it("renders equipped cosmetics on member cards and the expanded planet only", () => {
    const { container } = render(<WorldCommunity name="Together" members={[{
      nickname: "Nova",
      avatar: "feminine",
      stage: 0,
      current_planet_tokens: 100,
      lifetime_tokens: 100,
      growth_credit: 0.2,
      progress_to_next: 0.04,
      incomplete: false,
      objects: [],
      equipped_cosmetics: [
        { slot_id: "sky", sku: "star_cluster_v2" },
        { slot_id: "forecourt", sku: "pond" },
        { slot_id: "future_slot", sku: "future_item" },
      ],
      token_rank: 1,
      civilization_rank: 1,
    }]} />);

    expect(container.querySelectorAll('[data-cosmetic="star_cluster"]')).toHaveLength(2);
    expect(container.querySelectorAll('[data-cosmetic="pond"]')).toHaveLength(2);
    expect(container.querySelectorAll('[data-cosmetic="future_item"]')).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Nova의 행성 크게 보기" }));
    expect(container.querySelectorAll('[data-cosmetic="star_cluster"]')).toHaveLength(2);
    expect(container.querySelectorAll('[data-cosmetic="pond"]')).toHaveLength(2);
    expect(screen.getByRole("region", { name: "Nova의 행성 자세히 보기" })).toBeInTheDocument();
    expect(screen.queryByText(/지갑 잔액|구매 기록|보관함/)).not.toBeInTheDocument();
  });

  it("lets the expanded public planet speak while gallery cards remain member selectors", () => {
    const publicObject = { stage: 1, ordinal: 1, kind: "tree", x: 10, y: 30, seed: 1 } as const;
    render(<WorldCommunity name="Together" members={[
      { ...member("Nova"), stage: 1, progress_to_next: 0.9, incomplete: true, objects: [publicObject] },
      member("Mira", 2),
    ]} />);

    expect(screen.getAllByRole("button", { name: "행성에게 말 걸기" })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: "아바타에게 말 걸기" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "행성에게 말 걸기" }));
    expect(screen.getByRole("status")).toHaveTextContent("다음 시대");
    expect(screen.getByRole("status")).not.toHaveTextContent(/기록|나무/);

    fireEvent.click(screen.getByRole("button", { name: "Mira의 행성 크게 보기" }));
    expect(screen.getByRole("region", { name: "Mira의 행성 자세히 보기" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "행성에게 말 걸기" })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Mira의 행성 크게 보기" })).toHaveAttribute("aria-pressed", "true");
  });

  it("shows invite state and revokes an unused invitation", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_world_invites") return [{...issued, code: undefined, status: "active", used_at: null, revoked_at: null}];
      if (command === "revoke_world_invite") return {status: "revoked"};
      return issued;
    });
    render(<InvitePanel memberCount={2} isOwner contextKey="owner" />);
    expect(await screen.findByText("사용 가능")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "초대 철회" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("revoke_world_invite", {inviteId: "invite-one"}));
    expect(screen.getByText(/정확한 값과 순위/)).toBeInTheDocument();
  });

  it("distinguishes queued, paused, and failed sync", () => {
    const { rerender } = render(<SyncStatus status="queued" pending={3} onPause={vi.fn()} onResume={vi.fn()} />);
    expect(screen.getByText(/3개 대기/)).toBeInTheDocument();
    rerender(<SyncStatus status="paused" pending={3} onPause={vi.fn()} onResume={vi.fn()} />);
    expect(screen.getByRole("button", { name: "동기화 재개" })).toBeInTheDocument();
    rerender(<SyncStatus status="failed" pending={3} onPause={vi.fn()} onResume={vi.fn()} />);
    expect(screen.getByText(/연결을 확인/)).toBeInTheDocument();
  });
});
