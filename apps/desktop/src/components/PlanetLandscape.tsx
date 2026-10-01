import {
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { CSSProperties } from "react";
import type { PendingShopAction } from "../hooks/useShopActions";
import type {
  AvatarEquipment,
  EquippedCosmetic,
  LandscapeInstance,
  LandscapePlacement as ShopLandscapePlacement,
  PlanetAvatar,
  PlanetObject,
  ShopActionResult,
  ShopProduct,
  ShopRequest,
  ShopState,
} from "../types/usage";
import { AvatarSprite } from "./AvatarSprite";
import { LandscapeObjectSprite } from "./LandscapeObjectSprite";
import { PlanetObjectSprite } from "./PlanetObjectSprite";
import { objectName, STAGE_NAMES } from "./PlanetScene";
import { PlanetLandscapeDecorations } from "./PlanetLandscapeDecorations";
import { styleIdForSku } from "./cosmeticStyles";
import { restDuration, stepDuration } from "./sceneMotion";
import {
  clampLandscapeCamera,
  fitLandscape,
  focusLandscape,
  landscapeViewBox,
  screenToLandscape,
  zoomLandscape,
  type LandscapeCamera,
  type LandscapeViewport,
} from "./planetLandscapeCamera";
import {
  cosmeticLandscapeBounds,
  LANDSCAPE_CELL_HEIGHT,
  LANDSCAPE_CELL_WIDTH,
  LANDSCAPE_CELL_X_ORIGIN,
  LANDSCAPE_CELL_Y_ORIGIN,
  LANDSCAPE_WALKWAY_END_COLUMNS,
  LANDSCAPE_WALKWAY_ROWS,
  LANDSCAPE_WALKWAY_X_OFFSET,
  layoutLandscape,
  landscapeObjectId,
  type LandscapeBounds,
  type LandscapePlacement,
} from "./planetLandscapeLayout";
import { validatePlacement } from "./landscapeEditing";

const SKY_BAND_HEIGHT = 220;
const DEFAULT_VIEWPORT: LandscapeViewport = { width: 1200, height: 420 };
const LANDSCAPE_WALK_STOPS = 24;
const LANDSCAPE_WALK_CONNECTOR_STEPS = 7;
const LANDSCAPE_WALK_POINT_COUNT = LANDSCAPE_WALK_STOPS * 2 + LANDSCAPE_WALK_CONNECTOR_STEPS - 1;
const LANDSCAPE_WALK_START = Math.floor(LANDSCAPE_WALK_STOPS / 2);

export type PlanetExplorationState = {
  camera: LandscapeCamera;
  selectedObjectId: string | null;
};

export type PlanetLandscapeProps = {
  stage: number;
  progress: number;
  avatar: PlanetAvatar;
  avatarEquipment?: AvatarEquipment;
  objects: PlanetObject[];
  equippedCosmetics: EquippedCosmetic[];
  incomplete: boolean;
  cycleId: string;
  exploration: PlanetExplorationState;
  onExplorationChange: (state: PlanetExplorationState) => void;
  selectedCosmeticSku?: string | null;
  shopState?: ShopState | null;
  shopAccountId?: string | null;
  terrainObjects?: PlanetObject[];
  selectedLandscapeInstanceId?: string | null;
  pendingShopAction?: PendingShopAction | null;
  onShopAction?: (request: ShopRequest) => Promise<ShopActionResult | null>;
  onSelectLandscapeInstance?: (instanceId: string | null) => void;
  createShopRequestId?: () => string;
};

type ActiveDrag = {
  pointerId: number;
  startX: number;
  startY: number;
  centerX: number;
  centerY: number;
  viewWidth: number;
  viewHeight: number;
  viewportWidth: number;
  viewportHeight: number;
};

type LandscapeShopDrag = {
  pointerId: number | null;
  pointerStart: { x: number; y: number } | null;
  hasMoved: boolean;
  instanceId: string;
  sku: string;
  cycleId: string;
  accountId: string;
  expectedVersion: number;
  offsetX: number;
  offsetY: number;
  x: number;
  y: number;
  valid: boolean;
};

function defaultShopRequestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return `shop-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function placementWidth(product: ShopProduct): number {
  return product.placement_zone === "sky" ? 96 : 64;
}

function naturalKey(cycleId: string, object: PlanetObject): string {
  return `${cycleId}:${object.stage}:${object.ordinal}`;
}

export function planetLandscapeBounds(objects: readonly PlanetObject[]): LandscapeBounds {
  const terrain = layoutLandscape(objects).bounds;
  return {
    x: terrain.x,
    y: terrain.y - SKY_BAND_HEIGHT,
    width: terrain.width,
    height: terrain.height + SKY_BAND_HEIGHT,
  };
}

function sameCamera(left: LandscapeCamera, right: LandscapeCamera): boolean {
  return Math.abs(left.centerX - right.centerX) < 0.001
    && Math.abs(left.centerY - right.centerY) < 0.001
    && Math.abs(left.zoom - right.zoom) < 0.001;
}

function landscapeWalkPoints(terrain: LandscapeBounds): Array<{ x: number; y: number }> {
  const left = terrain.x + LANDSCAPE_CELL_X_ORIGIN + LANDSCAPE_WALKWAY_END_COLUMNS[0] * LANDSCAPE_CELL_WIDTH + LANDSCAPE_WALKWAY_X_OFFSET;
  const right = terrain.x + LANDSCAPE_CELL_X_ORIGIN + LANDSCAPE_WALKWAY_END_COLUMNS[1] * LANDSCAPE_CELL_WIDTH + LANDSCAPE_WALKWAY_X_OFFSET;
  const firstY = terrain.y + LANDSCAPE_CELL_Y_ORIGIN + LANDSCAPE_WALKWAY_ROWS[0] * LANDSCAPE_CELL_HEIGHT;
  const secondY = terrain.y + LANDSCAPE_CELL_Y_ORIGIN + LANDSCAPE_WALKWAY_ROWS[1] * LANDSCAPE_CELL_HEIGHT;
  const xStops = Array.from({ length: LANDSCAPE_WALK_STOPS }, (_, index) => (
    left + ((right - left) * index) / (LANDSCAPE_WALK_STOPS - 1)
  ));
  return [
    ...xStops.map((x) => ({ x, y: firstY })),
    ...Array.from({ length: LANDSCAPE_WALK_CONNECTOR_STEPS }, (_, index) => {
      const step = index + 1;
      return { x: right, y: firstY + ((secondY - firstY) * step) / (LANDSCAPE_WALK_CONNECTOR_STEPS + 1) };
    }),
    ...xStops.slice(0, -1).reverse().map((x) => ({ x, y: secondY })),
  ];
}

function planLandscapeWalk(startIndex: number, previousDestination: number | null, random: () => number): number[] {
  const sample = () => Math.max(0, Math.min(1 - Number.EPSILON, random()));
  const lastPoint = LANDSCAPE_WALK_POINT_COUNT - 1;
  const start = Math.max(0, Math.min(lastPoint, Number.isFinite(startIndex) ? Math.trunc(startIndex) : 0));
  const directions = [-1, 1].filter((direction) => start + direction >= 0 && start + direction <= lastPoint);
  let direction = directions[Math.floor(sample() * directions.length)];
  let available = direction < 0 ? start : lastPoint - start;
  let steps = 1 + Math.floor(sample() * Math.min(5, available));

  if (start + direction * steps === previousDestination) {
    const alternatives = directions.filter((candidate) => candidate !== direction);
    if (alternatives.length > 0) {
      direction = alternatives[Math.floor(sample() * alternatives.length)];
      available = direction < 0 ? start : lastPoint - start;
      steps = Math.min(steps, available);
    } else {
      const maxSteps = Math.min(5, available);
      const alternateSteps = Array.from({ length: maxSteps }, (_, index) => index + 1)
        .filter((candidate) => candidate !== steps && start + direction * candidate !== previousDestination);
      if (alternateSteps.length > 0) steps = alternateSteps[Math.floor(sample() * alternateSteps.length)];
    }
  }

  return Array.from({ length: steps }, (_, index) => start + direction * (index + 1));
}

export function PlanetLandscape({
  stage,
  progress,
  avatar,
  avatarEquipment,
  objects,
  equippedCosmetics,
  incomplete,
  cycleId,
  exploration,
  onExplorationChange,
  selectedCosmeticSku = null,
  shopState = null,
  shopAccountId = null,
  terrainObjects,
  selectedLandscapeInstanceId = null,
  pendingShopAction = null,
  onShopAction,
  onSelectLandscapeInstance,
  createShopRequestId = defaultShopRequestId,
}: PlanetLandscapeProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<ActiveDrag | null>(null);
  const shopDragRef = useRef<LandscapeShopDrag | null>(null);
  const shopRequestGenerationRef = useRef(0);
  const shopRequestInFlightRef = useRef(false);
  const [viewport, setViewport] = useState(DEFAULT_VIEWPORT);
  const [isDragging, setIsDragging] = useState(false);
  const [isShopDragging, setIsShopDragging] = useState(false);
  const [shopDraft, setShopDraft] = useState<LandscapeShopDrag | null>(null);
  const [shopRequestPending, setShopRequestPending] = useState(false);
  const [shopPlacementNotice, setShopPlacementNotice] = useState<string | null>(null);
  const [isIntersecting, setIsIntersecting] = useState(() => typeof IntersectionObserver === "undefined");
  const [documentVisible, setDocumentVisible] = useState(() => typeof document === "undefined" || !document.hidden);
  const [avatarPosition, setAvatarPosition] = useState(LANDSCAPE_WALK_START);
  const avatarPositionRef = useRef(LANDSCAPE_WALK_START);
  const previousDestinationRef = useRef<number | null>(null);
  const [avatarStepDuration, setAvatarStepDuration] = useState(380);
  const [avatarFacing, setAvatarFacing] = useState<"left" | "right">("right");
  const [avatarWalking, setAvatarWalking] = useState(false);
  const [eyesClosed, setEyesClosed] = useState(false);
  const [prefersReducedMotion, setPrefersReducedMotion] = useState(() => (
    typeof window !== "undefined"
    && typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches
  ));
  const motionActive = isIntersecting && documentVisible;
  const layoutSource = terrainObjects ?? objects;
  const layout = useMemo(() => layoutLandscape(layoutSource), [layoutSource]);
  const avatarWalkPoints = useMemo(() => landscapeWalkPoints(layout.bounds), [layout.bounds]);
  const landscapeBounds = useMemo(() => ({
    x: layout.bounds.x,
    y: layout.bounds.y - SKY_BAND_HEIGHT,
    width: layout.bounds.width,
    height: layout.bounds.height + SKY_BAND_HEIGHT,
  }), [layout.bounds]);
  const activeShopState = shopState
    && shopState.current_cycle_id === cycleId
    && (shopAccountId === null || shopAccountId === shopState.account_id)
    ? shopState
    : null;
  const activeShopAccountId = shopAccountId ?? activeShopState?.account_id ?? null;
  const shopContextIdentity = `${shopAccountId ?? ""}/${shopState?.account_id ?? ""}/${shopState?.current_cycle_id ?? ""}/${cycleId}`;
  const removedNaturalKeys = useMemo(() => new Set(
    activeShopState?.removed_natural_keys
      .filter((key) => key.cycle_id === cycleId)
      .map((key) => `${key.cycle_id}:${key.stage}:${key.ordinal}`) ?? [],
  ), [activeShopState, cycleId]);
  const visibleNaturalIds = useMemo(() => new Set(objects.map(landscapeObjectId)), [objects]);
  const visibleNaturalPlacements = useMemo(
    () => layout.objects.filter((placement) => visibleNaturalIds.has(placement.id)
      && !removedNaturalKeys.has(naturalKey(cycleId, placement.object))),
    [cycleId, layout.objects, removedNaturalKeys, visibleNaturalIds],
  );
  const shopInstancesById = useMemo(() => new Map(
    (activeShopState?.landscape_instances ?? []).map((instance) => [instance.instance_id, instance]),
  ), [activeShopState]);
  const shopProductsBySku = useMemo(() => new Map(
    (activeShopState?.products ?? []).map((product) => [product.sku, product]),
  ), [activeShopState]);
  const shopPlacementsById = useMemo(() => new Map(
    (activeShopState?.placements ?? [])
      .filter((placement) => placement.cycle_id === cycleId)
      .map((placement) => [placement.instance_id, placement]),
  ), [activeShopState, cycleId]);
  const placedShopObjects = useMemo(() => (activeShopState?.placements ?? [])
    .filter((placement) => placement.cycle_id === cycleId)
    .map((placement) => {
      const instance = shopInstancesById.get(placement.instance_id);
      const product = instance ? shopProductsBySku.get(instance.sku) : undefined;
      return instance && product ? { placement, instance, product } : null;
    })
    .filter((entry): entry is { placement: ShopLandscapePlacement; instance: LandscapeInstance; product: ShopProduct } => entry !== null)
    .sort((left, right) => left.placement.y - right.placement.y
      || left.placement.instance_id.localeCompare(right.placement.instance_id)),
  [activeShopState, cycleId, shopInstancesById, shopProductsBySku]);
  const stageName = STAGE_NAMES[stage] ?? STAGE_NAMES[4];
  const selected = visibleNaturalPlacements.find((placement) => placement.id === exploration.selectedObjectId) ?? null;
  const selectedShopPlacement = selectedLandscapeInstanceId
    ? placedShopObjects.find(({ instance }) => instance.instance_id === selectedLandscapeInstanceId) ?? null
    : null;
  const viewBox = landscapeViewBox(landscapeBounds, viewport, exploration.camera);
  const avatarPoint = avatarWalkPoints[avatarPosition] ?? avatarWalkPoints[LANDSCAPE_WALK_START];
  const avatarX = avatarPoint.x;
  const avatarY = avatarPoint.y;

  useEffect(() => {
    const node = viewportRef.current;
    if (!node || typeof IntersectionObserver === "undefined") {
      setIsIntersecting(true);
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      setIsIntersecting(entries.some((entry) => entry.target === node && entry.isIntersecting));
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const updateVisibility = () => setDocumentVisible(!document.hidden);
    document.addEventListener("visibilitychange", updateVisibility);
    return () => document.removeEventListener("visibilitychange", updateVisibility);
  }, []);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const updatePreference = (event: MediaQueryListEvent | MediaQueryList) => setPrefersReducedMotion(event.matches);
    setPrefersReducedMotion(query.matches);
    if (typeof query.addEventListener === "function") {
      query.addEventListener("change", updatePreference);
      return () => query.removeEventListener("change", updatePreference);
    }
    query.addListener(updatePreference);
    return () => query.removeListener(updatePreference);
  }, []);

  useEffect(() => {
    shopRequestGenerationRef.current += 1;
    shopRequestInFlightRef.current = false;
    shopDragRef.current = null;
    dragRef.current = null;
    setShopDraft(null);
    setShopRequestPending(false);
    setIsShopDragging(false);
    setIsDragging(false);
    setShopPlacementNotice(null);
  }, [activeShopAccountId, cycleId, shopContextIdentity]);

  useEffect(() => {
    if (!pendingShopAction) return;
    shopDragRef.current = null;
    setShopDraft(null);
    setIsShopDragging(false);
  }, [pendingShopAction?.request.request_id, pendingShopAction?.status]);

  useEffect(() => {
    if (!shopDraft) return;
    const instance = shopInstancesById.get(shopDraft.instanceId);
    if (instance && instance.placement_version === shopDraft.expectedVersion) return;
    const pointerId = shopDragRef.current?.instanceId === shopDraft.instanceId
      ? shopDragRef.current.pointerId
      : null;
    shopDragRef.current = null;
    setShopDraft(null);
    setIsShopDragging(false);
    if (pointerId !== null && viewportRef.current?.hasPointerCapture?.(pointerId)) {
      viewportRef.current.releasePointerCapture?.(pointerId);
    }
  }, [shopDraft?.instanceId, shopDraft?.expectedVersion, shopInstancesById]);

  useEffect(() => () => {
    shopRequestGenerationRef.current += 1;
  }, []);

  useEffect(() => {
    if (!motionActive) {
      setAvatarWalking(false);
      setEyesClosed(false);
      return;
    }
    let movementTimer: number | undefined;
    let blinkTimer: number | undefined;
    let blinkCloseTimer: number | undefined;
    let walking = false;
    let cancelled = false;

    const scheduleWalk = () => {
      movementTimer = window.setTimeout(() => {
        const start = avatarPositionRef.current;
        const route = planLandscapeWalk(start, previousDestinationRef.current, Math.random);
        previousDestinationRef.current = start;
        const duration = stepDuration(Math.random);
        setAvatarStepDuration(duration);
        let step = 0;
        walking = true;
        setAvatarWalking(true);
        setEyesClosed(false);
        const moveNext = () => {
          if (cancelled) return;
          const next = route[step];
          step += 1;
          if (next === undefined) {
            walking = false;
            setAvatarWalking(false);
            scheduleWalk();
            return;
          }
          const previous = avatarPositionRef.current;
          const previousX = avatarWalkPoints[previous]?.x;
          const nextX = avatarWalkPoints[next]?.x;
          if (previousX !== undefined && nextX !== undefined && previousX !== nextX) {
            setAvatarFacing(nextX < previousX ? "left" : "right");
          }
          avatarPositionRef.current = next;
          setAvatarPosition(next);
          movementTimer = window.setTimeout(moveNext, duration);
        };
        moveNext();
      }, restDuration(Math.random));
    };

    const scheduleBlink = () => {
      blinkTimer = window.setTimeout(() => {
        if (walking) {
          scheduleBlink();
          return;
        }
        setEyesClosed(true);
        blinkCloseTimer = window.setTimeout(() => {
          setEyesClosed(false);
          scheduleBlink();
        }, 130);
      }, 3000 + Math.random() * 2500);
    };

    scheduleWalk();
    scheduleBlink();
    return () => {
      cancelled = true;
      window.clearTimeout(movementTimer);
      window.clearTimeout(blinkTimer);
      window.clearTimeout(blinkCloseTimer);
    };
  }, [motionActive]);

  useEffect(() => {
    const node = viewportRef.current;
    if (!node || typeof ResizeObserver === "undefined") return;

    const updateViewport = (width: number, height: number) => {
      if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return;
      setViewport((current) => Math.abs(current.width - width) < 0.5 && Math.abs(current.height - height) < 0.5
        ? current
        : { width, height });
    };
    const initialSize = node.getBoundingClientRect();
    updateViewport(initialSize.width, initialSize.height);
    const observer = new ResizeObserver((entries) => {
      const entry = entries.find((candidate) => candidate.target === node);
      if (entry) updateViewport(entry.contentRect.width, entry.contentRect.height);
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const camera = clampLandscapeCamera(landscapeBounds, viewport, exploration.camera);
    if (!sameCamera(camera, exploration.camera)) onExplorationChange({ ...exploration, camera });
  }, [landscapeBounds, viewport, exploration, onExplorationChange]);

  useEffect(() => {
    if (exploration.selectedObjectId && !selected) {
      onExplorationChange({ ...exploration, selectedObjectId: null });
    }
  }, [exploration, onExplorationChange, selected]);

  useEffect(() => {
    if (!selectedCosmeticSku) return;
    const styleId = styleIdForSku(selectedCosmeticSku);
    const slotId = styleId === "star_cluster" || styleId === "aurora" || styleId === "meteor_shower" ? "sky"
      : styleId === "thin_ring" || styleId === "double_ring" || styleId === "moonlets" ? "ring"
        : styleId === "flag" || styleId === "crystal_tower" || styleId === "flower_garden" || styleId === "observatory" ? "surface"
          : styleId === "pond" || styleId === "lantern" || styleId === "rover" || styleId === "greenhouse" ? "forecourt"
            : null;
    if (!slotId) return;
    const equipped = equippedCosmetics.find((item) => item.slot_id === slotId && styleIdForSku(item.sku) === styleId);
    if (!equipped) return;
    const target = cosmeticLandscapeBounds(layout.bounds, equipped.slot_id, styleId);
    onExplorationChange({
      ...exploration,
      camera: focusLandscape(landscapeBounds, viewport, exploration.camera, target),
    });
  }, [selectedCosmeticSku]);

  function updateCamera(camera: LandscapeCamera) {
    onExplorationChange({ ...exploration, camera });
  }

  function selectPlacement(placement: LandscapePlacement) {
    onExplorationChange({
      selectedObjectId: placement.id,
      camera: focusLandscape(landscapeBounds, viewport, exploration.camera, placement.bounds),
    });
  }

  function worldPoint(clientX: number, clientY: number) {
    const node = viewportRef.current;
    if (!node) return null;
    const rect = node.getBoundingClientRect();
    return screenToLandscape({ x: clientX, y: clientY }, rect, viewBox);
  }

  function draftAt(
    instance: LandscapeInstance,
    product: ShopProduct,
    point: { x: number; y: number },
    offsetX: number,
    offsetY: number,
    pointerId: number | null,
    pointerStart: { x: number; y: number } | null = null,
    hasMoved = pointerId === null,
    expectedVersion = instance.placement_version,
  ): LandscapeShopDrag | null {
    if (!activeShopState || !activeShopAccountId) return null;
    const x = Math.round((point.x - offsetX) * 100) / 100;
    const y = Math.round((point.y - offsetY) * 100) / 100;
    return {
      pointerId,
      pointerStart,
      hasMoved,
      instanceId: instance.instance_id,
      sku: instance.sku,
      cycleId,
      accountId: activeShopAccountId,
      expectedVersion,
      offsetX,
      offsetY,
      x,
      y,
      valid: validatePlacement(product, { x, y }, layout.bounds),
    };
  }

  function startShopPointerDrag(event: ReactPointerEvent<HTMLDivElement>, instance: LandscapeInstance, point: { x: number; y: number }, placement?: ShopLandscapePlacement) {
    if (!onShopAction || pendingShopAction || shopRequestPending || shopRequestInFlightRef.current) return false;
    const product = shopProductsBySku.get(instance.sku);
    if (!product) return false;
    const draft = draftAt(
      instance,
      product,
      point,
      placement ? point.x - placement.x : placementWidth(product) / 2,
      placement ? point.y - placement.y : 32,
      event.pointerId,
      { x: event.clientX, y: event.clientY },
      false,
    );
    if (!draft) return false;
    event.preventDefault();
    dragRef.current = null;
    shopDragRef.current = draft;
    setShopDraft(draft);
    setIsShopDragging(true);
    setIsDragging(false);
    setShopPlacementNotice(null);
    if (placement) onSelectLandscapeInstance?.(instance.instance_id);
    event.currentTarget.setPointerCapture?.(event.pointerId);
    return true;
  }

  function requestCanBeSent(draft: LandscapeShopDrag): boolean {
    const instance = shopInstancesById.get(draft.instanceId);
    const product = shopProductsBySku.get(draft.sku);
    return Boolean(
      activeShopState
      && onShopAction
      && !pendingShopAction
      && !shopRequestPending
      && !shopRequestInFlightRef.current
      && activeShopAccountId === draft.accountId
      && activeShopState.current_cycle_id === draft.cycleId
      && cycleId === draft.cycleId
      && instance
      && instance.sku === draft.sku
      && instance.placement_version === draft.expectedVersion
      && product
      && validatePlacement(product, { x: draft.x, y: draft.y }, layout.bounds),
    );
  }

  async function sendShopRequest(request: ShopRequest, accountId: string, requestCycleId: string, successStatus: ShopActionResult["status"], actionName: string) {
    if (!onShopAction || !activeShopState || pendingShopAction || shopRequestPending || shopRequestInFlightRef.current
      || activeShopAccountId !== accountId || activeShopState.current_cycle_id !== requestCycleId || cycleId !== requestCycleId) return;
    shopRequestInFlightRef.current = true;
    setShopRequestPending(true);
    const generation = ++shopRequestGenerationRef.current;
    try {
      const result = await onShopAction(request);
      if (generation !== shopRequestGenerationRef.current) return;
      if (!result || result.request_id !== request.request_id
        || result.state.account_id !== accountId
        || result.state.current_cycle_id !== requestCycleId) {
        setShopPlacementNotice(`${actionName} 결과를 확인할 수 없습니다. 상점에서 요청 상태를 확인해 주세요.`);
      } else if (result.status !== successStatus) {
        setShopPlacementNotice(`${actionName}할 수 없습니다. 위치와 보유 상태를 확인해 주세요.`);
      } else {
        setShopPlacementNotice(null);
      }
    } catch {
      if (generation === shopRequestGenerationRef.current) {
        setShopPlacementNotice(`${actionName} 결과를 확인할 수 없습니다. 상점에서 요청 상태를 확인해 주세요.`);
      }
    } finally {
      if (generation === shopRequestGenerationRef.current) {
        shopRequestInFlightRef.current = false;
        setShopRequestPending(false);
      }
    }
  }

  async function submitShopPlacement(draft: LandscapeShopDrag) {
    const currentInstance = shopInstancesById.get(draft.instanceId);
    if (!currentInstance || currentInstance.placement_version !== draft.expectedVersion) {
      shopDragRef.current = null;
      setShopDraft((current) => current?.instanceId === draft.instanceId
        && current.expectedVersion === draft.expectedVersion
        ? null
        : current);
      setIsShopDragging(false);
      setShopPlacementNotice("장식 상태가 변경되어 설치를 취소했습니다.");
      return;
    }
    if (!requestCanBeSent(draft)) return;
    const currentPlacement = shopPlacementsById.get(draft.instanceId);
    if (currentPlacement && currentPlacement.x === draft.x && currentPlacement.y === draft.y) {
      setShopDraft(null);
      setShopPlacementNotice(null);
      return;
    }
    const request: ShopRequest = {
      kind: "place",
      request_id: createShopRequestId(),
      cycle_id: draft.cycleId,
      instance_id: draft.instanceId,
      expected_version: draft.expectedVersion,
      x: draft.x,
      y: draft.y,
    };
    await sendShopRequest(request, draft.accountId, draft.cycleId, "placed", "설치");
  }

  function retrieveShopInstance(instanceId: string) {
    const instance = shopInstancesById.get(instanceId);
    if (!activeShopState || !activeShopAccountId || !instance || !shopPlacementsById.has(instanceId)
      || pendingShopAction || shopRequestInFlightRef.current) return;
    void sendShopRequest({
      kind: "retrieve",
      request_id: createShopRequestId(),
      cycle_id: cycleId,
      instance_id: instance.instance_id,
      expected_version: instance.placement_version,
    }, activeShopAccountId, cycleId, "retrieved", "보관");
  }

  function startKeyboardPlacement(instanceId: string) {
    if (!activeShopState || !activeShopAccountId || !onShopAction || pendingShopAction || shopRequestInFlightRef.current) return;
    const instance = shopInstancesById.get(instanceId);
    const product = instance ? shopProductsBySku.get(instance.sku) : undefined;
    if (!instance || !product) return;
    const currentPlacement = shopPlacementsById.get(instanceId);
    const footprintWidth = placementWidth(product);
    const minimumY = product.placement_zone === "sky" ? layout.bounds.y - SKY_BAND_HEIGHT : layout.bounds.y;
    const maximumY = product.placement_zone === "sky"
      ? layout.bounds.y - 64
      : layout.bounds.y + layout.bounds.height - 64;
    let found: { x: number; y: number } | null = currentPlacement
      ? { x: currentPlacement.x, y: currentPlacement.y }
      : null;
    if (!found) {
      for (let y = minimumY; y <= maximumY && !found; y += 8) {
        for (let x = layout.bounds.x; x + footprintWidth <= layout.bounds.x + layout.bounds.width; x += 8) {
          if (validatePlacement(product, { x, y }, layout.bounds)) {
            found = { x, y };
            break;
          }
        }
      }
    }
    if (!found) {
      setShopPlacementNotice("설치할 수 있는 위치가 없습니다.");
      return;
    }
    const draft = draftAt(instance, product, found, 0, 0, null);
    if (!draft) return;
    setShopDraft(draft);
    setShopPlacementNotice(null);
  }

  function moveKeyboardDraft(dx: number, dy: number) {
    const current = shopDraft;
    if (!current || current.pointerId !== null) return false;
    const product = shopProductsBySku.get(current.sku);
    if (!product) return true;
    const next = draftAt(
      shopInstancesById.get(current.instanceId) ?? {
        instance_id: current.instanceId,
        sku: current.sku,
        placement_version: current.expectedVersion,
        variation_index: 0,
        seed: "",
        variation_version: 1,
      },
      product,
      { x: current.x + dx, y: current.y + dy },
      0,
      0,
      null,
      null,
      true,
      current.expectedVersion,
    );
    if (next) setShopDraft(next);
    return true;
  }

  function handlePointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    const target = event.target;
    if (event.button !== 0 || (target instanceof Element && target.closest("[data-landscape-hit-id]"))) return;
    if (pendingShopAction || shopRequestInFlightRef.current) return;
    const point = worldPoint(event.clientX, event.clientY);
    if (point && target instanceof Element) {
      const shopHit = target.closest<SVGGElement>("[data-shop-instance-id]");
      const instanceId = shopHit?.getAttribute("data-shop-instance-id") ?? null;
      const existing = instanceId ? shopInstancesById.get(instanceId) : undefined;
      const placement = instanceId ? shopPlacementsById.get(instanceId) : undefined;
      if (existing && placement && startShopPointerDrag(event, existing, point, placement)) return;
      if (shopHit) return;
    }
    if (point && selectedLandscapeInstanceId && activeShopState && onShopAction) {
      const selectedInstance = shopInstancesById.get(selectedLandscapeInstanceId);
      if (selectedInstance && !shopPlacementsById.has(selectedLandscapeInstanceId)
        && startShopPointerDrag(event, selectedInstance, point)) return;
    }
    event.preventDefault();
    const camera = clampLandscapeCamera(landscapeBounds, viewport, exploration.camera);
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      centerX: camera.centerX,
      centerY: camera.centerY,
      viewWidth: viewBox.width,
      viewHeight: viewBox.height,
      viewportWidth: viewport.width,
      viewportHeight: viewport.height,
    };
    setIsDragging(true);
    event.currentTarget.setPointerCapture?.(event.pointerId);
  }

  function handlePointerMove(event: ReactPointerEvent<HTMLDivElement>) {
    const shopDrag = shopDragRef.current;
    if (shopDrag && shopDrag.pointerId === event.pointerId) {
      const point = worldPoint(event.clientX, event.clientY);
      const product = shopProductsBySku.get(shopDrag.sku);
      const instance = shopInstancesById.get(shopDrag.instanceId);
      if (point && product && instance) {
        const start = shopDrag.pointerStart ?? { x: event.clientX, y: event.clientY };
        const hasMoved = shopDrag.hasMoved || Math.hypot(event.clientX - start.x, event.clientY - start.y) >= 4;
        const next = draftAt(instance, product, point, shopDrag.offsetX, shopDrag.offsetY, shopDrag.pointerId, start, hasMoved, shopDrag.expectedVersion);
        if (next) {
          shopDragRef.current = next;
          setShopDraft(next);
        }
      }
      event.preventDefault();
      return;
    }
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    updateCamera(clampLandscapeCamera(landscapeBounds, viewport, {
      ...exploration.camera,
      centerX: drag.centerX + ((drag.startX - event.clientX) * drag.viewWidth) / drag.viewportWidth,
      centerY: drag.centerY + ((drag.startY - event.clientY) * drag.viewHeight) / drag.viewportHeight,
    }));
  }

  function finishPointer(event: ReactPointerEvent<HTMLDivElement>, cancelled = false) {
    const shopDrag = shopDragRef.current;
    if (shopDrag?.pointerId === event.pointerId) {
      const finalPoint = cancelled ? null : worldPoint(event.clientX, event.clientY);
      const product = shopProductsBySku.get(shopDrag.sku);
      const instance = shopInstancesById.get(shopDrag.instanceId);
      const start = shopDrag.pointerStart ?? { x: event.clientX, y: event.clientY };
      const hasMoved = shopDrag.hasMoved || Math.hypot(event.clientX - start.x, event.clientY - start.y) >= 4;
      const completedDraft = finalPoint && product && instance
        ? draftAt(instance, product, finalPoint, shopDrag.offsetX, shopDrag.offsetY, shopDrag.pointerId, start, hasMoved, shopDrag.expectedVersion)
        : { ...shopDrag, hasMoved };
      shopDragRef.current = null;
      setShopDraft(null);
      setIsShopDragging(false);
      if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
      if (!cancelled && completedDraft && completedDraft.valid && completedDraft.hasMoved) void submitShopPlacement(completedDraft);
      return;
    }
    if (dragRef.current?.pointerId !== event.pointerId) return;
    dragRef.current = null;
    setIsDragging(false);
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function handleSceneKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (shopDraft?.pointerId === null) {
      if (event.key === "Escape") {
        event.preventDefault();
        setShopDraft(null);
        return;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        if (shopDraft.valid) void submitShopPlacement(shopDraft);
        return;
      }
      const direction: Record<string, { x: number; y: number }> = {
        ArrowLeft: { x: -8, y: 0 },
        ArrowRight: { x: 8, y: 0 },
        ArrowUp: { x: 0, y: -8 },
        ArrowDown: { x: 0, y: 8 },
      };
      const placementOffset = direction[event.key];
      if (placementOffset) {
        event.preventDefault();
        moveKeyboardDraft(placementOffset.x, placementOffset.y);
      }
      return;
    }
    if ((event.key === " " || event.key === "Enter") && selectedLandscapeInstanceId && activeShopState) {
      event.preventDefault();
      startKeyboardPlacement(selectedLandscapeInstanceId);
      return;
    }
    const stepX = viewBox.width * 0.12;
    const stepY = viewBox.height * 0.12;
    const offsets: Record<string, { x: number; y: number }> = {
      ArrowLeft: { x: -stepX, y: 0 },
      ArrowRight: { x: stepX, y: 0 },
      ArrowUp: { x: 0, y: -stepY },
      ArrowDown: { x: 0, y: stepY },
    };
    const offset = offsets[event.key];
    if (!offset) return;
    event.preventDefault();
    const camera = clampLandscapeCamera(landscapeBounds, viewport, exploration.camera);
    updateCamera(clampLandscapeCamera(landscapeBounds, viewport, {
      ...camera,
      centerX: camera.centerX + offset.x,
      centerY: camera.centerY + offset.y,
    }));
  }

  function handleObjectKeyDown(event: ReactKeyboardEvent<SVGGElement>, placement: LandscapePlacement) {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    event.stopPropagation();
    selectPlacement(placement);
  }

  return (
    <section
      className="planet-landscape"
      aria-label="행성 풍경"
      data-cycle-id={cycleId}
      data-incomplete={incomplete}
      data-motion-active={motionActive}
      data-camera-center-x={exploration.camera.centerX}
      data-camera-center-y={exploration.camera.centerY}
      data-camera-zoom={exploration.camera.zoom}
    >
      <div className="planet-landscape-controls" role="group" aria-label="풍경 카메라 조작">
        <button type="button" aria-label="축소" onClick={() => updateCamera(zoomLandscape(landscapeBounds, viewport, exploration.camera, 1 / 1.4))}>−</button>
        <button type="button" aria-label="확대" onClick={() => updateCamera(zoomLandscape(landscapeBounds, viewport, exploration.camera, 1.4))}>+</button>
        <button type="button" aria-label="전체 보기" onClick={() => updateCamera(fitLandscape(landscapeBounds))}>전체 보기</button>
      </div>
      <div
        className={`planet-landscape-viewport${isDragging ? " is-dragging" : ""}${isShopDragging ? " is-shop-dragging" : ""}`}
        ref={viewportRef}
        role="group"
        aria-label="행성 풍경 탐사"
        tabIndex={0}
        onKeyDown={handleSceneKeyDown}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={(event) => finishPointer(event)}
        onPointerCancel={(event) => finishPointer(event, true)}
        onLostPointerCapture={(event) => finishPointer(event, true)}
      >
        <svg
          className="planet-landscape-svg"
          viewBox={`${viewBox.x} ${viewBox.y} ${viewBox.width} ${viewBox.height}`}
          style={{ transition: prefersReducedMotion ? "none" : undefined }}
          preserveAspectRatio="xMidYMid meet"
          role="group"
          aria-label={`${stageName} 평면 풍경, 다음 시대 진행도 ${Math.round(progress * 100)}%`}
          shapeRendering="crispEdges"
        >
          <PlanetLandscapeDecorations
            stage={stage}
            bounds={layout.bounds}
            viewBox={viewBox}
            equippedCosmetics={equippedCosmetics}
            selectedCosmeticSku={selectedCosmeticSku}
          />
          <g className="planet-landscape-objects">
            {visibleNaturalPlacements.map((placement) => {
              const objectLabel = `${objectName(placement.object.kind)} ${placement.object.ordinal + 1}번째, ${STAGE_NAMES[placement.object.stage] ?? STAGE_NAMES[4]}`;
              const isSelected = placement.id === exploration.selectedObjectId;
              return (
                <g
                  key={placement.id}
                  className={isSelected ? "planet-landscape-object is-selected" : "planet-landscape-object"}
                  data-landscape-object-id={placement.id}
                  data-landscape-hit-id={placement.id}
                  data-object-stage={placement.object.stage}
                  data-object-kind={placement.object.kind}
                  role="button"
                  aria-label={objectLabel}
                  aria-pressed={isSelected}
                  tabIndex={0}
                  onClick={() => selectPlacement(placement)}
                  onKeyDown={(event) => handleObjectKeyDown(event, placement)}
                >
                  {isSelected && <rect
                    className="planet-landscape-selection-highlight"
                    x={placement.bounds.x - 3}
                    y={placement.bounds.y - 3}
                    width={placement.bounds.width + 6}
                    height={placement.bounds.height + 6}
                    fill="#f1cf89"
                    fillOpacity=".35"
                    stroke="#f1cf89"
                    strokeWidth="2"
                  />}
                  <PlanetObjectSprite object={placement.object} x={placement.x} y={placement.y} scale={1.45} />
                </g>
              );
            })}
          </g>
          <g className="planet-landscape-shop-objects">
            {placedShopObjects.map(({ placement, instance, product }) => {
              const isSelected = instance.instance_id === selectedLandscapeInstanceId;
              return (
                <g
                  key={instance.instance_id}
                  className={`planet-landscape-shop-object${isSelected ? " is-selected" : ""}`}
                  data-shop-instance-id={instance.instance_id}
                  data-shop-sku={instance.sku}
                  data-shop-placement-version={instance.placement_version}
                  transform={`translate(${placement.x} ${placement.y})`}
                  role="button"
                  aria-label={`${product.display_name}, 설치된 장식`}
                  aria-pressed={isSelected}
                  tabIndex={0}
                  onClick={() => onSelectLandscapeInstance?.(instance.instance_id)}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter" && event.key !== " ") return;
                    event.preventDefault();
                    event.stopPropagation();
                    onSelectLandscapeInstance?.(instance.instance_id);
                  }}
                >
                  <LandscapeObjectSprite instance={instance} product={product} selected={isSelected} />
                </g>
              );
            })}
            {shopDraft && activeShopState && shopDraft.cycleId === cycleId && shopDraft.accountId === activeShopAccountId && (() => {
              const instance = shopInstancesById.get(shopDraft.instanceId);
              const product = shopProductsBySku.get(shopDraft.sku);
              if (!instance || !product) return null;
              return (
                <g
                  className={`planet-landscape-shop-preview${shopDraft.valid ? " is-valid" : " is-invalid"}`}
                  data-shop-preview-instance-id={shopDraft.instanceId}
                  data-shop-preview-valid={shopDraft.valid}
                  data-shop-preview-x={shopDraft.x}
                  data-shop-preview-y={shopDraft.y}
                  transform={`translate(${shopDraft.x} ${shopDraft.y})`}
                  pointerEvents="none"
                  opacity=".72"
                >
                  <rect
                    x="0"
                    y="0"
                    width={placementWidth(product)}
                    height="64"
                    fill={shopDraft.valid ? "#9bd7b4" : "#e78b7a"}
                    fillOpacity=".08"
                    stroke={shopDraft.valid ? "#9bd7b4" : "#e78b7a"}
                    strokeWidth="2"
                    strokeDasharray="4 3"
                  />
                  <LandscapeObjectSprite instance={instance} product={product} />
                </g>
              );
            })()}
          </g>
          <g
            className={`planet-landscape-avatar${avatarWalking ? " is-walking" : ""}`}
            data-avatar-walking={avatarWalking}
            style={{ "--avatar-step-ms": `${avatarStepDuration}ms` } as CSSProperties}
            transform={`translate(${avatarX} ${avatarY})`}
          >
            <g transform="scale(1.2)">
              <AvatarSprite
                avatar={avatar}
                className="planet-landscape-avatar-sprite"
                facing={avatarFacing}
                eyesClosed={eyesClosed}
                walking={avatarWalking}
                equipment={avatarEquipment}
              />
            </g>
          </g>
        </svg>
      </div>
      <section className="planet-landscape-object-list" role="region" aria-label="오브젝트 목록">
        {visibleNaturalPlacements.length === 0
          ? <p>아직 생성된 오브젝트가 없습니다.</p>
          : <ol>
            {visibleNaturalPlacements.map((placement) => {
              const isSelected = placement.id === exploration.selectedObjectId;
              return (
                <li key={placement.id}>
                  <button
                    type="button"
                    data-object-list-id={placement.id}
                    aria-pressed={isSelected}
                    onClick={() => selectPlacement(placement)}
                  >
                    <span>{objectName(placement.object.kind)} {placement.object.ordinal + 1}번째</span>
                    <span>{STAGE_NAMES[placement.object.stage] ?? STAGE_NAMES[4]}</span>
                  </button>
                </li>
              );
            })}
          </ol>}
      </section>
      {selected && <section className="planet-landscape-selection" role="region" aria-label="선택한 오브젝트" aria-live="polite">
        <h2>{objectName(selected.object.kind)}</h2>
        <p>{STAGE_NAMES[selected.object.stage] ?? STAGE_NAMES[4]}</p>
        <p>{selected.object.ordinal + 1}번째 생성</p>
      </section>}
      {selectedShopPlacement && <div className="planet-landscape-shop-selection" role="group" aria-label="선택한 장식">
        <span>{selectedShopPlacement.product.display_name}</span>
        <button
          type="button"
          disabled={Boolean(pendingShopAction) || shopRequestPending || !onShopAction}
          onClick={() => retrieveShopInstance(selectedShopPlacement.instance.instance_id)}
        >보관</button>
      </div>}
      {(shopPlacementNotice || shopDraft?.pointerId === null) && <p className="planet-landscape-shop-status" role="status">
        {shopPlacementNotice ?? (shopDraft?.valid ? "방향키로 위치를 조정하고 Enter로 설치하세요. Esc를 누르면 취소합니다." : "설치할 수 없는 위치입니다. 방향키로 옮기거나 Esc로 취소하세요.")}
      </p>}
    </section>
  );
}
