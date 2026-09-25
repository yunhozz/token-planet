import type { WorldSnapshot } from "../types/usage";

export function UsageSummary({ snapshot }: { snapshot: WorldSnapshot }) {
  const known = snapshot.usage.confirmed_subtotal;
  return (
      <section className="usage-summary" aria-label="개인 전체 토큰 사용량">
      <div className="usage-heading">
        <h2>개인 전체 사용량</h2>
        {snapshot.incomplete && <span className="incomplete-label">집계 불완전</span>}
      </div>
      <p className="usage-number">{known === null ? "아직 확인된 사용량 없음" : known.toLocaleString("ko-KR")}</p>
      <p className="usage-footnote">
        {known === null ? "기록을 찾으면 여기에 합계를 표시합니다." : snapshot.incomplete ? "개편 전 기록을 포함한 개인 통계입니다. 확인된 토큰만 합산했습니다." : "개편 전 기록을 포함한 Codex와 Claude Code 개인 통계입니다."}
      </p>
    </section>
  );
}
