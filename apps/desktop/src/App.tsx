import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { PlanetScene } from "./components/PlanetScene";
import { SourceStatus } from "./components/SourceStatus";
import { UsageSummary } from "./components/UsageSummary";
import type { Agent, WorldSnapshot } from "./types/usage";
import "./App.css";

const EMPTY_SNAPSHOT: WorldSnapshot = {
  usage: {
    codex: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    claude_code: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    confirmed_subtotal: null, complete_total: null, scanned_at_utc: "",
  },
  growth_credit: 0, stage: 0, progress_to_next: 0, incomplete: true,
};

function App() {
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [detail, setDetail] = useState(false);
  const [error, setError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    let active = true;
    invoke<WorldSnapshot | null>("current_usage").then((value) => {
      if (active) setSnapshot(value);
    }).catch(() => { if (active) setError(true); });
    const unlisten = listen<WorldSnapshot>("usage-updated", (event) => {
      if (active) { setSnapshot(event.payload); setError(false); }
    });
    return () => { active = false; void unlisten.then((stop) => stop()); };
  }, []);

  async function refresh() {
    setRefreshing(true);
    try {
      setSnapshot(await invoke<WorldSnapshot>("refresh_usage"));
      setError(false);
    } catch {
      setError(true);
    } finally {
      setRefreshing(false);
    }
  }

  async function toggleSource(agent: Agent, enabled: boolean) {
    try {
      setSnapshot(await invoke<WorldSnapshot>("set_source_enabled", { agent, enabled }));
      setError(false);
    } catch { setError(true); }
  }

  const stage = snapshot?.stage ?? 0;
  const progress = snapshot?.progress_to_next ?? 0;
  return (
    <main className={`app-shell ${detail ? "app-shell--detail" : ""}`}>
      <header className="topbar">
        <div className="brand"><span className="brand-symbol" aria-hidden="true">◌</span><span>Token World</span></div>
        <button className="icon-button" type="button" onClick={refresh} disabled={refreshing} aria-label="사용량 새로고침" title="사용량 새로고침">↻</button>
      </header>
      <div className="world-layout">
        <section className="world-visual" aria-label="나의 행성">
          <PlanetScene stage={stage} progress={progress} />
          <div className="stage-progress"><span>다음 변화까지</span><div className="progress-track" role="progressbar" aria-valuenow={Math.round(progress * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="행성 성장 진행도"><span style={{ width: `${progress * 100}%` }} /></div><span>{Math.round(progress * 100)}%</span></div>
        </section>
        <section className="world-info">
          <UsageSummary snapshot={snapshot ?? EMPTY_SNAPSHOT} />
          <div className="source-list" aria-label="수집 상태">
            {snapshot ? <><SourceStatus agent="codex" usage={snapshot.usage.codex} onToggle={detail ? toggleSource : undefined} /><SourceStatus agent="claude_code" usage={snapshot.usage.claude_code} onToggle={detail ? toggleSource : undefined} /></> : <p className="empty-note">로컬 기록을 확인하고 있습니다.</p>}
          </div>
          {error && <p className="error-note" role="alert">사용량을 읽지 못했습니다. 새로고침을 다시 시도하세요.</p>}
          {detail && <div className="detail-note"><h2>함께 만드는 세계</h2><p>확인된 토큰이 매일의 성장 크레딧으로 바뀝니다. 지금은 나만의 세계입니다.</p><p>기록과 대화 내용은 이 기기에만 남습니다.</p></div>}
          <footer className="bottom-actions"><span className="sync-note">이 기기에서만 저장 중</span><button className="text-button" type="button" onClick={() => setDetail(!detail)}>{detail ? "간단히 보기" : "세계 자세히 보기"}</button></footer>
        </section>
      </div>
    </main>
  );
}

export default App;
