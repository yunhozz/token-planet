import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { WorldSnapshot } from "../../types/usage";
import { UsageSummary } from "../UsageSummary";
import { SourceStatus } from "../SourceStatus";
import { PlanetScene } from "../PlanetScene";

const snapshot: WorldSnapshot = {
  usage: {
    codex: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: 125000, coverage: "complete" },
    claude_code: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    codex_source: "ready",
    claude_code_source: "usage_unavailable",
    confirmed_subtotal: 125000,
    complete_total: null,
    scanned_at_utc: "2026-09-25T00:00:00Z",
  },
  growth_credit: 1.17,
  stage: 0,
  progress_to_next: 0.234,
  incomplete: true,
  planet: {
    version: 1, profile: { nickname: "Orbit", avatar: "masculine" }, timezone: "Asia/Seoul", current_cycle_id: "cycle-1", cycle_started_at_utc: "2026-09-26T00:00:00Z", last_reset_at_utc: null,
    wallet_balance: 0, wallet_credits: [], current_planet_tokens: 125000, lifetime_tokens: 125000, growth_credit: 1.17, stage: 0, progress_to_next: 0.234, incomplete: true,
    can_reset: true, reset_available_at_utc: null, objects: [],
  },
};

describe("local world summary", () => {
  it("labels a known subtotal as incomplete without making missing Claude usage zero", () => {
    render(<><UsageSummary snapshot={snapshot} /><SourceStatus agent="claude_code" usage={snapshot.usage.claude_code} health={snapshot.usage.claude_code_source} /></>);
    expect(screen.getByText(/125,000/)).toBeInTheDocument();
    expect(screen.getByText(/집계 불완전/)).toBeInTheDocument();
    expect(screen.getByText(/사용량을 확인할 수 없음/)).toBeInTheDocument();
    expect(screen.queryByText(/Claude Code.*0 토큰/)).not.toBeInTheDocument();
  });

  it("shows no member names or member token breakdown in the world view", () => {
    render(<><UsageSummary snapshot={snapshot} /><PlanetScene stage={snapshot.stage} progress={snapshot.progress_to_next} /></>);
    expect(screen.getByRole("img", { name: /자연 생태계, 다음 시대까지 23%/ })).toBeInTheDocument();
    expect(screen.queryByText(/순위|멤버별|기여자별/)).not.toBeInTheDocument();
  });
});
