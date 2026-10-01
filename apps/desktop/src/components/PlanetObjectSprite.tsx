import type { CSSProperties, JSX } from "react";
import type { PlanetObject } from "../types/usage";

type MotionKind = "plant" | "creature" | "water" | "structure" | "signal" | "rock" | "ground";

function motionKind(kind: string): MotionKind {
  if (["tree", "fern", "crops"].includes(kind)) return "plant";
  if (kind === "creature") return "creature";
  if (kind === "water") return "water";
  if (["satellite", "rocket"].includes(kind)) return "signal";
  if (["path", "road", "rail"].includes(kind)) return "ground";
  if (kind === "rock") return "rock";
  return "structure";
}

export function PlanetObjectSprite({ object, x, y, scale }: { object: PlanetObject; x: number; y: number; scale: number }): JSX.Element {
  const color = object.stage < 2 ? "#7db978" : object.stage === 2 ? "#e8bd75" : object.stage === 3 ? "#d18473" : "#80d4ce";
  const phase = Math.abs(Math.trunc((Number.isFinite(object.seed) ? object.seed : object.ordinal) % 23));
  const motionStyle = {
    "--object-motion-duration": `${3.1 + phase * 0.11}s`,
    "--object-motion-delay": `${-(phase % 12) * 0.29}s`,
  } as CSSProperties;
  const motion = motionKind(object.kind);

  let artwork: JSX.Element;
  switch (object.kind) {
    case "rock":
      artwork = <><rect width="12" height="7" fill="#87949b" /><rect x="3" y="-4" width="6" height="4" fill="#aeb7ae" /><rect className="planet-object-rock-glint" x="4" y="-3" width="2" height="2" fill="#e5ead7" /></>;
      break;
    case "water":
      artwork = <><rect width="18" height="5" fill="#70c7c8" /><rect className="planet-object-water-glint" x="4" y="-3" width="10" height="3" fill="#a6e2d1" /><path className="planet-object-water-ripple" d="M 2 3 h 5 m 7 0 h 3" stroke="#e0fff0" strokeWidth="1" /></>;
      break;
    case "tree": case "fern":
      artwork = <g className="planet-object-plant-art"><rect x="6" y="7" width="4" height="9" fill="#956449" /><rect x="2" y="2" width="12" height="7" fill="#548f61" /><rect x="5" y="-2" width="7" height="5" fill="#83bf76" /></g>;
      break;
    case "creature":
      artwork = <><rect x="1" y="2" width="13" height="8" fill="#f0b969" /><rect x="11" width="6" height="6" fill="#f0b969" /><rect x="14" y="1" width="1" height="1" fill="#292d3f" /><rect x="3" y="9" width="2" height="4" fill="#9b6658" /><rect x="11" y="9" width="2" height="4" fill="#9b6658" /></>;
      break;
    case "camp": case "cottage": case "house": case "market":
      artwork = <><rect x="1" y="5" width="18" height="12" fill={object.stage === 1 ? "#bd805c" : "#e9d39a"} /><path d="M-2 6 10 -3 22 6Z" fill={object.stage < 2 ? "#d27f68" : "#985e62"} /><rect x="8" y="10" width="4" height="7" fill="#514257" /><rect className="planet-object-window" x="3" y="8" width="3" height="3" fill="#78bfc1" /></>;
      break;
    case "crops": case "path": case "road": case "rail":
      artwork = <><rect width="20" height="5" fill={object.kind === "path" ? "#b28a67" : object.kind === "road" || object.kind === "rail" ? "#68717a" : "#92b95c"} /><g className={object.kind === "crops" ? "planet-object-crop-stems" : undefined}><rect x="3" y="-4" width="2" height="4" fill={color} /><rect x="9" y="-4" width="2" height="4" fill={color} /><rect x="15" y="-4" width="2" height="4" fill={color} /></g></>;
      break;
    case "well": case "workshop": case "plaza": case "district": case "power": case "factory": case "tower": case "laboratory": case "habitat": case "solar":
      artwork = <><rect x="1" y="3" width="18" height="15" fill={color} /><rect x="4" y="-2" width="12" height="6" fill={color} /><rect className="planet-object-window" x="5" y="7" width="3" height="4" fill="#a4ddcc" /><rect className="planet-object-window planet-object-window--late" x="12" y="7" width="3" height="4" fill="#a4ddcc" /><rect x="9" y="13" width="3" height="5" fill="#41445a" />{object.kind === "factory" && <g className="planet-object-factory-signal"><rect x="16" y="-8" width="3" height="6" fill="#9bb7a0" /><rect className="planet-object-signal-light" x="17" y="-10" width="2" height="2" fill="#f1cf89" /></g>}</>;
      break;
    case "satellite": case "rocket":
      artwork = <><rect x="7" y="-2" width="6" height="17" fill="#e7ddbd" /><path d="m7 1-5 6h5m6-6 5 6h-5" fill="#70c7c8" /><rect x="8" y="-6" width="4" height="4" fill="#e17e6e" /><rect x="8" y="14" width="4" height="4" fill="#e17e6e" /><circle className="planet-object-signal-light" cx="10" cy="-8" r="2" fill="#f1cf89" /></>;
      break;
    default:
      artwork = <><rect x="2" y="2" width="16" height="11" fill={color} /><rect x="5" y="-2" width="10" height="4" fill="#e7ddbd" /><rect className="planet-object-window" x="6" y="5" width="3" height="3" fill="#a4ddcc" /><rect className="planet-object-window planet-object-window--late" x="12" y="5" width="3" height="3" fill="#a4ddcc" /></>;
      break;
  }

  return <g className="planet-object-sprite" transform={`translate(${x} ${y})`}>
    <g transform={`scale(${scale})`}>
      <g className={`planet-object-motion planet-object-motion--${motion}`} style={motionStyle}>
        {artwork}
      </g>
    </g>
  </g>;
}
