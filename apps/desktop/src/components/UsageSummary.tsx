import type { WorldSnapshot } from "../types/usage";

export function UsageSummary({ snapshot }: { snapshot: WorldSnapshot }) {
  const known = snapshot.usage.confirmed_subtotal;
  return (
    <section className="usage-summary" aria-label="내 토큰 사용량">
      <div className="usage-heading">
        <h2>내가 보탠 토큰</h2>
        {snapshot.incomplete && <span className="incomplete-label">집계 불완전</span>}
      </div>
      <p className="usage-number">{known === null ? "아직 확인된 사용량 없음" : known.toLocaleString("ko-KR")}</p>
      <p className="usage-footnote">
        {known === null ? "기록을 찾으면 여기에 합계를 표시합니다." : snapshot.incomplete ? "확인된 토큰만 합산했습니다. 나머지 기록은 아래에서 확인하세요." : "Codex와 Claude Code에서 확인된 합계입니다."}
      </p>
    </section>
  );
}
