import type { Agent, TokenUsage } from "../types/usage";

const names: Record<Agent, string> = { codex: "Codex", claude_code: "Claude Code" };
const labels = {
  complete: "확인됨",
  partial: "일부만 확인됨",
  unavailable: "사용량을 확인할 수 없음",
  unsupported: "기록 형식 확인 필요",
  user_disabled: "사용 안 함",
} as const;

export function SourceStatus({ agent, usage, onToggle }: { agent: Agent; usage: TokenUsage; onToggle?: (agent: Agent, enabled: boolean) => void }) {
  const count = usage.total_tokens;
  return (
    <div className="source-row">
      <div className={`source-mark source-mark--${agent}`} aria-hidden="true" />
      <div className="source-copy">
        <span className="source-name">{names[agent]}</span>
        <span className="source-state">{labels[usage.coverage]}</span>
      </div>
      <span className="source-count">{count === null ? "—" : `${count.toLocaleString("ko-KR")} 토큰`}</span>
      {onToggle && <button className="source-toggle" type="button" onClick={() => onToggle(agent, usage.coverage === "user_disabled")}>{usage.coverage === "user_disabled" ? "다시 포함" : "사용 안 함"}</button>}
    </div>
  );
}
