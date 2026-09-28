import type { WorldSnapshot } from "../types/usage";
import { FormattedNumber } from "./FormattedNumber";

export function UsageSummary({ snapshot }: { snapshot: WorldSnapshot }) {
  const known = snapshot.usage.confirmed_subtotal;
  return (
      <section className="usage-summary" aria-label="전체 사용량 (과거 포함)">
      <div className="usage-heading">
        <h2>전체 사용량 (과거 포함)</h2>
        {snapshot.incomplete && <span className="incomplete-label">일부 기록 확인 중</span>}
      </div>
      <p className="usage-number">{known === null ? "아직 확인된 사용량 없음" : <FormattedNumber value={known} />}</p>
      <p className="usage-footnote">
        {known === null ? "기록을 찾으면 합계가 표시됩니다." : snapshot.incomplete ? "확인된 기록만 합산했습니다." : "Codex와 Claude Code의 개인 사용량입니다."}
      </p>
    </section>
  );
}
