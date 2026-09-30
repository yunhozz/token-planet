import type { JSX } from "react";
import type { PlanetObject } from "../types/usage";

export function PlanetObjectSprite({ object, x, y, scale }: { object: PlanetObject; x: number; y: number; scale: number }): JSX.Element {
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
