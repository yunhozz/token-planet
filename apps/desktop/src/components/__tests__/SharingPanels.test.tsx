import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SharingSetup } from "../SharingSetup";
import { InvitePanel } from "../InvitePanel";
import { SyncStatus } from "../SyncStatus";
import { WorldCommunity } from "../WorldCommunity";
import { planetGrowth, type SharingState } from "../../lib/sharing";
import type { WorldPlanet } from "../../types/usage";

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

  it("prefills the nickname and offers a personal code join", () => {
    render(<SharingSetup phase="signed_in" initialNickname="Orbit" onStartAnonymousSession={vi.fn()} onCreateWorld={vi.fn()} onJoinWorld={vi.fn()} />);
    expect(screen.getByRole("button", { name: "세계 만들기" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "코드로 참여" })).toBeInTheDocument();
    expect(screen.getByLabelText("공동 행성에서 사용할 닉네임")).toHaveValue("Orbit");
    expect(screen.getByLabelText("소유자가 알려준 10자리 코드")).toHaveAttribute("maxlength", "10");
  });
});

describe("shared world controls", () => {
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

  it("shows group count and lets the owner create an invitation", () => {
    const onRotate = vi.fn();
    render(<InvitePanel memberCount={2} isOwner memberCode="AB12CD34EF" onRotate={onRotate} />);
    expect(screen.getByText(/2명/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "내 개인 코드 다시 발급" }));
    expect(onRotate).toHaveBeenCalledOnce();
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

    expect(container.querySelectorAll('[data-cosmetic="thin_ring"]')).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Nova의 행성 크게 보기" }));
    expect(container.querySelectorAll('[data-cosmetic="thin_ring"]')).toHaveLength(2);
    expect(screen.getByRole("region", { name: "Nova의 행성 자세히 보기" })).toBeInTheDocument();
    expect(screen.queryByText(/지갑 잔액|구매 기록|보관함/)).not.toBeInTheDocument();
  });

  it("shows the current code and the member ranking disclosure", () => {
    render(<InvitePanel memberCount={2} isOwner memberCode="AB12CD34EF" onRotate={vi.fn()} />);
    expect(screen.getByText("AB12CD34EF")).toBeInTheDocument();
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
