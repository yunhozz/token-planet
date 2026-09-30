import {
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { EquippedCosmetic, PlanetAvatar, PlanetObject } from "../types/usage";
import { AvatarSprite } from "./AvatarSprite";
import { PlanetObjectSprite } from "./PlanetObjectSprite";
import { objectName, STAGE_NAMES } from "./PlanetScene";
import { PlanetLandscapeDecorations } from "./PlanetLandscapeDecorations";
import {
  clampLandscapeCamera,
  fitLandscape,
  focusLandscape,
  landscapeViewBox,
  zoomLandscape,
  type LandscapeCamera,
  type LandscapeViewport,
} from "./planetLandscapeCamera";
import { layoutLandscape, type LandscapeBounds, type LandscapePlacement } from "./planetLandscapeLayout";

const SKY_BAND_HEIGHT = 220;
const DEFAULT_VIEWPORT: LandscapeViewport = { width: 1200, height: 420 };

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
}: PlanetLandscapeProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<ActiveDrag | null>(null);
  const [viewport, setViewport] = useState(DEFAULT_VIEWPORT);
  const [isDragging, setIsDragging] = useState(false);
  const [reducedMotion] = useState(() => typeof window !== "undefined"
    && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true);
  const layout = useMemo(() => layoutLandscape(objects), [objects]);
  const landscapeBounds = useMemo(() => ({
    x: layout.bounds.x,
    y: layout.bounds.y - SKY_BAND_HEIGHT,
    width: layout.bounds.width,
    height: layout.bounds.height + SKY_BAND_HEIGHT,
  }), [layout.bounds]);
  const stageName = STAGE_NAMES[stage] ?? STAGE_NAMES[4];
  const selected = layout.objects.find((placement) => placement.id === exploration.selectedObjectId) ?? null;
  const viewBox = landscapeViewBox(landscapeBounds, viewport, exploration.camera);
  const avatarX = layout.bounds.x + layout.bounds.width / 2;
  const avatarY = layout.bounds.y + layout.bounds.height - 51;

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
          style={reducedMotion ? { transition: "none" } : undefined}
        >
          <PlanetLandscapeDecorations
            stage={stage}
            bounds={layout.bounds}
            viewBox={viewBox}
            equippedCosmetics={equippedCosmetics}
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
                  <PlanetObjectSprite object={placement.object} x={placement.x} y={placement.y} scale={1} />
                </g>
              );
            })}
          </g>
          <g className="planet-landscape-avatar" transform={`translate(${avatarX} ${avatarY})`}>
            <AvatarSprite avatar={avatar} className="planet-landscape-avatar-sprite" />
          </g>
        </svg>
      </div>
      {selected && <section className="planet-landscape-selection" role="region" aria-label="선택한 오브젝트" aria-live="polite">
        <h2>{objectName(selected.object.kind)}</h2>
        <p>{STAGE_NAMES[selected.object.stage] ?? STAGE_NAMES[4]}</p>
        <p>{selected.object.ordinal + 1}번째 생성</p>
      </section>}
      <section className="planet-landscape-object-list" role="region" aria-label="오브젝트 목록">
        {layout.objects.length === 0
          ? <p className="empty-note">아직 생성된 오브젝트가 없습니다.</p>
          : <ol>{layout.objects.map((placement) => {
            const objectLabel = `${objectName(placement.object.kind)} ${placement.object.ordinal + 1}번째, ${STAGE_NAMES[placement.object.stage] ?? STAGE_NAMES[4]}`;
            return <li key={placement.id}><button
              type="button"
              data-object-list-id={placement.id}
              aria-label={objectLabel}
              aria-pressed={placement.id === exploration.selectedObjectId}
              onClick={() => selectPlacement(placement)}
            >{objectName(placement.object.kind)} <span>{placement.object.ordinal + 1}번째</span> <span>{STAGE_NAMES[placement.object.stage] ?? STAGE_NAMES[4]}</span></button></li>;
          })}</ol>}
      </section>
    </section>
  );
}
