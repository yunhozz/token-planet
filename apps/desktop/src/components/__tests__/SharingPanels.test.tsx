import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SharingSetup } from "../SharingSetup";
import { InvitePanel } from "../InvitePanel";
import { SyncStatus } from "../SyncStatus";
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
        member_count: 2, known_tokens: 200000, growth_credit: 2, stage: 1,
        progress_to_next: 0.4, incomplete: false },
    };
    expect(planetGrowth({ stage: 0, progress_to_next: 0.1 }, state)).toEqual({ stage: 1, progress: 0.4 });
    expect(Object.keys(planetGrowth(null, state))).toEqual(["stage", "progress"]);
  });

  it("shows group count and lets the owner create an invitation", () => {
    const onCreate = vi.fn();
    render(<InvitePanel memberCount={2} isOwner invite={null} invites={[]} onCreate={onCreate} onRevoke={vi.fn()} />);
    expect(screen.getByText(/2명/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "초대 코드 만들기" }));
    expect(onCreate).toHaveBeenCalledOnce();
  });

  it("shows the current code but no member usage breakdown", () => {
    render(<InvitePanel memberCount={2} isOwner invite={{ invite_id: "i", code: "private-code", expires_at: "2026-10-02T00:00:00Z" }} invites={[]} onCreate={vi.fn()} onRevoke={vi.fn()} />);
    expect(screen.getByText("private-code")).toBeInTheDocument();
    expect(screen.queryByText(/멤버별|순위/)).not.toBeInTheDocument();
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
