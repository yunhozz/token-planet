import {
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { CSSProperties } from "react";
import type { EquippedCosmetic, PlanetAvatar, PlanetObject } from "../types/usage";
import { AvatarSprite } from "./AvatarSprite";
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
  type LandscapeBounds,
  type LandscapePlacement,
} from "./planetLandscapeLayout";

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
  objects: PlanetObject[];
  equippedCosmetics: EquippedCosmetic[];
  incomplete: boolean;
  cycleId: string;
  exploration: PlanetExplorationState;
  onExplorationChange: (state: PlanetExplorationState) => void;
  selectedCosmeticSku?: string | null;
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
  objects,
  equippedCosmetics,
  incomplete,
  cycleId,
  exploration,
  onExplorationChange,
  selectedCosmeticSku = null,
}: PlanetLandscapeProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<ActiveDrag | null>(null);
  const [viewport, setViewport] = useState(DEFAULT_VIEWPORT);
  const [isDragging, setIsDragging] = useState(false);
  const [isIntersecting, setIsIntersecting] = useState(() => typeof IntersectionObserver === "undefined");
  const [documentVisible, setDocumentVisible] = useState(() => typeof document === "undefined" || !document.hidden);
  const [avatarPosition, setAvatarPosition] = useState(LANDSCAPE_WALK_START);
  const avatarPositionRef = useRef(LANDSCAPE_WALK_START);
  const previousDestinationRef = useRef<number | null>(null);
  const [avatarStepDuration, setAvatarStepDuration] = useState(380);
  const [avatarFacing, setAvatarFacing] = useState<"left" | "right">("right");
  const [avatarWalking, setAvatarWalking] = useState(false);
  const [eyesClosed, setEyesClosed] = useState(false);
  const motionActive = isIntersecting && documentVisible;
  const layout = useMemo(() => layoutLandscape(objects), [objects]);
  const avatarWalkPoints = useMemo(() => landscapeWalkPoints(layout.bounds), [layout.bounds]);
  const landscapeBounds = useMemo(() => ({
    x: layout.bounds.x,
    y: layout.bounds.y - SKY_BAND_HEIGHT,
    width: layout.bounds.width,
    height: layout.bounds.height + SKY_BAND_HEIGHT,
  }), [layout.bounds]);
  const stageName = STAGE_NAMES[stage] ?? STAGE_NAMES[4];
  const selected = layout.objects.find((placement) => placement.id === exploration.selectedObjectId) ?? null;
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

  function handlePointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    const target = event.target;
    if (event.button !== 0 || (target instanceof Element && target.closest("[data-landscape-hit-id]"))) return;
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
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    updateCamera(clampLandscapeCamera(landscapeBounds, viewport, {
      ...exploration.camera,
      centerX: drag.centerX + ((drag.startX - event.clientX) * drag.viewWidth) / drag.viewportWidth,
      centerY: drag.centerY + ((drag.startY - event.clientY) * drag.viewHeight) / drag.viewportHeight,
    }));
  }

  function finishPointer(event: ReactPointerEvent<HTMLDivElement>) {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    dragRef.current = null;
    setIsDragging(false);
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function handleSceneKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
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
        className={`planet-landscape-viewport${isDragging ? " is-dragging" : ""}`}
        ref={viewportRef}
        role="group"
        aria-label="행성 풍경 탐사"
        tabIndex={0}
        onKeyDown={handleSceneKeyDown}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={finishPointer}
        onPointerCancel={finishPointer}
      >
        <svg
          className="planet-landscape-svg"
          viewBox={`${viewBox.x} ${viewBox.y} ${viewBox.width} ${viewBox.height}`}
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
            {layout.objects.map((placement) => {
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
          <g
            className={`planet-landscape-avatar${avatarWalking ? " is-walking" : ""}`}
            data-avatar-walking={avatarWalking}
            style={{ "--avatar-step-ms": `${avatarStepDuration}ms` } as CSSProperties}
            transform={`translate(${avatarX} ${avatarY})`}
          >
            <g transform="scale(1.2)">
              <AvatarSprite avatar={avatar} className="planet-landscape-avatar-sprite" facing={avatarFacing} eyesClosed={eyesClosed} walking={avatarWalking} />
            </g>
          </g>
        </svg>
      </div>
      {selected && <section className="planet-landscape-selection" role="region" aria-label="선택한 오브젝트" aria-live="polite">
        <h2>{objectName(selected.object.kind)}</h2>
        <p>{STAGE_NAMES[selected.object.stage] ?? STAGE_NAMES[4]}</p>
        <p>{selected.object.ordinal + 1}번째 생성</p>
      </section>}
    </section>
  );
}
