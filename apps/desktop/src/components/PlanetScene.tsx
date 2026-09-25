import { useId } from "react";
import type { PlanetAvatar, PlanetObject } from "../types/usage";

export const STAGE_NAMES = ["자연 생태계", "정착·농경", "마을·초기 도시", "산업 문명", "첨단·우주 문명"];
const STAGE_THRESHOLDS = [5, 20, 50, 100];
const OBJECT_INTERVALS = [1, 2, 4, 8, 16];

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

export function PlanetScene({ stage, progress, avatar = "masculine", objects = [], compact = false }: { stage: number; progress: number; avatar?: PlanetAvatar; objects?: PlanetObject[]; compact?: boolean }) {
  const name = STAGE_NAMES[stage] ?? STAGE_NAMES[4];
  const clipId = `planet-clip-${useId().replace(/:/g, "")}`;
  const tiles = new Map<string, { column: number; row: number; objects: PlanetObject[] }>();
  for (const object of objects) {
    const column = Math.min(9, Math.floor(object.x / 10));
    const row = Math.min(4, Math.max(0, Math.floor((object.y - 28) / 11)));
    const key = `${column}-${row}`;
    const tile = tiles.get(key) ?? { column, row, objects: [] };
    tile.objects.push(object);
    tiles.set(key, tile);
  }
  return (
    <figure className={`planet-figure ${compact ? "planet-figure--compact" : ""}`}>
      <svg className="planet-svg" viewBox="0 0 360 320" role="img" aria-label={stage >= 4 ? `${name}, 최종 시대에서 발전이 계속됩니다` : `${name}, 다음 시대까지 ${Math.round(progress * 100)}%`} shapeRendering="crispEdges">
        <defs>
          <clipPath id={clipId}><circle cx="180" cy="157" r="107" /></clipPath>
        </defs>
        <g className="planet-stars" fill="#f6df9d">
          <rect x="49" y="53" width="4" height="4"/><rect x="290" y="68" width="3" height="3"/><rect x="278" y="199" width="4" height="4"/><rect x="68" y="220" width="3" height="3"/><rect x="113" y="35" width="3" height="3"/><rect x="245" y="245" width="3" height="3"/>
        </g>
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
          {[...tiles.entries()].map(([key, tile]) => {
            const x = 90 + tile.column * 16.5;
            const y = 150 + tile.row * 11;
            const visible = tile.objects.slice(-4);
            return <g key={key}>
              {visible.map((object, index) => <ObjectSprite key={`${object.stage}-${object.ordinal}`} object={object} x={x + (tile.objects.length === 1 ? 0 : (index % 2) * 8.5)} y={y + (tile.objects.length === 1 ? 0 : Math.floor(index / 2) * 8)} scale={tile.objects.length === 1 ? 0.55 : 0.42} />)}
              {tile.objects.length > 4 && <g transform={`translate(${x + 9} ${y + 8})`}><rect width="13" height="8" fill="#29354a" stroke="#f0d288" strokeWidth=".7"/><text x="6.5" y="6" fill="#f6eed3" fontSize="5" textAnchor="middle">+{tile.objects.length - 4 > 99 ? "99+" : tile.objects.length - 4}</text></g>}
            </g>;
          })}
          <g transform="translate(169 126) scale(1.05)">
            <rect x="5" y="1" width="7" height="2" fill={avatar === "feminine" ? "#51395f" : "#253c55"}/>
            <rect x="3" y="3" width="11" height="5" fill={avatar === "feminine" ? "#51395f" : "#253c55"}/>
            <rect x="4" y="5" width="9" height="6" fill="#f3c995"/>
            {avatar === "feminine" && <><rect x="2" y="5" width="2" height="7" fill="#51395f"/><rect x="13" y="5" width="2" height="7" fill="#51395f"/></>}
            <rect x="5" y="7" width="1" height="1" fill="#292b3b"/><rect x="10" y="7" width="1" height="1" fill="#292b3b"/>
            <rect x="4" y="12" width="9" height="5" fill={avatar === "feminine" ? "#d57875" : "#4c9b9a"}/>
            <rect x="5" y="17" width="3" height="2" fill="#34374a"/><rect x="10" y="17" width="3" height="2" fill="#34374a"/>
          </g>
          <path d="M180 50a107 107 0 0 1 0 214c44-48 63-155 0-214Z" fill="#19233a" opacity=".2"/>
        </g>
        <circle cx="180" cy="157" r="107" fill="none" stroke="#b8ebce" strokeWidth="2"/>
      </svg>
      {!compact && <figcaption className="planet-caption">{name}</figcaption>}
    </figure>
  );
}
