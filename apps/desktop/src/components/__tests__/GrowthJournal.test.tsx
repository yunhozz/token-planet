import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { GrowthJournal as GrowthJournalData } from "../../types/usage";
import { GrowthJournal } from "../GrowthJournal";

const emptyActions = { onReload: vi.fn(), onDelete: vi.fn() };

afterEach(() => vi.useRealTimers());

describe("growth journal", () => {
  it("combines device and agent totals before growth and omits unrecorded dates", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-28T12:00:00Z"));
    const journal: GrowthJournalData = {
      generation: 0,
      deleted_at_utc: null,
      timezone: "Asia/Seoul",
      cycles: [{
        cycle_id: "cycle-current",
        started_at_utc: "2026-09-27T00:00:00Z",
        ended_at_utc: null,
        wallet_credit: null,
        wallet_credit_at_utc: null,
      }],
      entries: [
        { device_id: "device-1", cycle_id: "cycle-current", bucket_date: "2026-09-28", agent: "codex", revision: 1, generation: 0, present: true, confirmed_tokens: 50_000, coverage: "complete", payload_hash: "" },
        { device_id: "device-2", cycle_id: "cycle-current", bucket_date: "2026-09-28", agent: "codex", revision: 1, generation: 0, present: true, confirmed_tokens: 50_000, coverage: "complete", payload_hash: "" },
        { device_id: "device-2", cycle_id: "cycle-current", bucket_date: "2026-09-28", agent: "claude_code", revision: 1, generation: 0, present: true, confirmed_tokens: 100_000, coverage: "complete", payload_hash: "" },
      ],
    };

    const { container } = render(<GrowthJournal journal={journal} busy={false} error="" canDelete={false} {...emptyActions} />);
    expect(screen.getByText(/200,000/)).toBeInTheDocument();
    expect(screen.getByText(/성장 \+1\.584963/)).toBeInTheDocument();
    expect(container.querySelectorAll(".growth-journal-day")).toHaveLength(1);
  });

  it("keeps records from either side of a reset in their own cycle", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-28T12:00:00Z"));
    const journal: GrowthJournalData = {
      generation: 0,
      deleted_at_utc: null,
      timezone: "Asia/Seoul",
      cycles: [
        { cycle_id: "cycle-before", started_at_utc: "2026-09-27T00:00:00Z", ended_at_utc: "2026-09-28T14:30:00Z", wallet_credit: 120_000, wallet_credit_at_utc: "2026-09-28T14:30:00Z" },
        { cycle_id: "cycle-after", started_at_utc: "2026-09-28T14:30:00Z", ended_at_utc: null, wallet_credit: null, wallet_credit_at_utc: null },
      ],
      entries: [
        { device_id: "device-1", cycle_id: "cycle-before", bucket_date: "2026-09-28", agent: "codex", revision: 1, generation: 0, present: true, confirmed_tokens: 120_000, coverage: "complete", payload_hash: "" },
        { device_id: "device-1", cycle_id: "cycle-after", bucket_date: "2026-09-28", agent: "codex", revision: 1, generation: 0, present: true, confirmed_tokens: 80_000, coverage: "complete", payload_hash: "" },
      ],
    };

    const { container } = render(<GrowthJournal journal={journal} busy={false} error="" canDelete={false} {...emptyActions} />);
    expect(container.querySelector(".growth-journal-total")?.textContent).toContain("80");
    fireEvent.change(screen.getByLabelText("행성 주기"), { target: { value: "cycle-before" } });
    expect(container.querySelector(".growth-journal-total")?.textContent).toContain("120,000");
    expect(screen.getByText(/지갑 적립 120,000 토큰/)).toBeInTheDocument();
    expect(container.querySelector(".growth-journal-cycle-note")?.textContent).toContain("2026. 09. 28.");
  });
});
