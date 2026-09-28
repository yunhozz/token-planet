import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SharingSetup } from "../SharingSetup";
import { InvitePanel } from "../InvitePanel";
import { SyncStatus } from "../SyncStatus";
import { WorldCommunity } from "../WorldCommunity";
import { planetGrowth, type SharingState } from "../../lib/sharing";

describe("sharing setup", () => {
  it("keeps solo use available while offering email code sign-in", () => {
    render(<SharingSetup phase="signed_out" onRequestCode={vi.fn()} onVerifyCode={vi.fn()} onCreateWorld={vi.fn()} onJoinWorld={vi.fn()} />);
    expect(screen.getByText(/나만의 세계/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "인증코드 받기" })).toBeInTheDocument();
    expect(screen.getByLabelText("이메일")).toBeInTheDocument();
  });

  it("offers one world creation or invite joining after sign-in", () => {
    render(<SharingSetup phase="signed_in" onRequestCode={vi.fn()} onVerifyCode={vi.fn()} onCreateWorld={vi.fn()} onJoinWorld={vi.fn()} />);
    expect(screen.getByRole("button", { name: "세계 만들기" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "초대 코드로 참여" })).toBeInTheDocument();
  });
});

describe("shared world controls", () => {
  it("passes only group stage and progress to the planet scene", () => {
    const state: SharingState = {
      phase: "shared", email: "member@example.test", sync_status: "synced", pending: 0, last_synced_at: null,
      world: { id: "world", name: "Together", timezone: "Asia/Seoul", is_owner: false,
        member_count: 2 },
      planet_members: [],
    };
    expect(planetGrowth({ stage: 0, progress_to_next: 0.1 }, state)).toEqual({ stage: 0, progress: 0.1 });
    expect(Object.keys(planetGrowth(null, state))).toEqual(["stage", "progress"]);
  });

  it("shows group count and lets the owner create an invitation", () => {
    const onCreate = vi.fn();
    render(<InvitePanel memberCount={2} isOwner invite={null} invites={[]} onCreate={onCreate} onRevoke={vi.fn()} />);
    expect(screen.getByText(/2명/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "초대 코드 만들기" }));
    expect(onCreate).toHaveBeenCalledOnce();
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
      equipped_cosmetics: [{ slot_id: "ring", sku: "thin_ring" }],
      token_rank: 1,
      civilization_rank: 1,
    }]} />);

    expect(container.querySelectorAll('[data-cosmetic="thin_ring"]')).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Nova의 행성 크게 보기" }));
    expect(container.querySelectorAll('[data-cosmetic="thin_ring"]')).toHaveLength(2);
    expect(screen.queryByText(/지갑 잔액|구매 기록|보관함/)).not.toBeInTheDocument();
  });

  it("shows the current code and the member ranking disclosure", () => {
    render(<InvitePanel memberCount={2} isOwner invite={{ invite_id: "i", code: "private-code", expires_at: "2026-10-02T00:00:00Z" }} invites={[]} onCreate={vi.fn()} onRevoke={vi.fn()} />);
    expect(screen.getByText("private-code")).toBeInTheDocument();
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
