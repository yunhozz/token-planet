import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { InvitePanel } from "./components/InvitePanel";
import { GrowthJournal } from "./components/GrowthJournal";
import { FormattedNumber } from "./components/FormattedNumber";
import { PlanetProfileSetup } from "./components/PlanetProfileSetup";
import { objectName, objectProgress, PlanetScene, STAGE_NAMES } from "./components/PlanetScene";
import { SharingSetup } from "./components/SharingSetup";
import { SourceStatus } from "./components/SourceStatus";
import { SyncStatus } from "./components/SyncStatus";
import { UsageSummary } from "./components/UsageSummary";
import { WorldCommunity } from "./components/WorldCommunity";
import { CosmeticShop } from "./components/CosmeticShop";
import { sharing, type SharingState, type WorldMember } from "./lib/sharing";
import type { Agent, CosmeticEquipAction, CosmeticPurchaseAction, CosmeticShopState, EquippedCosmetic, GrowthJournal as GrowthJournalData, PlanetAvatar, WorldSnapshot } from "./types/usage";
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
  const [detailTab, setDetailTab] = useState<"planet" | "group">("planet");
  const detailTabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [error, setError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [shared, setShared] = useState<SharingState | null>(null);
  const [memberCode, setMemberCode] = useState<string | null>(null);
  const [members, setMembers] = useState<WorldMember[]>([]);
  const [newOwnerId, setNewOwnerId] = useState("");
  const [sharingBusy, setSharingBusy] = useState(false);
  const [sharingError, setSharingError] = useState("");
  const [planetBusy, setPlanetBusy] = useState(false);
  const [planetError, setPlanetError] = useState("");
  const [resetCheckAt, setResetCheckAt] = useState(0);
  const [journalOpen, setJournalOpen] = useState(false);
  const [journal, setJournal] = useState<GrowthJournalData | null>(null);
  const [journalOwner, setJournalOwner] = useState("");
  const [journalBusy, setJournalBusy] = useState(false);
  const [journalError, setJournalError] = useState("");
  const cosmeticShopContext = JSON.stringify([shared?.user_id ?? null, snapshot?.planet.current_cycle_id ?? null]);
  const cosmeticShopContextRef = useRef(cosmeticShopContext);
  cosmeticShopContextRef.current = cosmeticShopContext;
  const [cosmeticShopEntry, setCosmeticShopEntry] = useState<{ context: string; state: CosmeticShopState } | null>(null);
  const cosmeticShop = cosmeticShopEntry?.context === cosmeticShopContext ? cosmeticShopEntry.state : null;
  const [cosmeticPreview, setCosmeticPreview] = useState<EquippedCosmetic[] | null>(null);
  const [cosmeticShopError, setCosmeticShopError] = useState("");

  function storeCosmeticShop(context: string, shop: CosmeticShopState) {
    if (cosmeticShopContextRef.current === context) setCosmeticShopEntry({ context, state: shop });
  }

  useEffect(() => {
    let active = true;
    sharing.state().then((value) => { if (active) setShared(value); })
      .catch(() => { if (active) setSharingError("공동 세계 연결을 확인하세요."); })
      .then(() => invoke<WorldSnapshot | null>("current_usage"))
      .then((value) => { if (active) setSnapshot(value); })
      .catch(() => { if (active) setError(true); });
    const unlisten = listen<WorldSnapshot>("usage-updated", (event) => {
      if (active) { setSnapshot(event.payload); setError(false); }
    }).catch(() => () => {});
    const unlistenCompact = listen("show-compact", () => {
      if (active) {
        setDetail(false);
        setDetailTab("planet");
        setCosmeticPreview(null);
      }
    }).catch(() => () => {});
    const unlistenSync = listen("sync-status-updated", () => {
      void sharing.state().then((value) => { if (active) setShared(value); }).catch(() => {});
      void invoke<WorldSnapshot | null>("current_usage").then((value) => { if (active && value) setSnapshot(value); }).catch(() => {});
      const context = cosmeticShopContextRef.current;
      void invoke<CosmeticShopState>("get_shop_state")
        .then((value) => { if (active) storeCosmeticShop(context, value); })
        .catch(() => {});
    }).catch(() => () => {});
    return () => { active = false; void unlisten.then((stop) => stop()); void unlistenCompact.then((stop) => stop()); void unlistenSync.then((stop) => stop()); };
  }, []);

  useEffect(() => {
    if (shared?.phase !== "shared") {
      setMemberCode(null);
      setMembers([]);
      setNewOwnerId("");
      return;
    }
    let active = true;
    setMemberCode(null);
    sharing.getMyMemberCode().then((value) => { if (active) setMemberCode(value); }).catch((cause) => {
      if (active) setSharingError(typeof cause === "string" ? cause : "내 개인 코드를 불러오지 못했습니다.");
    });
    if (shared.world?.is_owner) {
      sharing.listMembers().then((value) => {
        if (active) {
          setMembers(value);
          setNewOwnerId((current) => value.some((member) => member.user_id === current && member.role === "member") ? current : "");
        }
      }).catch(() => {});
    } else {
      setMembers([]);
      setNewOwnerId("");
    }
    return () => { active = false; };
  }, [shared]);

  useEffect(() => {
    const context = cosmeticShopContext;
    let active = true;
    setCosmeticShopEntry(null);
    setCosmeticPreview(null);
    setCosmeticShopError("");
    invoke<CosmeticShopState>("get_shop_state")
      .then((value) => { if (active) storeCosmeticShop(context, value); })
      .catch((cause) => { if (active) setCosmeticShopError(typeof cause === "string" ? cause : "상점 상태를 불러오지 못했습니다."); });
    return () => { active = false; };
  }, [cosmeticShopContext]);

  async function changeSharing(action: () => Promise<SharingState>) {
    setSharingBusy(true);
    setSharingError("");
    try { setShared(await action()); }
    catch (cause) { setSharingError(typeof cause === "string" ? cause : "공동 세계 요청을 완료하지 못했습니다."); }
    finally {
      try { setSnapshot(await invoke<WorldSnapshot | null>("current_usage")); }
      catch { setError(true); }
      setSharingBusy(false);
    }
  }

  async function rotateMemberCode() {
    setSharingBusy(true);
    setSharingError("");
    try {
      setMemberCode(await sharing.rotateMyMemberCode());
    } catch (cause) { setSharingError(typeof cause === "string" ? cause : "초대 코드를 다시 발급하지 못했습니다."); }
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
    if (!window.confirm("현재 행성을 초기화할까요? 확인된 이번 행성 토큰은 지갑에 적립되고, 자연 생태계부터 다시 시작합니다.")) return;
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

  async function purchaseCosmetic(sku: string): Promise<CosmeticPurchaseAction> {
    const context = cosmeticShopContext;
    const action = await invoke<CosmeticPurchaseAction>("purchase_cosmetic", { sku });
    storeCosmeticShop(context, action.state);
    if (cosmeticShopContextRef.current === context) setCosmeticShopError("");
    return action;
  }

  async function equipCosmetic(
    slotId: string,
    sku: string | null,
    cycleId: string,
    expectedVersion: number,
  ): Promise<CosmeticEquipAction> {
    const context = cosmeticShopContext;
    const action = await invoke<CosmeticEquipAction>("equip_cosmetic", { slotId, sku, cycleId, expectedVersion });
    storeCosmeticShop(context, action.state);
    if (cosmeticShopContextRef.current === context) setCosmeticShopError("");
    return action;
  }

  async function retryLoadCosmeticShop() {
    const context = cosmeticShopContext;
    setCosmeticShopError("");
    try { storeCosmeticShop(context, await invoke<CosmeticShopState>("get_shop_state")); }
    catch (cause) {
      if (cosmeticShopContextRef.current === context) {
        setCosmeticShopError(typeof cause === "string" ? cause : "상점 상태를 불러오지 못했습니다.");
      }
    }
  }

  async function refreshCosmeticShop() {
    const context = cosmeticShopContext;
    const updated = await invoke<CosmeticShopState>("get_shop_state");
    storeCosmeticShop(context, updated);
    return updated;
  }

  async function changeView() {
    const next = !detail;
    try { await invoke("set_detail_view", { detail: next }); } catch { /* Browser previews have no native window. */ }
    setDetail(next);
    if (next) setDetailTab("planet");
    if (!next) { setJournalOpen(false); setCosmeticPreview(null); }
  }

  function selectDetailTab(tab: "planet" | "group") {
    if (tab === "group") setCosmeticPreview(null);
    setDetailTab(tab);
  }

  function handleDetailTabKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    const currentIndex = detailTab === "planet" ? 0 : 1;
    const nextIndex = event.key === "ArrowRight" ? (currentIndex + 1) % 2
      : event.key === "ArrowLeft" ? (currentIndex + 1) % 2
        : event.key === "Home" ? 0
          : event.key === "End" ? 1
            : -1;
    if (nextIndex < 0) return;
    event.preventDefault();
    const nextTab = nextIndex === 0 ? "planet" : "group";
    selectDetailTab(nextTab);
    detailTabRefs.current[nextIndex]?.focus();
  }

  async function loadGrowthJournal() {
    setJournalBusy(true);
    setJournalError("");
    setJournal(null);
    try {
      const value = await invoke<GrowthJournalData>("get_growth_journal");
      setJournal(value);
      setJournalOwner(shared?.user_id ?? "local");
    } catch (cause) {
      setJournalError(typeof cause === "string" ? cause : "성장 일지를 불러오지 못했습니다.");
    } finally {
      setJournalBusy(false);
    }
  }

  async function deleteGrowthJournal() {
    setJournalBusy(true);
    setJournalError("");
    try {
      const value = await invoke<GrowthJournalData>("delete_growth_journal");
      setJournal(value);
      setJournalOwner(shared?.user_id ?? "local");
    } catch (cause) {
      setJournalError(typeof cause === "string" ? cause : "개인 일지를 삭제하지 못했습니다.");
    } finally {
      setJournalBusy(false);
    }
  }

  const view = snapshot ?? EMPTY_SNAPSHOT;
  const planet = view.planet;
  const profile = planet.profile;
  const canReset = planet.can_reset || Boolean(planet.reset_available_at_utc && resetCheckAt >= Date.parse(planet.reset_available_at_utc));
  const nextObjectProgress = objectProgress(planet.growth_credit, planet.stage);
  const confirmedCosmetics = cosmeticShop?.current_cycle_id === planet.current_cycle_id
    ? cosmeticShop.equipped
    : [];
  const sceneCosmetics = cosmeticPreview ?? confirmedCosmetics;
  const availableWalletBalance = cosmeticShop?.available_balance ?? planet.wallet_balance;
  useEffect(() => {
    if (planet.can_reset || !planet.reset_available_at_utc) return;
    const wait = Math.max(0, Date.parse(planet.reset_available_at_utc) - Date.now()) + 25;
    const timer = window.setTimeout(() => setResetCheckAt(Date.now()), wait);
    return () => window.clearTimeout(timer);
  }, [planet.can_reset, planet.reset_available_at_utc]);
  useEffect(() => {
    if (journalOpen) void loadGrowthJournal();
  }, [journalOpen, shared?.user_id]);
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
        {!detail ? <>
          <section className="world-visual" aria-label="나의 행성">
            <PlanetScene key={planet.current_cycle_id} stage={planet.stage} progress={planet.progress_to_next} avatar={profile!.avatar} objects={planet.objects} equippedCosmetics={sceneCosmetics} compact animate={detail} />
          </section>
          <section className="planet-summary" aria-label="행성 요약">
            <div className="planet-summary-heading">
              <h1>{profile!.nickname}의 행성</h1>
              <div className="summary-era"><span>현재 시대</span><strong>{STAGE_NAMES[planet.stage] ?? STAGE_NAMES[4]}</strong></div>
            </div>
            <div className="summary-token">
              <span className="ledger-label">이번 행성 토큰</span>
              <strong className="ledger-value"><FormattedNumber value={planet.current_planet_tokens} /></strong>
              {planet.incomplete && <span className="incomplete-label">일부 기록 확인 중</span>}
            </div>
            <div className="stage-progress stage-progress--summary">
              {planet.stage < 4 ? <div className="progress-row"><span>다음 시대 진행도</span><div className="progress-track" role="progressbar" aria-valuenow={Math.round(planet.progress_to_next * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="다음 시대 진행도"><span style={{ width: `${planet.progress_to_next * 100}%` }} /></div><span>{Math.round(planet.progress_to_next * 100)}%</span></div> : <p className="final-stage-note">최종 시대 · 발전은 계속됩니다</p>}
            </div>
            <button className="detail-open-button" type="button" onClick={() => void changeView()}>행성·그룹 자세히 보기</button>
          </section>
          <UsageSummary snapshot={view} />
          <div className="source-list" aria-label="수집 상태">
            <SourceStatus agent="codex" usage={view.usage.codex} health={view.usage.codex_source} />
            <SourceStatus agent="claude_code" usage={view.usage.claude_code} health={view.usage.claude_code_source} />
          </div>
        </> : <>
          <div className="detail-navigation">
            <button className="text-button" type="button" onClick={() => void changeView()}>행성으로 돌아가기</button>
            <div className="detail-tabs" role="tablist" aria-label="행성 자세히 보기" onKeyDown={handleDetailTabKeyDown}>
              <button ref={(node) => { detailTabRefs.current[0] = node; }} id="tab-personal" type="button" role="tab" aria-controls="panel-personal" aria-selected={detailTab === "planet"} tabIndex={detailTab === "planet" ? 0 : -1} onClick={() => selectDetailTab("planet")}>내 행성</button>
              <button ref={(node) => { detailTabRefs.current[1] = node; }} id="tab-group" type="button" role="tab" aria-controls="panel-group" aria-selected={detailTab === "group"} tabIndex={detailTab === "group" ? 0 : -1} onClick={() => selectDetailTab("group")}>그룹</button>
            </div>
          </div>
          {detailTab === "planet" ? <section id="panel-personal" className="detail-panel personal-panel" role="tabpanel" aria-labelledby="tab-personal" tabIndex={0}>
            <div className="personal-hero">
              <section className="world-visual" aria-label="나의 행성">
                <PlanetScene key={planet.current_cycle_id} stage={planet.stage} progress={planet.progress_to_next} avatar={profile!.avatar} objects={planet.objects} equippedCosmetics={sceneCosmetics} animate={detail} />
              </section>
              <div className="personal-quick-facts">
                <h1>{profile!.nickname}의 행성</h1>
                <div className="summary-era"><span>현재 시대</span><strong>{STAGE_NAMES[planet.stage] ?? STAGE_NAMES[4]}</strong></div>
                <div className="summary-token">
                  <span className="ledger-label">이번 행성 토큰</span>
                  <strong className="ledger-value"><FormattedNumber value={planet.current_planet_tokens} /></strong>
                  {planet.incomplete && <span className="incomplete-label">일부 기록 확인 중</span>}
                </div>
                <div className="stage-progress">
                  {planet.stage < 4 ? <div className="progress-row"><span>다음 시대 진행도</span><div className="progress-track" role="progressbar" aria-valuenow={Math.round(planet.progress_to_next * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="다음 시대 진행도"><span style={{ width: `${planet.progress_to_next * 100}%` }} /></div><span>{Math.round(planet.progress_to_next * 100)}%</span></div> : <p className="final-stage-note">최종 시대 · 발전은 계속됩니다</p>}
                </div>
              </div>
            </div>
            <div className="personal-information">
              <section className="planet-action-panel" aria-label="행성 기록">
                <h2>행성 기록</h2>
                <div className="planet-ledger">
                  <div className="ledger-cell"><span className="ledger-label">누적 토큰 (개편 후)</span><strong className="ledger-value"><FormattedNumber value={planet.lifetime_tokens} /></strong>{planet.incomplete && <span className="incomplete-label">일부 기록 확인 중</span>}</div>
                  <div className="ledger-cell"><span className="ledger-label">성장 점수</span><strong className="ledger-value"><FormattedNumber value={planet.growth_credit} maximumFractionDigits={12} /></strong>{planet.incomplete && <span className="incomplete-label">일부 기록 확인 중</span>}</div>
                  <div className="ledger-cell"><span className="ledger-label">지갑 잔액</span><strong className="ledger-value"><FormattedNumber value={availableWalletBalance} /></strong></div>
                </div>
                <div className="stage-progress object-progress">
                  <div className="progress-row"><span>다음 오브젝트까지</span><div className="progress-track progress-track--object" role="progressbar" aria-valuenow={Math.round(nextObjectProgress * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="다음 오브젝트 생성 진행도"><span style={{ width: `${nextObjectProgress * 100}%` }} /></div><span>{Math.round(nextObjectProgress * 100)}%</span></div>
                  <p className="recent-object">최근 생성: {planet.objects.length ? objectName(planet.objects[planet.objects.length - 1].kind) : "아직 없음"}</p>
                </div>
              </section>
              <UsageSummary snapshot={view} />
              {cosmeticShop
                ? <CosmeticShop
                  state={cosmeticShop}
                  onPurchase={purchaseCosmetic}
                  onEquip={equipCosmetic}
                  onPreviewChange={setCosmeticPreview}
                  onRefresh={refreshCosmeticShop}
                />
                : !cosmeticShopError && <div className="cosmetic-load-state" role="status"><p>상점을 불러오고 있습니다.</p></div>}
              <div className="source-list" aria-label="수집 상태">
                <SourceStatus agent="codex" usage={view.usage.codex} health={view.usage.codex_source} onToggle={toggleSource} onSelectFolder={selectFolder} />
                <SourceStatus agent="claude_code" usage={view.usage.claude_code} health={view.usage.claude_code_source} onToggle={toggleSource} onSelectFolder={selectFolder} />
              </div>
              <button
                className="growth-journal-entry-button"
                type="button"
                aria-expanded={journalOpen}
                onClick={() => setJournalOpen((open) => !open)}
              >
                {journalOpen ? "성장 일지 접기" : "성장 일지 보기"}
              </button>
              {journalOpen && <>
                {journalBusy && !journal && <p className="growth-journal-empty">성장 일지를 불러오는 중입니다.</p>}
                {journal && journalOwner === (shared?.user_id ?? "local") && <GrowthJournal
                  journal={journal}
                  busy={journalBusy}
                  error={journalError}
                  canDelete={shared?.phase === "signed_in" || shared?.phase === "shared"}
                  onReload={() => void loadGrowthJournal()}
                  onDelete={() => void deleteGrowthJournal()}
                />}
              </>}
              <div className="reset-row">
                <span className="reset-note">확인된 이번 행성 토큰은 지갑에 적립되며, 행성을 자연 생태계부터 다시 시작합니다.{!canReset && planet.reset_available_at_utc && <><br />다음 초기화 가능: {new Date(planet.reset_available_at_utc).toLocaleString("ko-KR")}</>}</span>
                <button className="reset-button" type="button" onClick={() => void resetPlanet()} disabled={!canReset || planetBusy}>{planetBusy ? "처리 중" : "행성 초기화"}</button>
              </div>
            </div>
          </section> : <section id="panel-group" className="detail-panel group-panel" role="tabpanel" aria-labelledby="tab-group" tabIndex={0}>
            <div className="sharing-stack">
              {shared?.phase === "shared" && shared.world && <WorldCommunity name={shared.world.name} members={shared.planet_members ?? []} />}
              {(shared?.phase === "signed_out" || shared?.phase === "signed_in") && <SharingSetup phase={shared.phase} initialNickname={profile?.nickname ?? ""} busy={sharingBusy} onStartAnonymousSession={() => changeSharing(sharing.startAnonymousSession)} onCreateWorld={(name, nickname) => changeSharing(() => sharing.createWorld(name, nickname))} onJoinWorld={(code, nickname) => changeSharing(() => sharing.joinWorld(code, nickname))} />}
              {shared?.phase === "shared" && shared.world && <>
                <InvitePanel memberCount={shared.world.member_count} isOwner={shared.world.is_owner} memberCode={memberCode} busy={sharingBusy} onRotate={rotateMemberCode} />
                <section className="sharing-panel sharing-manage" aria-label="세계 관리">
                  <h2>세계 관리</h2>
                  {shared.world.is_owner && shared.world.member_count > 1 && <div className="owner-transfer"><label htmlFor="new-owner">소유권을 넘길 참여자</label><select id="new-owner" value={newOwnerId} onChange={(event) => setNewOwnerId(event.target.value)}><option value="">참여자 선택</option>{members.filter((member) => member.role === "member").map((member) => <option key={member.user_id} value={member.user_id}>참여자 {member.user_id.slice(-8)}</option>)}</select><button type="button" disabled={!newOwnerId || sharingBusy} onClick={() => void changeSharing(() => sharing.transferOwner(newOwnerId))}>소유권 이전</button></div>}
                  <button type="button" disabled={sharingBusy} onClick={() => { if (window.confirm("공동 세계의 내 집계를 삭제하고 동기화를 일시정지할까요?")) void changeSharing(sharing.deleteUsage); }}>공유된 내 집계 삭제</button>
                  <button type="button" disabled={sharingBusy} onClick={() => { if (window.confirm("이 세계에서 나갈까요? 공유한 내 집계도 삭제됩니다.")) void changeSharing(sharing.leave); }}>세계에서 나가기</button>
                </section>
              </>}
              {shared?.phase === "unavailable" && <p className="detail-note">공동 세계 서버 설정을 확인하세요. 나의 행성은 이 기기에서 계속 자랍니다.</p>}
              <details className="privacy-disclosure">
                <summary>그룹에는 행성 모습과 집계값만 공유됩니다</summary>
                <p>그룹 멤버에게는 닉네임, 아바타, 행성 모습, 이번 행성 토큰, 누적 토큰과 성장 점수가 표시됩니다.</p>
                <p>프롬프트, 응답, 파일 경로와 원본 세션 기록은 기기 밖으로 전송하지 않습니다.</p>
              </details>
            </div>
          </section>}
        </>}
        <div className="app-feedback">
          {error && <p className="error-note" role="alert">사용량을 읽지 못했습니다. 새로고침을 다시 시도하세요.</p>}
          {planetError && <p className="error-note" role="alert">{planetError}</p>}
          {sharingError && <p className="error-note" role="alert">{sharingError}</p>}
          {cosmeticShopError && <p className="error-note" role="alert"><span>{cosmeticShopError}</span> <button className="error-retry" type="button" onClick={() => void retryLoadCosmeticShop()}>다시 불러오기</button></p>}
          {journalError && <p className="error-note" role="alert">{journalError} <button className="error-retry" type="button" onClick={() => void loadGrowthJournal()}>다시 불러오기</button></p>}
        </div>
        <footer className="bottom-actions"><SyncStatus status={shared?.sync_status ?? "local"} pending={shared?.pending ?? 0} lastSyncedAt={shared?.last_synced_at} onPause={() => changeSharing(() => sharing.pause(true))} onResume={() => { if (window.confirm("동기화를 재개하면 내 행성 상태를 서버에 다시 동기화합니다. 공동 세계에 참여 중이면 행성 모습, 이번 행성 토큰, 누적 토큰과 성장 점수가 멤버에게 공개됩니다.")) void changeSharing(() => sharing.pause(false)); }} /></footer>
      </div>
    </main>
  );
}

export default App;
