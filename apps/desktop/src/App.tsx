import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { InvitePanel } from "./components/InvitePanel";
import { GrowthJournal } from "./components/GrowthJournal";
import { FormattedNumber } from "./components/FormattedNumber";
import { LoadingStatus } from "./components/LoadingStatus";
import { PlanetProfileSetup } from "./components/PlanetProfileSetup";
import { objectName, objectProgress, PlanetScene, STAGE_NAMES } from "./components/PlanetScene";
import { fitLandscape } from "./components/planetLandscapeCamera";
import { PlanetLandscape, planetLandscapeBounds, type PlanetExplorationState } from "./components/PlanetLandscape";
import { SharingSetup } from "./components/SharingSetup";
import { SourceStatus } from "./components/SourceStatus";
import { SyncStatus } from "./components/SyncStatus";
import { UsageSummary } from "./components/UsageSummary";
import { WorldCommunity } from "./components/WorldCommunity";
import { CosmeticShop } from "./components/CosmeticShop";
import { sharing, type SharingState, type WorldMember } from "./lib/sharing";
import { openFeatureWindow, type FeatureWindow } from "./lib/featureWindows";
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

const COSMETIC_SHOP_READ_SUPERSEDED = "cosmetic-shop-read-superseded";

type CosmeticShopLoad = {
  generation: number;
  pending: Promise<CosmeticShopState> | null;
};

function isCosmeticShopReadSuperseded(cause: unknown) {
  return cause instanceof Error && cause.message === COSMETIC_SHOP_READ_SUPERSEDED;
}

function broadcastDesktopEvent(name: string, source?: FeatureWindow | "main") {
  return isTauri() ? emit(name, source ?? null).catch(() => {}) : Promise.resolve();
}

function worldContext(shared: SharingState | null, snapshot: WorldSnapshot | null) {
  return JSON.stringify([shared?.phase ?? null, shared?.user_id ?? null,
    shared?.world?.id ?? null, shared?.world?.is_owner ?? false,
    snapshot?.planet.current_cycle_id ?? null]);
}

function App() {
  const requestedWindow = new URLSearchParams(window.location.search).get("window");
  const featureWindow: FeatureWindow | null = requestedWindow === "cosmetic-shop" || requestedWindow === "growth-journal"
    ? requestedWindow
    : null;
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [detail, setDetail] = useState(false);
  const [detailTab, setDetailTab] = useState<"planet" | "group">("planet");
  const [planetExplorationEntry, setPlanetExplorationEntry] = useState<{ context: string; value: PlanetExplorationState } | null>(null);
  const detailTabRefs = useRef<(HTMLButtonElement | null)[]>([]);
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
  const [featureWindowError, setFeatureWindowError] = useState("");
  const [featureWindowRetry, setFeatureWindowRetry] = useState<FeatureWindow | null>(null);
  const cosmeticShopContext = JSON.stringify([shared?.user_id ?? null, snapshot?.planet.current_cycle_id ?? null]);
  const cosmeticShopContextRef = useRef(cosmeticShopContext);
  cosmeticShopContextRef.current = cosmeticShopContext;
  const cosmeticShopLoads = useRef(new Map<string, CosmeticShopLoad>());
  const [cosmeticShopEntry, setCosmeticShopEntry] = useState<{ context: string; state: CosmeticShopState } | null>(null);
  const cosmeticShop = cosmeticShopEntry?.context === cosmeticShopContext ? cosmeticShopEntry.state : null;
  const [cosmeticPreview, setCosmeticPreview] = useState<EquippedCosmetic[] | null>(null);
  const [cosmeticShopError, setCosmeticShopError] = useState("");
  const [cosmeticShopLoadingContext, setCosmeticShopLoadingContext] = useState<string | null>(null);
  const featureActionsBlocked = sharedLoading || sharedContextLockedRef.current;

  function cosmeticShopLoad(context: string) {
    let load = cosmeticShopLoads.current.get(context);
    if (!load) {
      load = { generation: 0, pending: null };
      cosmeticShopLoads.current.set(context, load);
    }
    return load;
  }

  function storeCosmeticShop(context: string, shop: CosmeticShopState, generation?: number) {
    const load = cosmeticShopLoad(context);
    if (!sharedContextLockedRef.current && cosmeticShopContextRef.current === context && (generation === undefined || load.generation === generation)) {
      setCosmeticShopEntry({ context, state: shop });
    }
  }

  function invalidateCosmeticShopReads(context: string) {
    const load = cosmeticShopLoad(context);
    load.generation += 1;
    load.pending = null;
    if (cosmeticShopContextRef.current === context) {
      setCosmeticShopLoadingContext((current) => current === context ? null : current);
    }
    return load.generation;
  }

  function lockWorldContext() {
    worldTransitionEpoch.current += 1;
    sharedContextLockedRef.current = true;
    sharedRequestId.current += 1;
    setSharedLoading(true);
    setCosmeticShopEntry(null);
    setCosmeticPreview(null);
    setCosmeticShopError("");
    invalidateCosmeticShopReads(cosmeticShopContextRef.current);
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
    cosmeticShopContextRef.current = JSON.stringify([nextUserId, nextSnapshot?.planet.current_cycle_id ?? null]);
    setShared(nextShared);
    setSnapshot(nextSnapshot);
    setSharedLoading(false);
    return true;
  }

  function loadCosmeticShop(context: string): Promise<CosmeticShopState> {
    if (sharedContextLockedRef.current) return Promise.reject(new Error(COSMETIC_SHOP_READ_SUPERSEDED));
    const load = cosmeticShopLoad(context);
    if (cosmeticShopContextRef.current === context) {
      setCosmeticShopLoadingContext(context);
      setCosmeticShopError("");
    }
    if (load.pending) return load.pending;

    const generation = load.generation;
    let pending: Promise<CosmeticShopState>;
    pending = invoke<CosmeticShopState>("get_shop_state")
      .then((value) => {
        if (sharedContextLockedRef.current || load.generation !== generation || cosmeticShopContextRef.current !== context) {
          throw new Error(COSMETIC_SHOP_READ_SUPERSEDED);
        }
        storeCosmeticShop(context, value, generation);
        if (cosmeticShopContextRef.current === context) setCosmeticShopError("");
        return value;
      })
      .finally(() => {
        if (load.pending === pending) {
          load.pending = null;
          if (load.generation === generation && cosmeticShopContextRef.current === context) {
            setCosmeticShopLoadingContext((current) => current === context ? null : current);
          }
        }
      });
    load.pending = pending;
    return pending;
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
        if (!invalidate) void loadCosmeticShop(cosmeticShopContextRef.current).catch(() => {});
        if (!invalidate && contextChanged && featureWindow === "growth-journal") void loadGrowthJournal();
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
        setCosmeticPreview(null);
      }
    }).catch(() => () => {});
    const unlistenWorldState = listen("world-state-updated", () => {
      if (active) void refreshSharedContext(sharedContextLockedRef.current);
    }).catch(() => () => {});
    const unlistenWorldContextChanging = listen<string | null>("world-context-changing", (event) => {
      if (active && event.payload !== (featureWindow ?? "main")) lockWorldContext();
    }).catch(() => () => {});
    const unlistenCosmeticShop = listen("cosmetic-shop-updated", () => {
      if (!active || sharedContextLockedRef.current) return;
      setCosmeticPreview(null);
      const context = cosmeticShopContextRef.current;
      invalidateCosmeticShopReads(context);
      void loadCosmeticShop(context).catch((cause) => {
        if (active && cosmeticShopContextRef.current === context && !isCosmeticShopReadSuperseded(cause)) {
          setCosmeticShopError(typeof cause === "string" ? cause : "상점 상태를 불러오지 못했습니다.");
        }
      });
    }).catch(() => () => {});
    const unlistenSync = listen("sync-status-updated", () => {
      if (active && !sharedContextLockedRef.current) void refreshSharedContext(false);
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
      if (event.key !== "Escape" || detail || featureWindow !== null || !isTauri()) return;
      void invoke("hide_popover").catch(() => {
        setTransitionError("팝오버를 닫지 못했습니다. 다시 시도하세요.");
      });
    }
    window.addEventListener("keydown", hidePopoverOnEscape);
    return () => window.removeEventListener("keydown", hidePopoverOnEscape);
  }, [detail, featureWindow]);

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

  useEffect(() => {
    if (sharedContextLockedRef.current) return;
    const context = cosmeticShopContext;
    let active = true;
    setCosmeticShopEntry(null);
    setCosmeticPreview(null);
    setCosmeticShopError("");
    void loadCosmeticShop(context).catch((cause) => {
      if (active && cosmeticShopContextRef.current === context && !isCosmeticShopReadSuperseded(cause)) {
        setCosmeticShopError(typeof cause === "string" ? cause : "상점 상태를 불러오지 못했습니다.");
      }
    });
    return () => { active = false; };
  }, [cosmeticShopContext, sharedLoading]);

  async function changeSharing(action: () => Promise<SharingState>) {
    lockWorldContext();
    setSharingBusy(true);
    setSharingError("");
    await broadcastDesktopEvent("world-context-changing", featureWindow ?? "main");
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
    await broadcastDesktopEvent("world-context-changing", featureWindow ?? "main");
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
    if (sourceBusyRef.current !== null || refreshing || planetBusy) return;
    if (!window.confirm("현재 행성을 초기화할까요? 확인된 이번 행성 토큰은 지갑에 적립되고, 자연 생태계부터 다시 시작합니다.")) return;
    setPlanetBusy(true);
    setPlanetError("");
    try {
      setSnapshot(await invoke<WorldSnapshot>("reset_planet"));
      broadcastDesktopEvent("world-state-updated");
    }
    catch (cause) { setPlanetError(typeof cause === "string" ? cause : "행성을 초기화하지 못했습니다."); }
    finally { setPlanetBusy(false); }
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

  async function purchaseCosmetic(sku: string): Promise<CosmeticPurchaseAction> {
    const context = cosmeticShopContext;
    const epoch = worldTransitionEpoch.current;
    if (sharedContextLockedRef.current || context !== cosmeticShopContextRef.current) {
      throw "계정 또는 행성 상태를 확인하는 동안에는 구매할 수 없습니다.";
    }
    const action = await invoke<CosmeticPurchaseAction>("purchase_cosmetic", { sku });
    if (sharedContextLockedRef.current || epoch !== worldTransitionEpoch.current || context !== cosmeticShopContextRef.current) {
      throw "계정 또는 행성 상태가 바뀌었습니다. 상점 상태를 다시 확인하세요.";
    }
    const generation = invalidateCosmeticShopReads(context);
    storeCosmeticShop(context, action.state, generation);
    if (cosmeticShopContextRef.current === context) setCosmeticShopError("");
    broadcastDesktopEvent("cosmetic-shop-updated");
    return action;
  }

  async function equipCosmetic(
    slotId: string,
    sku: string | null,
    cycleId: string,
    expectedVersion: number,
  ): Promise<CosmeticEquipAction> {
    const context = cosmeticShopContext;
    const epoch = worldTransitionEpoch.current;
    if (sharedContextLockedRef.current || context !== cosmeticShopContextRef.current) {
      throw "계정 또는 행성 상태를 확인하는 동안에는 장착을 변경할 수 없습니다.";
    }
    const action = await invoke<CosmeticEquipAction>("equip_cosmetic", { slotId, sku, cycleId, expectedVersion });
    if (sharedContextLockedRef.current || epoch !== worldTransitionEpoch.current || context !== cosmeticShopContextRef.current) {
      throw "계정 또는 행성 상태가 바뀌었습니다. 상점 상태를 다시 확인하세요.";
    }
    const generation = invalidateCosmeticShopReads(context);
    storeCosmeticShop(context, action.state, generation);
    if (cosmeticShopContextRef.current === context) setCosmeticShopError("");
    broadcastDesktopEvent("cosmetic-shop-updated");
    return action;
  }

  async function retryLoadCosmeticShop() {
    if (sharedContextLockedRef.current) return;
    const context = cosmeticShopContext;
    setCosmeticShopError("");
    try { await loadCosmeticShop(context); }
    catch (cause) {
      if (cosmeticShopContextRef.current === context && !isCosmeticShopReadSuperseded(cause)) {
        setCosmeticShopError(typeof cause === "string" ? cause : "상점 상태를 불러오지 못했습니다.");
      }
    }
  }

  async function refreshCosmeticShop() {
    if (sharedContextLockedRef.current) throw new Error(COSMETIC_SHOP_READ_SUPERSEDED);
    const context = cosmeticShopContext;
    return loadCosmeticShop(context);
  }

  function updateCosmeticPreview(preview: EquippedCosmetic[] | null) {
    if (sharedContextLockedRef.current) return;
    setCosmeticPreview(preview);
  }

  async function launchFeatureWindow(feature: FeatureWindow) {
    if (sharedContextLockedRef.current) return;
    setFeatureWindowError("");
    setFeatureWindowRetry(null);
    try {
      await openFeatureWindow(feature);
    } catch {
      setFeatureWindowError("새 창을 열지 못했습니다. 다시 시도하세요.");
      setFeatureWindowRetry(feature);
    }
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
    camera: fitLandscape(sceneBounds),
    selectedObjectId: null,
  }), [sceneBounds]);
  const planetExploration = planetExplorationEntry?.context === explorationContext
    ? planetExplorationEntry.value
    : initialExploration;
  useEffect(() => {
    setPlanetExplorationEntry((current) => current?.context === explorationContext
      ? current
      : { context: explorationContext, value: initialExploration });
  }, [explorationContext, initialExploration]);
  const updatePlanetExploration = (value: PlanetExplorationState) => {
    setPlanetExplorationEntry({ context: explorationContext, value });
  };
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
    if (featureWindow === "growth-journal" && !sharedContextLockedRef.current) void loadGrowthJournal();
  }, [featureWindow, journalContext, sharedLoading]);
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

  if (featureWindow === "cosmetic-shop") {
    return (
      <main className="app-shell app-shell--feature-window">
        <header className="topbar">
          <div className="brand"><span className="brand-symbol" aria-hidden="true" /><span>행성 꾸미기</span></div>
          <button className="icon-button" type="button" onClick={refresh} disabled={refreshing || sourceBusy !== null || planetBusy} aria-busy={refreshing || undefined} aria-label={refreshing ? "사용량을 갱신하는 중" : "사용량 새로고침"} title="사용량 새로고침">↻</button>
        </header>
        <div className="feature-window-content shop-window-content">
          <section className="shop-planet-preview" aria-label="행성 미리보기">
            <PlanetScene key={planet.current_cycle_id} stage={planet.stage} progress={planet.progress_to_next} avatar={profile!.avatar} objects={planet.objects} equippedCosmetics={sceneCosmetics} animate incomplete={planet.incomplete} cycleId={planet.current_cycle_id} />
            <div className="shop-planet-preview-copy">
              <h1>{profile!.nickname}의 행성</h1>
              <p>장식 미리보기를 눌러 행성 모습을 확인하세요.</p>
              <strong>{STAGE_NAMES[planet.stage] ?? STAGE_NAMES[4]}</strong>
            </div>
          </section>
          <section className="shop-window-store" aria-label="행성 꾸미기 상점">
            {cosmeticShop && <CosmeticShop
              key={cosmeticShopContext}
              state={cosmeticShop}
              onPurchase={purchaseCosmetic}
              onEquip={equipCosmetic}
              onPreviewChange={updateCosmeticPreview}
              onRefresh={refreshCosmeticShop}
              actionsDisabled={featureActionsBlocked}
              showCollapseButton={false}
              initiallyOpen
            />}
            {featureActionsBlocked
              ? <LoadingStatus className="cosmetic-load-state" label="계정과 행성 상태를 확인하고 있습니다." />
              : cosmeticShopLoadingContext === cosmeticShopContext
              ? <LoadingStatus className="cosmetic-load-state" label={cosmeticShop ? "상점 상태를 갱신하고 있습니다." : "상점을 불러오고 있습니다."} />
              : !cosmeticShop && !cosmeticShopError && <LoadingStatus className="cosmetic-load-state" label="상점을 불러오고 있습니다." />}
          </section>
          <div className="app-feedback">
            {error && <p className="error-note" role="alert">사용량을 읽지 못했습니다. 새로고침을 다시 시도하세요.</p>}
            {sharingError && <p className="error-note" role="alert">{sharingError} <button className="error-retry" type="button" onClick={() => void refresh()} disabled={refreshing}>다시 확인</button></p>}
            {cosmeticShopError && <p className="error-note" role="alert"><span>{cosmeticShopError}</span> <button className="error-retry" type="button" onClick={() => void retryLoadCosmeticShop()}>다시 불러오기</button></p>}
          </div>
        </div>
      </main>
    );
  }

  if (featureWindow === "growth-journal") {
    return (
      <main className="app-shell app-shell--feature-window">
        <header className="topbar">
          <div className="brand"><span className="brand-symbol" aria-hidden="true" /><span>Token Planet</span></div>
          <button className="icon-button" type="button" onClick={() => sharedContextLockedRef.current ? void refresh() : void loadGrowthJournal()} disabled={refreshing || (journalBusy && !featureActionsBlocked)} aria-busy={refreshing || journalBusy || undefined} aria-label={featureActionsBlocked ? "계정 상태 다시 확인" : "성장 일지 새로고침"} title={featureActionsBlocked ? "계정 상태 다시 확인" : "성장 일지 새로고침"}>↻</button>
        </header>
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

  return (
    <main className={`app-shell ${detail ? "app-shell--detail" : "app-shell--popover"}`} aria-label={detail ? undefined : "행성 팝오버"}>
      <header className="topbar">
        <div className="brand"><span className="brand-symbol" aria-hidden="true" /><span>Token Planet</span></div>
        <button className="icon-button" type="button" onClick={refresh} disabled={refreshing || sourceBusy !== null || planetBusy} aria-busy={refreshing || undefined} aria-label={refreshing ? "사용량을 갱신하는 중" : "사용량 새로고침"} title="사용량 새로고침">↻</button>
      </header>
      {refreshing && <LoadingStatus className="loading-status--refresh" label="사용량 기록을 갱신하고 있습니다." />}
      <div className="world-layout">
        {!detail ? <>
          <section className="world-visual" aria-label="나의 행성">
            <PlanetScene key={planet.current_cycle_id} stage={planet.stage} progress={planet.progress_to_next} avatar={profile!.avatar} objects={planet.objects} equippedCosmetics={sceneCosmetics} compact animate={detail} interactive publicOnly={false} incomplete={planet.incomplete} cycleId={planet.current_cycle_id} />
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
          <div className="source-list" role="region" aria-label="수집 상태">
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
              <PlanetLandscape
                stage={planet.stage}
                progress={planet.progress_to_next}
                avatar={profile!.avatar}
                objects={planet.objects}
                equippedCosmetics={sceneCosmetics}
                incomplete={planet.incomplete}
                cycleId={planet.current_cycle_id}
                exploration={planetExploration}
                onExplorationChange={updatePlanetExploration}
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
              <div className="planet-feature-actions" aria-label="행성 도구">
                <button type="button" disabled={featureActionsBlocked} onClick={() => void launchFeatureWindow("cosmetic-shop")}>행성 꾸미기</button>
                <button type="button" disabled={featureActionsBlocked} onClick={() => void launchFeatureWindow("growth-journal")}>성장 일지 보기</button>
              </div>
              <div className="source-list" aria-label="수집 상태">
                <SourceStatus agent="codex" usage={view.usage.codex} health={view.usage.codex_source} onToggle={toggleSource} onSelectFolder={selectFolder} busy={sourceBusy !== null || planetBusy || refreshing} pending={sourceBusy === "codex"} error={sourceError?.agent === "codex" ? sourceError.message : undefined} />
                <SourceStatus agent="claude_code" usage={view.usage.claude_code} health={view.usage.claude_code_source} onToggle={toggleSource} onSelectFolder={selectFolder} busy={sourceBusy !== null || planetBusy || refreshing} pending={sourceBusy === "claude_code"} error={sourceError?.agent === "claude_code" ? sourceError.message : undefined} />
              </div>
              <div className="reset-row">
                <span className="reset-note">확인된 이번 행성 토큰은 지갑에 적립되며, 행성을 자연 생태계부터 다시 시작합니다.{!canReset && planet.reset_available_at_utc && <><br />다음 초기화 가능: {new Date(planet.reset_available_at_utc).toLocaleString("ko-KR")}</>}</span>
                <button className="reset-button" type="button" onClick={() => void resetPlanet()} disabled={!canReset || planetBusy || sourceBusy !== null || refreshing}>{planetBusy ? "처리 중" : "행성 초기화"}</button>
              </div>
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
          {featureWindowError && <p className="error-note" role="alert"><span>{featureWindowError}</span>{featureWindowRetry && <> <button className="error-retry" type="button" onClick={() => void launchFeatureWindow(featureWindowRetry)}>다시 시도</button></>}</p>}
          {cosmeticShopError && <p className="error-note" role="alert"><span>{cosmeticShopError}</span> <button className="error-retry" type="button" onClick={() => void retryLoadCosmeticShop()}>다시 불러오기</button></p>}
          {journalError && <p className="error-note" role="alert">{journalError} <button className="error-retry" type="button" onClick={() => void loadGrowthJournal()}>다시 불러오기</button></p>}
        </div>
        <footer className="bottom-actions"><SyncStatus status={shared?.sync_status ?? "local"} pending={shared?.pending ?? 0} lastSyncedAt={shared?.last_synced_at} onPause={() => changeSharing(() => sharing.pause(true))} onResume={() => { if (window.confirm("동기화를 재개하면 내 행성 상태를 서버에 다시 동기화합니다. 공동 세계에 참여 중이면 행성 모습, 이번 행성 토큰, 누적 토큰과 성장 점수가 멤버에게 공개됩니다.")) void changeSharing(() => sharing.pause(false)); }} /></footer>
      </div>
    </main>
  );
}

export default App;
