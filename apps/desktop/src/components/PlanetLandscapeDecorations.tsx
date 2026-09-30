import type { JSX } from "react";
import type { EquippedCosmetic } from "../types/usage";
import { styleIdForSku } from "./cosmeticStyles";
import type { LandscapeBounds } from "./planetLandscapeLayout";

type Props = {
  stage: number;
  bounds: LandscapeBounds;
  viewBox: LandscapeBounds;
  equippedCosmetics: EquippedCosmetic[];
};

const SKY_COLORS = ["#23445b", "#28475a", "#304456", "#343b50", "#263c59"];
const DISTANT_COLORS = ["#32656a", "#3e6b65", "#59685d", "#56535c", "#3e5968"];
const GROUND_COLORS = ["#5f967f", "#7c9b65", "#8a9b62", "#796d63", "#63837a"];
const RECOGNIZED_COSMETICS = new Set([
  "star_cluster", "aurora", "thin_ring", "double_ring", "flag", "crystal_tower",
  "meteor_shower", "moonlets", "flower_garden", "observatory", "pond", "lantern", "rover", "greenhouse",
]);

function eraIndex(stage: number): number {
  return Number.isFinite(stage) ? Math.max(0, Math.min(4, Math.floor(stage))) : 0;
}

function positiveModulo(value: number, divisor: number): number {
  return ((value % divisor) + divisor) % divisor;
}

function visibleCells(viewBox: LandscapeBounds, spacing: number): number[] {
  const firstCell = Math.floor(viewBox.x / spacing);
  const cellCount = Math.ceil(viewBox.width / spacing) + 2;
  return Array.from({ length: cellCount }, (_, index) => firstCell + index);
}

export function PlanetLandscapeDecorations({ stage, bounds, viewBox, equippedCosmetics }: Props): JSX.Element {
  const era = eraIndex(stage);
  const groundTop = bounds.y;
  const groundBottom = Math.max(groundTop + bounds.height, viewBox.y + viewBox.height);
  const roadY = groundTop + Math.floor(bounds.height * 0.68);
  const surfaceX = bounds.x + bounds.width - 140;
  const forecourtX = bounds.x + bounds.width - 84;
  const emblemX = bounds.x + bounds.width - 82;
  const emblemY = groundTop - 134;
  const distantCells = visibleCells(viewBox, 360);
  const starCells = visibleCells(viewBox, 145);
  const grassCells = visibleCells(viewBox, 116);
  const skyCosmeticX = bounds.x + bounds.width * 0.18;
  const hasCosmetic = (slotId: string, styleId: string) => equippedCosmetics.some((item) =>
    item.slot_id === slotId
      && styleIdForSku(item.sku) === styleId
      && RECOGNIZED_COSMETICS.has(styleId),
  );
  const hasRingEmblem = ["thin_ring", "double_ring", "moonlets"].some((styleId) => hasCosmetic("ring", styleId));

  return (
    <g aria-hidden="true">
      <rect className="planet-landscape-sky" x={viewBox.x} y={viewBox.y} width={viewBox.width} height={viewBox.height} fill={SKY_COLORS[era]} />
      <g className="planet-landscape-stars" fill="#e8d59d">
        {starCells.map((cell) => {
          const x = cell * 145 + 27;
          const y = groundTop - 116 + positiveModulo(cell * 47, 93);
          const size = positiveModulo(cell, 3) === 0 ? 4 : 3;
          return <rect key={cell} x={x} y={y} width={size} height={size} />;
        })}
      </g>
      <g className="planet-landscape-distant-ground" fill={DISTANT_COLORS[era]}>
        {distantCells.map((cell) => {
          const x = cell * 360;
          const towerHeight = 18 + positiveModulo(cell * 17, 21);
          const mesaHeight = 22 + positiveModulo(cell * 23, 24);
          return <path key={cell} d={`M ${x} ${groundTop} h 42 v -${towerHeight} h 34 v 11 h 39 v -${mesaHeight} h 51 v 16 h 44 v -${towerHeight - 5} h 55 v 14 h 37 v -${mesaHeight - 4} h 35 v 13 h 23 L ${x + 360} ${groundTop} Z`} />;
        })}
      </g>
      <rect className="planet-landscape-ground" x={viewBox.x} y={groundTop} width={viewBox.width} height={groundBottom - groundTop} fill={GROUND_COLORS[era]} />
      <path d={`M ${viewBox.x} ${groundTop + 36} h ${viewBox.width} v 17 h -${viewBox.width} Z`} fill="#95a867" opacity=".28" />
      {era >= 1 && era <= 2 && <g data-landscape-decoration="cultivated-fields" fill="#a2ad6c" opacity=".8" shapeRendering="crispEdges">
        {Array.from({ length: Math.ceil(viewBox.width / 280) + 1 }, (_, index) => {
          const x = Math.floor(viewBox.x / 280) * 280 + index * 280 + 78;
          return <g key={x}><path d={`M ${x} ${groundTop + 94} h 68 v 4 h -68 Z M ${x + 8} ${groundTop + 105} h 60 v 3 h -60 Z M ${x + 16} ${groundTop + 115} h 52 v 3 h -52 Z`} /><path d={`M ${x + 12} ${groundTop + 89} v 5 M ${x + 31} ${groundTop + 89} v 5 M ${x + 51} ${groundTop + 89} v 5`} stroke="#5e895d" strokeWidth="2" /></g>;
        })}
      </g>}
      {era >= 3 && <g data-landscape-decoration="distant-industry" fill="#515562" shapeRendering="crispEdges">
        {Array.from({ length: Math.ceil(viewBox.width / 430) + 1 }, (_, index) => {
          const x = Math.floor(viewBox.x / 430) * 430 + index * 430 + 170;
          return <g key={x}><path d={`M ${x} ${groundTop} v -38 h 24 v 38 M ${x + 30} ${groundTop} v -56 h 18 v 56 M ${x + 55} ${groundTop} v -31 h 33 v 31`} /><path d={`M ${x + 5} ${groundTop - 29} h 5 v 7 h -5 M ${x + 35} ${groundTop - 48} h 7 v 8 h -7 M ${x + 65} ${groundTop - 24} h 7 v 6 h -7`} fill="#9bb7a0" /></g>;
        })}
      </g>}
      {era === 4 && <g data-landscape-decoration="orbital-relays" fill="none" stroke="#8ed4cf" strokeWidth="3" shapeRendering="crispEdges">
        {Array.from({ length: Math.ceil(viewBox.width / 520) + 1 }, (_, index) => {
          const x = Math.floor(viewBox.x / 520) * 520 + index * 520 + 260;
          return <g key={x}><path d={`M ${x} ${groundTop - 64} v 30 m -11 -19 h 22 M ${x} ${groundTop - 50} l -18 -11 M ${x} ${groundTop - 50} l 18 -11`} /><rect x={x - 3} y={groundTop - 70} width="6" height="6" fill="#f1cf89" /></g>;
        })}
      </g>}
      {era === 0 ? (
        <g data-landscape-decoration="natural-stream" fill="none" stroke="#5db4b2" strokeWidth="10" shapeRendering="crispEdges">
          <path d={`M ${viewBox.x} ${groundTop + 172} h 90 v 8 h 110 v -12 h 85 v 12 h 80 v -9 h 85 v 8 h ${viewBox.width}`} />
        </g>
      ) : (
        <g data-landscape-decoration="era-road" shapeRendering="crispEdges">
          <path d={`M ${viewBox.x} ${roadY} h ${viewBox.width}`} stroke={era >= 3 ? "#53616c" : "#ab8b5f"} strokeWidth={era >= 3 ? 14 : 10} />
          <path d={`M ${viewBox.x} ${roadY - 2} h ${viewBox.width}`} stroke={era >= 4 ? "#80d2c8" : "#d1b57a"} strokeWidth="2" opacity=".8" />
          {era >= 2 && visibleCells(viewBox, 280).map((cell) => {
            const x = cell * 280 + 92;
            return <path key={cell} d={`M ${x} ${groundTop + 78} v ${bounds.height * 0.52} M ${x + 136} ${groundTop + 102} v ${bounds.height * 0.48}`} stroke={era >= 3 ? "#64717b" : "#b08f60"} strokeWidth="7" />;
          })}
          {era >= 3 && <path d={`M ${viewBox.x} ${roadY + 18} h ${viewBox.width}`} stroke="#d3b875" strokeWidth="3" strokeDasharray="18 14" />}
        </g>
      )}
      <g className="planet-landscape-ground-dressing" fill="#b4bd76" shapeRendering="crispEdges">
        {grassCells.map((cell) => {
          const x = cell * 116 + 31;
          const y = groundTop + 94 + positiveModulo(cell * 53, Math.max(80, Math.floor(bounds.height * 0.43)));
          const width = 6 + positiveModulo(cell * 7, 4);
          return <g key={cell} data-landscape-dressing="grass"><rect x={x} y={y} width={width} height="3" /><rect x={x + 5} y={y - 4} width="3" height="4" /></g>;
        })}
      </g>
      <g data-landscape-slot="sky" shapeRendering="crispEdges">
        {hasCosmetic("sky", "star_cluster") && <g data-cosmetic="star_cluster" fill="#fff0ad">
          <rect x={skyCosmeticX} y={groundTop - 168} width="7" height="7" />
          <rect x={skyCosmeticX + 24} y={groundTop - 143} width="4" height="4" />
          <rect x={skyCosmeticX + 57} y={groundTop - 181} width="5" height="5" />
          <rect x={skyCosmeticX + 89} y={groundTop - 157} width="3" height="3" />
          <rect x={skyCosmeticX + 113} y={groundTop - 172} width="6" height="6" />
        </g>}
        {hasCosmetic("sky", "aurora") && <g data-cosmetic="aurora" fill="none" stroke="#9fe2ce" opacity=".75">
          <path d={`M ${skyCosmeticX} ${groundTop - 112} h 48 v -13 h 72 v 10 h 66 v -17 h 60`} strokeWidth="6" />
          <path d={`M ${skyCosmeticX + 28} ${groundTop - 91} h 48 v -9 h 72 v 8 h 54`} stroke="#73cbca" strokeWidth="3" />
        </g>}
        {hasCosmetic("sky", "meteor_shower") && <g data-cosmetic="meteor_shower">
          <path d={`M ${bounds.x + bounds.width * 0.72} ${groundTop - 177} l 28 -22 h 5 l -26 25 Z M ${bounds.x + bounds.width * 0.52} ${groundTop - 123} l 19 -15 h 4 l -18 18 Z`} fill="#f3d38e" />
          <path d={`M ${bounds.x + bounds.width * 0.73} ${groundTop - 174} l 15 -12 M ${bounds.x + bounds.width * 0.53} ${groundTop - 120} l 10 -8`} stroke="#fff0ad" strokeWidth="2" />
        </g>}
      </g>
      <g data-landscape-slot="ring" shapeRendering="crispEdges">
        {hasRingEmblem && <g className="planet-landscape-ring-emblem">
          <circle cx={emblemX} cy={emblemY} r="22" fill="#3d7d82" stroke="#a6d9a2" strokeWidth="3" />
          <path d={`M ${emblemX - 14} ${emblemY + 12} a 20 20 0 0 0 28 -25 c -4 10 -14 18 -28 25 Z`} fill="#274b64" opacity=".65" />
          {hasCosmetic("ring", "thin_ring") && <g data-cosmetic="thin_ring" fill="none" stroke="#f1cf89" strokeWidth="3">
            <ellipse cx={emblemX} cy={emblemY} rx="35" ry="10" transform={`rotate(-18 ${emblemX} ${emblemY})`} />
          </g>}
          {hasCosmetic("ring", "double_ring") && <g data-cosmetic="double_ring" fill="none" stroke="#a6d9a2">
            <ellipse cx={emblemX} cy={emblemY} rx="39" ry="14" transform={`rotate(-18 ${emblemX} ${emblemY})`} strokeWidth="3" />
            <ellipse cx={emblemX} cy={emblemY} rx="31" ry="8" transform={`rotate(-18 ${emblemX} ${emblemY})`} strokeWidth="2" />
          </g>}
          {hasCosmetic("ring", "moonlets") && <g data-cosmetic="moonlets" fill="#d7d5bd" stroke="#7e9a9a" strokeWidth="2">
            <ellipse cx={emblemX} cy={emblemY} rx="44" ry="17" transform={`rotate(-18 ${emblemX} ${emblemY})`} fill="none" strokeWidth="1.5" />
            <rect x={emblemX - 37} y={emblemY - 3} width="7" height="7" />
            <rect x={emblemX + 27} y={emblemY + 9} width="9" height="9" />
            <rect x={emblemX + 12} y={emblemY - 18} width="5" height="5" fill="#f0d28b" />
          </g>}
        </g>}
      </g>
      <g data-landscape-slot="surface" shapeRendering="crispEdges">
        {hasCosmetic("surface", "flag") && <g data-cosmetic="flag" transform={`translate(${surfaceX} ${groundTop + 60})`}>
          <rect x="7" y="0" width="4" height="40" fill="#8c6655" />
          <path d="M 11 2 h 32 v 16 h -18 l -14 9 Z" fill="#ec8c78" stroke="#572f4b" strokeWidth="2" />
          <rect x="2" y="39" width="25" height="5" fill="#926e58" />
        </g>}
        {hasCosmetic("surface", "crystal_tower") && <g data-cosmetic="crystal_tower" transform={`translate(${surfaceX} ${groundTop + 55})`}>
          <rect x="4" y="39" width="42" height="13" fill="#536c91" stroke="#c2c6cf" strokeWidth="2" />
          <path d="M 12 39 V 14 L 27 0 L 42 14 V 39 Z" fill="#a6d9e2" stroke="#e3f0de" strokeWidth="2" />
          <path d="M 27 4 V 37 M 16 24 H 38" stroke="#6ba9bb" strokeWidth="3" />
          <rect x="22" y="43" width="10" height="9" fill="#455471" />
        </g>}
        {hasCosmetic("surface", "flower_garden") && <g data-cosmetic="flower_garden" transform={`translate(${surfaceX} ${groundTop + 102})`}>
          <path d="M 0 31 h 72 v 7 H 0 Z" fill="#8c7653" />
          <path d="M 7 27 h 58 v 4 H 7 Z" fill="#5d9561" />
          <path d="M 16 26 v -11 m 18 11 V 8 m 21 18 V 13" stroke="#4d875a" strokeWidth="2" />
          <rect x="12" y="10" width="7" height="7" fill="#efad7a" />
          <rect x="31" y="5" width="8" height="8" fill="#f1d27f" />
          <rect x="52" y="9" width="7" height="7" fill="#d992b1" />
          <rect x="14" y="12" width="2" height="2" fill="#fff0ad" />
          <rect x="34" y="8" width="2" height="2" fill="#fff0ad" />
          <rect x="54" y="11" width="2" height="2" fill="#fff0ad" />
        </g>}
        {hasCosmetic("surface", "observatory") && <g data-cosmetic="observatory" transform={`translate(${surfaceX} ${groundTop + 55})`}>
          <rect x="1" y="28" width="52" height="35" fill="#596c82" stroke="#d6c58d" strokeWidth="2" />
          <path d="M 0 29 a 27 19 0 0 1 54 0 Z" fill="#8ec7c5" stroke="#f0d28b" strokeWidth="2" />
          <path d="M 23 5 h 8 v 23 h -8 Z M 18 14 h 18 v 5 H 18 Z" fill="#d7e6cd" />
          <rect x="8" y="38" width="7" height="7" fill="#f6d58c" />
          <rect x="39" y="38" width="7" height="7" fill="#f6d58c" />
          <rect x="22" y="48" width="10" height="15" fill="#39475e" />
        </g>}
      </g>
      <g data-landscape-slot="forecourt" shapeRendering="crispEdges">
        {hasCosmetic("forecourt", "pond") && <g data-cosmetic="pond" transform={`translate(${forecourtX} ${groundTop + 198})`}>
          <path d="M 0 8 h 62 v 22 H 0 Z" fill="#8d7757" stroke="#d9c288" strokeWidth="2" />
          <path d="M 6 12 h 50 v 13 H 6 Z" fill="#4c9f9e" />
          <path d="M 13 14 h 13 v 3 H 13 Z M 37 20 h 13 v 3 H 37 Z" fill="#b6e3ca" />
          <rect x="-5" y="30" width="72" height="5" fill="#677b63" />
        </g>}
        {hasCosmetic("forecourt", "lantern") && <g data-cosmetic="lantern" transform={`translate(${forecourtX} ${groundTop + 176})`}>
          <rect x="-3" y="55" width="34" height="5" fill="#677b63" />
          <rect x="12" y="20" width="4" height="35" fill="#8f6b4e" />
          <rect x="5" y="5" width="18" height="18" fill="#e8c67d" stroke="#754e52" strokeWidth="2" />
          <rect x="9" y="9" width="10" height="10" fill="#fff0ad" />
          <path d="M 3 5 h 22 M 7 0 h 14" stroke="#754e52" strokeWidth="2" />
        </g>}
        {hasCosmetic("forecourt", "rover") && <g data-cosmetic="rover" transform={`translate(${forecourtX} ${groundTop + 204})`}>
          <rect x="2" y="10" width="50" height="18" fill="#ce9b68" stroke="#564d58" strokeWidth="2" />
          <rect x="12" y="0" width="21" height="12" fill="#8ac8c4" stroke="#564d58" strokeWidth="2" />
          <rect x="21" y="-7" width="4" height="8" fill="#e8d58f" />
          <circle cx="14" cy="29" r="6" fill="#34475a" stroke="#e2d59d" strokeWidth="2" />
          <circle cx="42" cy="29" r="6" fill="#34475a" stroke="#e2d59d" strokeWidth="2" />
          <rect x="4" y="16" width="8" height="5" fill="#f2d287" />
          <path d="M 34 13 h 14 v 4 H 34 Z" fill="#a3ddd1" />
        </g>}
        {hasCosmetic("forecourt", "greenhouse") && <g data-cosmetic="greenhouse" transform={`translate(${forecourtX - 14} ${groundTop + 194})`}>
          <path d="M 1 38 V 13 L 34 1 L 67 13 V 38 Z" fill="#84c8bd" fillOpacity=".78" stroke="#d8e6bd" strokeWidth="2" />
          <path d="M 5 15 h 58 M 13 12 v 26 m 21 -30 v 30 m 22 -26 v 26" stroke="#607e79" strokeWidth="2" />
          <rect x="27" y="25" width="14" height="13" fill="#8eb76c" />
          <path d="M 34 25 v -9 m -5 9 l 5 -5 l 5 5" fill="#79a966" stroke="#477451" strokeWidth="2" />
          <rect x="-3" y="38" width="74" height="5" fill="#677b63" />
        </g>}
      </g>
    </g>
  );
}
