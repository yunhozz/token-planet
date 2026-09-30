import { type CSSProperties, useEffect, useId, useMemo, useRef, useState } from "react";
import { AvatarSprite } from "./AvatarSprite";
import type { EquippedCosmetic, PlanetAvatar, PlanetObject } from "../types/usage";
import { styleIdForSku } from "./cosmeticStyles";
import { planWalk, restDuration, stepDuration, WALK_POINTS } from "./sceneMotion";
import { pickDialogue, type DialogueTarget } from "./sceneDialogue";

export { planWalk, WALK_POINTS } from "./sceneMotion";

export const STAGE_NAMES = ["자연 생태계", "정착·농경", "마을·초기 도시", "산업 문명", "첨단·우주 문명"];
const STAGE_THRESHOLDS = [5, 20, 50, 100];
const OBJECT_INTERVALS = [1, 2, 4, 8, 16];

function objectIdentity(object: PlanetObject) {
  return `${object.stage}-${object.ordinal}`;
}

export function objectProgress(growthCredit: number, stage: number) {
  let start = 0;
  let remainder = 0;
  for (let index = 0; index <= stage; index += 1) {
    const end = index === 4 ? growthCredit : Math.min(growthCredit, STAGE_THRESHOLDS[index]);
    const available = Math.max(0, end - start) + remainder;
    if (index === stage) return (available % OBJECT_INTERVALS[index]) / OBJECT_INTERVALS[index];
    remainder = available % OBJECT_INTERVALS[index];
    start = STAGE_THRESHOLDS[index];
  }
  return 0;
}

export function objectName(kind: string) {
  const names: Record<string, string> = {
    rock: "바위", water: "물", tree: "나무", fern: "양치식물", creature: "기초 생물",
    camp: "야영지", crops: "경작지", cottage: "오두막", path: "오솔길", well: "우물",
    house: "집", workshop: "작업장", plaza: "광장", road: "도로", market: "시장",
    factory: "공장", power: "전력 시설", rail: "철도", tower: "타워", district: "도시 구역",
    laboratory: "연구 시설", satellite: "위성", rocket: "발사 시설", solar: "태양 전지", habitat: "궤도 거주지",
  };
  return names[kind] ?? "행성 오브젝트";
}

function ObjectSprite({ object, x, y, scale }: { object: PlanetObject; x: number; y: number; scale: number }) {
  const transform = `translate(${x} ${y}) scale(${scale})`;
  const color = object.stage < 2 ? "#7db978" : object.stage === 2 ? "#e8bd75" : object.stage === 3 ? "#d18473" : "#80d4ce";
  switch (object.kind) {
    case "rock": return <g transform={transform}><rect width="12" height="7" fill="#87949b"/><rect x="3" y="-4" width="6" height="4" fill="#aeb7ae"/></g>;
    case "water": return <g transform={transform}><rect width="18" height="5" fill="#70c7c8"/><rect x="4" y="-3" width="10" height="3" fill="#a6e2d1"/></g>;
    case "tree": case "fern": return <g transform={transform}><rect x="6" y="7" width="4" height="9" fill="#956449"/><rect x="2" y="2" width="12" height="7" fill="#548f61"/><rect x="5" y="-2" width="7" height="5" fill="#83bf76"/></g>;
    case "creature": return <g transform={transform}><rect x="1" y="2" width="13" height="8" fill="#f0b969"/><rect x="11" width="6" height="6" fill="#f0b969"/><rect x="14" y="1" width="1" height="1" fill="#292d3f"/><rect x="3" y="9" width="2" height="4" fill="#9b6658"/><rect x="11" y="9" width="2" height="4" fill="#9b6658"/></g>;
    case "camp": case "cottage": case "house": case "market": return <g transform={transform}><rect x="1" y="5" width="18" height="12" fill={object.stage === 1 ? "#bd805c" : "#e9d39a"}/><path d="M-2 6 10 -3 22 6Z" fill={object.stage < 2 ? "#d27f68" : "#985e62"}/><rect x="8" y="10" width="4" height="7" fill="#514257"/><rect x="3" y="8" width="3" height="3" fill="#78bfc1"/></g>;
    case "crops": case "path": case "road": case "rail": return <g transform={transform}><rect width="20" height="5" fill={object.kind === "path" ? "#b28a67" : object.kind === "road" || object.kind === "rail" ? "#68717a" : "#92b95c"}/><rect x="3" y="-4" width="2" height="4" fill={color}/><rect x="9" y="-4" width="2" height="4" fill={color}/><rect x="15" y="-4" width="2" height="4" fill={color}/></g>;
    case "well": case "workshop": case "plaza": case "district": case "power": case "factory": case "tower": case "laboratory": case "habitat": case "solar": return <g transform={transform}><rect x="1" y="3" width="18" height="15" fill={color}/><rect x="4" y="-2" width="12" height="6" fill={color}/><rect x="5" y="7" width="3" height="4" fill="#a4ddcc"/><rect x="12" y="7" width="3" height="4" fill="#a4ddcc"/><rect x="9" y="13" width="3" height="5" fill="#41445a"/></g>;
    case "satellite": case "rocket": return <g transform={transform}><rect x="7" y="-2" width="6" height="17" fill="#e7ddbd"/><path d="m7 1-5 6h5m6-6 5 6h-5" fill="#70c7c8"/><rect x="8" y="-6" width="4" height="4" fill="#e17e6e"/><rect x="8" y="14" width="4" height="4" fill="#e17e6e"/></g>;
    default: return <g transform={transform}><rect x="2" y="2" width="16" height="11" fill={color}/><rect x="5" y="-2" width="10" height="4" fill="#e7ddbd"/><rect x="6" y="5" width="3" height="3" fill="#a4ddcc"/><rect x="12" y="5" width="3" height="3" fill="#a4ddcc"/></g>;
  }
}

export function PlanetScene({ stage, progress, avatar = "masculine", objects = [], equippedCosmetics = [], compact = false, animate = false, interactive = false, publicOnly = false, incomplete = false, cycleId = "" }: { stage: number; progress: number; avatar?: PlanetAvatar; objects?: PlanetObject[]; equippedCosmetics?: Pick<EquippedCosmetic, "slot_id" | "sku">[]; compact?: boolean; animate?: boolean; interactive?: boolean; publicOnly?: boolean; incomplete?: boolean; cycleId?: string }) {
  const name = STAGE_NAMES[stage] ?? STAGE_NAMES[4];
  const clipId = `planet-clip-${useId().replace(/:/g, "")}`;
  const sceneRef = useRef<HTMLElement | null>(null);
  const initialPosition = Math.floor(WALK_POINTS.length / 2);
  const [avatarPosition, setAvatarPosition] = useState(initialPosition);
  const avatarPositionRef = useRef(initialPosition);
  const previousDestinationRef = useRef<number | null>(null);
  const [avatarStepDuration, setAvatarStepDuration] = useState(380);
  const [avatarFacing, setAvatarFacing] = useState<"left" | "right">("right");
  const [avatarWalking, setAvatarWalking] = useState(false);
  const [eyesClosed, setEyesClosed] = useState(false);
  const [isIntersecting, setIsIntersecting] = useState(() => typeof IntersectionObserver === "undefined");
  const [documentVisible, setDocumentVisible] = useState(() => !document.hidden);
  const [reducedMotion, setReducedMotion] = useState(() => typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [sceneEntering, setSceneEntering] = useState(false);
  const sceneHasEntered = useRef(false);
  const [enteringObjects, setEnteringObjects] = useState<Set<string>>(() => new Set());
  const knownObjectIds = useRef(new Set(objects.map(objectIdentity)));
  const knownObjectCycleId = useRef(cycleId);
  const pendingNewObjectKind = useRef<string | null>(null);
  const previousDialogue = useRef<string | null>(null);
  const [speechLine, setSpeechLine] = useState<string | null>(null);
  const speechTimer = useRef<number | null>(null);
  const motionActive = animate && !compact && isIntersecting && documentVisible && !reducedMotion;
  const motionState = compact ? "paused" : reducedMotion ? "reduced" : motionActive ? "active" : "paused";
  const tiles = useMemo(() => {
    const result = new Map<string, { column: number; row: number; objects: PlanetObject[] }>();
    for (const object of objects) {
      const column = Math.min(9, Math.floor(object.x / 10));
      const row = Math.min(4, Math.max(0, Math.floor((object.y - 28) / 11)));
      const key = `${column}-${row}`;
      const tile = result.get(key) ?? { column, row, objects: [] };
      tile.objects.push(object);
      result.set(key, tile);
    }
    return result;
  }, [objects]);
  const hasCosmetic = (slotId: string, styleId: string) => equippedCosmetics.some(
    (item) => item.slot_id === slotId && styleIdForSku(item.sku) === styleId,
  );
  const avatarPoint = WALK_POINTS[avatarPosition];
  const interactionStyle = {
    "--avatar-hit-left": `${(avatarPoint.x + 10) / 3.6}%`,
    "--avatar-hit-top": `${(avatarPoint.y + 12) / 3.2}%`,
    "--avatar-bubble-left": `${(avatarPoint.x + 10) / 3.6}%`,
    "--avatar-bubble-top": `${avatarPoint.y / 3.2}%`,
    "--avatar-step-ms": `${avatarStepDuration}ms`,
  } as CSSProperties;

  useEffect(() => {
    const node = sceneRef.current;
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
    const node = sceneRef.current;
    if (!node) return;
    const finishAnimation = (event: Event) => {
      if (event.target === node.querySelector(".planet-svg")) setSceneEntering(false);
      if (event.target instanceof Element) {
        const identity = event.target.getAttribute("data-object-id");
        if (identity) finishObjectEntrance(identity);
      }
    };
    node.addEventListener("animationend", finishAnimation);
    return () => node.removeEventListener("animationend", finishAnimation);
  }, []);

  useEffect(() => {
    const updateVisibility = () => setDocumentVisible(!document.hidden);
    document.addEventListener("visibilitychange", updateVisibility);
    return () => document.removeEventListener("visibilitychange", updateVisibility);
  }, []);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const updatePreference = () => setReducedMotion(preference.matches);
    updatePreference();
    preference.addEventListener?.("change", updatePreference);
    return () => preference.removeEventListener?.("change", updatePreference);
  }, []);

  useEffect(() => {
    if (reducedMotion) {
      sceneHasEntered.current = true;
      setSceneEntering(false);
      return;
    }
    if (motionActive && !sceneHasEntered.current) {
      sceneHasEntered.current = true;
      setSceneEntering(true);
    }
  }, [motionActive, reducedMotion]);

  useEffect(() => {
    if (knownObjectCycleId.current !== cycleId) {
      knownObjectCycleId.current = cycleId;
      knownObjectIds.current = new Set(objects.map(objectIdentity));
      pendingNewObjectKind.current = null;
      return;
    }
    const added = objects.filter((object) => !knownObjectIds.current.has(objectIdentity(object)));
    objects.forEach((object) => knownObjectIds.current.add(objectIdentity(object)));
    if (!publicOnly && added.length > 0) pendingNewObjectKind.current = added[added.length - 1].kind;
    if (motionActive && added.length) {
      setEnteringObjects((current) => new Set([...current, ...added.map(objectIdentity)]));
    }
  }, [objects, motionActive, cycleId, publicOnly]);

  useEffect(() => {
    if (!motionActive || speechLine) {
      setAvatarWalking(false);
      setEyesClosed(false);
      if (!motionActive) {
        setSceneEntering(false);
        setEnteringObjects(new Set());
      }
      return;
    }
    let movementTimer: number | undefined;
    let blinkTimer: number | undefined;
    let blinkCloseTimer: number | undefined;
    let walking = false;
    let cancelled = false;

    const scheduleWalk = () => {
      movementTimer = window.setTimeout(() => {
        const startIndex = avatarPositionRef.current;
        const route = planWalk(startIndex, previousDestinationRef.current, Math.random);
        previousDestinationRef.current = startIndex;
        const duration = stepDuration(Math.random);
        setAvatarStepDuration(duration);
        let step = 0;
        walking = true;
        setAvatarWalking(true);
        setEyesClosed(false);
        const moveNext = () => {
          if (cancelled) return;
          const nextPosition = route[step];
          step += 1;
          if (nextPosition === undefined) {
            walking = false;
            setAvatarWalking(false);
            scheduleWalk();
            return;
          }
          const previousPosition = avatarPositionRef.current;
          setAvatarFacing(nextPosition < previousPosition ? "left" : "right");
          avatarPositionRef.current = nextPosition;
          setAvatarPosition(nextPosition);
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
      if (movementTimer !== undefined) window.clearTimeout(movementTimer);
      if (blinkTimer !== undefined) window.clearTimeout(blinkTimer);
      if (blinkCloseTimer !== undefined) window.clearTimeout(blinkCloseTimer);
    };
  }, [motionActive, speechLine]);

  useEffect(() => () => {
    if (speechTimer.current !== null) window.clearTimeout(speechTimer.current);
  }, []);

  function speak(target: DialogueTarget) {
    const newObjectKind = publicOnly ? null : pendingNewObjectKind.current;
    const line = pickDialogue({
      target,
      stage,
      progress,
      incomplete,
      newObjectKind,
      publicOnly,
      previous: previousDialogue.current,
    }, Math.random);
    previousDialogue.current = line;
    if (newObjectKind) pendingNewObjectKind.current = null;
    setSpeechLine(line);
    if (speechTimer.current !== null) window.clearTimeout(speechTimer.current);
    speechTimer.current = window.setTimeout(() => {
      speechTimer.current = null;
      setSpeechLine(null);
    }, 4_000);
  }

  function finishObjectEntrance(identity: string) {
    setEnteringObjects((current) => {
      if (!current.has(identity)) return current;
      const next = new Set(current);
      next.delete(identity);
      return next;
    });
  }

  return (
    <figure ref={sceneRef} className={`planet-figure ${compact ? "planet-figure--compact" : ""}`} data-motion={motionState}>
      <div className="planet-scene-canvas">
        <svg className={`planet-svg${motionActive ? " planet-svg--floating" : ""}${motionActive && sceneEntering ? " planet-svg--entering" : ""}`} viewBox="0 0 360 320" role="img" aria-label={stage >= 4 ? `${name}, 최종 시대에서 발전이 계속됩니다` : `${name}, 다음 시대까지 ${Math.round(progress * 100)}%`} shapeRendering="crispEdges">
        <defs>
          <clipPath id={clipId}><circle cx="180" cy="157" r="107" /></clipPath>
        </defs>
        <g className="planet-stars" fill="#f6df9d">
          <rect x="49" y="53" width="4" height="4"/><rect x="290" y="68" width="3" height="3"/><rect x="278" y="199" width="4" height="4"/><rect x="68" y="220" width="3" height="3"/><rect x="113" y="35" width="3" height="3"/><rect x="245" y="245" width="3" height="3"/>
        </g>
        {hasCosmetic("ring", "thin_ring") && <ellipse data-cosmetic="thin_ring" cx="180" cy="157" rx="146" ry="39" transform="rotate(-18 180 157)" fill="none" stroke="#f1cf89" strokeWidth="3"/>}
        {hasCosmetic("ring", "double_ring") && <g data-cosmetic="double_ring" fill="none" stroke="#a6d9a2"><ellipse cx="180" cy="157" rx="151" ry="43" transform="rotate(-18 180 157)" strokeWidth="3"/><ellipse cx="180" cy="157" rx="141" ry="34" transform="rotate(-18 180 157)" strokeWidth="2"/></g>}
        {hasCosmetic("ring", "moonlets") && <g data-cosmetic="moonlets" fill="#d7d5bd" stroke="#7e9a9a" strokeWidth="2" shapeRendering="crispEdges"><ellipse cx="180" cy="157" rx="157" ry="49" transform="rotate(-18 180 157)" fill="none" stroke="#82a9a6" strokeWidth="1.5"/><rect x="62" y="126" width="10" height="10"/><rect x="104" y="103" width="6" height="6"/><rect x="243" y="202" width="12" height="12"/><rect x="286" y="168" width="7" height="7"/><rect x="198" y="213" width="5" height="5" fill="#f0d28b"/></g>}
        <circle cx="180" cy="157" r="119" fill="#22364b" stroke="#e6c987" strokeWidth="4"/>
        <circle cx="180" cy="157" r="111" fill="#72c5bd" stroke="#3c6570" strokeWidth="3"/>
        <g clipPath={`url(#${clipId})`}>
          <rect x="65" y="55" width="230" height="210" fill={stage === 0 ? "#63b8b4" : "#438f83"}/>
          <path d="M64 146h49v-13h23v11h16v-18h25v-8h24v13h22v-11h28v24h49v140H64Z" fill={stage === 0 ? "#7bbd77" : "#9fc36f"}/>
          <path d="M64 209h51v-12h24v10h28v-16h21v13h33v-11h26v16h50v70H64Z" fill="#80b96d"/>
          {stage > 0 && <g fill="#cda36c"><path d="M76 178h218v8H76zM85 194h200v5H85z"/><path d="M104 163h5v38h-5zM156 163h5v38h-5zM210 163h5v38h-5zM262 163h5v38h-5z"/></g>}
          {stage > 1 && <path d="M68 219h220v8H68zm30 5h8v38h-8zm70 0h8v38h-8zm72 0h8v38h-8z" fill="#a58a67"/>}
          {stage > 2 && <g fill="#5d6172"><path d="M95 132h17v31H95zM121 120h24v43h-24zM174 128h18v35h-18zM211 114h26v49h-26zM249 127h19v36h-19z"/><path d="M99 137h4v6h-4zm10 0h2v6h-2zm17-12h5v6h-5zm9 0h4v6h-4zm78-4h5v7h-5zm11 0h5v7h-5z" fill="#9fdbce"/></g>}
          {stage > 3 && <g fill="#d4e7cb"><path d="M169 96h9v28h-9zM173 84h2v12h-2zM157 101h9v5h-9zm24 0h9v5h-9z"/><path d="M243 91h18v5h-18zm6-5h6v15h-6z" fill="#65c9c7"/></g>}
          {hasCosmetic("sky", "star_cluster") && <g data-cosmetic="star_cluster" fill="#fff0ad"><rect x="99" y="95" width="6" height="6"/><rect x="111" y="81" width="4" height="4"/><rect x="126" y="101" width="3" height="3"/><rect x="141" y="88" width="5" height="5"/><rect x="154" y="105" width="3" height="3"/><rect x="119" y="116" width="4" height="4"/></g>}
          {hasCosmetic("sky", "aurora") && <g data-cosmetic="aurora" fill="none" stroke="#a6e6cf" strokeWidth="5" opacity=".72"><path d="M81 121h17v-8h15v-8h14v7h15v-8h15"/><path d="M94 136h18v-7h15v-8h18v5h14" stroke="#82d7d5" strokeWidth="3"/></g>}
          {hasCosmetic("sky", "meteor_shower") && <g data-cosmetic="meteor_shower" shapeRendering="crispEdges"><path d="m94 96 18-15h4L98 98Z" fill="#f3d38e"/><path d="m96 95 8-7" stroke="#fff0ad" strokeWidth="2"/><path d="m233 122 20-17h4l-18 18Z" fill="#9de0d3"/><path d="m236 120 8-7" stroke="#e6f1cf" strokeWidth="2"/><rect x="135" y="71" width="4" height="4" fill="#fff0ad"/><rect x="260" y="143" width="3" height="3" fill="#fff0ad"/></g>}
          {hasCosmetic("surface", "flower_garden") && <g data-cosmetic="flower_garden" transform="translate(119 220)" shapeRendering="crispEdges"><path d="M0 27h39v7H0z" fill="#8c7653"/><path d="M4 23h31v4H4z" fill="#5d9561"/><path d="M8 22v-9m10 9V9m10 13v-8" stroke="#4d875a" strokeWidth="2"/><rect x="5" y="10" width="6" height="6" fill="#efad7a"/><rect x="16" y="6" width="7" height="7" fill="#f1d27f"/><rect x="27" y="11" width="6" height="6" fill="#d992b1"/><rect x="7" y="12" width="2" height="2" fill="#fff0ad"/><rect x="18" y="8" width="2" height="2" fill="#fff0ad"/><rect x="29" y="13" width="2" height="2" fill="#fff0ad"/></g>}
          {hasCosmetic("surface", "observatory") && <g data-cosmetic="observatory" transform="translate(217 215)" shapeRendering="crispEdges"><rect x="1" y="20" width="31" height="24" fill="#596c82" stroke="#d6c58d" strokeWidth="2"/><path d="M0 20a17 13 0 0 1 34 0Z" fill="#8ec7c5" stroke="#f0d28b" strokeWidth="2"/><path d="M13 3h7v17h-7zM9 10h15v5H9z" fill="#d7e6cd"/><rect x="6" y="26" width="5" height="5" fill="#f6d58c"/><rect x="21" y="26" width="5" height="5" fill="#f6d58c"/><rect x="14" y="33" width="5" height="11" fill="#39475e"/></g>}
          {[...tiles.entries()].map(([key, tile]) => {
            const x = 90 + tile.column * 16.5;
            const y = 150 + tile.row * 11;
            const visible = tile.objects.slice(-4);
            return <g key={key}>
              {visible.map((object, index) => {
                const identity = objectIdentity(object);
                return <g key={identity} data-object-id={identity} className={motionActive && enteringObjects.has(identity) ? "planet-object--entering" : undefined}><ObjectSprite object={object} x={x + (tile.objects.length === 1 ? 0 : (index % 2) * 8.5)} y={y + (tile.objects.length === 1 ? 0 : Math.floor(index / 2) * 8)} scale={tile.objects.length === 1 ? 0.55 : 0.42} /></g>;
              })}
              {tile.objects.length > 4 && <g transform={`translate(${x + 9} ${y + 8})`}><rect width="13" height="8" fill="#29354a" stroke="#f0d288" strokeWidth=".7"/><text x="6.5" y="6" fill="#f6eed3" fontSize="5" textAnchor="middle">+{tile.objects.length - 4 > 99 ? "99+" : tile.objects.length - 4}</text></g>}
            </g>;
          })}
          <g data-planet-avatar="true" className={avatarWalking ? "planet-avatar planet-avatar--walking" : "planet-avatar"} style={{ "--avatar-step-ms": `${avatarStepDuration}ms` } as CSSProperties} transform={`translate(${WALK_POINTS[avatarPosition].x} ${WALK_POINTS[avatarPosition].y}) scale(1.05)`}>
            <AvatarSprite avatar={avatar} className="planet-scene-avatar" facing={avatarFacing} eyesClosed={eyesClosed} walking={avatarWalking} />
          </g>
          {hasCosmetic("surface", "flag") && <g data-cosmetic="flag" transform="translate(172 220)" shapeRendering="crispEdges"><rect x="8" y="0" width="4" height="35" fill="#8c6655"/><path d="M12 2h24v11H22l-10 7Z" fill="#ec8c78" stroke="#572f4b" strokeWidth="2"/><rect x="3" y="34" width="17" height="4" fill="#926e58"/></g>}
          {hasCosmetic("surface", "crystal_tower") && <g data-cosmetic="crystal_tower" transform="translate(168 218)" shapeRendering="crispEdges"><rect x="2" y="23" width="27" height="15" fill="#536c91" stroke="#c2c6cf" strokeWidth="2"/><path d="M8 23V9l8-8 8 8v14Z" fill="#a6d9e2" stroke="#e3f0de" strokeWidth="2"/><path d="M16 3v19M10 14h12" stroke="#6ba9bb" strokeWidth="2"/><rect x="11" y="28" width="9" height="10" fill="#455471"/></g>}
          <path d="M180 50a107 107 0 0 1 0 214c44-48 63-155 0-214Z" fill="#19233a" opacity=".2"/>
        </g>
        <circle cx="180" cy="157" r="107" fill="none" stroke="#b8ebce" strokeWidth="2"/>
        {hasCosmetic("forecourt", "pond") && <g data-cosmetic="pond" transform="translate(151 276)" shapeRendering="crispEdges"><path d="M0 9h58v16H0z" fill="#8d7757" stroke="#d9c288" strokeWidth="2"/><path d="M5 12h48v9H5z" fill="#4c9f9e"/><path d="M12 13h10v2H12zm23 5h12v2H35z" fill="#b6e3ca"/><rect x="-7" y="25" width="72" height="4" fill="#677b63"/></g>}
        {hasCosmetic("forecourt", "lantern") && <g data-cosmetic="lantern" transform="translate(169 270)" shapeRendering="crispEdges"><rect x="-4" y="34" width="30" height="4" fill="#677b63"/><rect x="9" y="11" width="4" height="23" fill="#8f6b4e"/><rect x="3" y="2" width="16" height="14" fill="#e8c67d" stroke="#754e52" strokeWidth="2"/><rect x="7" y="5" width="8" height="8" fill="#fff0ad"/><path d="M1 2h20M5 -2h12" stroke="#754e52" strokeWidth="2"/></g>}
        {hasCosmetic("forecourt", "rover") && <g data-cosmetic="rover" transform="translate(152 279)" shapeRendering="crispEdges"><rect x="2" y="9" width="48" height="16" fill="#ce9b68" stroke="#564d58" strokeWidth="2"/><rect x="11" y="1" width="20" height="10" fill="#8ac8c4" stroke="#564d58" strokeWidth="2"/><rect x="20" y="-5" width="3" height="7" fill="#e8d58f"/><circle cx="13" cy="26" r="5" fill="#34475a" stroke="#e2d59d" strokeWidth="2"/><circle cx="40" cy="26" r="5" fill="#34475a" stroke="#e2d59d" strokeWidth="2"/><rect x="4" y="14" width="7" height="4" fill="#f2d287"/><path d="M33 12h12v3H33z" fill="#a3ddd1"/></g>}
        {hasCosmetic("forecourt", "greenhouse") && <g data-cosmetic="greenhouse" transform="translate(147 273)" shapeRendering="crispEdges"><path d="M1 34V12L33 1l32 11v22Z" fill="#84c8bd" fillOpacity=".78" stroke="#d8e6bd" strokeWidth="2"/><path d="M5 14h55M11 11v23m22-30v30m21-23v23" stroke="#607e79" strokeWidth="2"/><rect x="26" y="22" width="14" height="12" fill="#8eb76c"/><path d="M33 22v-8m-4 8 4-4 4 4" fill="#79a966" stroke="#477451" strokeWidth="2"/><rect x="-4" y="34" width="74" height="4" fill="#677b63"/></g>}
        </svg>
        {interactive && <div className="planet-interaction-layer" data-avatar-walking={avatarWalking ? "true" : "false"} style={interactionStyle}>
          <button type="button" className="planet-hit-area" aria-label="행성에게 말 걸기" onClick={() => speak("planet")} />
          <button type="button" className="avatar-hit-area" aria-label="아바타에게 말 걸기" onClick={() => speak("avatar")} />
          {speechLine && <div className="planet-speech-bubble" role="status" aria-atomic="true">{speechLine}</div>}
        </div>}
      </div>
      {!compact && <figcaption className="planet-caption">{name}</figcaption>}
    </figure>
  );
}
