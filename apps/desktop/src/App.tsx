import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { InvitePanel } from "./components/InvitePanel";
import { PlanetProfileSetup } from "./components/PlanetProfileSetup";
import { objectName, objectProgress, PlanetScene, STAGE_NAMES } from "./components/PlanetScene";
import { SharingSetup } from "./components/SharingSetup";
import { SourceStatus } from "./components/SourceStatus";
import { SyncStatus } from "./components/SyncStatus";
import { UsageSummary } from "./components/UsageSummary";
import { WorldCommunity } from "./components/WorldCommunity";
import { AvatarSprite } from "./components/AvatarSprite";
import { sharing, type InviteInfo, type InviteLink, type SharingState, type WorldMember } from "./lib/sharing";
import type { Agent, PlanetAvatar, WorldSnapshot } from "./types/usage";
import "./App.css";

const EMPTY_SNAPSHOT: WorldSnapshot = {
  usage: {
    codex: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    claude_code: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, coverage: "unavailable" },
    codex_source: "usage_unavailable", claude_code_source: "usage_unavailable",
    confirmed_subtotal: null, complete_total: null, scanned_at_utc: "",
  },
  growth_credit: 0, stage: 0, progress_to_next: 0, incomplete: true,
  planet: {
    version: 1, profile: null, timezone: "", current_cycle_id: "", cycle_started_at_utc: "", last_reset_at_utc: null,
    wallet_balance: 0, wallet_credits: [], current_planet_tokens: 0, lifetime_tokens: 0,
    growth_credit: 0, stage: 0, progress_to_next: 0, incomplete: true,
    can_reset: true, reset_available_at_utc: null, objects: [],
  },
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
  const [planetBusy, setPlanetBusy] = useState(false);
  const [planetError, setPlanetError] = useState("");
  const [resetCheckAt, setResetCheckAt] = useState(0);

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
      void invoke<WorldSnapshot | null>("current_usage").then((value) => { if (active && value) setSnapshot(value); }).catch(() => {});
    }).catch(() => () => {});
    return () => { active = false; void unlisten.then((stop) => stop()); void unlistenCompact.then((stop) => stop()); void unlistenSync.then((stop) => stop()); };
  }, []);

  useEffect(() => {
    if (shared?.phase !== "shared" || !shared.world?.is_owner) return;
    let active = true;
    sharing.listInvites().then((value) => { if (active) setInvites(value); }).catch(() => {});
    sharing.listMembers().then((value) => {
      if (active) {
        setMembers(value);
        setNewOwnerId((current) => value.some((member) => member.user_id === current && member.role === "member") ? current : "");
      }
    }).catch(() => {});
    return () => { active = false; };
  }, [shared]);

  async function changeSharing(action: () => Promise<SharingState>) {
    setSharingBusy(true);
    setSharingError("");
    try { setShared(await action()); }
    catch (cause) { setSharingError(typeof cause === "string" ? cause : "공동 세계 요청을 완료하지 못했습니다."); }
    finally { setSharingBusy(false); }
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
    } catch (cause) { setSharingError(typeof cause === "string" ? cause : "초대 코드를 만들지 못했습니다."); }
    finally { setSharingBusy(false); }
  }

  async function revokeInvite(inviteId: string) {
    setSharingBusy(true);
    setSharingError("");
    try {
      await sharing.revokeInvite(inviteId);
      setInvites(await sharing.listInvites());
      if (invite?.invite_id === inviteId) setInvite(null);
    } catch (cause) { setSharingError(typeof cause === "string" ? cause : "초대를 취소하지 못했습니다."); }
    finally { setSharingBusy(false); }
  }

  async function refresh() {
    setRefreshing(true);
    try {
      setSnapshot(await invoke<WorldSnapshot>("refresh_usage"));
      try { setShared(await sharing.state()); } catch { setSharingError("공동 세계 연결을 확인하세요."); }
      setError(false);
    } catch { setError(true); }
    finally { setRefreshing(false); }
  }

  async function saveProfile(nickname: string, avatar: PlanetAvatar) {
    setPlanetBusy(true);
    setPlanetError("");
    try { setSnapshot(await invoke<WorldSnapshot>("set_planet_profile", { nickname, avatar })); }
    catch (cause) { setPlanetError(typeof cause === "string" ? cause : "행성 프로필을 저장하지 못했습니다."); }
    finally { setPlanetBusy(false); }
  }

  async function resetPlanet() {
    if (!window.confirm("현재 행성을 초기화할까요? 확인된 이번 행성의 토큰은 지갑에 한 번 적립됩니다.")) return;
    setPlanetBusy(true);
    setPlanetError("");
    try { setSnapshot(await invoke<WorldSnapshot>("reset_planet")); }
    catch (cause) { setPlanetError(typeof cause === "string" ? cause : "행성을 초기화하지 못했습니다."); }
    finally { setPlanetBusy(false); }
  }

  async function toggleSource(agent: Agent, enabled: boolean) {
    try { setSnapshot(await invoke<WorldSnapshot>("set_source_enabled", { agent, enabled })); setError(false); }
    catch { setError(true); }
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

  const view = snapshot ?? EMPTY_SNAPSHOT;
  const planet = view.planet;
  const profile = planet.profile;
  const canReset = planet.can_reset || Boolean(planet.reset_available_at_utc && resetCheckAt >= Date.parse(planet.reset_available_at_utc));
  const nextObjectProgress = objectProgress(planet.growth_credit, planet.stage);
  useEffect(() => {
    if (planet.can_reset || !planet.reset_available_at_utc) return;
    const wait = Math.max(0, Date.parse(planet.reset_available_at_utc) - Date.now()) + 25;
    const timer = window.setTimeout(() => setResetCheckAt(Date.now()), wait);
    return () => window.clearTimeout(timer);
  }, [planet.can_reset, planet.reset_available_at_utc]);
  if (snapshot && !profile) {
    return <>
      <PlanetProfileSetup busy={planetBusy} onSave={saveProfile} />
      {planetError && <p className="error-note setup-error" role="alert">{planetError}</p>}
    </>;
  }
  if (!snapshot) return <main className="setup-screen"><div className="setup-mark" aria-hidden="true"><span /></div><p className="pixel-kicker">Token Planet</p><h1>행성 기록을 불러오고 있습니다</h1>{error && <p className="error-note" role="alert">기기 안의 사용량 원장을 열지 못했습니다.</p>}</main>;

  return (
    <main className={`app-shell ${detail ? "app-shell--detail" : ""}`}>
      <header className="topbar">
        <div className="brand"><span className="brand-symbol" aria-hidden="true" /><span>Token Planet</span></div>
        <button className="icon-button" type="button" onClick={refresh} disabled={refreshing} aria-label="사용량 새로고침" title="사용량 새로고침">↻</button>
      </header>
      <div className="world-layout">
        <section className="world-visual" aria-label="나의 행성">
          <PlanetScene stage={planet.stage} progress={planet.progress_to_next} avatar={profile!.avatar} objects={planet.objects} />
          <div className="stage-progress">
            {planet.stage < 4 ? <div className="progress-row"><span>다음 시대까지</span><div className="progress-track" role="progressbar" aria-valuenow={Math.round(planet.progress_to_next * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="다음 시대 진행도"><span style={{ width: `${planet.progress_to_next * 100}%` }} /></div><span>{Math.round(planet.progress_to_next * 100)}%</span></div> : <p className="final-stage-note">최종 시대 · 발전은 계속됩니다</p>}
            <div className="progress-row"><span>다음 오브젝트까지</span><div className="progress-track progress-track--object" role="progressbar" aria-valuenow={Math.round(nextObjectProgress * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="다음 오브젝트 생성 진행도"><span style={{ width: `${nextObjectProgress * 100}%` }} /></div><span>{Math.round(nextObjectProgress * 100)}%</span></div>
            <p className="recent-object">최근 생성: {planet.objects.length ? objectName(planet.objects[planet.objects.length - 1].kind) : "아직 없음"}</p>
          </div>
        </section>
        <section className="world-info">
          <div className="planet-action-panel" aria-label="현재 행성 상태">
            <div className="planet-profile-line"><AvatarSprite avatar={profile!.avatar} /><strong>{profile!.nickname}의 행성</strong></div>
            <div className="planet-ledger">
              <div className="ledger-cell"><span className="ledger-label">현재 행성 토큰</span><strong className="ledger-value">{planet.current_planet_tokens.toLocaleString("ko-KR")}</strong></div>
              <div className="ledger-cell"><span className="ledger-label">개편 후 누적</span><strong className="ledger-value">{planet.lifetime_tokens.toLocaleString("ko-KR")}</strong></div>
              <div className="ledger-cell"><span className="ledger-label">문명 발전 점수</span><strong className="ledger-value">{planet.growth_credit.toLocaleString("ko-KR", { maximumFractionDigits: 12 })}</strong></div>
              <div className="ledger-cell"><span className="ledger-label">지갑 잔액</span><strong className="ledger-value">{planet.wallet_balance.toLocaleString("ko-KR")}</strong></div>
            </div>
            <p className="credit-line"><span>현재 시대</span><strong>{STAGE_NAMES[planet.stage] ?? STAGE_NAMES[4]}</strong></p>
            {planet.incomplete && <p className="usage-footnote">확인된 토큰만 성장에 반영했습니다. 집계되지 않은 기록이 있습니다.</p>}
            <div className="reset-row">
              <span className="reset-note">초기화하면 이번 행성의 확인된 토큰을 지갑에 적립하고 자연 생태계부터 다시 시작합니다.{!canReset && planet.reset_available_at_utc && <><br />다음 초기화 가능: {new Date(planet.reset_available_at_utc).toLocaleString("ko-KR")}</>}</span>
              <button className="reset-button" type="button" onClick={() => void resetPlanet()} disabled={!canReset || planetBusy}>{planetBusy ? "처리 중" : "행성 초기화"}</button>
            </div>
          </div>
          <UsageSummary snapshot={view} />
          <div className="source-list" aria-label="수집 상태">
            <SourceStatus agent="codex" usage={view.usage.codex} health={view.usage.codex_source} onToggle={detail ? toggleSource : undefined} onSelectFolder={detail ? selectFolder : undefined} />
            <SourceStatus agent="claude_code" usage={view.usage.claude_code} health={view.usage.claude_code_source} onToggle={detail ? toggleSource : undefined} onSelectFolder={detail ? selectFolder : undefined} />
          </div>
          {error && <p className="error-note" role="alert">사용량을 읽지 못했습니다. 새로고침을 다시 시도하세요.</p>}
          {planetError && <p className="error-note" role="alert">{planetError}</p>}
          {detail && <div className="sharing-stack">
            {shared?.phase === "shared" && shared.world && <WorldCommunity name={shared.world.name} members={shared.planet_members ?? []} />}
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
            {shared?.phase === "unavailable" && <p className="detail-note">공동 세계 서버 설정을 확인하세요. 나의 행성은 이 기기에서 계속 자랍니다.</p>}
            <p className="privacy-note">프롬프트, 응답, 파일 경로와 원본 세션 기록은 기기 밖으로 전송하지 않습니다. 그룹에는 닉네임, 아바타, 행성 모습, 개편 후 사용량과 성장 점수만 표시합니다.</p>
          </div>}
          {sharingError && detail && <p className="error-note" role="alert">{sharingError}</p>}
          <footer className="bottom-actions"><SyncStatus status={shared?.sync_status ?? "local"} pending={shared?.pending ?? 0} lastSyncedAt={shared?.last_synced_at} onPause={() => changeSharing(() => sharing.pause(true))} onResume={() => { if (window.confirm("동기화를 재개하면 내 행성 상태를 서버에 다시 동기화합니다. 공동 세계에 참여 중이면 개편 후 누적 사용량, 행성 모습과 발전 점수가 멤버에게 공개됩니다.")) void changeSharing(() => sharing.pause(false)); }} /><button className="text-button" type="button" onClick={changeView}>{detail ? "행성으로 돌아가기" : "행성·그룹 자세히 보기"}</button></footer>
        </section>
      </div>
    </main>
  );
}

export default App;
