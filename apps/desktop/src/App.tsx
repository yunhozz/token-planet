import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { InvitePanel } from "./components/InvitePanel";
import { GrowthJournal } from "./components/GrowthJournal";
import { FormattedNumber } from "./components/FormattedNumber";
import { LoadingStatus } from "./components/LoadingStatus";
import { NaturalRemovalDialog } from "./components/NaturalRemovalDialog";
import { PlanetProfileSetup } from "./components/PlanetProfileSetup";
import { objectName, objectProgress, PlanetScene, STAGE_NAMES } from "./components/PlanetScene";
import { initialLandscapeCamera } from "./components/planetLandscapeCamera";
import { PlanetLandscape, planetLandscapeBounds, type PlanetExplorationState } from "./components/PlanetLandscape";
import { SharingSetup } from "./components/SharingSetup";
import { SourceStatus } from "./components/SourceStatus";
import { SyncStatus } from "./components/SyncStatus";
import { UsageSummary } from "./components/UsageSummary";
import { WorldCommunity } from "./components/WorldCommunity";
import { ShopPanel, type ShopPanelPreview } from "./components/ShopPanel";
import { ShopProductThumbnail } from "./components/ShopProductThumbnail";
import { useCompactPopup } from "./hooks/useCompactPopup";
import { useShopActions } from "./hooks/useShopActions";
import { sharing, type SharingState, type WorldMember } from "./lib/sharing";
import type { Agent, GrowthJournal as GrowthJournalData, NaturalObjectKey, PlanetAvatar, ShopActionResult, ShopQuote, ShopRequest, ShopState, WorldSnapshot } from "./types/usage";
import "./App.css";

type FeatureScreen = "planet" | "cosmetic-shop" | "growth-journal";
type ActiveNaturalRemoval = {
  context: string;
  key: NaturalObjectKey;
  label: string;
  generation: number;
};

type ConfirmedResetViewUnavailable = {
  kind: "confirmed_reset_view_unavailable";
  account_id: string;
  request_id: string;
  expected_old_cycle_id: string;
  new_cycle_id: string;
};

type ResetViewRefreshRequired = ConfirmedResetViewUnavailable & { context: string };

function FeatureToolMenu({
  active,
  disabled,
  onNavigate,
}: {
  active: Exclude<FeatureScreen, "planet"> | "planet";
  disabled: boolean;
  onNavigate: (screen: Exclude<FeatureScreen, "planet">) => void;
}) {
  return <nav className="feature-tool-menu" aria-label="행성 도구">
    <button type="button" data-feature-screen-trigger="cosmetic-shop" aria-current={active === "cosmetic-shop" ? "page" : undefined} disabled={disabled} onClick={() => onNavigate("cosmetic-shop")}>행성 상점</button>
    <button type="button" data-feature-screen-trigger="growth-journal" aria-current={active === "growth-journal" ? "page" : undefined} disabled={disabled} onClick={() => onNavigate("growth-journal")}>성장 일지 보기</button>
  </nav>;
}

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
    can_reset: true, reset_available_at_utc: null, objects: [], removed_natural_keys: [],
  },
};

function broadcastDesktopEvent(name: string) {
  return isTauri() ? emit(name, "main").catch(() => {}) : Promise.resolve();
}

function newShopRequestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return `shop-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function sameNaturalKey(left: NaturalObjectKey, right: NaturalObjectKey): boolean {
  return left.cycle_id === right.cycle_id && left.stage === right.stage && left.ordinal === right.ordinal;
}

function worldContext(shared: SharingState | null, snapshot: WorldSnapshot | null) {
  return JSON.stringify([shared?.phase ?? null, shared?.user_id ?? null,
    shared?.world?.id ?? null, shared?.world?.is_owner ?? false,
    snapshot?.planet.current_cycle_id ?? null]);
}

function confirmedResetViewUnavailable(error: unknown): ConfirmedResetViewUnavailable | null {
  if (!error || typeof error !== "object") return null;
  const value = error as Record<string, unknown>;
  if (value.kind !== "confirmed_reset_view_unavailable"
    || typeof value.account_id !== "string"
    || typeof value.request_id !== "string"
    || !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value.request_id)
    || typeof value.expected_old_cycle_id !== "string"
    || typeof value.new_cycle_id !== "string"
    || !value.new_cycle_id
    || value.new_cycle_id === value.expected_old_cycle_id) return null;
  return value as ConfirmedResetViewUnavailable;
}

function App() {
  const requestedWindow = new URLSearchParams(window.location.search).get("window");
  const initialFeatureScreen: FeatureScreen = requestedWindow === "cosmetic-shop" || requestedWindow === "growth-journal"
    ? requestedWindow
    : "planet";
  const [featureScreen, setFeatureScreen] = useState<FeatureScreen>(initialFeatureScreen);
  const featureScreenRef = useRef(featureScreen);
  const featureScreenEpoch = useRef(0);
  featureScreenRef.current = featureScreen;
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [detail, setDetail] = useState(initialFeatureScreen !== "planet");
  const popupRef = useRef<HTMLElement | null>(null);
  const compactHeight = useCompactPopup(popupRef, !detail && navigator.platform.startsWith("Mac") && Boolean(snapshot?.planet.profile));
  const [detailTab, setDetailTab] = useState<"planet" | "group">("planet");
  const [planetExplorationEntry, setPlanetExplorationEntry] = useState<{ context: string; value: PlanetExplorationState } | null>(null);
  const detailTabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const featureScreenHeadingRef = useRef<HTMLHeadingElement | null>(null);
  const featureReturnFocusRef = useRef<Exclude<FeatureScreen, "planet"> | null>(null);
  const [error, setError] = useState(false);
  const [transitionError, setTransitionError] = useState("");
  const [transitionTarget, setTransitionTarget] = useState<boolean | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [shared, setShared] = useState<SharingState | null>(null);
  const [sharedLoading, setSharedLoading] = useState(true);
  const sharedContextLockedRef = useRef(true);
  const worldContextRef = useRef(worldContext(null, null));
  const worldTransitionEpoch = useRef(0);
  const sharingActionErrorRef = useRef(false);
  const sharedRequestId = useRef(0);
  const sharedPhase = shared?.phase ?? null;
  const sharedUserId = shared?.user_id ?? null;
  const sharedWorldId = shared?.world?.id ?? null;
  const sharedIsOwner = shared?.world?.is_owner ?? false;
  const sharedMemberCount = shared?.world?.member_count ?? null;
  const memberContext = JSON.stringify([sharedPhase, sharedUserId, sharedWorldId, sharedIsOwner]);
  const memberListContext = JSON.stringify([memberContext, sharedMemberCount]);
  const [memberCodeEntry, setMemberCodeEntry] = useState<{ context: string; value: string } | null>(null);
  const [memberCodeLoadedContext, setMemberCodeLoadedContext] = useState<string | null>(null);
  const [membersEntry, setMembersEntry] = useState<{ context: string; value: WorldMember[] } | null>(null);
  const [newOwnerEntry, setNewOwnerEntry] = useState<{ context: string; value: string } | null>(null);
  const memberCode = memberCodeEntry?.context === memberContext ? memberCodeEntry.value : null;
  const members = membersEntry?.context === memberListContext ? membersEntry.value : [];
  const newOwnerId = newOwnerEntry?.context === memberContext ? newOwnerEntry.value : "";
  const setNewOwnerId = (value: string) => setNewOwnerEntry({ context: memberContext, value });
  const [sharingBusy, setSharingBusy] = useState(false);
  const [sharingError, setSharingError] = useState("");
  const [planetBusy, setPlanetBusy] = useState(false);
  const resetPendingOwnerRef = useRef<{ context: string; epoch: number } | null>(null);
  const [resetViewRefreshRequired, setResetViewRefreshRequired] = useState<ResetViewRefreshRequired | null>(null);
  const [planetError, setPlanetError] = useState("");
  const [sourceBusy, setSourceBusy] = useState<Agent | null>(null);
  const sourceBusyRef = useRef<Agent | null>(null);
  const [sourceError, setSourceError] = useState<{ agent: Agent; message: string } | null>(null);
  const [memberRefreshRevision, setMemberRefreshRevision] = useState(0);
  const [resetCheckAt, setResetCheckAt] = useState(0);
  const [journal, setJournal] = useState<GrowthJournalData | null>(null);
  const [journalOwner, setJournalOwner] = useState("");
  const [journalBusy, setJournalBusy] = useState(false);
  const [journalError, setJournalError] = useState("");
  const journalContext = shared?.user_id ?? "local";
  const journalContextRef = useRef(journalContext);
  journalContextRef.current = journalContext;
  const journalRequestId = useRef(0);
  const featureActionsBlocked = sharedLoading || sharedContextLockedRef.current;
  const shopAccount = shared?.phase === "signed_out" && shared.user_id === null
    ? { account_id: "local", is_guest: true }
    : (shared && (shared.phase === "signed_in" || shared.phase === "shared") && shared.user_id
      ? { account_id: `account:${shared.user_id}`, is_guest: false }
      : null);
  const shopIdentity = shopAccount && snapshot?.planet.current_cycle_id
    ? JSON.stringify([shopAccount.account_id, snapshot.planet.current_cycle_id])
    : "unverified-shop-context";
  const shopActionsAvailable = Boolean(shopAccount && snapshot?.planet.current_cycle_id)
    && !featureActionsBlocked && shared?.sync_status !== "paused";
  const shopActions = useShopActions({
    account_id: shopAccount?.account_id ?? "",
    current_cycle_id: snapshot?.planet.current_cycle_id ?? "",
    is_guest: shopAccount?.is_guest ?? false,
    online: typeof navigator !== "undefined" && navigator.onLine,
    actions_available: shopActionsAvailable,
    unavailable_reason: featureActionsBlocked
      ? "계정과 행성 상태를 확인하고 있습니다."
      : shared?.sync_status === "paused"
        ? "동기화가 일시 정지되어 상점 작업을 사용할 수 없습니다."
        : !shopAccount
          ? "확인된 계정에서만 상점을 사용할 수 있습니다."
          : null,
  });
  const shopRefreshRef = useRef(shopActions.refresh);
  shopRefreshRef.current = shopActions.refresh;
  const shopContextRef = useRef(shopIdentity);
  shopContextRef.current = shopIdentity;
  const [shopPreview, setShopPreview] = useState<ShopPanelPreview | null>(null);
  const [naturalRemoval, setNaturalRemoval] = useState<ActiveNaturalRemoval | null>(null);
  const [naturalRemovalQuote, setNaturalRemovalQuote] = useState<ShopQuote | null>(null);
  const [naturalRemovalError, setNaturalRemovalError] = useState<string | null>(null);
  const naturalRemovalGeneration = useRef(0);
  const naturalRemovalSubmitting = useRef(false);
  const [selectedLandscapeEntry, setSelectedLandscapeEntry] = useState<{ context: string; instanceId: string } | null>(null);
  const selectedLandscapeInstanceId = selectedLandscapeEntry?.context === shopIdentity ? selectedLandscapeEntry.instanceId : null;

  useEffect(() => {
    if (!shopActionsAvailable) return;
    setShopPreview(null);
    setSelectedLandscapeEntry((current) => current?.context === shopIdentity ? current : null);
    void shopActions.refresh();
  }, [shopIdentity, shopActionsAvailable]);

  function lockWorldContext() {
    worldTransitionEpoch.current += 1;
    sharedContextLockedRef.current = true;
    naturalRemovalGeneration.current += 1;
    naturalRemovalSubmitting.current = false;
    setNaturalRemoval(null);
    setNaturalRemovalQuote(null);
    setNaturalRemovalError(null);
    if (resetPendingOwnerRef.current) {
      resetPendingOwnerRef.current = null;
      setPlanetBusy(false);
      setPlanetError("");
    }
    sharedRequestId.current += 1;
    setSharedLoading(true);
    setShopPreview(null);
    setSelectedLandscapeEntry(null);
    setPlanetExplorationEntry(null);
    journalRequestId.current += 1;
    setJournal(null);
    setJournalOwner("");
    setJournalError("");
  }

  function acceptWorldContext(nextShared: SharingState, nextSnapshot: WorldSnapshot | null, epoch: number, context: string) {
    if (epoch !== worldTransitionEpoch.current || context !== worldContextRef.current) return false;
    const nextUserId = nextShared.user_id ?? null;
    worldContextRef.current = worldContext(nextShared, nextSnapshot);
    sharedContextLockedRef.current = false;
    journalContextRef.current = nextUserId ?? "local";
    setShared(nextShared);
    setSnapshot(nextSnapshot);
    setSharedLoading(false);
    return true;
  }

  useEffect(() => {
    let active = true;
    const refreshSharedContext = async (invalidate = true) => {
      if (invalidate) lockWorldContext();
      const epoch = worldTransitionEpoch.current;
      const context = worldContextRef.current;
      const requestId = ++sharedRequestId.current;
      let nextShared: SharingState | null = null;
      let nextSnapshot: WorldSnapshot | null = null;
      let sharedLoaded = false;
      let snapshotLoaded = false;
      try {
        nextShared = await sharing.state();
        sharedLoaded = true;
      } catch {
        if (active && requestId === sharedRequestId.current) setSharingError("공동 세계 연결을 확인하세요.");
      }
      try {
        nextSnapshot = await invoke<WorldSnapshot | null>("current_usage");
        snapshotLoaded = true;
      } catch {
        if (active && requestId === sharedRequestId.current) setError(true);
      }
      if (!active || requestId !== sharedRequestId.current
        || epoch !== worldTransitionEpoch.current || context !== worldContextRef.current) return;
      if (sharedLoaded && snapshotLoaded && nextShared) {
        const contextChanged = worldContext(nextShared, nextSnapshot) !== context;
        if (contextChanged && !sharedContextLockedRef.current) {
          lockWorldContext();
        }
        if (!acceptWorldContext(nextShared, nextSnapshot, worldTransitionEpoch.current, context)) return;
        if (sharingActionErrorRef.current) sharingActionErrorRef.current = false;
        else setSharingError("");
        setError(false);

        if (!invalidate && contextChanged && featureScreenRef.current === "growth-journal") void loadGrowthJournal();
      } else if (invalidate || sharedContextLockedRef.current) {
        sharedContextLockedRef.current = true;
        setSharedLoading(true);
      }
    };
    const unlisten = listen<WorldSnapshot>("usage-updated", (event) => {
      if (active) { setSnapshot(event.payload); setError(false); }
    }).catch(() => () => {});
    const unlistenScanFailed = listen("usage-scan-failed", () => {
      if (active) setError(true);
    }).catch(() => () => {});
    const unlistenCompact = listen("show-compact", () => {
      if (active) {
        setDetail(false);
        setDetailTab("planet");
        featureScreenRef.current = "planet";
        featureScreenEpoch.current += 1;
        setFeatureScreen("planet");
        setShopPreview(null);
      }
    }).catch(() => () => {});
    const unlistenWorldState = listen("world-state-updated", () => {
      if (active) void refreshSharedContext(sharedContextLockedRef.current);
    }).catch(() => () => {});
    const unlistenWorldContextChanging = listen<string | null>("world-context-changing", (event) => {
      if (active && event.payload !== "main") lockWorldContext();
    }).catch(() => () => {});
    const unlistenCosmeticShop = listen("cosmetic-shop-updated", () => {
      if (!active || sharedContextLockedRef.current) return;
      setShopPreview(null);
      void shopRefreshRef.current();
    }).catch(() => () => {});
    const unlistenSync = listen("sync-status-updated", () => {
      if (!active || sharedContextLockedRef.current) return;
      const capturedContext = worldContextRef.current;
      void refreshSharedContext(false).finally(() => {
        if (active && !sharedContextLockedRef.current && worldContextRef.current === capturedContext) {
          void shopRefreshRef.current();
        }
      });
    }).catch(() => () => {});
    void Promise.all([unlisten, unlistenScanFailed]).then(() => {
      if (active) void refreshSharedContext();
    });
    return () => {
      active = false;
      sharedRequestId.current += 1;
      void unlisten.then((stop) => stop());
      void unlistenScanFailed.then((stop) => stop());
      void unlistenCompact.then((stop) => stop());
      void unlistenWorldState.then((stop) => stop());
      void unlistenWorldContextChanging.then((stop) => stop());
      void unlistenCosmeticShop.then((stop) => stop());
      void unlistenSync.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    function hidePopoverOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape" || detail || !isTauri()) return;
      void invoke("hide_popover").catch(() => {
        setTransitionError("팝오버를 닫지 못했습니다. 다시 시도하세요.");
      });
    }
    window.addEventListener("keydown", hidePopoverOnEscape);
    return () => window.removeEventListener("keydown", hidePopoverOnEscape);
  }, [detail]);

  useEffect(() => {
    const context = JSON.stringify([sharedPhase, sharedUserId, sharedWorldId, sharedIsOwner]);
    const listContext = JSON.stringify([context, sharedMemberCount]);
    setMemberCodeEntry((current) => current?.context === context ? current : null);
    setMembersEntry(null);
    setNewOwnerEntry({ context, value: "" });
    if (sharedPhase !== "shared") {
      return;
    }
    let active = true;
    sharing.getMyMemberCode().then((value) => {
      if (active) {
        setMemberCodeEntry({ context, value });
        setMemberCodeLoadedContext(context);
      }
    }).catch((cause) => {
      if (active) {
        setMemberCodeLoadedContext(context);
        setSharingError(typeof cause === "string" ? cause : "내 개인 코드를 불러오지 못했습니다.");
      }
    });
    if (sharedIsOwner) {
      sharing.listMembers().then((value) => {
        if (active) {
          setMembersEntry({ context: listContext, value });
          setNewOwnerEntry((current) => {
            const currentId = current?.context === context ? current.value : "";
            return { context, value: value.some((member) => member.user_id === currentId && member.role === "member") ? currentId : "" };
          });
        }
      }).catch(() => {
        if (active) {
          setMembersEntry({ context: listContext, value: [] });
          setSharingError("참여자 목록을 불러오지 못했습니다.");
        }
      });
    } else {
      setMembersEntry({ context: listContext, value: [] });
    }
    return () => { active = false; };
  }, [sharedPhase, sharedUserId, sharedWorldId, sharedIsOwner, sharedMemberCount, memberRefreshRevision]);

  async function changeSharing(action: () => Promise<SharingState>) {
    lockWorldContext();
    setSharingBusy(true);
    setSharingError("");
    await broadcastDesktopEvent("world-context-changing");
    const epoch = worldTransitionEpoch.current;
    const context = worldContextRef.current;
    try {
      const nextShared = await action();
      if (epoch === worldTransitionEpoch.current && context === worldContextRef.current) setShared(nextShared);
    }
    catch (cause) {
      if (epoch === worldTransitionEpoch.current && context === worldContextRef.current) {
        sharingActionErrorRef.current = true;
        setSharingError(typeof cause === "string" ? cause : "공동 세계 요청을 완료하지 못했습니다.");
      }
    }
    finally {
      try {
        const nextSnapshot = await invoke<WorldSnapshot | null>("current_usage");
        if (epoch === worldTransitionEpoch.current && context === worldContextRef.current) setSnapshot(nextSnapshot);
      }
      catch { if (epoch === worldTransitionEpoch.current && context === worldContextRef.current) setError(true); }
      setSharingBusy(false);
      void broadcastDesktopEvent("world-state-updated");
    }
  }

  async function rotateMemberCode() {
    const context = memberContext;
    setSharingBusy(true);
    setSharingError("");
    try {
      setMemberCodeEntry({ context, value: await sharing.rotateMyMemberCode() });
    } catch (cause) { setSharingError(typeof cause === "string" ? cause : "초대 코드를 다시 발급하지 못했습니다."); }
    finally { setSharingBusy(false); }
  }

  async function refresh() {
    if (sourceBusyRef.current !== null || refreshing || planetBusy) return;
    setRefreshing(true);
    lockWorldContext();
    await broadcastDesktopEvent("world-context-changing");
    const epoch = worldTransitionEpoch.current;
    const context = worldContextRef.current;
    try {
      const nextSnapshot = await invoke<WorldSnapshot>("refresh_usage");
      const nextShared = await sharing.state();
      if (!acceptWorldContext(nextShared, nextSnapshot, epoch, context)) return;
      setMemberRefreshRevision((revision) => revision + 1);
      broadcastDesktopEvent("world-state-updated");
      setError(false);
    } catch {
      if (epoch !== worldTransitionEpoch.current || context !== worldContextRef.current) return;
      sharedContextLockedRef.current = true;
      setSharedLoading(true);
      setSharingError("계정 또는 행성 상태를 확인하지 못했습니다. 다시 시도하세요.");
      setError(true);
    } finally { setRefreshing(false); }
  }

  async function saveProfile(nickname: string, avatar: PlanetAvatar) {
    if (sourceBusyRef.current !== null || refreshing || planetBusy) return;
    setPlanetBusy(true);
    setPlanetError("");
    try {
      setSnapshot(await invoke<WorldSnapshot>("set_planet_profile", { nickname, avatar }));
      broadcastDesktopEvent("world-state-updated");
    }
    catch (cause) { setPlanetError(typeof cause === "string" ? cause : "행성 프로필을 저장하지 못했습니다."); }
    finally { setPlanetBusy(false); }
  }

  async function resetPlanet() {
    const resetAccountId = shopAccount?.account_id ?? null;
    const resetUserId = shopAccount && !shopAccount.is_guest ? shared?.user_id ?? null : null;
    const resetCycleId = snapshot?.planet.current_cycle_id ?? null;
    const resetContext = shopContextRef.current;
    const resetWorldContext = worldContextRef.current;
    const resetEpoch = worldTransitionEpoch.current;
    const resetOwner = { context: resetContext, epoch: resetEpoch };
    const isSignedResetEligible = Boolean(!shopAccount?.is_guest
      && resetUserId
      && currentShopState
      && currentShopState.account_id === resetAccountId
      && currentShopState.current_cycle_id === resetCycleId
      && !currentShopState.guest_import_pending
      && shopActions.canAct
      && !shopActions.pending
      && shopActionsAvailable
      && planet.can_reset);
    const isGuestResetEligible = shopAccount?.is_guest === true && canReset;
    const resetContextIsCurrent = () => resetAccountId !== null
      && resetCycleId !== null
      && shopActionsAvailable
      && !sharedContextLockedRef.current
      && shopContextRef.current === resetContext
      && worldContextRef.current === resetWorldContext
      && worldTransitionEpoch.current === resetEpoch;
    const thisResetIsCurrent = () => resetPendingOwnerRef.current === resetOwner && resetContextIsCurrent();
    if ((!isGuestResetEligible && !isSignedResetEligible)
      || !resetContextIsCurrent()
      || resetViewRefreshRequired?.context === resetContext
      || resetPendingOwnerRef.current !== null
      || sourceBusyRef.current !== null || refreshing || planetBusy) return;
    if (!window.confirm("현재 행성을 초기화할까요? 확인된 이번 행성 토큰은 지갑에 적립되고, 자연 생태계부터 다시 시작합니다.")) return;
    if ((!isGuestResetEligible && !isSignedResetEligible) || !resetContextIsCurrent()) return;
    resetPendingOwnerRef.current = resetOwner;
    setPlanetBusy(true);
    setPlanetError("");
    try {
      const updated = await invoke<WorldSnapshot>("reset_planet");
      if (!thisResetIsCurrent()) return;
      setSnapshot(updated);
      broadcastDesktopEvent("world-state-updated");
    }
    catch (cause) {
      if (!thisResetIsCurrent()) return;
      const confirmed = confirmedResetViewUnavailable(cause);
      if (confirmed && resetUserId === confirmed.account_id && resetCycleId === confirmed.expected_old_cycle_id) {
        setResetViewRefreshRequired({ ...confirmed, context: resetContext });
        setPlanetError("");
      } else {
        setPlanetError(typeof cause === "string" ? cause : "행성을 초기화하지 못했습니다.");
      }
    }
    finally {
      if (resetPendingOwnerRef.current === resetOwner) {
        resetPendingOwnerRef.current = null;
        setPlanetBusy(false);
      }
    }
  }

  async function runSourceAction(agent: Agent, fallback: string, action: () => Promise<WorldSnapshot | null>) {
    if (sourceBusyRef.current !== null || refreshing || planetBusy) return;
    sourceBusyRef.current = agent;
    setSourceBusy(agent);
    setSourceError(null);
    try {
      const updated = await action();
      if (updated) {
        setSnapshot(updated);
        setError(false);
        broadcastDesktopEvent("world-state-updated");
      }
    } catch (cause) {
      setSourceError({ agent, message: typeof cause === "string" ? cause : fallback });
    } finally {
      sourceBusyRef.current = null;
      setSourceBusy(null);
    }
  }

  async function toggleSource(agent: Agent, enabled: boolean) {
    await runSourceAction(agent, "기록 사용 설정을 변경하지 못했습니다.", () =>
      invoke<WorldSnapshot>("set_source_enabled", { agent, enabled }));
  }

  async function selectFolder(agent: Agent) {
    await runSourceAction(agent, "기록 폴더를 변경하지 못했습니다.", () =>
      invoke<WorldSnapshot | null>("choose_source_folder", { agent }));
  }

  async function applyShopAction(request: ShopRequest): Promise<ShopActionResult | null> {
    if (sharedContextLockedRef.current || shopContextRef.current !== shopIdentity || !shopActionsAvailable) return null;
    return shopActions.apply(request);
  }

  function updateShopPreview(preview: ShopPanelPreview | null) {
    if (sharedContextLockedRef.current || featureScreenRef.current !== "cosmetic-shop"
      || shopContextRef.current !== shopIdentity) return;
    setShopPreview(preview);
  }

  function launchFeatureScreen(feature: Exclude<FeatureScreen, "planet">) {
    if (sharedContextLockedRef.current) return;
    if (featureScreenRef.current === feature) return;
    featureReturnFocusRef.current = feature;
    featureScreenRef.current = feature;
    featureScreenEpoch.current += 1;
    setFeatureScreen(feature);
  }

  function returnToPlanetDetail() {
    setShopPreview(null);
    setDetail(true);
    setDetailTab("planet");
    featureScreenRef.current = "planet";
    featureScreenEpoch.current += 1;
    setFeatureScreen("planet");
  }

  async function changeView(next = !detail) {
    setTransitionError("");
    setTransitionTarget(null);
    try {
      if (isTauri()) await invoke("set_detail_view", { detail: next });
      setDetail(next);
      if (next) setDetailTab("planet");
    } catch {
      setTransitionError("화면을 전환하지 못했습니다. 다시 시도하세요.");
      setTransitionTarget(next);
    }
  }

  function selectDetailTab(tab: "planet" | "group") {
    if (tab === "group") setShopPreview(null);
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
    if (sharedContextLockedRef.current) return;
    const context = journalContextRef.current;
    const requestId = ++journalRequestId.current;
    setJournalBusy(true);
    setJournalError("");
    setJournal(null);
    try {
      const value = await invoke<GrowthJournalData>("get_growth_journal");
      if (sharedContextLockedRef.current || requestId !== journalRequestId.current || journalContextRef.current !== context) return;
      setJournal(value);
      setJournalOwner(context);
    } catch (cause) {
      if (!sharedContextLockedRef.current && requestId === journalRequestId.current && journalContextRef.current === context) {
        setJournalError(typeof cause === "string" ? cause : "성장 일지를 불러오지 못했습니다.");
      }
    } finally {
      if (!sharedContextLockedRef.current && requestId === journalRequestId.current && journalContextRef.current === context) setJournalBusy(false);
    }
  }

  async function deleteGrowthJournal() {
    if (sharedContextLockedRef.current) return;
    const context = journalContext;
    const requestId = ++journalRequestId.current;
    setJournalBusy(true);
    setJournalError("");
    try {
      const value = await invoke<GrowthJournalData>("delete_growth_journal");
      if (sharedContextLockedRef.current || requestId !== journalRequestId.current || journalContextRef.current !== context) return;
      setJournal(value);
      setJournalOwner(context);
    } catch (cause) {
      if (!sharedContextLockedRef.current && requestId === journalRequestId.current && journalContextRef.current === context) {
        setJournalError(typeof cause === "string" ? cause : "개인 일지를 삭제하지 못했습니다.");
      }
    } finally {
      if (!sharedContextLockedRef.current && requestId === journalRequestId.current && journalContextRef.current === context) setJournalBusy(false);
    }
  }

  const view = snapshot ?? EMPTY_SNAPSHOT;
  const planet = view.planet;
  const profile = planet.profile;
  const explorationContext = worldContext(shared, snapshot);
  const sceneBounds = useMemo(() => planetLandscapeBounds(planet.objects), [planet.objects]);
  const initialExploration = useMemo<PlanetExplorationState>(() => ({
    camera: initialLandscapeCamera(sceneBounds),
    selectedObjectId: null,
  }), [sceneBounds]);
  const planetExploration = !sharedContextLockedRef.current && planetExplorationEntry?.context === explorationContext
    ? planetExplorationEntry.value
    : initialExploration;
  useEffect(() => {
    if (sharedContextLockedRef.current) {
      setPlanetExplorationEntry(null);
      return;
    }
    setPlanetExplorationEntry((current) => current?.context === explorationContext
      ? current
      : { context: explorationContext, value: initialExploration });
  }, [explorationContext, initialExploration, sharedLoading]);
  const capturedExplorationContext = explorationContext;
  const capturedExplorationEpoch = worldTransitionEpoch.current;
  const updatePlanetExploration = (value: PlanetExplorationState) => {
    if (sharedContextLockedRef.current || capturedExplorationEpoch !== worldTransitionEpoch.current
      || capturedExplorationContext !== worldContextRef.current) return;
    setPlanetExplorationEntry({ context: capturedExplorationContext, value });
  };
  const currentShopState: ShopState | null = shopAccount && shopActions.state?.account_id === shopAccount.account_id
    && shopActions.state.current_cycle_id === planet.current_cycle_id
    ? shopActions.state
    : null;
  const canReset = planet.can_reset || Boolean(planet.reset_available_at_utc && resetCheckAt >= Date.parse(planet.reset_available_at_utc));
  const activeResetViewRefresh = resetViewRefreshRequired?.context === shopIdentity ? resetViewRefreshRequired : null;
  const signedResetAvailable = Boolean(shopAccount && !shopAccount.is_guest
    && currentShopState
    && currentShopState.account_id === shopAccount.account_id
    && currentShopState.current_cycle_id === planet.current_cycle_id
    && !currentShopState.guest_import_pending
    && shopActions.canAct
    && !shopActions.pending
    && shopActionsAvailable
    && planet.can_reset);
  const resetAvailable = shopAccount?.is_guest ? canReset : signedResetAvailable;
  const resetAccessMessage = !shopAccount
    ? "계정 상태 확인 중이므로 초기화할 수 없습니다."
    : activeResetViewRefresh
      ? "서버에서 행성 초기화가 완료되었습니다. 새 행성 상태를 불러와 주세요."
      : shopAccount.is_guest
        ? null
        : !shopActionsAvailable
          ? shopActions.unavailableReason ?? "로그인 행성 상점 작업을 사용할 수 없습니다."
        : !currentShopState
          ? "로그인 행성 상점 상태를 확인하고 있습니다."
          : !shopActions.canAct
            ? shopActions.unavailableReason ?? "로그인 행성 상점 작업을 사용할 수 없습니다."
            : !planet.can_reset
              ? "서버에서 초기화 대기 시간이 끝나지 않은 것으로 확인했습니다."
              : null;
  const nextObjectProgress = objectProgress(planet.growth_credit, planet.stage);
  const availableWalletBalance = currentShopState?.available_balance ?? planet.wallet_balance;
  const removedNaturalIds = new Set([
    ...(planet.removed_natural_keys ?? []),
    ...(currentShopState?.removed_natural_keys ?? []),
  ].map(({ stage, ordinal }) => `${stage}:${ordinal}`));
  const visiblePlanetObjects = planet.objects.filter((object) => !removedNaturalIds.has(`${object.stage}:${object.ordinal}`));
  const activeNaturalRemoval = naturalRemoval
    && naturalRemoval.context === shopIdentity
    && featureScreen === "planet"
    && detail
    && detailTab === "planet"
    && !sharedContextLockedRef.current
    ? naturalRemoval
    : null;
  const activeNaturalRemovalPending = activeNaturalRemoval && shopActions.pending?.request.kind === "remove_natural"
    && sameNaturalKey(shopActions.pending.request.key, activeNaturalRemoval.key)
    ? shopActions.pending
    : null;
  const activeNaturalRemovalError = activeNaturalRemovalPending?.status === "uncertain"
    ? `${activeNaturalRemovalPending.error ?? "요청 결과를 확인하지 못했습니다."} 제거 확인을 누르면 같은 요청 ID로 다시 확인합니다.`
    : naturalRemovalError;
  const canRequestNaturalRemoval = Boolean(shopAccount && currentShopState
    && currentShopState.account_id === shopAccount.account_id
    && currentShopState.current_cycle_id === planet.current_cycle_id
    && shopActions.canAct
    && shopActionsAvailable
    && (!shopActions.pending || (
      shopActions.pending.status === "uncertain"
      && shopActions.pending.request.kind === "remove_natural"
    ))
    && featureScreen === "planet"
    && detail
    && detailTab === "planet");
  const canRequestNaturalRemovalForKey = (key: NaturalObjectKey) => {
    const pending = shopActions.pending;
    return !pending || (pending.status === "uncertain"
      && pending.request.kind === "remove_natural"
      && sameNaturalKey(pending.request.key, key));
  };
  useEffect(() => {
    naturalRemovalGeneration.current += 1;
    naturalRemovalSubmitting.current = false;
    setNaturalRemoval(null);
    setNaturalRemovalQuote(null);
    setNaturalRemovalError(null);
  }, [shopIdentity, featureScreen, detail, detailTab]);
  useEffect(() => {
    if (planet.can_reset || !planet.reset_available_at_utc) return;
    const wait = Math.max(0, Date.parse(planet.reset_available_at_utc) - Date.now()) + 25;
    const timer = window.setTimeout(() => setResetCheckAt(Date.now()), wait);
    return () => window.clearTimeout(timer);
  }, [planet.can_reset, planet.reset_available_at_utc]);
  useEffect(() => {
    if (featureScreen !== "growth-journal") return;
    if (!sharedContextLockedRef.current) void loadGrowthJournal();
    return () => { journalRequestId.current += 1; };
  }, [featureScreen, journalContext, sharedLoading]);
  useEffect(() => {
    if (initialFeatureScreen !== "planet" && isTauri()) {
      void invoke("set_detail_view", { detail: true }).catch(() => {});
    }
  }, []);

  function requestNaturalRemoval(key: NaturalObjectKey, label: string) {
    const context = shopContextRef.current;
    if (!canRequestNaturalRemoval || !currentShopState || sharedContextLockedRef.current
      || context !== shopIdentity || key.cycle_id !== planet.current_cycle_id
      || !visiblePlanetObjects.some((object) => object.stage === key.stage && object.ordinal === key.ordinal)) return;

    naturalRemovalGeneration.current += 1;
    const generation = naturalRemovalGeneration.current;
    setNaturalRemoval({ context, key, label, generation });
    const pending = shopActions.pending;
    if (pending) {
      if (pending.status !== "uncertain" || pending.request.kind !== "remove_natural"
        || !sameNaturalKey(pending.request.key, key)) return;
      setNaturalRemovalQuote(pending.request.quote);
      setNaturalRemovalError("요청 결과를 확인하지 못했습니다. 제거 확인을 누르면 같은 요청 ID로 다시 확인합니다.");
      return;
    }

    setNaturalRemovalQuote(null);
    setNaturalRemovalError(null);
    void shopActions.quote({ kind: "remove_natural", key }).then((quote) => {
      if (generation !== naturalRemovalGeneration.current || shopContextRef.current !== context
        || sharedContextLockedRef.current || !quote || quote.target.kind !== "remove_natural"
        || !sameNaturalKey(quote.target.key, key)) return;
      setNaturalRemovalQuote(quote);
    }).catch((cause: unknown) => {
      if (generation !== naturalRemovalGeneration.current || shopContextRef.current !== context
        || sharedContextLockedRef.current) return;
      setNaturalRemovalError(cause instanceof Error ? cause.message : "제거 견적을 확인하지 못했습니다.");
    });
  }

  function closeNaturalRemoval() {
    naturalRemovalGeneration.current += 1;
    naturalRemovalSubmitting.current = false;
    setNaturalRemoval(null);
    setNaturalRemovalQuote(null);
    setNaturalRemovalError(null);
  }

  async function confirmNaturalRemoval() {
    const target = activeNaturalRemoval;
    const context = target?.context;
    const pending = shopActions.pending;
    const matchingPending = Boolean(target && pending?.request.kind === "remove_natural"
      && sameNaturalKey(pending.request.key, target.key));
    if (!target || !context || !currentShopState || !canRequestNaturalRemoval
      || naturalRemovalSubmitting.current || sharedContextLockedRef.current
      || context !== shopContextRef.current || context !== shopIdentity
      || target.key.cycle_id !== planet.current_cycle_id
      || (pending && (!matchingPending || pending.status !== "uncertain"))) return;

    const quote = naturalRemovalQuote;
    if (!pending && (!quote || quote.target.kind !== "remove_natural"
      || !sameNaturalKey(quote.target.key, target.key))) return;

    const generation = target.generation;
    naturalRemovalSubmitting.current = true;
    try {
      let result: ShopActionResult | null;
      if (pending?.status === "uncertain" && matchingPending) {
        result = await shopActions.retryPending();
      } else {
        const request: ShopRequest = {
          kind: "remove_natural",
          request_id: newShopRequestId(),
          key: target.key,
          expected_version: 0,
          quote: quote!,
        };
        result = await applyShopAction(request);
      }

      if (!result || generation !== naturalRemovalGeneration.current
        || shopContextRef.current !== context || sharedContextLockedRef.current) return;
      if (result.status === "quote_changed") {
        const confirmedQuote = result.confirmed_quote;
        if (confirmedQuote?.target.kind === "remove_natural"
          && sameNaturalKey(confirmedQuote.target.key, target.key)) {
          setNaturalRemovalQuote(confirmedQuote);
          setNaturalRemovalError("제거 비용이 변경되었습니다. 새 금액을 확인한 뒤 다시 눌러 주세요.");
        } else {
          setNaturalRemovalQuote(null);
          setNaturalRemovalError("새 제거 비용을 확인하지 못했습니다. 다시 견적을 확인해 주세요.");
        }
        return;
      }
      if (result.status === "removed" || result.status === "already_removed") {
        closeNaturalRemoval();
        return;
      }
      setNaturalRemovalError(result.status === "insufficient_balance"
        ? "잔액이 부족해 자연물을 제거하지 못했습니다."
        : "자연물을 제거하지 못했습니다. 상태를 확인한 뒤 다시 시도해 주세요.");
    } catch (cause) {
      if (generation === naturalRemovalGeneration.current
        && shopContextRef.current === context && !sharedContextLockedRef.current) {
        setNaturalRemovalError(cause instanceof Error
          ? cause.message
          : "요청 결과를 확인하지 못했습니다. 연결 상태를 확인한 뒤 다시 시도해 주세요.");
      }
    } finally {
      if (generation === naturalRemovalGeneration.current) naturalRemovalSubmitting.current = false;
    }
  }

  useEffect(() => {
    if (featureScreen !== "planet") {
      featureScreenHeadingRef.current?.focus();
      return;
    }
    const opener = featureReturnFocusRef.current;
    const openerButton = opener
      ? document.querySelector<HTMLButtonElement>(`[data-feature-screen-trigger="${opener}"]`)
      : null;
    if (openerButton) openerButton.focus();
    else if (detail) detailTabRefs.current[0]?.focus();
    featureReturnFocusRef.current = null;
  }, [featureScreen, snapshot === null, profile?.nickname]);
  if (snapshot && !profile) {
    return <>
      <PlanetProfileSetup busy={planetBusy || sourceBusy !== null || refreshing} onSave={saveProfile} />
      {planetError && <p className="error-note setup-error" role="alert">{planetError}</p>}
    </>;
  }
  if (!snapshot) return (
    <main className="setup-screen">
      <div className="setup-mark" aria-hidden="true"><span /></div>
      <p className="pixel-kicker">Token Planet</p>
      <h1>행성 기록을 불러오고 있습니다</h1>
      {(!error || refreshing) && <LoadingStatus label={refreshing ? "사용량 원장을 다시 읽고 있습니다." : "기기 안의 사용량 원장을 읽고 있습니다."} />}
      {error && <>
        <p className="error-note" role="alert">기기 안의 사용량 원장을 열지 못했습니다.</p>
        <button className="error-retry" type="button" onClick={() => void refresh()} disabled={refreshing} aria-busy={refreshing || undefined}>
          {refreshing ? "다시 읽는 중" : "다시 시도"}
        </button>
      </>}
    </main>
  );

  if (featureScreen === "cosmetic-shop") {
    const activeShopState = shopAccount && shopActions.state?.account_id === shopAccount.account_id
      && shopActions.state.current_cycle_id === planet.current_cycle_id
      ? shopActions.state
      : null;
    const previewProduct = shopPreview?.product ?? null;
    const previewEquipment = shopPreview?.kind === "avatar"
      ? shopPreview.equipment
      : activeShopState?.avatar_equipment;
    return (
      <main className="app-shell app-shell--feature-screen">
        <header className="topbar feature-screen-topbar">
          <div className="brand"><span className="brand-symbol" aria-hidden="true" /><span>Token Planet</span></div>
          <FeatureToolMenu active={featureScreen} disabled={featureActionsBlocked} onNavigate={launchFeatureScreen} />
          <h1 ref={featureScreenHeadingRef} className="feature-screen-title" tabIndex={-1}>행성 상점</h1>
          <button className="icon-button" type="button" onClick={() => void shopActions.refresh()} disabled={!shopAccount || featureActionsBlocked} aria-label="상점 상태 새로고침" title="상점 상태 새로고침">↻</button>
        </header>
        <div className="feature-screen-secondary-nav"><button type="button" className="feature-screen-back" onClick={returnToPlanetDetail}>← 행성으로 돌아가기</button></div>
        <div className="feature-window-content shop-window-content">
          <section className="shop-planet-preview" aria-label="행성 미리보기">
            <PlanetScene key={planet.current_cycle_id} stage={planet.stage} progress={planet.progress_to_next} avatar={profile!.avatar} objects={planet.objects} equippedCosmetics={[]} avatarEquipment={previewEquipment} animate incomplete={planet.incomplete} cycleId={planet.current_cycle_id} />
            <div className="shop-planet-preview-copy">
              <h2>{profile!.nickname}의 행성</h2>
              <p>{previewProduct ? `${previewProduct.display_name} 미리보기 중` : "상품을 선택해 행성이나 아바타 모습을 미리 보세요."}</p>
              <strong>{STAGE_NAMES[planet.stage] ?? STAGE_NAMES[4]}</strong>
              <p className="shop-reward-timezone">보상 기준 시간대 · {activeShopState?.reward_state.reward_timezone ?? "확인 중"}</p>
              {previewProduct && <div className="shop-selected-preview" aria-live="polite">
                <ShopProductThumbnail
                  product={previewProduct}
                  instance={shopPreview?.kind === "landscape" ? shopPreview.instance : undefined}
                  avatar={profile!.avatar}
                  equipment={previewEquipment}
                  className="shop-selected-preview-art"
                />
                <div><span>현재 미리보기</span><b>{previewProduct.display_name}</b><small>{shopPreview?.kind === "avatar" ? "확정된 상점 응답을 받으면 행성 아바타에 적용됩니다." : "구매 후 풍경에서 위치를 정하고 설치할 수 있습니다."}</small></div>
                <button type="button" onClick={() => setShopPreview(null)}>미리보기 해제</button>
              </div>}
            </div>
          </section>
          <section className="shop-window-store">
            {featureActionsBlocked && <LoadingStatus className="cosmetic-load-state" label="계정과 행성 상태를 확인하고 있습니다." />}
            {!featureActionsBlocked && !shopAccount && <p className="error-note" role="status">확인된 계정 상태에서 상점을 불러올 수 있습니다.</p>}
            {!featureActionsBlocked && shopAccount && !activeShopState && !shopActions.error && <LoadingStatus className="cosmetic-load-state" label="상점을 불러오고 있습니다." />}
            {activeShopState && <ShopPanel
              key={shopIdentity}
              state={activeShopState}
              quote={shopActions.quote}
              apply={applyShopAction}
              retryPending={shopActions.retryPending}
              pending={shopActions.pending}
              disabledReason={shopActions.unavailableReason}
              avatar={profile!.avatar}
              onPreviewChange={updateShopPreview}
              onSelectLandscapeInstance={(instanceId) => {
                if (shopContextRef.current !== shopIdentity || sharedContextLockedRef.current) return;
                setSelectedLandscapeEntry({ context: shopIdentity, instanceId });
                returnToPlanetDetail();
              }}
            />}
          </section>
          <div className="app-feedback">
            {shopActions.error && <p className="error-note" role="alert"><span>{shopActions.error}</span> <button className="error-retry" type="button" onClick={() => void shopActions.refresh()} disabled={!shopAccount || featureActionsBlocked}>상점 다시 확인</button></p>}
            {error && <p className="error-note" role="alert">사용량을 읽지 못했습니다. 새로고침을 다시 시도하세요.</p>}
            {sharingError && <p className="error-note" role="alert">{sharingError} <button className="error-retry" type="button" onClick={() => void refresh()} disabled={refreshing}>다시 확인</button></p>}
          </div>
        </div>
      </main>
    );
  }

  if (featureScreen === "growth-journal") {
    return (
      <main className="app-shell app-shell--feature-screen">
        <header className="topbar feature-screen-topbar">
          <div className="brand"><span className="brand-symbol" aria-hidden="true" /><span>Token Planet</span></div>
          <FeatureToolMenu active={featureScreen} disabled={featureActionsBlocked} onNavigate={launchFeatureScreen} />
          <h1 ref={featureScreenHeadingRef} className="feature-screen-title" tabIndex={-1}>성장 일지</h1>
          <button className="icon-button" type="button" onClick={() => sharedContextLockedRef.current ? void refresh() : void loadGrowthJournal()} disabled={refreshing || (journalBusy && !featureActionsBlocked)} aria-busy={refreshing || journalBusy || undefined} aria-label={featureActionsBlocked ? "계정 상태 다시 확인" : "성장 일지 새로고침"} title={featureActionsBlocked ? "계정 상태 다시 확인" : "성장 일지 새로고침"}>↻</button>
        </header>
        <div className="feature-screen-secondary-nav"><button type="button" className="feature-screen-back" onClick={returnToPlanetDetail}>← 행성으로 돌아가기</button></div>
        <div className="feature-window-content growth-journal-window-content">
          {featureActionsBlocked && <LoadingStatus label="계정과 행성 상태를 확인하고 있습니다." />}
          {journalBusy && <LoadingStatus label={journal ? "기록 동기화를 기다리며 성장 일지를 갱신하고 있습니다." : "기록 동기화를 기다리며 성장 일지를 불러오고 있습니다."} />}
          {journal && journalOwner === journalContext && <GrowthJournal
            journal={journal}
            busy={journalBusy || featureActionsBlocked}
            error={journalError}
            canDelete={!featureActionsBlocked && (shared?.phase === "signed_in" || shared?.phase === "shared")}
            onReload={() => void loadGrowthJournal()}
            onDelete={() => void deleteGrowthJournal()}
          />}
          {!journal && !journalBusy && journalError && <p className="error-note" role="alert">{journalError} <button className="error-retry" type="button" onClick={() => void loadGrowthJournal()} disabled={featureActionsBlocked}>다시 불러오기</button></p>}
          {sharingError && <p className="error-note" role="alert">{sharingError} <button className="error-retry" type="button" onClick={() => void refresh()} disabled={refreshing}>다시 확인</button></p>}
        </div>
      </main>
    );
  }

  const macosPopup = !detail && navigator.platform.startsWith("Mac");
  const compactPopup = macosPopup && compactHeight;

  return (
    <main ref={popupRef} className={`app-shell ${detail ? "app-shell--detail" : "app-shell--popover"}${macosPopup ? " app-shell--macos-popover" : ""}${compactPopup ? " app-shell--compact-popover" : ""}`} aria-label={detail ? undefined : "행성 팝오버"}>
      <header className="topbar">
        <div className="brand"><span className="brand-symbol" aria-hidden="true" /><span>Token Planet</span></div>
        {detail && <FeatureToolMenu active="planet" disabled={featureActionsBlocked} onNavigate={launchFeatureScreen} />}
        <button className="icon-button" type="button" onClick={refresh} disabled={refreshing || sourceBusy !== null || planetBusy} aria-busy={refreshing || undefined} aria-label={refreshing ? "사용량을 갱신하는 중" : "사용량 새로고침"} title="사용량 새로고침">↻</button>
      </header>
      {refreshing && <LoadingStatus className="loading-status--refresh" label="사용량 기록을 갱신하고 있습니다." />}
      <div className="world-layout">
        {!detail ? <>
          <section className="world-visual" aria-label="나의 행성">
            <PlanetScene key={planet.current_cycle_id} stage={planet.stage} progress={planet.progress_to_next} avatar={profile!.avatar} objects={visiblePlanetObjects} equippedCosmetics={[]} avatarEquipment={currentShopState?.avatar_equipment} compact animate={detail} interactive publicOnly={false} incomplete={planet.incomplete} cycleId={planet.current_cycle_id} />
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
          {!compactPopup && <>
            <UsageSummary snapshot={view} />
            <div className="source-list" role="region" aria-label="수집 상태">
              <SourceStatus agent="codex" usage={view.usage.codex} health={view.usage.codex_source} />
              <SourceStatus agent="claude_code" usage={view.usage.claude_code} health={view.usage.claude_code_source} />
            </div>
          </>}
        </> : <>
          <div className="detail-navigation">
            <button className="text-button detail-return-button" type="button" onClick={() => void changeView()}>← 행성으로 돌아가기</button>
            <div className="detail-tabs" role="tablist" aria-label="행성 자세히 보기" onKeyDown={handleDetailTabKeyDown}>
              <button ref={(node) => { detailTabRefs.current[0] = node; }} id="tab-personal" type="button" role="tab" aria-controls="panel-personal" aria-selected={detailTab === "planet"} tabIndex={detailTab === "planet" ? 0 : -1} onClick={() => selectDetailTab("planet")}>내 행성</button>
              <button ref={(node) => { detailTabRefs.current[1] = node; }} id="tab-group" type="button" role="tab" aria-controls="panel-group" aria-selected={detailTab === "group"} tabIndex={detailTab === "group" ? 0 : -1} onClick={() => selectDetailTab("group")}>그룹</button>
            </div>
          </div>
          {detailTab === "planet" ? <section id="panel-personal" className="detail-panel personal-panel" role="tabpanel" aria-labelledby="tab-personal" tabIndex={0}>
            <div className="personal-hero">
              <PlanetLandscape
                stage={planet.stage}
                progress={planet.progress_to_next}
                avatar={profile!.avatar}
                objects={visiblePlanetObjects}
                equippedCosmetics={[]}
                avatarEquipment={currentShopState?.avatar_equipment}
                incomplete={planet.incomplete}
                cycleId={planet.current_cycle_id}
                exploration={planetExploration}
                onExplorationChange={updatePlanetExploration}
                shopState={currentShopState}
                shopAccountId={shopAccount?.account_id ?? null}
                terrainObjects={planet.objects}
                selectedLandscapeInstanceId={selectedLandscapeInstanceId}
                pendingShopAction={shopActions.pending}
                onShopAction={applyShopAction}
                onSelectLandscapeInstance={(instanceId) => {
                  if (sharedContextLockedRef.current || shopContextRef.current !== shopIdentity) return;
                  setSelectedLandscapeEntry(instanceId ? { context: shopIdentity, instanceId } : null);
                }}
                onRequestNaturalRemoval={canRequestNaturalRemoval ? requestNaturalRemoval : undefined}
                canRequestNaturalRemoval={canRequestNaturalRemovalForKey}
              />
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
            <section className="shop-effects-summary" aria-label="상점 효과">
              <div className="shop-effects-summary-heading">
                <h2>활성 상점 효과</h2>
                <button type="button" onClick={() => launchFeatureScreen("cosmetic-shop")} disabled={featureActionsBlocked}>행성 상점 열기</button>
              </div>
              {currentShopState ? <>
                <dl>
                  <div><dt>토큰 획득</dt><dd>{(currentShopState.effects.token_earning_bps / 100).toFixed(2)}%</dd></div>
                  <div><dt>문명 성장</dt><dd>{(currentShopState.effects.civilization_growth_bps / 100).toFixed(2)}%</dd></div>
                  <div><dt>상점 할인</dt><dd>{(currentShopState.effects.shop_discount_bps / 100).toFixed(2)}%</dd></div>
                  <div><dt>초기화 대기시간</dt><dd>{(currentShopState.effects.reset_cooldown_bps / 100).toFixed(2)}%</dd></div>
                  <div><dt>자연물 제거 할인</dt><dd>{(currentShopState.effects.natural_removal_discount_bps / 100).toFixed(2)}%</dd></div>
                  <div><dt>시대 보상</dt><dd><FormattedNumber value={currentShopState.effects.era_reward_tokens} /> 토큰</dd></div>
                  <div><dt>연속 보상</dt><dd><FormattedNumber value={currentShopState.effects.streak_reward_tokens} /> 토큰</dd></div>
                </dl>
                <p>보상 기준 시간대 · {currentShopState.reward_state.reward_timezone}</p>
              </> : <p>{shopActions.error ?? "상점 효과를 확인하는 중입니다."}</p>}
            </section>
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
              <div className="source-list" aria-label="수집 상태">
                <SourceStatus agent="codex" usage={view.usage.codex} health={view.usage.codex_source} onToggle={toggleSource} onSelectFolder={selectFolder} busy={sourceBusy !== null || planetBusy || refreshing} pending={sourceBusy === "codex"} error={sourceError?.agent === "codex" ? sourceError.message : undefined} />
                <SourceStatus agent="claude_code" usage={view.usage.claude_code} health={view.usage.claude_code_source} onToggle={toggleSource} onSelectFolder={selectFolder} busy={sourceBusy !== null || planetBusy || refreshing} pending={sourceBusy === "claude_code"} error={sourceError?.agent === "claude_code" ? sourceError.message : undefined} />
              </div>
              <div className="reset-row">
                <span className="reset-note">{resetAccessMessage ?? "확인된 이번 행성 토큰은 지갑에 적립되며, 행성을 자연 생태계부터 다시 시작합니다."}{shopAccount?.is_guest && !canReset && planet.reset_available_at_utc && <><br />다음 초기화 가능: {new Date(planet.reset_available_at_utc).toLocaleString("ko-KR")}</>}</span>
                <button className="reset-button" type="button" onClick={() => void resetPlanet()} disabled={!resetAvailable || featureActionsBlocked || activeResetViewRefresh !== null || planetBusy || sourceBusy !== null || refreshing}>{planetBusy ? "처리 중" : activeResetViewRefresh ? "초기화 완료" : "행성 초기화"}</button>
              </div>
              {activeResetViewRefresh && <p className="reset-completed-note" role="status">
                서버에서 초기화가 완료됐습니다. 화면 갱신만 필요합니다.
                {" "}<button className="error-retry" type="button" onClick={() => void refresh()} disabled={planetBusy || sourceBusy !== null || refreshing}>완료된 초기화 상태 새로고침</button>
              </p>}
            </div>
          </section> : <section id="panel-group" className="detail-panel group-panel" role="tabpanel" aria-labelledby="tab-group" tabIndex={0}>
            <div className="sharing-stack">
              {sharedLoading && <LoadingStatus label="기록 동기화를 기다리며 그룹 정보를 확인하고 있습니다." />}
                {shared?.phase === "shared" && shared.world && <>
                  <WorldCommunity name={shared.world.name} members={shared.planet_members ?? []} />
                  {((sharedPhase === "shared" && memberCodeLoadedContext !== memberContext) || (sharedIsOwner && membersEntry?.context !== memberListContext))
                    ? <LoadingStatus label="초대 및 참여자 정보를 불러오고 있습니다." />
                    : <>
                      <InvitePanel memberCount={shared.world.member_count} isOwner={shared.world.is_owner} memberCode={memberCode} busy={sharingBusy} onRotate={rotateMemberCode} />
                      <section className="sharing-panel sharing-manage" aria-label="세계 관리">
                        <h2>세계 관리</h2>
                        {shared.world.is_owner && shared.world.member_count > 1 && <div className="owner-transfer"><label htmlFor="new-owner">소유권을 넘길 참여자</label><select id="new-owner" value={newOwnerId} onChange={(event) => setNewOwnerId(event.target.value)}><option value="">참여자 선택</option>{members.filter((member) => member.role === "member").map((member) => <option key={member.user_id} value={member.user_id}>참여자 {member.user_id.slice(-8)}</option>)}</select><button type="button" disabled={!newOwnerId || sharingBusy} onClick={() => void changeSharing(() => sharing.transferOwner(newOwnerId))}>소유권 이전</button></div>}
                        <button type="button" disabled={sharingBusy} onClick={() => { if (window.confirm("공동 세계의 내 집계를 삭제하고 동기화를 일시정지할까요?")) void changeSharing(sharing.deleteUsage); }}>공유된 내 집계 삭제</button>
                        <button type="button" disabled={sharingBusy} onClick={() => { if (window.confirm("이 세계에서 나갈까요? 공유한 내 집계도 삭제됩니다.")) void changeSharing(sharing.leave); }}>세계에서 나가기</button>
                      </section>
                    </>}
                </>}
                {(shared?.phase === "signed_out" || shared?.phase === "signed_in") && <SharingSetup phase={shared.phase} initialNickname={profile?.nickname ?? ""} busy={sharingBusy} onStartAnonymousSession={() => changeSharing(sharing.startAnonymousSession)} onCreateWorld={(name, nickname) => changeSharing(() => sharing.createWorld(name, nickname))} onJoinWorld={(code, nickname) => changeSharing(() => sharing.joinWorld(code, nickname))} />}
                {shared?.phase === "unavailable" && <p className="detail-note">공동 세계 서버 설정을 확인하세요. 나의 행성은 이 기기에서 계속 자랍니다.</p>}
                {!shared && !sharedLoading && <p className="detail-note">공동 세계 정보를 확인하지 못했습니다.</p>}
              <details className="privacy-disclosure">
                <summary>그룹에는 행성 모습과 집계값만 공유됩니다</summary>
                <p>그룹 멤버에게는 닉네임, 아바타, 행성 모습, 이번 행성 토큰, 누적 토큰과 성장 점수가 표시됩니다.</p>
                <p>프롬프트, 응답, 파일 경로와 원본 세션 기록은 기기 밖으로 전송하지 않습니다.</p>
              </details>
            </div>
          </section>}
        </>}
        <div className="app-feedback">
          {transitionError && <p className="error-note" role="alert"><span>{transitionError}</span>{transitionTarget !== null && <> <button className="error-retry" type="button" onClick={() => void changeView(transitionTarget)}>다시 시도</button></>}</p>}
          {error && <p className="error-note" role="alert">사용량을 읽지 못했습니다. 새로고침을 다시 시도하세요.</p>}
          {planetError && <p className="error-note" role="alert">{planetError}</p>}
          {sharingError && <p className="error-note" role="alert">{sharingError}</p>}
        </div>
        {!compactPopup && <footer className="bottom-actions"><SyncStatus status={shared?.sync_status ?? "local"} pending={shared?.pending ?? 0} lastSyncedAt={shared?.last_synced_at} onPause={() => changeSharing(() => sharing.pause(true))} onResume={() => { if (window.confirm("동기화를 재개하면 내 행성 상태를 서버에 다시 동기화합니다. 공동 세계에 참여 중이면 행성 모습, 이번 행성 토큰, 누적 토큰과 성장 점수가 멤버에게 공개됩니다.")) void changeSharing(() => sharing.pause(false)); }} /></footer>}
      </div>
      {activeNaturalRemoval && currentShopState && <NaturalRemovalDialog
        target={activeNaturalRemoval.key}
        quote={naturalRemovalQuote}
        pending={activeNaturalRemovalPending?.status === "submitting"}
        allowUnaffordableRetry={activeNaturalRemovalPending?.status === "uncertain"}
        confirmedWalletBalance={currentShopState.available_balance}
        onConfirm={() => void confirmNaturalRemoval()}
        onCancel={closeNaturalRemoval}
        objectName={activeNaturalRemoval.label}
        error={activeNaturalRemovalError}
      />}
    </main>
  );
}

export default App;
