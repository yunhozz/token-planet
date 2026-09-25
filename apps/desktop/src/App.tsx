import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { PlanetScene } from "./components/PlanetScene";
import { SourceStatus } from "./components/SourceStatus";
import { UsageSummary } from "./components/UsageSummary";
import { SharingSetup } from "./components/SharingSetup";
import { InvitePanel } from "./components/InvitePanel";
import { SyncStatus } from "./components/SyncStatus";
import { planetGrowth, sharing, type InviteInfo, type InviteLink, type SharingState, type WorldMember } from "./lib/sharing";
import type { Agent, WorldSnapshot } from "./types/usage";
import "./App.css";

const EMPTY_SNAPSHOT: WorldSnapshot = {
  usage: {
    codex: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    claude_code: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    codex_source: "usage_unavailable", claude_code_source: "usage_unavailable",
    confirmed_subtotal: null, complete_total: null, scanned_at_utc: "",
  },
  growth_credit: 0, stage: 0, progress_to_next: 0, incomplete: true,
};

function App() {
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [detail, setDetail] = useState(false);
  const [error, setError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [shared, setShared] = useState<SharingState | null>(null);
  const [invite, setInvite] = useState<InviteLink | null>(null);
  const [invites, setInvites] = useState<InviteInfo[]>([]);
  const [members, setMembers] = useState<WorldMember[]>([]);
  const [newOwnerId, setNewOwnerId] = useState("");
  const [sharingBusy, setSharingBusy] = useState(false);
  const [sharingError, setSharingError] = useState("");

  useEffect(() => {
    let active = true;
    invoke<WorldSnapshot | null>("current_usage").then((value) => {
      if (active) setSnapshot(value);
    }).catch(() => { if (active) setError(true); });
    sharing.state().then((value) => { if (active) setShared(value); }).catch(() => { if (active) setSharingError("공동 세계 연결을 확인하세요."); });
    const unlisten = listen<WorldSnapshot>("usage-updated", (event) => {
      if (active) { setSnapshot(event.payload); setError(false); }
    }).catch(() => () => {});
    const unlistenCompact = listen("show-compact", () => { if (active) setDetail(false); }).catch(() => () => {});
    const unlistenSync = listen("sync-status-updated", () => {
      void sharing.state().then((value) => { if (active) setShared(value); }).catch(() => {});
    }).catch(() => () => {});
    return () => { active = false; void unlisten.then((stop) => stop()); void unlistenCompact.then((stop) => stop()); void unlistenSync.then((stop) => stop()); };
  }, []);

  useEffect(() => {
    if (shared?.phase !== "shared" || !shared.world?.is_owner) return;
    let active = true;
    sharing.listInvites().then((value) => { if (active) setInvites(value); }).catch(() => {});
    sharing.listMembers().then((value) => { if (active) setMembers(value); }).catch(() => {});
    return () => { active = false; };
  }, [shared?.phase, shared?.world?.id, shared?.world?.is_owner, shared?.world?.member_count]);

  async function changeSharing(action: () => Promise<SharingState>) {
    setSharingBusy(true);
    setSharingError("");
    try {
      setShared(await action());
    } catch (cause) {
      setSharingError(typeof cause === "string" ? cause : "공동 세계 요청을 완료하지 못했습니다.");
    } finally {
      setSharingBusy(false);
    }
  }

  async function requestCode(email: string) {
    setSharingBusy(true);
    setSharingError("");
    try { await sharing.requestCode(email); }
    catch (cause) {
      setSharingError(typeof cause === "string" ? cause : "인증코드를 보내지 못했습니다.");
      throw cause;
    } finally { setSharingBusy(false); }
  }

  async function createInvite() {
    setSharingBusy(true);
    setSharingError("");
    try {
      setInvite(await sharing.createInvite());
      setInvites(await sharing.listInvites());
    } catch (cause) {
      setSharingError(typeof cause === "string" ? cause : "초대 코드를 만들지 못했습니다.");
    } finally { setSharingBusy(false); }
  }

  async function revokeInvite(inviteId: string) {
    setSharingBusy(true);
    setSharingError("");
    try {
      await sharing.revokeInvite(inviteId);
      setInvites(await sharing.listInvites());
      if (invite?.invite_id === inviteId) setInvite(null);
    } catch (cause) {
      setSharingError(typeof cause === "string" ? cause : "초대를 취소하지 못했습니다.");
    } finally { setSharingBusy(false); }
  }

  async function refresh() {
    setRefreshing(true);
    try {
      setSnapshot(await invoke<WorldSnapshot>("refresh_usage"));
      try { setShared(await sharing.state()); } catch { setSharingError("공동 세계 연결을 확인하세요."); }
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

  async function selectFolder(agent: Agent) {
    try {
      const updated = await invoke<WorldSnapshot | null>("choose_source_folder", { agent });
      if (updated) { setSnapshot(updated); setError(false); }
    } catch { setError(true); }
  }

  async function changeView() {
    const next = !detail;
    try { await invoke("set_detail_view", { detail: next }); } catch { /* Browser previews have no native window. */ }
    setDetail(next);
  }

  const { stage, progress } = planetGrowth(snapshot, shared);
  return (
    <main className={`app-shell ${detail ? "app-shell--detail" : ""}`}>
      <header className="topbar">
        <div className="brand"><span className="brand-symbol" aria-hidden="true">◌</span><span>Token World</span></div>
        <button className="icon-button" type="button" onClick={refresh} disabled={refreshing} aria-label="사용량 새로고침" title="사용량 새로고침">↻</button>
      </header>
      <div className="world-layout">
        <section className="world-visual" aria-label={shared?.phase === "shared" ? "함께 키우는 행성" : "나의 행성"}>
          <PlanetScene stage={stage} progress={progress} />
          <div className="stage-progress"><span>다음 변화까지</span><div className="progress-track" role="progressbar" aria-valuenow={Math.round(progress * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="행성 성장 진행도"><span style={{ width: `${progress * 100}%` }} /></div><span>{Math.round(progress * 100)}%</span></div>
        </section>
        <section className="world-info">
          <UsageSummary snapshot={snapshot ?? EMPTY_SNAPSHOT} />
          {shared?.phase === "shared" && shared.world && <div className="group-total"><span>{shared.world.name} · 함께 확인한 토큰</span><strong>{shared.world.known_tokens?.toLocaleString() ?? "아직 확인 중"}</strong>{shared.world.incomplete && <small>일부 사용량을 확인할 수 없음</small>}</div>}
          <div className="source-list" aria-label="수집 상태">
            {snapshot ? <><SourceStatus agent="codex" usage={snapshot.usage.codex} health={snapshot.usage.codex_source} onToggle={detail ? toggleSource : undefined} onSelectFolder={detail ? selectFolder : undefined} /><SourceStatus agent="claude_code" usage={snapshot.usage.claude_code} health={snapshot.usage.claude_code_source} onToggle={detail ? toggleSource : undefined} onSelectFolder={detail ? selectFolder : undefined} /></> : <p className="empty-note">로컬 기록을 확인하고 있습니다.</p>}
          </div>
          {error && <p className="error-note" role="alert">사용량을 읽지 못했습니다. 새로고침을 다시 시도하세요.</p>}
          {detail && <div className="sharing-stack">
            {shared?.phase === "signed_out" || shared?.phase === "signed_in" ? <SharingSetup phase={shared.phase} busy={sharingBusy} onRequestCode={requestCode} onVerifyCode={(email, code) => changeSharing(() => sharing.verifyCode(email, code))} onCreateWorld={(name) => changeSharing(() => sharing.createWorld(name))} onJoinWorld={(code) => changeSharing(() => sharing.joinWorld(code))} /> : null}
            {shared?.phase === "shared" && shared.world && <>
              <InvitePanel memberCount={shared.world.member_count} isOwner={shared.world.is_owner} invite={invite} invites={invites} onCreate={createInvite} onRevoke={revokeInvite} />
              <section className="sharing-panel sharing-manage" aria-label="세계 관리">
                <h2>세계 관리</h2>
                {shared.world.is_owner && shared.world.member_count > 1 && <div className="owner-transfer"><label htmlFor="new-owner">소유권을 넘길 참여자</label><select id="new-owner" value={newOwnerId} onChange={(event) => setNewOwnerId(event.target.value)}><option value="">참여자 선택</option>{members.filter((member) => member.role === "member").map((member) => <option key={member.user_id} value={member.user_id}>참여자 {member.user_id.slice(-8)}</option>)}</select><button type="button" disabled={!newOwnerId || sharingBusy} onClick={() => void changeSharing(() => sharing.transferOwner(newOwnerId))}>소유권 이전</button></div>}
                <button type="button" disabled={sharingBusy} onClick={() => { if (window.confirm("공동 세계의 내 집계를 삭제하고 동기화를 일시정지할까요?")) void changeSharing(sharing.deleteUsage); }}>공유된 내 집계 삭제</button>
                <button type="button" disabled={sharingBusy} onClick={() => { if (window.confirm("이 세계에서 나갈까요? 공유한 내 집계도 삭제됩니다.")) void changeSharing(sharing.leave); }}>세계에서 나가기</button>
              </section>
            </>}
            {shared && shared.phase !== "unavailable" && shared.phase !== "signed_out" && <button className="signout-button" type="button" onClick={() => void changeSharing(sharing.signOut)}>로그아웃</button>}
            {shared?.phase === "unavailable" && <p className="detail-note">공동 세계는 아직 준비 중입니다. 나만의 세계는 계속 사용할 수 있습니다.</p>}
            <p className="privacy-note">원본 기록과 대화 내용은 이 기기에 남고, 공동 세계에는 일별 집계만 보냅니다.</p>
          </div>}
          {sharingError && detail && <p className="error-note" role="alert">{sharingError}</p>}
          <footer className="bottom-actions"><SyncStatus status={shared?.sync_status ?? "local"} pending={shared?.pending ?? 0} lastSyncedAt={shared?.last_synced_at} onPause={() => changeSharing(() => sharing.pause(true))} onResume={() => changeSharing(() => sharing.pause(false))} /><button className="text-button" type="button" onClick={changeView}>{detail ? "간단히 보기" : "세계 자세히 보기"}</button></footer>
        </section>
      </div>
    </main>
  );
}

export default App;
