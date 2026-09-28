import { useEffect, useMemo, useState } from "react";
import { FormattedNumber } from "./FormattedNumber";
import { STAGE_NAMES } from "./PlanetScene";
import type { GrowthJournal as GrowthJournalData, GrowthJournalCycle, GrowthJournalEntry, UsageCoverage } from "../types/usage";

type Props = {
  journal: GrowthJournalData;
  busy: boolean;
  error: string;
  canDelete: boolean;
  onReload: () => void;
  onDelete: () => void;
};

type AgentSummary = { total: number | null; coverage: UsageCoverage | null };
type DaySummary = {
  date: string;
  total: number | null;
  credit: number | null;
  codex: AgentSummary;
  claudeCode: AgentSummary;
};

const MILESTONES = [5, 20, 50, 100];

function validTimezone(value: string | null): string {
  if (!value) return "UTC";
  try {
    new Intl.DateTimeFormat("en", { timeZone: value });
    return value;
  } catch {
    return "UTC";
  }
}

function dateInTimezone(value: Date, timezone: string): string {
  const parts = new Intl.DateTimeFormat("en", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(value);
  const part = (type: string) => parts.find((item) => item.type === type)?.value ?? "00";
  return `${part("year")}-${part("month")}-${part("day")}`;
}

function shiftDate(value: string, days: number): string {
  const shifted = new Date(`${value}T12:00:00Z`);
  shifted.setUTCDate(shifted.getUTCDate() + days);
  return shifted.toISOString().slice(0, 10);
}

function daysBetween(start: string, end: string): number {
  return Math.max(0, Math.round((Date.parse(`${end}T12:00:00Z`) - Date.parse(`${start}T12:00:00Z`)) / 86_400_000));
}

function formatDateTime(value: string, timezone: string): string {
  return new Date(value).toLocaleString("ko-KR", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  });
}

function summarizeAgent(entries: GrowthJournalEntry[]): AgentSummary {
  if (!entries.length) return { total: null, coverage: null };
  const active = entries.filter((entry) => entry.coverage !== "user_disabled");
  if (!active.length) return { total: null, coverage: "user_disabled" };
  let total: number | null = null;
  for (const entry of active) {
    if (entry.confirmed_tokens === null) continue;
    total = (total ?? 0) + entry.confirmed_tokens;
  }
  const incomplete = active.some((entry) => entry.coverage !== "complete");
  const coverage: UsageCoverage = total !== null
    ? incomplete ? "partial" : "complete"
    : active.some((entry) => entry.coverage === "partial")
      ? "partial"
      : active.some((entry) => entry.coverage === "unsupported") ? "unsupported" : "unavailable";
  return { total, coverage };
}

function summarizeDays(entries: GrowthJournalEntry[]): DaySummary[] {
  const grouped = new Map<string, GrowthJournalEntry[]>();
  for (const entry of entries) {
    const day = grouped.get(entry.bucket_date) ?? [];
    day.push(entry);
    grouped.set(entry.bucket_date, day);
  }
  return [...grouped.entries()].map(([date, dayEntries]) => {
    const codex = summarizeAgent(dayEntries.filter((entry) => entry.agent === "codex"));
    const claudeCode = summarizeAgent(dayEntries.filter((entry) => entry.agent === "claude_code"));
    const known = [codex.total, claudeCode.total].filter((value): value is number => value !== null);
    const total = known.length ? known.reduce((sum, value) => sum + value, 0) : null;
    return {
      date,
      total,
      credit: total === null ? null : Math.log2(1 + total / 100_000),
      codex,
      claudeCode,
    };
  }).sort((left, right) => left.date.localeCompare(right.date));
}

function cycleEndDate(cycle: GrowthJournalCycle, timezone: string, today: string): string {
  return cycle.ended_at_utc ? dateInTimezone(new Date(cycle.ended_at_utc), timezone) : today;
}

function coverageLabel(coverage: UsageCoverage | null): string {
  switch (coverage) {
    case "complete": return "완전 수집";
    case "partial": return "부분 수집";
    case "unsupported": return "지원되지 않는 기록";
    case "user_disabled": return "수집 중지";
    case "unavailable": return "확인 불가";
    default: return "기록 없음";
  }
}

export function GrowthJournal({ journal, busy, error, canDelete, onReload, onDelete }: Props) {
  const timezone = validTimezone(journal.timezone);
  const today = dateInTimezone(new Date(), timezone);
  const cycles = useMemo(
    () => [...journal.cycles].sort((left, right) => {
      const leftStart = left.started_at_utc ? Date.parse(left.started_at_utc) : Number.MIN_SAFE_INTEGER;
      const rightStart = right.started_at_utc ? Date.parse(right.started_at_utc) : Number.MIN_SAFE_INTEGER;
      return leftStart - rightStart;
    }),
    [journal.cycles],
  );
  const activeCycle = cycles.find((cycle) => cycle.ended_at_utc === null) ?? cycles[cycles.length - 1];
  const [cycleId, setCycleId] = useState(activeCycle?.cycle_id ?? "");
  const [windowEnd, setWindowEnd] = useState(activeCycle ? cycleEndDate(activeCycle, timezone, today) : today);
  const selectedCycle = cycles.find((cycle) => cycle.cycle_id === cycleId) ?? activeCycle;

  useEffect(() => {
    if (!activeCycle) return;
    if (!cycles.some((cycle) => cycle.cycle_id === cycleId)) {
      setCycleId(activeCycle.cycle_id);
      setWindowEnd(cycleEndDate(activeCycle, timezone, today));
    }
  }, [activeCycle, cycleId, cycles, timezone, today]);

  useEffect(() => {
    setCycleId(activeCycle?.cycle_id ?? "");
    setWindowEnd(activeCycle ? cycleEndDate(activeCycle, timezone, today) : today);
  }, [journal.generation, journal.deleted_at_utc]);

  const allDays = useMemo(
    () => summarizeDays(journal.entries.filter((entry) => entry.cycle_id === selectedCycle?.cycle_id)),
    [journal.entries, selectedCycle?.cycle_id],
  );
  const milestones = useMemo(() => {
    const reached = new Map<number, string>();
    let cumulative = 0;
    for (const day of allDays) {
      if (day.credit === null) continue;
      cumulative += day.credit;
      for (const threshold of MILESTONES) {
        if (!reached.has(threshold) && cumulative >= threshold) reached.set(threshold, day.date);
      }
    }
    return reached;
  }, [allDays]);
  const rangeStart = shiftDate(windowEnd, -6);
  const visibleDays = allDays
    .filter((day) => day.date >= rangeStart && day.date <= windowEnd)
    .sort((left, right) => right.date.localeCompare(left.date));
  const latestEnd = selectedCycle ? cycleEndDate(selectedCycle, timezone, today) : today;
  const cycleCredits = [...milestones.entries()].sort(([left], [right]) => left - right);

  function selectCycle(nextCycleId: string) {
    const nextCycle = cycles.find((cycle) => cycle.cycle_id === nextCycleId);
    setCycleId(nextCycleId);
    if (nextCycle) setWindowEnd(cycleEndDate(nextCycle, timezone, today));
  }

  return (
    <section className="growth-journal" aria-label="성장 일지">
      <div className="growth-journal-heading">
        <div>
          <p className="pixel-kicker">Token Planet · 기록</p>
          <h2>성장 일지</h2>
        </div>
        <button type="button" className="growth-journal-refresh" onClick={onReload} disabled={busy} aria-label="성장 일지 새로고침">↻</button>
      </div>

      <div className="growth-journal-controls">
        <label htmlFor="journal-cycle">행성 주기</label>
        <select id="journal-cycle" value={selectedCycle?.cycle_id ?? ""} onChange={(event) => selectCycle(event.target.value)} disabled={!cycles.length}>
          {cycles.map((cycle, index) => (
            <option key={cycle.cycle_id} value={cycle.cycle_id}>
              {cycle.ended_at_utc ? `주기 ${index + 1} · 초기화 ${dateInTimezone(new Date(cycle.ended_at_utc), timezone)}` : "현재 진행 중"}
            </option>
          ))}
        </select>
        <div className="growth-journal-pager" aria-label="날짜 범위">
          <button type="button" onClick={() => setWindowEnd(shiftDate(windowEnd, -7))} aria-label="이전 7일">←</button>
          <span>{rangeStart} — {windowEnd}</span>
          <button type="button" onClick={() => setWindowEnd(shiftDate(windowEnd, Math.min(7, daysBetween(windowEnd, latestEnd))))} disabled={windowEnd >= latestEnd} aria-label="다음 7일">→</button>
        </div>
      </div>

      {selectedCycle && (
        <div className="growth-journal-cycle-note">
          <strong>{selectedCycle.ended_at_utc ? "초기화 완료" : "현재 주기 진행 중"}</strong>
          <span>시작 {selectedCycle.started_at_utc ? formatDateTime(selectedCycle.started_at_utc, timezone) : "시각 확인 불가"}</span>
          {selectedCycle.ended_at_utc && <span>초기화 {formatDateTime(selectedCycle.ended_at_utc, timezone)}</span>}
          {selectedCycle.ended_at_utc && <span>지갑 적립 {selectedCycle.wallet_credit === null ? "원장 값 없음" : <><FormattedNumber value={selectedCycle.wallet_credit} /> 토큰</>}</span>}
        </div>
      )}

      {cycleCredits.length > 0 && (
        <div className="growth-journal-milestones" aria-label="시대 도달 기록">
          {cycleCredits.map(([threshold, date]) => (
            <span key={threshold}>{STAGE_NAMES[MILESTONES.indexOf(threshold) + 1]} · {date}</span>
          ))}
        </div>
      )}

      {busy && !journal.cycles.length && <p className="growth-journal-empty">성장 일지를 불러오는 중입니다.</p>}
      {error && <p className="error-note" role="alert">{error}</p>}
      {!busy && !journal.cycles.length && <p className="growth-journal-empty">확인된 행성 주기가 없습니다.</p>}
      {selectedCycle && visibleDays.length === 0 && <p className="growth-journal-empty">이 기간에 확인된 기록이 없습니다.</p>}
      {visibleDays.length > 0 && (
        <ol className="growth-journal-days">
          {visibleDays.map((day) => (
            <li key={day.date} className="growth-journal-day">
              <div className="growth-journal-day-top">
                <time dateTime={day.date}>{day.date}</time>
                <span>{day.credit === null ? "성장 점수 확인 불가" : <>성장 +<FormattedNumber value={day.credit} maximumFractionDigits={6} /></>}</span>
              </div>
              <strong className="growth-journal-total">{day.total === null ? "확인 불가" : <FormattedNumber value={day.total} />} <small>확인 토큰</small></strong>
              <div className="growth-journal-agents">
                <span><b>Codex</b><em>{coverageLabel(day.codex.coverage)}</em><strong>{day.codex.total === null ? "확인 불가" : <FormattedNumber value={day.codex.total} />}</strong></span>
                <span><b>Claude Code</b><em>{coverageLabel(day.claudeCode.coverage)}</em><strong>{day.claudeCode.total === null ? "확인 불가" : <FormattedNumber value={day.claudeCode.total} />}</strong></span>
              </div>
              {[5, 20, 50, 100].filter((threshold) => milestones.get(threshold) === day.date).map((threshold) => (
                <span className="growth-journal-era" key={threshold}>{STAGE_NAMES[MILESTONES.indexOf(threshold) + 1]} 시대 도달</span>
              ))}
            </li>
          ))}
        </ol>
      )}

      <p className="growth-journal-footnote">확인된 기록만 표시합니다. 원본 사용 기록은 이 기기에 남아 있습니다.</p>
      {canDelete && (
        <button
          className="growth-journal-delete"
          type="button"
          disabled={busy}
          onClick={() => {
            if (window.confirm("계정에 저장된 개인 성장 일지를 삭제할까요? 기존 기록은 숨기고 삭제 시점 이후의 기록부터 다시 표시합니다.")) onDelete();
          }}
        >
          개인 일지 삭제
        </button>
      )}
    </section>
  );
}
